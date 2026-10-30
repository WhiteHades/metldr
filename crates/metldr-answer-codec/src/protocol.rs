use std::num::NonZeroU32;

use metldr_conversation::MAX_GENERATED_BYTES;
use serde::{Deserialize, Deserializer, Serialize};

const MAX_FOLLOWUP_BYTES: usize = 8_192;
const MAX_FRAME_ID_BYTES: usize = 32;
const MAX_THREAD_ID_BYTES: usize = 512;
const MAX_OWNED_ID_BYTES: usize = 128;
const MAX_BINDING_ESCAPED: usize =
    6 * (MAX_FRAME_ID_BYTES + MAX_THREAD_ID_BYTES + 2 * MAX_OWNED_ID_BYTES);
const BINDING_OVERHEAD: usize = 160;
const ENVELOPE_OVERHEAD: usize = 64;
pub const MAX_INITIALIZE_BYTES: usize = MAX_BINDING_ESCAPED + BINDING_OVERHEAD + ENVELOPE_OVERHEAD;
pub const MAX_CHILD_BYTES: usize =
    MAX_BINDING_ESCAPED + BINDING_OVERHEAD + 6 * MAX_FOLLOWUP_BYTES + ENVELOPE_OVERHEAD;
pub const MAX_HOST_BYTES: usize =
    MAX_BINDING_ESCAPED + BINDING_OVERHEAD + 6 * MAX_GENERATED_BYTES + ENVELOPE_OVERHEAD;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Binding(RawBinding);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawBinding {
    v: u8,
    frame_id: String,
    thread_id: String,
    run_id: String,
    answer_id: String,
}

impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        RawBinding::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

