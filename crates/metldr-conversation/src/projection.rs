use serde::{Deserialize, Serialize};

use crate::content::{Completion, Entry};
use crate::key::validate_id;
use crate::{MAX_ENTRIES, MAX_MESSAGES, MAX_TEXT_BYTES, MAX_TOOLS, MessageKey, ToolId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectedEntry {
    Message {
        key: MessageKey,
        start: usize,
        end: usize,
        completion: Completion,
    },
    Tool {
        id: ToolId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub output: String,
    pub conversation: Vec<ProjectedEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryRef<'a> {
    Message {
        key: &'a MessageKey,
        text: &'a str,
        completion: &'a Completion,
    },
    Tool {
        id: &'a ToolId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Display {
    projection: Projection,
}

impl Display {
    pub fn new(projection: Projection) -> Result<Self, String> {
        validate_projection(&projection)?;
        Ok(Self { projection })
    }

    pub fn text(&self) -> &str {
        &self.projection.output
    }

    pub fn entries(&self) -> impl Iterator<Item = EntryRef<'_>> {
        self.projection
            .conversation
            .iter()
            .map(|entry| match entry {
                ProjectedEntry::Message {
                    key,
                    start,
                    end,
                    completion,
                } => EntryRef::Message {
                    key,
                    text: &self.projection.output[*start..*end],
                    completion,
                },
                ProjectedEntry::Tool { id } => EntryRef::Tool { id },
            })
    }
}

pub(crate) fn from_native_entries(entries: &[Entry], length: usize) -> Projection {
    let mut output = String::with_capacity(length);
    let conversation = entries
        .iter()
        .map(|entry| match entry {
            Entry::Tool { id } => ProjectedEntry::Tool { id: id.clone() },
            Entry::Message {
                key,
                text,
                completion,
            } => {
                let start = output.len();
                output.push_str(text);
                ProjectedEntry::Message {
                    key: key.clone(),
                    start,
                    end: output.len(),
                    completion: completion.clone(),
                }
            }
        })
        .collect();
    Projection {
        output,
        conversation,
    }
}

fn validate_projection(projection: &Projection) -> Result<(), String> {
    if projection.output.len() > MAX_TEXT_BYTES {
        return Err("content exceeds 8192 UTF-8 bytes".into());
    }
    if projection.conversation.is_empty() {
        return Ok(());
    }
    if projection.conversation.len() > MAX_ENTRIES {
        return Err("too many conversation entries".into());
    }
    let mut cursor = 0;
    let mut messages: Vec<&MessageKey> = Vec::new();
    let mut tools: Vec<&ToolId> = Vec::new();
    for entry in &projection.conversation {
        match entry {
            ProjectedEntry::Message {
                key, start, end, ..
            } => {
                key.validate()?;
                if messages.contains(&key) {
                    return Err("duplicate message key".into());
                }
                messages.push(key);
                if *start != cursor
                    || *end < *start
                    || *end > projection.output.len()
                    || !projection.output.is_char_boundary(*start)
                    || !projection.output.is_char_boundary(*end)
                {
                    return Err("message ranges must partition output at UTF-8 boundaries".into());
                }
                cursor = *end;
            }
            ProjectedEntry::Tool { id } => {
                validate_id(id.as_str(), "tool id")?;
                if tools.contains(&id) {
                    return Err("duplicate tool id".into());
                }
                tools.push(id);
            }
        }
    }
    if cursor != projection.output.len() {
        return Err("message ranges must cover all output".into());
    }
    if messages.len() > MAX_MESSAGES {
        return Err("too many messages".into());
    }
    if tools.len() > MAX_TOOLS {
        return Err("too many observed tools".into());
    }
    Ok(())
}
