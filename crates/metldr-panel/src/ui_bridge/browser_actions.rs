use super::*;
use crate::browser_action_display::{self, Display};
use crate::model::{BrowserActionId, BrowserActionPhase, BrowserActionView};

mod observe;
pub(super) use observe::Observer;
pub use observe::UiBrowserActionSubscription;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    id: BrowserActionId,
    revision: u64,
}

impl Key {
    fn offered(&self, action: &BrowserActionView) -> bool {
        action.id == self.id
            && action.revision == self.revision
            && action.phase == BrowserActionPhase::Pending
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
    fn observe(&mut self, actions: &[BrowserActionView]) {
        if let Self::Waiting(key) = self
            && !actions.iter().any(|row| key.offered(row))
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
    active: Option<RunId>,
    actions: Vec<BrowserActionView>,
    approvable: Vec<BrowserActionId>,
    epoch: u64,
    operation: Operation,
}

impl State {
    fn read(core: &Core) -> Self {
        let view = core.page.view.borrow();
        let flow = core.flow();
        let epoch = flow.source_epoch();
        Self {
            connected: flow.is_connected(),
            stale: core.page.invalidated.get()
                || view.selection != core.scope
                || !core
                    .page
                    .current
                    .borrow()
                    .upgrade()
                    .is_some_and(|current| std::ptr::eq(current.as_ref(), core)),
            ready: core.owner.borrow().readable().is_ok()
                && !core.page.recovery.borrow().active()
                && !matches!(*core.owner.borrow(), Owner::Uncertain),
            recovery_required: matches!(*core.owner.borrow(), Owner::Uncertain),
            active: view.active_run().cloned(),
            actions: view.browser_actions.clone(),
            approvable: view
                .browser_actions
                .iter()
                .filter(|action| view.can_approve(action, epoch))
                .map(|action| action.id.clone())
                .collect(),
            epoch,
            operation: core.browser_operation.borrow().clone(),
        }
    }

    fn enabled(&self, action: &BrowserActionView, approve: bool) -> bool {
        self.connected
            && !self.stale
            && self.ready
            && matches!(self.operation, Operation::Idle)
            && self.active.as_ref() == Some(&action.run)
            && action.phase == BrowserActionPhase::Pending
            && (!approve || self.approvable.contains(&action.id))
    }
}

#[wasm_bindgen]
pub struct UiBrowserActions {
    core: Rc<Core>,
    activation: super::activation::Activation,
}

#[wasm_bindgen]
pub struct UiBrowserActionSnapshot {
    state: State,
}

#[wasm_bindgen]
impl UiBrowserActionSnapshot {
    #[wasm_bindgen(getter)]
    pub fn connected(&self) -> bool {
        self.state.connected
    }
    #[wasm_bindgen(getter)]
    pub fn stale(&self) -> bool {
        self.state.stale
    }
    #[wasm_bindgen(getter)]
    pub fn busy(&self) -> bool {
        !self.state.ready || !matches!(self.state.operation, Operation::Idle)
    }
    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.state.recovery_required || matches!(self.state.operation, Operation::Uncertain(_))
    }
    #[wasm_bindgen(getter)]
    pub fn notice(&self) -> Option<String> {
        if self.state.stale {
            return Some("The conversation changed. Reload to check its actions.".into());
        }
        if !self.state.connected {
            return Some("Connection lost. Reload to check the browser action.".into());
        }
        if self.state.recovery_required {
            return Some("Reload to check saved work before choosing again.".into());
        }
        if !self.state.ready {
            return Some("Opening conversation.".into());
        }
        match self.state.operation {
            Operation::Idle => None,
            Operation::Sending(_) => Some("Sending your decision.".into()),
            Operation::Waiting(_) => {
                Some("Decision received. Waiting for the action to update.".into())
            }
            Operation::Uncertain(_) => {
                Some("The action may have finished. Reload to check before choosing again.".into())
            }
        }
    }
    pub fn actions(&self) -> Vec<UiBrowserAction> {
        self.state
            .actions
            .iter()
            .map(|action| UiBrowserAction {
                action: action.clone(),
                display: browser_action_display::display(action),
                approve: self.state.enabled(action, true),
                decline: self.state.enabled(action, false),
            })
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiBrowserAction {
    action: BrowserActionView,
    display: Display,
    approve: bool,
    decline: bool,
}

#[wasm_bindgen]
impl UiBrowserAction {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.action.id.as_str().into()
    }
    #[wasm_bindgen(getter)]
    pub fn revision(&self) -> String {
        self.action.revision.to_string()
    }
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.action.run.as_str().into()
    }
    #[wasm_bindgen(getter)]
    pub fn url(&self) -> String {
        self.action.url.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.display.title.into()
    }
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> String {
        self.display.status.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn phase(&self) -> String {
        self.action.phase.to_string()
    }
    #[wasm_bindgen(getter)]
    pub fn receipt(&self) -> bool {
        self.display.receipt
    }
    #[wasm_bindgen(getter)]
    pub fn approve_enabled(&self) -> bool {
        self.approve
    }
    #[wasm_bindgen(getter)]
    pub fn decline_enabled(&self) -> bool {
        self.decline
    }
    pub fn details(&self) -> Vec<UiBrowserActionDetail> {
        self.display
            .details
            .iter()
            .map(|row| UiBrowserActionDetail {
                part: row.part,
                label: row.label,
                value: row.value.clone(),
            })
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiBrowserActionDetail {
    part: &'static str,
    label: &'static str,
    value: String,
}
#[wasm_bindgen]
impl UiBrowserActionDetail {
    #[wasm_bindgen(getter)]
    pub fn part(&self) -> String {
        self.part.into()
    }
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.label.into()
    }
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> String {
        self.value.clone()
    }
}

#[wasm_bindgen]
impl UiBridge {
    pub fn browser_actions(&self) -> Result<UiBrowserActions, JsValue> {
        Ok(UiBrowserActions {
            core: Rc::clone(&self.core),
            activation: super::activation::Activation::new().map_err(js_error)?,
        })
    }
}

#[wasm_bindgen]
impl UiBrowserActions {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiBrowserActionSnapshot) => void")]
        callback: Function,
    ) -> Result<UiBrowserActionSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.core), callback)
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn decide(
        &self,
        id: String,
        revision: String,
        approve: bool,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.activation.require(&event).map_err(js_error)?;
        let id = serde_json::from_value(serde_json::Value::String(id))
            .map_err(|_| crate::err("Invalid browser action identity."))?;
        let revision = revision
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| crate::err("Invalid browser action revision."))?;
        let key = Key { id, revision };
        let state = State::read(&self.core);
        if !state
            .actions
            .iter()
            .any(|row| key.offered(row) && state.enabled(row, approve))
        {
            return Err(crate::err("This browser action is no longer available."));
        }
        *self.core.browser_operation.borrow_mut() = Operation::Sending(key.clone());
        self.core.notify_browser_actions();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let result = core
                .flow()
                .decide_browser_action(core.scope.clone(), key.id.clone(), key.revision, approve)
                .await;
            let result = match result {
                Ok(view) => match core.accept(view) {
                    Ok(_) => {
                        *core.browser_operation.borrow_mut() = Operation::Waiting(key);
                        Ok(())
                    }
                    Err(error) => {
                        *core.browser_operation.borrow_mut() = Operation::Uncertain(key);
                        Err(error)
                    }
                },
                Err(error) => {
                    *core.browser_operation.borrow_mut() = Operation::Uncertain(key);
                    Err(error)
                }
            };
            core.notify_browser_actions();
            result.map(|()| JsValue::UNDEFINED).map_err(js_error)
        }))
    }
}

