use super::*;
use crate::model::{NativePermissionOption, NativePermissionView};

mod observe;
pub(super) use observe::Observer;
pub use observe::UiNativePermissionSubscription;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    run: RunId,
    request: RunId,
}

impl Key {
    fn offered(&self, permission: &NativePermissionView) -> bool {
        permission.run == self.run && permission.request == self.request
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
    fn observe(&mut self, permissions: &[NativePermissionView]) {
        if let Self::Waiting(key) = self
            && !permissions.iter().any(|row| key.offered(row))
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
    requests: Vec<NativePermissionView>,
    operation: Operation,
}

impl State {
    fn read(core: &Core) -> Self {
        let view = core.page.view.borrow();
        let stale = core.page.invalidated.get()
            || view.selection != core.scope
            || !core
                .page
                .current
                .borrow()
                .upgrade()
                .is_some_and(|item| std::ptr::eq(item.as_ref(), core));
        let connected = core.flow().is_connected();
        let ready = core.owner.borrow().readable().is_ok()
            && !core.page.recovery.borrow().active()
            && !matches!(*core.owner.borrow(), Owner::Uncertain);
        let recovery_required = matches!(*core.owner.borrow(), Owner::Uncertain);
        let active = view
            .runs
            .iter()
            .find(|run| !run.status.is_terminal())
            .map(|run| run.id.clone());
        Self {
            connected,
            stale,
            ready,
            recovery_required,
            active,
            requests: view.native_permissions.clone(),
            operation: core.native_operation.borrow().clone(),
        }
    }

    fn enabled(&self, permission: &NativePermissionView) -> bool {
        self.connected
            && !self.stale
            && self.ready
            && matches!(self.operation, Operation::Idle)
            && self.active.as_ref() == Some(&permission.run)
    }
}

#[wasm_bindgen]
pub struct UiNativePermissions {
    core: Rc<Core>,
    activation: super::activation::Activation,
}

#[wasm_bindgen]
pub struct UiNativePermissionSnapshot {
    state: State,
}

#[wasm_bindgen]
impl UiNativePermissionSnapshot {
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
            return Some("The conversation changed. Reload to check its requests.".into());
        }
        if !self.state.connected {
            return Some("Connection lost. Reload to check the pending request.".into());
        }
        if self.state.recovery_required {
            return Some("Reload to check saved work before replying.".into());
        }
        if !self.state.ready {
            return Some("Opening conversation.".into());
        }
        match self.state.operation {
            Operation::Idle => None,
            Operation::Sending(_) => Some("Sending your decision.".into()),
            Operation::Waiting(_) => {
                Some("Decision received. Waiting for the request to close.".into())
            }
            Operation::Uncertain(_) => Some(
                "No decision receipt arrived. Reload to check the request before replying again."
                    .into(),
            ),
        }
    }
    pub fn requests(&self) -> Vec<UiNativePermission> {
        self.state
            .requests
            .iter()
            .map(|item| UiNativePermission {
                permission: item.clone(),
                enabled: self.state.enabled(item),
            })
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiNativePermission {
    permission: NativePermissionView,
    enabled: bool,
}

#[wasm_bindgen]
impl UiNativePermission {
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.permission.run.as_str().into()
    }
    #[wasm_bindgen(getter)]
    pub fn request_id(&self) -> String {
        self.permission.request.as_str().into()
    }
    #[wasm_bindgen(getter)]
    pub fn agent(&self) -> String {
        self.permission.agent.label().into()
    }
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.permission.title.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn tool(&self) -> String {
        self.permission.tool.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn input(&self) -> String {
        self.permission.input.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn options(&self) -> Vec<UiNativePermissionOption> {
        self.permission
            .options
            .iter()
            .cloned()
            .map(|option| UiNativePermissionOption { option })
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiNativePermissionOption {
    option: NativePermissionOption,
}

#[wasm_bindgen]
impl UiNativePermissionOption {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.option.id.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.option.name.clone()
    }
}

#[wasm_bindgen]
impl UiBridge {
    pub fn native_permissions(&self) -> Result<UiNativePermissions, JsValue> {
        Ok(UiNativePermissions {
            core: Rc::clone(&self.core),
            activation: super::activation::Activation::new().map_err(js_error)?,
        })
    }
}

#[wasm_bindgen]
impl UiNativePermissions {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiNativePermissionSnapshot) => void")]
        callback: Function,
    ) -> Result<UiNativePermissionSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.core), callback)
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn reply(
        &self,
        run: String,
        request: String,
        option: Option<String>,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.activation.require(&event).map_err(js_error)?;
        let parse = |id| {
            serde_json::from_value(serde_json::Value::String(id))
                .map_err(|_| crate::err("Invalid native approval identity."))
        };
        let key = Key {
            run: parse(run)?,
            request: parse(request)?,
        };
        let state = State::read(&self.core);
        let offered = state.requests.iter().find(|row| key.offered(row));
        if !offered.is_some_and(|row| {
            state.enabled(row)
                && option
                    .as_ref()
                    .is_none_or(|selected| row.options.iter().any(|item| &item.id == selected))
        }) {
            return Err(crate::err("This native permission is no longer available."));
        }
        *self.core.native_operation.borrow_mut() = Operation::Sending(key.clone());
        self.core.notify_native_permissions();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let result = core
                .flow()
                .native_permission(
                    core.scope.clone(),
                    key.run.clone(),
                    key.request.clone(),
                    option,
                )
                .await;
            let result = match result {
                Ok(view) => match core.accept(view) {
                    Ok(_) => {
                        *core.native_operation.borrow_mut() = Operation::Waiting(key);
                        Ok(())
                    }
                    Err(error) => {
                        *core.native_operation.borrow_mut() = Operation::Uncertain(key);
                        Err(error)
                    }
                },
                Err(error) => {
                    *core.native_operation.borrow_mut() = Operation::Uncertain(key);
                    Err(error)
                }
            };
            core.notify_native_permissions();
            result.map(|()| JsValue::UNDEFINED).map_err(js_error)
        }))
    }
}

impl Core {
    pub(super) fn notify_native_permissions(&self) {
        self.page.notify_connection();
        {
            let view = self.page.view.borrow();
            let mut operation = self.native_operation.borrow_mut();
            if view.selection == self.scope {
                operation.observe(&view.native_permissions);
            }
        }
        let observers: Vec<_> = {
            let mut list = self.native_subscriptions.borrow_mut();
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

    #[test]
    fn observed_removal_clears_a_receipt_but_never_unknown_or_inflight_decisions() {
        let id = |suffix| {
            serde_json::from_value(serde_json::json!(format!(
                "0123456789abcdef0123456789abcdef:{suffix}"
            )))
            .unwrap()
        };
        let key = Key {
            run: id(1),
            request: id(2),
        };
        let mut waiting = Operation::Waiting(key.clone());
        waiting.observe(&[]);
        assert!(matches!(waiting, Operation::Idle));
        let mut uncertain = Operation::Uncertain(key.clone());
        uncertain.observe(&[]);
        assert!(matches!(uncertain, Operation::Uncertain(_)));
        let mut sending = Operation::Sending(key);
        sending.observe(&[]);
        assert!(matches!(sending, Operation::Sending(_)));
    }
}
