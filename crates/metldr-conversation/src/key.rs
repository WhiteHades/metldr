use serde::{Deserialize, Deserializer, Serialize};

use crate::{MAX_ID_BYTES, MAX_MESSAGES};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ArtifactId(String);

impl ArtifactId {
    pub fn new(id: String) -> Result<Self, String> {
        validate_id(&id, "artifact id")?;
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ArtifactId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageKey {
    Native { id: String },
    Observed { ordinal: u16 },
}

impl<'de> Deserialize<'de> for MessageKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Encoded {
            Native { id: String },
            Observed { ordinal: u16 },
        }
        let key = match Encoded::deserialize(deserializer)? {
            Encoded::Native { id } => Self::Native { id },
            Encoded::Observed { ordinal } => Self::Observed { ordinal },
        };
        key.validate().map_err(serde::de::Error::custom)?;
        Ok(key)
    }
}

impl MessageKey {
    pub fn native(id: String) -> Result<Self, String> {
        validate_id(&id, "message id")?;
        Ok(Self::Native { id })
    }

    pub fn observed(ordinal: u16) -> Result<Self, String> {
        if !(1..=MAX_MESSAGES as u16).contains(&ordinal) {
            return Err("observed message ordinal must be between 1 and 64".into());
        }
        Ok(Self::Observed { ordinal })
    }

    pub fn dom_key(&self) -> String {
        match self {
            Self::Native { id } => format!("native:{id}"),
            Self::Observed { ordinal } => format!("observed:{ordinal}"),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Native { id } => validate_id(id, "message id"),
            Self::Observed { ordinal } if (1..=MAX_MESSAGES as u16).contains(ordinal) => Ok(()),
            Self::Observed { .. } => {
                Err("observed message ordinal must be between 1 and 64".into())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(transparent)]
pub struct ToolId(String);

impl ToolId {
    pub fn new(id: String) -> Result<Self, String> {
        validate_id(&id, "tool id")?;
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ToolId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = String::deserialize(deserializer)?;
        Self::new(id).map_err(serde::de::Error::custom)
    }
}

pub(crate) fn validate_id(id: &str, name: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_ID_BYTES || id.chars().any(char::is_control) {
        return Err(format!(
            "{name} must be nonempty, at most 128 UTF-8 bytes, and contain no control characters"
        ));
    }
    Ok(())
}
