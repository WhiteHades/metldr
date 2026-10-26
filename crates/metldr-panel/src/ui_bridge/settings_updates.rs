use super::settings::Family;
use super::*;
use crate::model::{AgentSettingsView, ProviderSettingsView};
use wasm_bindgen_futures::spawn_local;

const MAX_SUBSCRIPTIONS: usize = 8;

#[derive(Clone, PartialEq, Eq)]
struct Gate {
    connected: bool,
    busy: bool,
    can_refresh: bool,
    uncertain: bool,
    invalidated: bool,
}

impl Gate {
    fn read(page: &PageSession, family: Family) -> Self {
        let connected = !page.invalidated.get() && page.flow.borrow().is_connected();
        let ready = page.controls_ready();
        let state = page.settings.borrow();
        Self {
            connected,
            busy: !connected || !ready || page.settings_blocked(),
            can_refresh: ready && state.can_refresh(family),
            uncertain: state.uncertain(family),
            invalidated: page.invalidated.get(),
        }
    }
}

#[derive(Clone)]
struct State {
    gate: Gate,
    providers: ProviderSettingsView,
    agents: AgentSettingsView,
    flow: FlowClient,
}

impl State {
    fn matches(&self, page: &PageSession, family: Family) -> bool {
        self.flow.same_connection(&page.flow.borrow())
            && self.gate == Gate::read(page, family)
            && self.providers == page.view.borrow().provider_settings
            && self.agents == page.view.borrow().agent_settings
    }

    fn read(page: &PageSession, family: Family) -> Self {
        Self {
            gate: Gate::read(page, family),
            providers: page.view.borrow().provider_settings.clone(),
            agents: page.view.borrow().agent_settings.clone(),
            flow: page.flow.borrow().clone(),
        }
    }
}

#[wasm_bindgen]
pub struct UiAgentSnapshot {
    page: Rc<PageSession>,
    state: State,
}

#[wasm_bindgen]
impl UiAgentSnapshot {
    #[wasm_bindgen(getter)]
    pub fn connected(&self) -> bool {
        self.state.gate.connected
    }
    #[wasm_bindgen(getter)]
    pub fn busy(&self) -> bool {
        self.state.gate.busy
    }
    #[wasm_bindgen(getter)]
    pub fn can_refresh(&self) -> bool {
        self.state.gate.can_refresh
    }
    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.state.gate.uncertain
    }
    #[wasm_bindgen(getter)]
    pub fn notice(&self) -> Option<String> {
        self.state
            .gate
            .invalidated
            .then(|| "Saved data changed. Reload the panel.".into())
    }
    pub fn configurations(&self) -> Vec<UiAgentConfiguration> {
        self.state
            .agents
            .agents
            .iter()
            .map(|row| UiAgentConfiguration::new(Rc::clone(&self.page), row.clone()))
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiProviderSnapshot {
    page: Rc<PageSession>,
    state: State,
    reset_keys: bool,
}

#[wasm_bindgen]
impl UiProviderSnapshot {
    #[wasm_bindgen(getter)]
    pub fn reset_keys(&self) -> bool {
        self.reset_keys
    }
    #[wasm_bindgen(getter)]
    pub fn connected(&self) -> bool {
        self.state.gate.connected
    }
    #[wasm_bindgen(getter)]
    pub fn busy(&self) -> bool {
        self.state.gate.busy
    }
    #[wasm_bindgen(getter)]
    pub fn can_refresh(&self) -> bool {
        self.state.gate.can_refresh
    }
    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.state.gate.uncertain
    }
    #[wasm_bindgen(getter)]
    pub fn cleanup_pending(&self) -> bool {
        self.state.providers.cleanup_pending
    }
    #[wasm_bindgen(getter)]
    pub fn notice(&self) -> Option<String> {
        if self.state.gate.invalidated {
            Some("Saved data changed. Reload the panel.".into())
        } else {
            self.state.providers.notice.clone()
        }
    }

    pub fn connections(&self) -> Vec<UiProviderConnection> {
        self.state
            .providers
            .connections
            .iter()
            .map(|row| UiProviderConnection::new(Rc::clone(&self.page), row.clone()))
            .collect()
    }
}

enum Lifecycle {
    Open(Option<Waker>),
    Closed,
}

enum Next<T> {
    Value(T),
    Replaced,
    Closed,
}

pub(super) struct Observer {
    page: Rc<PageSession>,
    family: Family,
    callback: Function,
    lifecycle: RefCell<Lifecycle>,
    last: RefCell<Option<State>>,
}

#[wasm_bindgen]
pub struct UiSettingsSubscription {
    observer: Rc<Observer>,
}

#[wasm_bindgen]
impl UiSettingsSubscription {
    pub fn close(&self) {
        self.observer.close();
    }
}

impl Drop for UiSettingsSubscription {
    fn drop(&mut self) {
        self.observer.close();
    }
}

#[wasm_bindgen]
impl UiProviderSettings {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiProviderSnapshot) => void")]
        callback: Function,
    ) -> Result<UiSettingsSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.page), Family::Providers, callback)
    }
}

#[wasm_bindgen]
impl UiAgentSettings {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiAgentSnapshot) => void")]
        callback: Function,
    ) -> Result<UiSettingsSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.page), Family::Agents, callback)
    }
}

