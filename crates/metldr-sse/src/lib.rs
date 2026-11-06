#![doc = include_str!("../README.md")]

type Result<T> = std::result::Result<T, String>;

#[cfg(test)]
mod checks;

pub struct Sse {
    provider: &'static str,
    line: Vec<u8>,
    data: String,
    event: String,
    started: bool,
    after_cr: bool,
    bound: usize,
}

impl Sse {
    pub fn new(provider: &'static str) -> Self {
        Self::with_bound(provider, 131_072)
    }

    pub fn with_bound(provider: &'static str, bound: usize) -> Self {
        Self {
            provider,
            line: Vec::new(),
            data: String::new(),
            event: String::new(),
            started: false,
            after_cr: false,
            bound,
        }
    }

    pub fn byte(&mut self, byte: u8) -> Result<Option<(String, String)>> {
        if self.after_cr && byte == b'\n' {
            self.after_cr = false;
            return Ok(None);
        }
        self.after_cr = byte == b'\r';
        if !matches!(byte, b'\r' | b'\n') {
            if self.line.len() >= self.bound {
                return Err(format!("{} SSE line exceeds bound", self.provider));
            }
            self.line.push(byte);
            return Ok(None);
        }
        let line = std::str::from_utf8(&self.line)
            .map_err(|_| format!("{} SSE is not UTF8", self.provider))?;
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
                    if self.data.len().saturating_add(value.len() + 1) > self.bound {
                        return Err(format!("{} SSE event exceeds bound", self.provider));
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
            Err(format!("{} SSE ended within an event", self.provider))
        }
    }
}
