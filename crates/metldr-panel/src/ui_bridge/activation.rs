use js_sys::{Function, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Event, Node};

use crate::model::FlowError;

pub(super) struct Activation {
    trusted: Function,
    kind: Function,
    phase: Function,
    current: Function,
    target: Function,
}

impl Activation {
    pub(super) fn new() -> Result<Self, FlowError> {
        let sample = Event::new("click").map_err(|_| rejected())?;
        let value: &JsValue = sample.as_ref();
        let object: &Object = value.unchecked_ref();
        let prototype = Object::get_prototype_of(value);
        Ok(Self {
            trusted: getter(object, "isTrusted")?,
            kind: getter(&prototype, "type")?,
            phase: getter(&prototype, "eventPhase")?,
            current: getter(&prototype, "currentTarget")?,
            target: getter(&prototype, "target")?,
        })
    }

    pub(super) fn require(&self, event: &JsValue) -> Result<(), FlowError> {
        self.require_kind(event, "click")
    }

    fn require_kind(&self, event: &JsValue, kind: &str) -> Result<(), FlowError> {
        // Invoke platform getters directly so a plain object or overridden field cannot supply trust.
        if self.trusted.call0(event).ok().and_then(|v| v.as_bool()) != Some(true)
            || self
                .kind
                .call0(event)
                .ok()
                .and_then(|v| v.as_string())
                .as_deref()
                != Some(kind)
            || !self
                .phase
                .call0(event)
                .ok()
                .and_then(|v| v.as_f64())
                .is_some_and(|phase| (1.0..=3.0).contains(&phase))
            || self.current.call0(event).map_or(true, |v| v.is_null())
        {
            return Err(rejected());
        }
        let target = self
            .target
            .call0(event)
            .map_err(|_| rejected())?
            .dyn_into::<Node>()
            .map_err(|_| rejected())?;
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(rejected)?;
        if !target
            .owner_document()
            .is_some_and(|owner| owner == document)
        {
            return Err(rejected());
        }
        Ok(())
    }
}

pub(super) struct SubmissionActivation {
    click: Activation,
    key: Function,
    shift: Function,
    composing: Function,
    repeat: Function,
}

impl SubmissionActivation {
    pub(super) fn new() -> Result<Self, FlowError> {
        let event = web_sys::KeyboardEvent::new("keydown").map_err(|_| rejected())?;
        let prototype = Object::get_prototype_of(event.as_ref());
        Ok(Self {
            click: Activation::new()?,
            key: getter(&prototype, "key")?,
            shift: getter(&prototype, "shiftKey")?,
            composing: getter(&prototype, "isComposing")?,
            repeat: getter(&prototype, "repeat")?,
        })
    }

    pub(super) fn require(&self, event: &JsValue) -> Result<(), FlowError> {
        if self
            .click
            .kind
            .call0(event)
            .ok()
            .and_then(|v| v.as_string())
            .as_deref()
            == Some("click")
        {
            return self.click.require(event);
        }
        self.click.require_kind(event, "keydown")?;
        if self
            .key
            .call0(event)
            .ok()
            .and_then(|v| v.as_string())
            .as_deref()
            != Some("Enter")
            || [&self.shift, &self.composing, &self.repeat]
                .iter()
                .any(|getter| getter.call0(event).ok().and_then(|v| v.as_bool()) != Some(false))
        {
            return Err(rejected());
        }
        Ok(())
    }
}

fn getter(object: &Object, name: &str) -> Result<Function, FlowError> {
    let descriptor = Object::get_own_property_descriptor(object, &name.into());
    Reflect::get(&descriptor, &"get".into())
        .map_err(|_| rejected())?
        .dyn_into()
        .map_err(|_| rejected())
}

fn rejected() -> FlowError {
    FlowError::InvalidInput("Use a current click in this panel to answer the request.".into())
}
