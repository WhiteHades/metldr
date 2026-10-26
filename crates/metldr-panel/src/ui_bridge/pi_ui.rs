use super::*;
use crate::model::{PiUiDialog, PiUiView, RunRoute, RunStatus};
use crate::pi_ui::display_plain;

mod display;
mod observe;
pub use display::*;
pub(super) use observe::Observer;
pub use observe::UiPiSubscription;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    run: RunId,
    request: RunId,
}
impl Key {
    fn offered(&self, ui: Option<&PiUiView>) -> bool {
        ui.is_some_and(|ui| {
            ui.run == self.run && ui.pending.iter().any(|p| p.request == self.request)
        })
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Operation {
    #[default]
    Idle,
    Sending(Key),
    Waiting(Key),
    Uncertain(Key),
}
impl Operation {
    fn observe(&mut self, ui: Option<&PiUiView>) {
        if let Self::Waiting(key) = self
            && !key.offered(ui)
        {
            *self = Self::Idle;
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct State {
    connected: bool,
    stale: bool,
    ready: bool,
    recovery_required: bool,
    running: bool,
    workflow: Option<String>,
    ui: Option<PiUiView>,
    operation: Operation,
}
impl State {
    fn read(core: &Core) -> Self {
        let view = core.page.view.borrow();
        let run = view
            .pi_ui
            .as_ref()
            .and_then(|ui| view.runs.iter().find(|r| r.id == ui.run));
        let workflow = run.and_then(|r| match &r.route {
            RunRoute::Pi { workflow, .. } => Some(workflow.as_str().to_owned()),
            _ => None,
        });
        Self {
            connected: core.flow().is_connected(),
            stale: core.page.invalidated.get()
                || view.selection != core.scope
                || !core
                    .page
                    .current
                    .borrow()
                    .upgrade()
                    .is_some_and(|p| std::ptr::eq(p.as_ref(), core)),
            ready: core.owner.borrow().readable().is_ok()
                && !core.page.recovery.borrow().active()
                && !matches!(*core.owner.borrow(), Owner::Uncertain),
            recovery_required: matches!(*core.owner.borrow(), Owner::Uncertain),
            running: workflow.is_some() && run.is_some_and(|r| r.status == RunStatus::Running),
            workflow,
            ui: view.pi_ui.clone(),
            operation: core.pi_operation.borrow().clone(),
        }
    }
    fn enabled(&self) -> bool {
        self.connected
            && !self.stale
            && self.ready
            && self.running
            && matches!(self.operation, Operation::Idle)
    }
}

enum Answer {
    Select(String),
    Confirm(bool),
    Text(String),
    Cancel,
}
impl Answer {
    fn offered(&self, dialog: &PiUiDialog) -> bool {
        match (self, dialog) {
            (Self::Select(value), PiUiDialog::Select { options, .. }) => options.contains(value),
            (Self::Confirm(_), PiUiDialog::Confirm { .. }) => true,
            (Self::Text(value), PiUiDialog::Input { .. } | PiUiDialog::Editor { .. }) => {
                serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= 32768 - 512)
            }
            (Self::Cancel, _) => true,
            _ => false,
        }
    }
}

#[wasm_bindgen]
pub struct UiPiUi {
    core: Rc<Core>,
    activation: super::activation::Activation,
}
#[wasm_bindgen]
impl UiBridge {
    pub fn pi_ui(&self) -> Result<UiPiUi, JsValue> {
        Ok(UiPiUi {
            core: Rc::clone(&self.core),
            activation: super::activation::Activation::new().map_err(js_error)?,
        })
    }
}
#[wasm_bindgen]
impl UiPiUi {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiPiSnapshot) => void")]
        callback: Function,
    ) -> Result<UiPiSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.core), callback)
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn select(
        &self,
        run: String,
        request: String,
        value: String,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.send(run, request, Answer::Select(value), event)
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn confirm(
        &self,
        run: String,
        request: String,
        value: bool,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.send(run, request, Answer::Confirm(value), event)
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn text(
        &self,
        run: String,
        request: String,
        value: String,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.send(run, request, Answer::Text(value), event)
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn cancel(
        &self,
        run: String,
        request: String,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.send(run, request, Answer::Cancel, event)
    }
    pub fn editor_text(
        &self,
        run: String,
        revision: String,
        destination: String,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<String, JsValue> {
        self.activation.require(&event).map_err(js_error)?;
        let state = State::read(&self.core);
        let editor = state
            .ui
            .as_ref()
            .filter(|ui| ui.run.as_str() == run)
            .and_then(|ui| ui.editor.as_ref());
        if state.connected
            && !state.stale
            && state.ready
            && !matches!(state.operation, Operation::Uncertain(_))
            && state
                .workflow
                .as_deref()
                .is_some_and(|workflow| destination == "pi" || destination == workflow)
        {
            if let Some(editor) = editor
                && editor.revision.to_string() == revision
            {
                return Ok(editor.text.clone());
            }
        }
        Err(crate::err("This Pi suggestion is no longer available."))
    }
}

impl UiPiUi {
    fn send(
        &self,
        run: String,
        request: String,
        answer: Answer,
        event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.activation.require(&event).map_err(js_error)?;
        let parse = |id| {
            serde_json::from_value(serde_json::Value::String(id))
                .map_err(|_| crate::err("Invalid Pi request identity."))
        };
        let key = Key {
            run: parse(run)?,
            request: parse(request)?,
        };
        let state = State::read(&self.core);
        let offered = state
            .ui
            .as_ref()
            .filter(|ui| ui.run == key.run)
            .and_then(|ui| ui.pending.iter().find(|p| p.request == key.request));
        if !state.enabled() || !offered.is_some_and(|p| answer.offered(&p.dialog)) {
            return Err(crate::err(
                "This Pi reply is no longer available or was not offered.",
            ));
        }
        *self.core.pi_operation.borrow_mut() = Operation::Sending(key.clone());
        self.core.notify_pi_ui();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let (kind, text, confirm) = match answer {
                Answer::Select(value) => ("select", Some(value), None),
                Answer::Confirm(value) => ("confirm", None, Some(value)),
                Answer::Text(value) => ("text", Some(value), None),
                Answer::Cancel => ("cancel", None, None),
            };
            let result = core
                .flow()
                .pi_ui_reply(
                    core.scope.clone(),
                    key.run.clone(),
                    key.request.clone(),
                    kind,
                    text,
                    confirm,
                )
                .await;
            let result = match result {
                Ok(view) => match core.accept(view) {
                    Ok(_) => {
                        *core.pi_operation.borrow_mut() = Operation::Waiting(key);
                        Ok(())
                    }
                    Err(error) => {
                        *core.pi_operation.borrow_mut() = Operation::Uncertain(key);
                        Err(error)
                    }
                },
                Err(error) => {
                    *core.pi_operation.borrow_mut() = Operation::Uncertain(key);
                    Err(error)
                }
            };
            core.notify_pi_ui();
            result.map(|()| JsValue::UNDEFINED).map_err(js_error)
        }))
    }
}
impl Core {
    pub(super) fn notify_pi_ui(&self) {
        self.page.notify_connection();
        {
            let view = self.page.view.borrow();
            if view.selection == self.scope {
                self.pi_operation.borrow_mut().observe(view.pi_ui.as_ref());
            }
        }
        let observers: Vec<_> = {
            let mut list = self.pi_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|o| !o.closed()));
            list.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for observer in observers {
            if observer.emit().is_err() {
                observer.close();
            }
            observer.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_answers_keep_raw_values_and_unknown_receipts_stay_locked() {
        let raw = "\u{1b}[31m<b>choice</b>\u{1b}[0m";
        let dialog = PiUiDialog::Select {
            title: "Choose".into(),
            options: vec![raw.into()],
        };
        assert!(Answer::Select(raw.into()).offered(&dialog));
        assert!(!Answer::Select(display_plain(raw)).offered(&dialog));
        assert!(!Answer::Text(raw.into()).offered(&dialog));
        let input = PiUiDialog::Input {
            title: "Name".into(),
            placeholder: None,
        };
        assert!(Answer::Text("مرحبا 🌱".into()).offered(&input));
        assert!(!Answer::Text("\n".repeat(16384)).offered(&input));
        let id = |n| {
            serde_json::from_value(serde_json::json!(format!(
                "0123456789abcdef0123456789abcdef:{n}"
            )))
            .unwrap()
        };
        let key = Key {
            run: id(1),
            request: id(2),
        };
        let mut waiting = Operation::Waiting(key.clone());
        waiting.observe(None);
        assert!(matches!(waiting, Operation::Idle));
        let mut sending = Operation::Sending(key.clone());
        sending.observe(None);
        assert!(matches!(sending, Operation::Sending(_)));
        let mut unknown = Operation::Uncertain(key);
        unknown.observe(None);
        assert!(matches!(unknown, Operation::Uncertain(_)));
    }
}
