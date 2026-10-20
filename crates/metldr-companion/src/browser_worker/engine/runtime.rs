use chromiumoxide::types::{CallId, EventMessage, Method, MethodId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

mod socket;

pub type Result<T> = std::result::Result<T, String>;
const MAX_MESSAGE: usize = 65_536;
const CAPACITY: usize = 64;
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RawEvent {
    pub method: MethodId,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub params: Value,
}

impl Method for RawEvent {
    fn identifier(&self) -> MethodId {
        self.method.clone()
    }
}

impl EventMessage for RawEvent {
    fn session_id(&self) -> Option<&str> {
        self.session.as_deref()
    }
}

impl RawEvent {
    fn validate(&self) -> Result<()> {
        validate_envelope(&self.method, self.session.as_deref(), &self.params)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireCommand {
    pub id: CallId,
    pub method: MethodId,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub params: Value,
}

impl WireCommand {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_envelope(&self.method, self.session.as_deref(), &self.params)
    }
}

fn validate_envelope(method: &str, session: Option<&str>, params: &Value) -> Result<()> {
    if method.is_empty()
        || method.len() > 128
        || !method
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        || session.is_some_and(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        || !params.is_object()
    {
        return Err("invalid browser protocol envelope".into());
    }
    Ok(())
}

mod transport;
pub use transport::{Dispatch, Output, Transport};

mod context;
#[doc(hidden)]
pub use context::{Change, ContextMap, FrameContext};
