use wasm_bindgen::prelude::*;

use crate::protocol::{AnswerFailureKind, ChildMessage};

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq)]
pub struct AnswerEvent(ChildMessage);

impl AnswerEvent {
    pub fn new(message: ChildMessage) -> Self {
        Self(message)
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnswerEventKind {
    Ready,
    Ack,
    Height,
    Followup,
    Failed,
    RuntimeFailed,
}

#[wasm_bindgen]
impl AnswerEvent {
    pub fn kind(&self) -> AnswerEventKind {
        match &self.0 {
            ChildMessage::Ready {} => AnswerEventKind::Ready,
            ChildMessage::Ack { .. } => AnswerEventKind::Ack,
            ChildMessage::Height { .. } => AnswerEventKind::Height,
            ChildMessage::Followup { .. } => AnswerEventKind::Followup,
            ChildMessage::Failed { .. } => AnswerEventKind::Failed,
            ChildMessage::RuntimeFailed { .. } => AnswerEventKind::RuntimeFailed,
        }
    }

    pub fn request_id(&self) -> Result<u32, JsValue> {
        match &self.0 {
            ChildMessage::Ack { request_id } | ChildMessage::Failed { request_id } => {
                Ok(request_id.get())
            }
            _ => Err(js_error("request id is not available")),
        }
    }

    pub fn height(&self) -> Result<f64, JsValue> {
        match &self.0 {
            ChildMessage::Height { height } => Ok(*height),
            _ => Err(js_error("height is not available")),
        }
    }

    pub fn text(&self) -> Result<String, JsValue> {
        match &self.0 {
            ChildMessage::Followup { text } => Ok(text.clone()),
            _ => Err(js_error("text is not available")),
        }
    }

    pub fn failure_kind(&self) -> Result<AnswerFailureKind, JsValue> {
        match self.0 {
            ChildMessage::RuntimeFailed { reason, .. } => Ok(reason),
            _ => Err(js_error("runtime failure is not available")),
        }
    }
}

fn js_error(message: &str) -> JsValue {
    JsValue::from_str(message)
}
