use serde::Deserialize;

use crate::thinking::ThinkingFilter;

const MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Deserialize)]
struct Record {
    done: Option<bool>,
    message: Option<Message>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

pub struct Decoder {
    pending: Vec<u8>,
    thinking: ThinkingFilter,
    done: bool,
    finished: bool,
    failed: bool,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            thinking: ThinkingFilter::new(),
            done: false,
            finished: false,
            failed: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<String, String> {
        if self.failed {
            return Err("stream decoder is in a failed state".into());
        }

        let mut output = String::new();
        for &byte in bytes {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.pending);
                if let Err(error) = self.process_line(&line, &mut output) {
                    self.failed = true;
                    return Err(error);
                }
            } else if self.done {
                if !byte.is_ascii_whitespace() {
                    self.failed = true;
                    return Err("data received after terminal stream record".into());
                }
            } else {
                self.pending.push(byte);
                if self.pending.len() > MAX_LINE_BYTES {
                    self.failed = true;
                    return Err("stream record exceeds size limit".into());
                }
            }
        }

        Ok(output)
    }

    pub fn finish(&mut self) -> Result<String, String> {
        if self.failed {
            return Err("stream decoder is in a failed state".into());
        }
        if self.finished {
            return Ok(String::new());
        }

        let mut output = String::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            if let Err(error) = self.process_line(&line, &mut output) {
                self.failed = true;
                return Err(error);
            }
        }
        if !self.done {
            self.failed = true;
            return Err("stream ended before terminal record".into());
        }

        output.push_str(&self.thinking.finish());
        self.finished = true;
        Ok(output)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    fn process_line(&mut self, line: &[u8], output: &mut String) -> Result<(), String> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        if self.done {
            return Err("data received after terminal stream record".into());
        }

        let text = std::str::from_utf8(line).map_err(|_| "stream record is not valid UTF-8")?;
        let record: Record = serde_json::from_str(text).map_err(|_| "invalid stream record")?;
        if record.error.is_some() {
            return Err("provider returned a stream error".into());
        }

        let done = record.done.ok_or("stream record is missing done status")?;
        if let Some(content) = record.message.and_then(|message| message.content) {
            output.push_str(&self.thinking.push(&content));
        }
        if done {
            self.done = true;
        }
        Ok(())
    }
}
