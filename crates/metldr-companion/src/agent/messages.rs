use crate::Result;
use metldr_conversation::{Change, MessageKey};
use serde_json::Value;

#[derive(Default)]
pub(super) struct Messages {
    ordinal: u16,
    active: Option<MessageKey>,
}

impl Messages {
    pub(super) fn event(&mut self, event: &Value) -> Result<Option<Change>> {
        match event["type"].as_str() {
            Some("message_start") if event["message"]["role"] == "assistant" => {
                if self.active.is_some() {
                    return Err("Pi assistant messages overlap".into());
                }
                let ordinal = self
                    .ordinal
                    .checked_add(1)
                    .ok_or("Pi message capacity overflow")?;
                let key = MessageKey::observed(ordinal)?;
                self.ordinal = ordinal;
                self.active = Some(key.clone());
                Ok(Some(Change::Begin(key)))
            }
            Some("message_update") if event["assistantMessageEvent"]["type"] == "text_delta" => {
                let key = self
                    .active
                    .clone()
                    .ok_or("Pi delta has no assistant start")?;
                let text = event["assistantMessageEvent"]["delta"]
                    .as_str()
                    .filter(|text| text.len() <= crate::TEXT_MAX)
                    .ok_or("invalid or oversized Pi text delta")?
                    .to_owned();
                Ok(Some(Change::Append { key, text }))
            }
            Some("message_end") if event["message"]["role"] == "assistant" => {
                let key = self
                    .active
                    .clone()
                    .ok_or("Pi assistant end has no matching start")?;
                let parts = event["message"]["content"]
                    .as_array()
                    .ok_or("Pi assistant final content is not an array")?;
                let mut text = String::new();
                for part in parts {
                    match part["type"].as_str() {
                        Some("text") => {
                            let value = part["text"].as_str().ok_or("invalid Pi final text")?;
                            if text.len().saturating_add(value.len()) > crate::TEXT_MAX {
                                return Err("Pi final text exceeds prototype bound".into());
                            }
                            text.push_str(value);
                        }
                        Some("thinking" | "toolCall") => (),
                        _ => return Err("unsupported Pi assistant content".into()),
                    }
                }
                self.active = None;
                Ok(Some(Change::Final { key, text }))
            }
            _ => Ok(None),
        }
    }
}
