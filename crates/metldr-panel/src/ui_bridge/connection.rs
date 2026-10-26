use super::*;

mod observe;
pub(super) use observe::Observer;
pub use observe::UiConnectionSubscription;

#[derive(Default)]
pub(super) struct Slot(Option<Rc<()>>);

impl Slot {
    fn reserve(&mut self) -> Result<Rc<()>, FlowError> {
        if self.active() {
            return Err(FlowError::HistoryBusy);
        }
        let token = Rc::new(());
        self.0 = Some(Rc::clone(&token));
        Ok(token)
    }

    pub(super) fn active(&self) -> bool {
        self.0.is_some()
    }

    fn owns(&self, token: &Rc<()>) -> bool {
        self.0
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, token))
    }

    fn release(&mut self, token: &Rc<()>) -> bool {
        if !self.owns(token) {
            return false;
        }
        self.0 = None;
        true
    }
}

pub(super) struct Recovery {
    page: Rc<PageSession>,
    core: std::rc::Weak<Core>,
    flow: FlowClient,
    scope: Scope,
    token: Rc<()>,
}

impl Recovery {
    pub(super) fn begin(page: Rc<PageSession>) -> Result<Self, FlowError> {
        if page.invalidated.get() || page.current.borrow().upgrade().is_none() {
            return Err(FlowError::HistoryStale);
        }
        let token = page.recovery.borrow_mut().reserve()?;
        let work = Self {
            core: page.current.borrow().clone(),
            flow: page.flow.borrow().clone(),
            scope: page.view.borrow().selection.clone(),
            token,
            page: Rc::clone(&page),
        };
        page.notify_settings();
        Ok(work)
    }

    fn validate(&self) -> Result<(), FlowError> {
        if !self.page.recovery.borrow().owns(&self.token)
            || self.page.invalidated.get()
            || self.core.upgrade().is_none()
            || !self.core.ptr_eq(&self.page.current.borrow())
            || !self.flow.same_connection(&self.page.flow.borrow())
        {
            return Err(FlowError::HistoryStale);
        }
        Ok(())
    }

    pub(super) async fn connect(&self) -> Result<FlowClient, FlowError> {
        self.validate()?;
        let flow = self.flow.reconnect().await?;
        self.validate()?;
        Ok(flow)
    }

    fn install(&self, flow: FlowClient, view: PageView) -> Result<PageView, FlowError> {
        self.validate()?;
        if !flow.is_connected() {
            return Err(FlowError::Disconnected);
        }
        let views = flow.observe()?;
        {
            let mut current = self.page.view.borrow_mut();
            if view.store != current.store || view.is_older_than(&current) {
                return Err(FlowError::HistoryStale);
            }
            *current = view.clone();
            self.page.flow.replace(flow.clone());
        }
        self.page.install_source_updates();
        self.page.install_connection_updates(flow, views);
        self.page.notify_settings();
        Ok(view)
    }

    pub(super) async fn exact(&self) -> Result<PageView, FlowError> {
        self.validate()?;
        if self.page.view.borrow().selection != self.scope {
            return Err(FlowError::HistoryStale);
        }
        let flow = self.connect().await?;
        let view = flow.current_page().await?;
        if view.selection != self.scope || self.page.view.borrow().selection != self.scope {
            return Err(FlowError::HistoryStale);
        }
        self.install(flow, view)
    }

    pub(super) fn navigation(
        &self,
        flow: FlowClient,
        view: PageView,
        expected: &crate::history::Context,
    ) -> Result<PageView, FlowError> {
        let selected = self.page.view.borrow().selection.clone();
        if view.selection != expected.scope
            || (selected != self.scope && selected != view.selection)
        {
            return Err(FlowError::HistoryStale);
        }
        self.install(flow, view)
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        let released = self.page.recovery.borrow_mut().release(&self.token);
        if released {
            self.page.notify_settings();
        }
    }
}

#[wasm_bindgen]
pub struct UiConnection {
    core: Rc<Core>,
}

#[wasm_bindgen]
#[derive(Clone, PartialEq, Eq)]
pub struct UiConnectionSnapshot {
    connected: bool,
    reconnecting: bool,
    can_reconnect: bool,
    stale: bool,
    uncertain: bool,
}

impl UiConnectionSnapshot {
    fn read(core: &Core) -> Self {
        let stale = core.ensure_current().is_err();
        let reconnecting = core.page.recovery.borrow().active();
        let connected = !stale && core.flow().is_connected();
        let available = matches!(
            *core.owner.borrow(),
            Owner::Idle | Owner::Running { .. } | Owner::Observing(_) | Owner::Uncertain
        ) && matches!(
            *core.page.settings.borrow(),
            settings::Operation::Idle | settings::Operation::Unknown(_)
        ) && !matches!(
            *core.native_operation.borrow(),
            native_permissions::Operation::Sending(_)
        ) && !matches!(
            *core.browser_operation.borrow(),
            browser_actions::Operation::Sending(_)
        ) && !matches!(*core.pi_operation.borrow(), pi_ui::Operation::Sending(_))
            && !matches!(
                *core.codex_operation.borrow(),
                codex_input::Operation::Sending(_)
            );
        Self {
            connected,
            reconnecting,
            can_reconnect: !stale && !reconnecting && available,
            stale,
            uncertain: matches!(*core.owner.borrow(), Owner::Uncertain),
        }
    }
}

#[wasm_bindgen]
impl UiConnectionSnapshot {
    #[wasm_bindgen(getter)]
    pub fn connected(&self) -> bool {
        self.connected
    }
    #[wasm_bindgen(getter)]
    pub fn reconnecting(&self) -> bool {
        self.reconnecting
    }
    #[wasm_bindgen(getter)]
    pub fn can_reconnect(&self) -> bool {
        self.can_reconnect
    }
    #[wasm_bindgen(getter)]
    pub fn stale(&self) -> bool {
        self.stale
    }
    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.uncertain
    }
}

#[wasm_bindgen]
impl UiBridge {
    pub fn connection(&self) -> UiConnection {
        UiConnection {
            core: Rc::clone(&self.core),
        }
    }
}

#[wasm_bindgen]
impl UiConnection {
    pub fn snapshot(&self) -> UiConnectionSnapshot {
        UiConnectionSnapshot::read(&self.core)
    }

    pub fn reconnect(&self) -> Result<Promise, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        if self.core.flow().is_connected() {
            return Ok(Promise::resolve(&JsValue::UNDEFINED));
        }
        if !self.snapshot().can_reconnect {
            return Err(js_error(FlowError::HistoryBusy));
        }
        let recovery = Recovery::begin(Rc::clone(&self.core.page)).map_err(js_error)?;
        Ok(future_to_promise(async move {
            recovery
                .exact()
                .await
                .map(|_| JsValue::UNDEFINED)
                .map_err(js_error)
        }))
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn recovery_reservation_rejects_overlap_and_stale_release() {
        let mut slot = Slot::default();
        let first = slot.reserve().unwrap();
        assert!(slot.reserve().is_err());
        assert!(slot.release(&first));
        let second = slot.reserve().unwrap();
        assert!(!slot.release(&first));
        assert!(slot.owns(&second));
        assert!(slot.active());
        assert!(slot.release(&second));
        assert!(!slot.active());
    }
}
