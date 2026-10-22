use crate::{Result, agent_model::Launch, provider_settings::Protocol};
use serde::Serialize;

#[derive(Serialize)]
pub(super) enum Route<'a> {
    Fixture,
    Text(Protocol, &'a str),
}

impl<'a> Route<'a> {
    pub(super) fn from_launch(launch: &'a Launch) -> Option<Self> {
        match launch {
            Launch::Fixture { .. } => Some(Self::Fixture),
            Launch::Text {
                protocol, binding, ..
            } => Some(Self::Text(*protocol, &binding.fingerprint)),
            _ => None,
        }
    }

    pub(super) fn key(&self, conversation: &str, source: &str) -> Result<String> {
        crate::agent_model::digest(&(conversation, source, self))
    }
}
