use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result, valid_token};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Counter(pub u64);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub store: String,
    pub conversation: String,
    pub epoch: Counter,
}

impl Scope {
    pub fn validate(&self) -> Result<()> {
        validate_scope(&self.store, &self.conversation, self.epoch)
    }
}

fn validate_scope(store: &str, conversation: &str, epoch: Counter) -> Result<()> {
    if store.len() != 32 || !store.bytes().all(|byte| byte.is_ascii_hexdigit()) || epoch.0 == 0 {
        return Err(Error::InvalidIdentity);
    }
    let Some(local) = conversation.strip_prefix(&format!("{store}:")) else {
        return Err(Error::InvalidIdentity);
    };
    if local.is_empty()
        || local.starts_with('0')
        || !local.bytes().all(|byte| byte.is_ascii_digit())
        || local
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .is_none()
    {
        return Err(Error::InvalidIdentity);
    }
    Ok(())
}

impl Serialize for Counter {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Counter {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let number = value.parse::<u64>().map_err(serde::de::Error::custom)?;
        if number.to_string() != value {
            return Err(serde::de::Error::custom("noncanonical counter"));
        }
        Ok(Self(number))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Run,
    Source,
    Action,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub store: String,
    pub conversation: String,
    pub epoch: Counter,
    pub kind: DocumentKind,
    pub document: String,
    pub revision: Counter,
    pub generation: String,
    pub ticket: String,
}

impl Identity {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_scope(&self.store, &self.conversation, self.epoch)?;
        let prefix = format!("{}:", self.store);
        let Some(local) = self.document.strip_prefix(&prefix) else {
            return Err(Error::InvalidIdentity);
        };
        if self.document.len() > 49_152
            || local
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .is_none()
            || self.revision.0 == 0
            || !valid_token(&self.generation, 64)
            || !valid_token(&self.ticket, 96)
        {
            return Err(Error::InvalidIdentity);
        }
        Ok(())
    }
}