impl Observer {
    fn subscribe(
        page: Rc<PageSession>,
        family: Family,
        callback: Function,
    ) -> Result<UiSettingsSubscription, JsValue> {
        let observer = Rc::new(Observer {
            page: Rc::clone(&page),
            family,
            callback,
            lifecycle: RefCell::new(Lifecycle::Open(None)),
            last: RefCell::new(None),
        });
        {
            let mut subscriptions = page.settings_subscriptions.borrow_mut();
            subscriptions.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed()));
            if subscriptions.len() >= MAX_SUBSCRIPTIONS {
                return Err(crate::err("Settings listener capacity reached."));
            }
            subscriptions.push(Rc::downgrade(&observer));
        }
        observer.emit().map_err(js_error)?;
        let watch = Rc::clone(&observer);
        spawn_local(async move {
            watch.watch().await;
        });
        Ok(UiSettingsSubscription { observer })
    }
}

impl PageSession {
    pub(super) fn notify_settings(&self) {
        self.notify_connection();
        let observers: Vec<_> = {
            let mut subscriptions = self.settings_subscriptions.borrow_mut();
            subscriptions.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed()));
            subscriptions
                .iter()
                .filter_map(std::rc::Weak::upgrade)
                .collect()
        };
        for observer in observers {
            if observer.emit().is_err() {
                observer.close();
            }
            observer.wake();
        }
        let current = self.current.borrow().upgrade();
        if let Some(core) = current {
            core.notify_native_permissions();
            core.notify_browser_actions();
            core.notify_pi_ui();
            core.notify_codex_input();
        }
    }
}

impl Observer {
    fn closed(&self) -> bool {
        matches!(*self.lifecycle.borrow(), Lifecycle::Closed)
    }

    fn wake(&self) {
        let wake = match &mut *self.lifecycle.borrow_mut() {
            Lifecycle::Open(wake) => wake.take(),
            Lifecycle::Closed => None,
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    fn close(&self) {
        let old = std::mem::replace(&mut *self.lifecycle.borrow_mut(), Lifecycle::Closed);
        if let Lifecycle::Open(Some(wake)) = old {
            wake.wake();
        }
    }

    fn emit(&self) -> Result<(), FlowError> {
        if self.closed()
            || self
                .last
                .borrow()
                .as_ref()
                .is_some_and(|state| state.matches(&self.page, self.family))
        {
            return Ok(());
        }
        let state = State::read(&self.page, self.family);
        let reset_keys = !state.gate.connected
            || self
                .last
                .borrow()
                .as_ref()
                .is_none_or(|previous| !state.flow.same_connection(&previous.flow));
        self.last.replace(Some(state.clone()));
        let snapshot = match self.family {
            Family::Providers => JsValue::from(UiProviderSnapshot {
                page: Rc::clone(&self.page),
                state,
                reset_keys,
            }),
            Family::Agents => JsValue::from(UiAgentSnapshot {
                page: Rc::clone(&self.page),
                state,
            }),
        };
        self.callback
            .call1(&JsValue::UNDEFINED, &snapshot)
            .map_err(|_| FlowError::Host("The settings listener disconnected.".into()))?;
        Ok(())
    }

    fn relevant(&self, view: &PageView) -> bool {
        let current = self.page.view.borrow();
        view.selection != current.selection
            || view.provider_settings != current.provider_settings
            || view.agent_settings != current.agent_settings
            || self
                .last
                .borrow()
                .as_ref()
                .is_none_or(|state| state.gate != Gate::read(&self.page, self.family))
    }

    async fn wait<F: Future>(&self, flow: &FlowClient, future: F) -> Next<F::Output> {
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(|context| {
            let mut state = self.lifecycle.borrow_mut();
            match &mut *state {
                Lifecycle::Closed => return Poll::Ready(Next::Closed),
                Lifecycle::Open(wake) => *wake = Some(context.waker().clone()),
            }
            drop(state);
            if !self.page.flow.borrow().same_connection(flow) {
                return Poll::Ready(Next::Replaced);
            }
            future.as_mut().poll(context).map(Next::Value)
        })
        .await
    }

    async fn wait_replacement(&self, flow: &FlowClient) -> bool {
        matches!(
            self.wait(flow, std::future::pending::<()>()).await,
            Next::Replaced
        )
    }

    async fn watch(self: Rc<Self>) {
        while !self.closed() {
            let flow = self.page.flow.borrow().clone();
            let mut views = match flow.observe() {
                Ok(views) => views,
                Err(_) => {
                    self.page.notify_settings();
                    if !self.wait_replacement(&flow).await {
                        break;
                    }
                    continue;
                }
            };
            loop {
                match self
                    .wait(&flow, views.changed_when(|view| self.relevant(view)))
                    .await
                {
                    Next::Closed => return,
                    Next::Replaced => break,
                    Next::Value(Ok(Some(view))) => {
                        if self.page.accept(view).is_err() {
                            if !self.wait_replacement(&flow).await {
                                return;
                            }
                            break;
                        }
                    }
                    Next::Value(Ok(None)) => {
                        self.page.notify_settings();
                        if !self.wait_replacement(&flow).await {
                            return;
                        }
                        break;
                    }
                    Next::Value(Err(_)) => {
                        self.page.notify_settings();
                        if flow.is_closed() {
                            if !self.wait_replacement(&flow).await {
                                return;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}
