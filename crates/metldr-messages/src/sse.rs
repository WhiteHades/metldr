use crate::Result;

#[derive(Default)]
pub(super) struct Sse {
    line: Vec<u8>,
    data: String,
    event: String,
    started: bool,
    after_cr: bool,
}

impl Sse {
    pub fn byte(&mut self, byte: u8) -> Result<Option<(String, String)>> {
        if self.after_cr && byte == b'\n' {
            self.after_cr = false;
            return Ok(None);
        }
        self.after_cr = byte == b'\r';
        if !matches!(byte, b'\r' | b'\n') {
            if self.line.len() >= 131_072 {
                return Err("Messages SSE line exceeds bound".into());
            }
            self.line.push(byte);
            return Ok(None);
        }
        let line = std::str::from_utf8(&self.line).map_err(|_| "Messages SSE is not UTF8")?;
        let line = if self.started {
            line
        } else {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        };
        self.started = true;
        let frame = if line.is_empty() {
            let event = std::mem::take(&mut self.event);
            if self.data.is_empty() {
                None
            } else {
                self.data.pop();
                Some((event, std::mem::take(&mut self.data)))
            }
        } else {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => {
                    if self.data.len().saturating_add(value.len() + 1) > 131_072 {
                        return Err("Messages SSE event exceeds bound".into());
                    }
                    self.data.push_str(value);
                    self.data.push('\n');
                }
                "event" => self.event = value.into(),
                _ => {}
            }
            None
        };
        self.line.clear();
        Ok(frame)
    }

    pub fn finish(self) -> Result<()> {
        if self.line.is_empty() && self.data.is_empty() && self.event.is_empty() {
            Ok(())
        } else {
            Err("Messages SSE ended within an event".into())
        }
    }
}