fn valid_frame_id(value: &str) -> bool {
    value.len() == MAX_FRAME_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

impl TryFrom<RawBinding> for Binding {
    type Error = String;

    fn try_from(raw: RawBinding) -> Result<Self, Self::Error> {
        if raw.v != 1
            || !valid_frame_id(&raw.frame_id)
            || raw.thread_id.is_empty()
            || raw.thread_id.len() > MAX_THREAD_ID_BYTES
            || raw.run_id.is_empty()
            || raw.run_id.len() > MAX_OWNED_ID_BYTES
            || raw.answer_id.is_empty()
            || raw.answer_id.len() > MAX_OWNED_ID_BYTES
        {
            return Err("invalid answer binding".into());
        }
        Ok(Self(raw))
    }
}

impl Binding {
    pub fn new(
        frame_id: String,
        thread_id: String,
        run_id: String,
        answer_id: String,
    ) -> Result<Self, String> {
        RawBinding {
            v: 1,
            frame_id,
            thread_id,
            run_id,
            answer_id,
        }
        .try_into()
    }

    pub fn set_frame_id(&mut self, frame_id: String) -> Result<(), String> {
        if !valid_frame_id(&frame_id) {
            return Err("invalid answer binding".into());
        }
        self.0.frame_id = frame_id;
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnswerFailureKind {
    Execution,
    Timeout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChildMessage {
    Ready {},
    Ack {
        request_id: NonZeroU32,
    },
    Height {
        height: f64,
    },
    Followup {
        text: String,
    },
    Failed {
        request_id: NonZeroU32,
    },
    RuntimeFailed {
        document_id: NonZeroU32,
        reason: AnswerFailureKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HostMessage {
    Preview {
        request_id: NonZeroU32,
        css: String,
        html: String,
    },
    Document {
        request_id: NonZeroU32,
        css: String,
        html: String,
    },
    Functions {
        request_id: NonZeroU32,
        code: String,
    },
    Expression {
        request_id: NonZeroU32,
        code: String,
    },
    FollowupConsumed {
        request_id: NonZeroU32,
    },
}

impl HostMessage {
    pub(crate) fn request_id(&self) -> NonZeroU32 {
        match self {
            Self::Preview { request_id, .. }
            | Self::Document { request_id, .. }
            | Self::Functions { request_id, .. }
            | Self::Expression { request_id, .. }
            | Self::FollowupConsumed { request_id } => *request_id,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<B, M> {
    binding: B,
    message: M,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Initialize<B> {
    kind: InitializationKind,
    binding: B,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum InitializationKind {
    Initialize,
}

pub fn decode_initialize(encoded: &str) -> Result<Binding, String> {
    if encoded.len() > MAX_INITIALIZE_BYTES {
        return Err("answer initialization exceeds its limit".into());
    }
    let init: Initialize<Binding> =
        serde_json::from_str(encoded).map_err(|_| "invalid answer initialization".to_owned())?;
    match init.kind {
        InitializationKind::Initialize => Ok(init.binding),
    }
}

pub fn encode_initialize(binding: &Binding) -> Result<String, String> {
    let encoded = serde_json::to_string(&Initialize {
        kind: InitializationKind::Initialize,
        binding,
    })
    .map_err(|error| error.to_string())?;
    bounded(encoded, MAX_INITIALIZE_BYTES)
}

pub fn encode_host(binding: &Binding, message: HostMessage) -> Result<String, String> {
    validate_host(&message)?;
    encode_envelope(binding, message, MAX_HOST_BYTES)
}

pub fn encode_child(binding: &Binding, message: ChildMessage) -> Result<String, String> {
    validate_child(&message)?;
    encode_envelope(binding, message, MAX_CHILD_BYTES)
}

fn encode_envelope<M: Serialize>(
    binding: &Binding,
    message: M,
    maximum: usize,
) -> Result<String, String> {
    let encoded =
        serde_json::to_string(&Envelope { binding, message }).map_err(|error| error.to_string())?;
    bounded(encoded, maximum)
}

pub fn decode_child(encoded: &str, expected: &Binding) -> Result<ChildMessage, String> {
    if encoded.len() > MAX_CHILD_BYTES {
        return Err("answer child message exceeds its limit".into());
    }
    let received: Envelope<Binding, ChildMessage> =
        serde_json::from_str(encoded).map_err(|_| "invalid answer child message".to_owned())?;
    if &received.binding != expected {
        return Err("answer binding does not match".into());
    }
    validate_child(&received.message)?;
    Ok(received.message)
}

pub fn decode_host(encoded: &str, expected: &Binding) -> Result<HostMessage, String> {
    if encoded.len() > MAX_HOST_BYTES {
        return Err("answer host command exceeds its limit".into());
    }
    let received: Envelope<Binding, HostMessage> =
        serde_json::from_str(encoded).map_err(|_| "invalid answer host command".to_owned())?;
    if &received.binding != expected {
        return Err("answer binding does not match".into());
    }
    validate_host(&received.message)?;
    Ok(received.message)
}

fn bounded(encoded: String, maximum: usize) -> Result<String, String> {
    if encoded.len() > maximum {
        Err("encoded answer message exceeds its limit".into())
    } else {
        Ok(encoded)
    }
}

fn valid_source(text: &str) -> bool {
    text.len() <= MAX_GENERATED_BYTES
}

fn validate_host(message: &HostMessage) -> Result<(), String> {
    let valid = match message {
        HostMessage::Preview { css, html, .. } | HostMessage::Document { css, html, .. } => {
            css.len().saturating_add(html.len()) <= MAX_GENERATED_BYTES
        }
        HostMessage::Functions { code, .. } | HostMessage::Expression { code, .. } => {
            valid_source(code)
        }
        HostMessage::FollowupConsumed { .. } => true,
    };
    if valid {
        Ok(())
    } else {
        Err("answer source exceeds its limit".into())
    }
}

fn validate_child(message: &ChildMessage) -> Result<(), String> {
    match message {
        ChildMessage::Height { height } if !height.is_finite() => {
            Err("answer height is invalid".into())
        }
        ChildMessage::Followup { text }
            if text.trim().is_empty() || text.len() > MAX_FOLLOWUP_BYTES =>
        {
            Err("answer follow-up is invalid".into())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod checks;
