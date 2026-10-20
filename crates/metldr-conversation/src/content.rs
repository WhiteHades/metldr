use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serialize};

use crate::key::validate_id;
use crate::projection::{Projection, from_native_entries};
use crate::{MAX_ENTRIES, MAX_MESSAGES, MAX_TEXT_BYTES, MAX_TOOLS, MessageKey, ToolId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completion {
    Streaming,
    FinalSnapshot,
    StreamEnded,
    Interrupted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Entry {
    Message {
        key: MessageKey,
        text: String,
        completion: Completion,
    },
    Tool {
        id: ToolId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ContentKind {
    Flat(String),
    Native(Vec<Entry>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Content(ContentKind);

impl Default for Content {
    fn default() -> Self {
        Self(ContentKind::Flat(String::new()))
    }
}

impl Content {
    pub fn flat(text: String) -> Result<Self, String> {
        let content = Self(ContentKind::Flat(text));
        content.validate()?;
        Ok(content)
    }

    pub fn native() -> Self {
        Self(ContentKind::Native(Vec::new()))
    }

    pub fn is_native(&self) -> bool {
        matches!(&self.0, ContentKind::Native(_))
    }

    pub fn text(&self) -> Cow<'_, str> {
        match &self.0 {
            ContentKind::Flat(text) => Cow::Borrowed(text),
            ContentKind::Native(entries) => Cow::Owned(
                entries
                    .iter()
                    .filter_map(|entry| match entry {
                        Entry::Message { text, .. } => Some(text.as_str()),
                        Entry::Tool { .. } => None,
                    })
                    .collect(),
            ),
        }
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            ContentKind::Flat(text) => text.len(),
            ContentKind::Native(entries) => entries
                .iter()
                .map(|entry| match entry {
                    Entry::Message { text, .. } => text.len(),
                    Entry::Tool { .. } => 0,
                })
                .sum(),
        }
    }

    pub fn change(&mut self, change: Change) -> Result<bool, String> {
        let mut next = self.clone();
        let changed = next.apply(change)?;
        if changed {
            next.validate()?;
            *self = next;
        }
        Ok(changed)
    }

    fn apply(&mut self, change: Change) -> Result<bool, String> {
        match (&mut self.0, change) {
            (ContentKind::Flat(text), Change::FlatAppend(delta)) => {
                if delta.is_empty() {
                    return Ok(false);
                }
                text.push_str(&delta);
                Ok(true)
            }
            (ContentKind::Native(entries), Change::Begin(key)) => {
                key.validate()?;
                if find_message(entries, &key).is_some() {
                    return Err("message key already exists".into());
                }
                entries.push(Entry::Message {
                    key,
                    text: String::new(),
                    completion: Completion::Streaming,
                });
                Ok(true)
            }
            (ContentKind::Native(entries), Change::Append { key, text }) => {
                key.validate()?;
                match find_message(entries, &key) {
                    Some(index) => match &mut entries[index] {
                        Entry::Message {
                            text: current,
                            completion: Completion::Streaming,
                            ..
                        } => {
                            current.push_str(&text);
                            Ok(!text.is_empty())
                        }
                        Entry::Message { .. } => Err("cannot append to a completed message".into()),
                        Entry::Tool { .. } => unreachable!(),
                    },
                    None => {
                        entries.push(Entry::Message {
                            key,
                            text,
                            completion: Completion::Streaming,
                        });
                        Ok(true)
                    }
                }
            }
            (ContentKind::Native(entries), Change::Final { key, text }) => {
                key.validate()?;
                match find_message(entries, &key) {
                    Some(index) => match &mut entries[index] {
                        Entry::Message {
                            text: current,
                            completion,
                            ..
                        } if *completion == Completion::FinalSnapshot => {
                            if *current == text {
                                Ok(false)
                            } else {
                                Err("conflicting final snapshot".into())
                            }
                        }
                        Entry::Message {
                            text: current,
                            completion,
                            ..
                        } => {
                            *current = text;
                            *completion = Completion::FinalSnapshot;
                            Ok(true)
                        }
                        Entry::Tool { .. } => unreachable!(),
                    },
                    None => {
                        entries.push(Entry::Message {
                            key,
                            text,
                            completion: Completion::FinalSnapshot,
                        });
                        Ok(true)
                    }
                }
            }
            (ContentKind::Flat(_), _) => Err("native changes require native content".into()),
            (ContentKind::Native(_), Change::FlatAppend(_)) => {
                Err("flat append requires flat content".into())
            }
        }
    }

    pub fn observe_tool(&mut self, id: ToolId) -> Result<bool, String> {
        let mut next = self.clone();
        let ContentKind::Native(entries) = &mut next.0 else {
            return Err("tools require native content".into());
        };
        if entries
            .iter()
            .any(|entry| matches!(entry, Entry::Tool { id: current } if current == &id))
        {
            return Ok(false);
        }
        entries.push(Entry::Tool { id });
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn end(&mut self, ending: Ending) -> Result<bool, String> {
        let ContentKind::Native(entries) = &mut self.0 else {
            return Ok(false);
        };
        let completion = match ending {
            Ending::Complete => Completion::StreamEnded,
            Ending::Interrupted => Completion::Interrupted,
        };
        let mut changed = false;
        for entry in entries {
            if let Entry::Message {
                completion: state @ Completion::Streaming,
                ..
            } = entry
            {
                *state = completion.clone();
                changed = true;
            }
        }
        Ok(changed)
    }

    pub fn projection(&self) -> Projection {
        match &self.0 {
            ContentKind::Flat(output) => Projection {
                output: output.clone(),
                conversation: Vec::new(),
            },
            ContentKind::Native(entries) => from_native_entries(entries, self.len()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match &self.0 {
            ContentKind::Flat(text) => check_text(text),
            ContentKind::Native(entries) => {
                let mut messages: Vec<&MessageKey> = Vec::new();
                let mut tools: Vec<&ToolId> = Vec::new();
                let mut bytes = 0;
                if entries.len() > MAX_ENTRIES {
                    return Err("too many conversation entries".into());
                }
                for entry in entries {
                    match entry {
                        Entry::Message { key, text, .. } => {
                            key.validate()?;
                            if messages.contains(&key) {
                                return Err("duplicate message key".into());
                            }
                            messages.push(key);
                            bytes += text.len();
                            check_text(text)?;
                        }
                        Entry::Tool { id } => {
                            validate_id(id.as_str(), "tool id")?;
                            if tools.contains(&id) {
                                return Err("duplicate tool id".into());
                            }
                            tools.push(id);
                        }
                    }
                }
                if messages.len() > MAX_MESSAGES {
                    return Err("too many messages".into());
                }
                if tools.len() > MAX_TOOLS {
                    return Err("too many observed tools".into());
                }
                if bytes > MAX_TEXT_BYTES {
                    return Err("content exceeds 8192 UTF-8 bytes".into());
                }
                Ok(())
            }
        }
    }
}

impl Serialize for Content {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        match &self.0 {
            ContentKind::Flat(text) => serializer.serialize_str(text),
            ContentKind::Native(entries) => NativeContent { entries }.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Encoded {
            Flat(String),
            Native(NativeContentOwned),
        }
        let content = match Encoded::deserialize(deserializer)? {
            Encoded::Flat(text) => Self(ContentKind::Flat(text)),
            Encoded::Native(native) => Self(ContentKind::Native(native.entries)),
        };
        content.validate().map_err(serde::de::Error::custom)?;
        Ok(content)
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct NativeContent<'a> {
    entries: &'a [Entry],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeContentOwned {
    entries: Vec<Entry>,
}

fn find_message(entries: &[Entry], key: &MessageKey) -> Option<usize> {
    entries
        .iter()
        .position(|entry| matches!(entry, Entry::Message { key: current, .. } if current == key))
}

fn check_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_TEXT_BYTES {
        Err("content exceeds 8192 UTF-8 bytes".into())
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    FlatAppend(String),
    Begin(MessageKey),
    Append { key: MessageKey, text: String },
    Final { key: MessageKey, text: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ending {
    Complete,
    Interrupted,
}
