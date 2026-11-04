use serde::{Deserialize, Serialize};

use crate::content::{ArtifactPresentation, Completion, Entry};
use crate::key::validate_id;
use crate::{ArtifactId, MAX_ENTRIES, MAX_MESSAGES, MAX_TEXT_BYTES, MAX_TOOLS, MessageKey, ToolId};

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
    Artifact {
        id: ArtifactId,
        #[serde(default)]
        presentation: ArtifactPresentation,
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
    Artifact {
        id: &'a ArtifactId,
        presentation: &'a ArtifactPresentation,
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
                ProjectedEntry::Artifact { id, presentation } => {
                    EntryRef::Artifact { id, presentation }
                }
            })
    }
}

pub(crate) fn from_native_entries(entries: &[Entry], length: usize) -> Projection {
    let mut output = String::with_capacity(length);
    let conversation = entries
        .iter()
        .map(|entry| match entry {
            Entry::Tool { id } => ProjectedEntry::Tool { id: id.clone() },
            Entry::Artifact { id, presentation } => ProjectedEntry::Artifact {
                id: id.clone(),
                presentation: presentation.clone(),
            },
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
    let mut artifacts: Vec<&ArtifactId> = Vec::new();
    let mut open_text = false;
    for entry in &projection.conversation {
        match entry {
            ProjectedEntry::Message {
                key,
                start,
                end,
                completion,
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
                open_text |= *completion == Completion::Streaming;
            }
            ProjectedEntry::Tool { id } => {
                validate_id(id.as_str(), "tool id")?;
                if tools.contains(&id) {
                    return Err("duplicate tool id".into());
                }
                tools.push(id);
            }
            ProjectedEntry::Artifact { id, presentation } => {
                if open_text {
                    return Err("artifact requires completed preceding text".into());
                }
                if artifacts.contains(&id) || artifacts.len() >= crate::MAX_ARTIFACTS {
                    return Err("duplicate or too many artifact references".into());
                }
                artifacts.push(id);
                if matches!(
                    presentation,
                    ArtifactPresentation::Generated { revision: 0, .. }
                ) {
                    return Err("generated artifact revision must be positive".into());
                }
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