impl Core {
    pub(super) fn notify_browser_actions(&self) {
        self.page.notify_connection();
        {
            let view = self.page.view.borrow();
            if view.selection == self.scope {
                self.browser_operation
                    .borrow_mut()
                    .observe(&view.browser_actions);
            }
        }
        let observers: Vec<_> = {
            let mut list = self.browser_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed()));
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
    use crate::model::BrowserActionIntent;

    #[test]
    fn stale_document_can_be_declined_and_observation_cannot_clear_unknown_decisions() {
        let id = serde_json::from_value(serde_json::json!("0123456789abcdef0123456789abcdef:2"))
            .unwrap();
        let run = serde_json::from_value(serde_json::json!("0123456789abcdef0123456789abcdef:1"))
            .unwrap();
        let mut action = BrowserActionView {
            id,
            revision: 1,
            run,
            tab: 1,
            document: "document".into(),
            url: "https://example.test/".into(),
            intent: BrowserActionIntent::Field {
                field_id: "field".into(),
                field_label: "Name".into(),
                previous_value: "".into(),
                value: "Alex".into(),
            },
            phase: BrowserActionPhase::Pending,
            outcome: None,
            reason: None,
            observed_selection: None,
            observed_scroll: None,
            observed_control: None,
            target_current: false,
        };
        let key = Key {
            id: action.id.clone(),
            revision: action.revision,
        };
        let mut state = State {
            connected: true,
            stale: false,
            ready: true,
            recovery_required: false,
            active: Some(action.run.clone()),
            actions: vec![action.clone()],
            approvable: vec![],
            epoch: 1,
            operation: Operation::Idle,
        };
        assert!(state.enabled(&action, false));
        assert!(!state.enabled(&action, true));
        state.approvable.push(action.id.clone());
        assert!(state.enabled(&action, true));
        state.operation = Operation::Uncertain(key.clone());
        assert!(!state.enabled(&action, false));
        assert!(!state.enabled(&action, true));
        action.revision = 2;
        action.phase = BrowserActionPhase::Approved;
        let mut waiting = Operation::Waiting(key.clone());
        waiting.observe(std::slice::from_ref(&action));
        assert!(matches!(waiting, Operation::Idle));
        let mut uncertain = Operation::Uncertain(key.clone());
        uncertain.observe(std::slice::from_ref(&action));
        uncertain.observe(&[]);
        assert!(matches!(uncertain, Operation::Uncertain(_)));
        let mut sending = Operation::Sending(key);
        sending.observe(std::slice::from_ref(&action));
        sending.observe(&[]);
        assert!(matches!(sending, Operation::Sending(_)));
    }
}
