use std::{cell::Cell, num::NonZeroU32, rc::Rc};

use wasm_bindgen::prelude::*;

use crate::AnswerEvent;
use crate::dom::{DocumentState, DomSession};
use crate::protocol::{
    Binding, ChildMessage, HostMessage, decode_child, decode_host, decode_initialize, encode_child,
};

#[wasm_bindgen]
pub struct GuestSession {
    binding: Binding,
    last_request_id: u32,
    document: Rc<Cell<DocumentState>>,
}

#[wasm_bindgen]
impl GuestSession {
    pub fn new(initialization: String) -> Result<GuestSession, JsValue> {
        let binding = decode_initialize(&initialization).map_err(|error| js_error(&error))?;
        Ok(Self {
            binding,
            last_request_id: 0,
            document: Rc::new(Cell::new(DocumentState::Empty)),
        })
    }

    pub fn receive(&mut self, encoded: String) -> Result<GuestCommand, JsValue> {
        let message = decode_host(&encoded, &self.binding).map_err(|error| js_error(&error))?;
        let request_id = message.request_id();
        if request_id.get() <= self.last_request_id {
            return Err(js_error("answer request is stale"));
        }
        self.last_request_id = request_id.get();
        self.document.set(match (&message, self.document.get()) {
            (HostMessage::Document { .. }, _) => DocumentState::Ready(request_id),
            (HostMessage::Preview { .. }, _) => DocumentState::Empty,
            (_, state @ DocumentState::Mounted(_)) => state,
            _ => DocumentState::Empty,
        });
        Ok(GuestCommand(message))
    }

    pub fn mount_dom(&mut self, parents: Vec<u32>) -> Result<DomSession, JsValue> {
        DomSession::mount(self.binding.clone(), &self.document, parents)
            .map_err(|error| js_error(&error))
    }

    pub fn worker_event(&self, encoded: String) -> Result<AnswerEvent, JsValue> {
        let message = decode_child(&encoded, &self.binding).map_err(|error| js_error(&error))?;
        if !matches!(
            message,
            ChildMessage::Ack { .. } | ChildMessage::Failed { .. } | ChildMessage::Followup { .. }
        ) {
            return Err(js_error("invalid worker event"));
        }
        Ok(AnswerEvent::new(message))
    }

    pub fn ready(&self) -> Result<String, JsValue> {
        self.send(ChildMessage::Ready {})
    }
    pub fn ack(&self, request_id: u32) -> Result<String, JsValue> {
        self.send(ChildMessage::Ack {
            request_id: nonzero(request_id)?,
        })
    }
    pub fn failed(&self, request_id: u32) -> Result<String, JsValue> {
        self.send(ChildMessage::Failed {
            request_id: nonzero(request_id)?,
        })
    }
    pub fn runtime_failed(
        &self,
        document_id: u32,
        reason: crate::AnswerFailureKind,
    ) -> Result<String, JsValue> {
        self.send(ChildMessage::RuntimeFailed {
            document_id: nonzero(document_id)?,
            reason,
        })
    }
    pub fn height(&self, height: f64) -> Result<String, JsValue> {
        self.send(ChildMessage::Height { height })
    }
    pub fn followup(&self, text: String) -> Option<String> {
        self.send(ChildMessage::Followup { text }).ok()
    }
}

impl GuestSession {
    fn send(&self, message: ChildMessage) -> Result<String, JsValue> {
        encode_child(&self.binding, message).map_err(|error| js_error(&error))
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestCommandKind {
    Preview,
    Document,
    Functions,
    Expression,
    FollowupConsumed,
}

#[wasm_bindgen]
pub struct GuestCommand(HostMessage);

#[wasm_bindgen]
impl GuestCommand {
    pub fn kind(&self) -> GuestCommandKind {
        match &self.0 {
            HostMessage::Preview { .. } => GuestCommandKind::Preview,
            HostMessage::Document { .. } => GuestCommandKind::Document,
            HostMessage::Functions { .. } => GuestCommandKind::Functions,
            HostMessage::Expression { .. } => GuestCommandKind::Expression,
            HostMessage::FollowupConsumed { .. } => GuestCommandKind::FollowupConsumed,
        }
    }
    pub fn request_id(&self) -> u32 {
        self.0.request_id().get()
    }
    pub fn css(&self) -> Result<String, JsValue> {
        match &self.0 {
            HostMessage::Preview { css, .. } | HostMessage::Document { css, .. } => Ok(css.clone()),
            _ => Err(js_error("css is not available")),
        }
    }
    pub fn html(&self) -> Result<String, JsValue> {
        match &self.0 {
            HostMessage::Preview { html, .. } | HostMessage::Document { html, .. } => {
                Ok(html.clone())
            }
            _ => Err(js_error("html is not available")),
        }
    }
    pub fn code(&self) -> Result<String, JsValue> {
        match &self.0 {
            HostMessage::Functions { code, .. } | HostMessage::Expression { code, .. } => {
                Ok(code.clone())
            }
            _ => Err(js_error("code is not available")),
        }
    }
}

fn js_error(message: &str) -> JsValue {
    JsValue::from_str(message)
}

fn nonzero(request_id: u32) -> Result<NonZeroU32, JsValue> {
    NonZeroU32::new(request_id).ok_or_else(|| js_error("answer request id must be positive"))
}
