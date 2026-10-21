use std::{
    cell::{Cell, RefCell},
    future::{Future, poll_fn},
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

use js_sys::{Array, Function, Object, Reflect};
use serde_json::{Value, json};
use wasm_bindgen::{JsValue, closure::Closure};

use crate::model::{
    AgentKind, AgentProfile, BrowserActionId, FlowError, PageView, ProviderProtocol, RunId,
    SourceId, SourceStatus, UserText,
};
use crate::{
    MAX_FRAME, MAX_PAGE, PANEL_PORT, await_call, call, chrome_api, get, js_message, promise,
    string_property,
};

mod capture;
mod protocol;
pub(crate) use protocol::StopReceipt;
use protocol::{Protocol, Reply, ReplyCell, ReplyKind, WatchCell};
type Result<T> = std::result::Result<T, FlowError>;

pub(crate) enum PageTextPrompt {
    Summary,
    Answer(UserText),
}

pub(crate) fn is_snapshot_frame(value: JsValue, kind: &str) -> bool {
    decode_frame(value).is_ok_and(|value| protocol::validate_snapshot_frame(&value, kind).is_ok())
}

thread_local! { static CAPTURE: RefCell<Option<Function>> = const { RefCell::new(None) }; }
pub(super) fn install_capture(capture: Function) {
    CAPTURE.with(|slot| *slot.borrow_mut() = Some(capture));
}

#[derive(Clone)]
pub(crate) struct FlowClient {
    inner: Rc<Connection>,
}

struct Connection {
    port: JsValue,
    capture: Function,
    protocol: Rc<RefCell<Protocol>>,
    nonce: String,
    next: Cell<u64>,
    epoch: Cell<u64>,
    selection_epoch: Cell<u64>,
    selected: RefCell<Option<capture::Selected>>,
    source_changed: RefCell<Option<Box<dyn Fn()>>>,
    listeners: RefCell<Vec<Listener>>,
}

struct Listener {
    event: JsValue,
    callback: Closure<dyn FnMut(JsValue)>,
}
impl Listener {
    fn new(target: &JsValue, event: &str, callback: impl FnMut(JsValue) + 'static) -> Result<Self> {
        let event = get(target, event).map_err(js_error)?;
        let callback = Closure::<dyn FnMut(JsValue)>::new(callback);
        call(&event, "addListener", &[callback.as_ref().clone()]).map_err(js_error)?;
        Ok(Self { event, callback })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = call(
            &self.event,
            "removeListener",
            &[self.callback.as_ref().clone()],
        );
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.listeners.get_mut().clear();
        let wakes = self
            .protocol
            .borrow_mut()
            .disconnect(FlowError::Disconnected, true);
        wake(wakes);
        let _ = call(&self.port, "disconnect", &[]);
    }
}

impl FlowClient {
    pub(crate) async fn connect() -> Result<Self> {
        let capture = CAPTURE
            .with(|slot| slot.borrow().clone())
            .ok_or_else(|| FlowError::Protocol("Page capture bridge is unavailable.".into()))?;
        let crypto = get(&js_sys::global(), "crypto").map_err(js_error)?;
        let nonce = call(&crypto, "randomUUID", &[])
            .map_err(js_error)?
            .as_string()
            .ok_or_else(|| FlowError::Protocol("Request identity is unavailable.".into()))?;
        let options = Object::new();
        Reflect::set(&options, &"name".into(), &PANEL_PORT.into()).map_err(js_error)?;
        let port = call(
            &chrome_api("runtime").map_err(js_error)?,
            "connect",
            &[options.into()],
        )
        .map_err(js_error)?;
        let inner = Rc::new(Connection {
            port,
            capture,
            protocol: Rc::new(RefCell::new(Protocol::new())),
            nonce,
            next: Cell::new(0),
            epoch: Cell::new(0),
            selection_epoch: Cell::new(0),
            selected: RefCell::new(None),
            source_changed: RefCell::new(None),
            listeners: RefCell::new(vec![]),
        });
        let weak = Rc::downgrade(&inner);
        let listener = Listener::new(&inner.port, "onMessage", move |message| {
            if let Some(inner) = weak.upgrade() {
                let result = decode_frame(message)
                    .and_then(|frame| inner.protocol.borrow_mut().receive(frame));
                let wakes = match result {
                    Ok(wakes) => wakes,
                    Err(error) => inner.protocol.borrow_mut().disconnect(error, false),
                };
                wake(wakes);
            }
        })?;
        inner.listeners.borrow_mut().push(listener);
        let weak = Rc::downgrade(&inner);
        let listener = Listener::new(&inner.port, "onDisconnect", move |_| {
            if let Some(inner) = weak.upgrade() {
                let wakes = inner
                    .protocol
                    .borrow_mut()
                    .disconnect(FlowError::Disconnected, true);
                wake(wakes);
            }
        })?;
        inner.listeners.borrow_mut().push(listener);
        for (namespace, event) in [
            ("tabs", "onActivated"),
            ("tabs", "onUpdated"),
            ("windows", "onFocusChanged"),
            ("permissions", "onAdded"),
            ("permissions", "onRemoved"),
        ] {
            let weak = Rc::downgrade(&inner);
            let listener = Listener::new(
                &chrome_api(namespace).map_err(js_error)?,
                event,
                move |details| {
                    if let Some(inner) = weak.upgrade() {
                        if capture::affects_selected(&inner, namespace, event, &details) {
                            capture::refresh(&inner);
                        }
                        inner.epoch.set(inner.epoch.get().wrapping_add(1));
                        if let Some(callback) = inner.source_changed.borrow().as_ref() {
                            callback();
                        }
                    }
                },
            )?;
            inner.listeners.borrow_mut().push(listener);
        }
        capture::refresh(&inner);
        Ok(Self { inner })
    }

    pub(crate) async fn current_page(&self) -> Result<PageView> {
        match self.request(json!({"op":"view"}), ReplyKind::Page).await? {
            Reply::Page(view) => Ok(self.check_source(view).await),
            _ => Err(FlowError::Protocol("Expected a saved view.".into())),
        }
    }

    pub(crate) async fn provider_save(
        &self,
        protocol: ProviderProtocol,
        expected_revision: u64,
        endpoint: String,
        model: String,
        key: Option<String>,
    ) -> Result<PageView> {
        if !crate::model::valid_https_endpoint(&endpoint)
            || !crate::model::valid_ascii_label(&model, 128)
            || key.as_ref().is_some_and(|value| {
                value.is_empty()
                    || value.len() > 4_096
                    || !value.bytes().all(|byte| byte.is_ascii_graphic())
            })
        {
            return Err(FlowError::InvalidInput(
                "Provider settings are invalid or exceed their limits.".into(),
            ));
        }
        let key = key.map_or_else(
            || json!({"kind":"keep"}),
            |value| json!({"kind":"replace","value":value}),
        );
        match self.request(json!({"op":"provider_save","protocol":protocol.name(),"expected_revision":expected_revision,"endpoint":endpoint,"model":model,"key":key}), ReplyKind::Page).await? {
            Reply::Page(view) => Ok(view),
            _ => Err(FlowError::Protocol("Expected the updated provider settings view.".into())),
        }
    }

    pub(crate) async fn provider_remove(
        &self,
        protocol: ProviderProtocol,
        expected_revision: u64,
    ) -> Result<PageView> {
        match self.request(json!({"op":"provider_remove","protocol":protocol.name(),"expected_revision":expected_revision}), ReplyKind::Page).await? {
            Reply::Page(view) => Ok(view),
            _ => Err(FlowError::Protocol("Expected the updated provider settings view.".into())),
        }
    }

    pub(crate) async fn provider_refresh(&self) -> Result<PageView> {
        match self
            .request(json!({"op":"provider_refresh"}), ReplyKind::Page)
            .await?
        {
            Reply::Page(view) => Ok(view),
            _ => Err(FlowError::Protocol(
                "Expected the refreshed provider settings view.".into(),
            )),
        }
    }

    pub(crate) async fn agent_save(
        &self,
        expected_revision: u64,
        profile: AgentProfile,
    ) -> Result<PageView> {
        match self
            .request(
                json!({"op":"agent_save","expected_revision":expected_revision,"profile":profile}),
                ReplyKind::Page,
            )
            .await?
        {
            Reply::Page(view) => Ok(view),
            _ => Err(FlowError::Protocol(
                "Expected the updated agent settings view.".into(),
            )),
        }
    }

    pub(crate) async fn agent_remove(
        &self,
        agent: AgentKind,
        expected_revision: u64,
    ) -> Result<PageView> {
        match self.request(json!({"op":"agent_remove","agent":agent.name(),"expected_revision":expected_revision}), ReplyKind::Page).await? {
            Reply::Page(view) => Ok(view),
            _ => Err(FlowError::Protocol("Expected the updated agent settings view.".into())),
        }
    }

    pub(crate) async fn agent_refresh(&self) -> Result<PageView> {
        match self
            .request(json!({"op":"agent_refresh"}), ReplyKind::Page)
            .await?
        {
            Reply::Page(view) => Ok(view),
            _ => Err(FlowError::Protocol(
                "Expected the refreshed agent settings view.".into(),
            )),
        }
    }

    // Start the permission request synchronously while the button still has user activation.
    pub(crate) fn summarize_selected(&self) -> impl Future<Output = Result<RunId>> + use<> {
        let page = capture::selected(self);
        let flow = self.clone();
        async move {
            let page = page.await?;
            flow.start(json!({"op":"summarize","page":page})).await
        }
    }

    pub(crate) fn capture_selected(&self) -> impl Future<Output = Result<PageView>> + use<> {
        let expected = self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .map(|view| (view.store.clone(), view.source().cloned()));
        let page = capture::selected(self);
        let flow = self.clone();
        async move {
            let (store, source) = expected.ok_or(FlowError::NoSource)?;
            let page = page.await?;
            match flow.request(json!({"op":"capture","expected_store":store,"expected_source":source,"page":page}), ReplyKind::Page).await? {
                Reply::Page(view) => Ok(flow.check_source(view).await),
                _ => Err(FlowError::Protocol("Expected the saved page receipt.".into())),
            }
        }
    }

    pub(crate) async fn ask(&self, source: SourceId, text: UserText) -> Result<RunId> {
        if self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .and_then(PageView::source)
            != Some(&source)
        {
            return Err(FlowError::NoSource);
        }
        self.start(json!({"op":"ask","source":source,"text":text.as_str()}))
            .await
    }

    pub(crate) async fn browser_task(
        &self,
        source: SourceId,
        text: UserText,
        fingerprint: Option<String>,
    ) -> Result<RunId> {
        self.start(json!({"op":"browser_task","source":source,"text":text.as_str(),"fingerprint":fingerprint}))
            .await
    }

    pub(crate) async fn responses_task(
        &self,
        source: SourceId,
        text: UserText,
        fingerprint: String,
    ) -> Result<RunId> {
        self.start(json!({"op":"responses_task","source":source,"text":text.as_str(),"fingerprint":fingerprint}))
            .await
    }

    pub(crate) async fn chat_task(
        &self,
        source: SourceId,
        text: UserText,
        fingerprint: String,
    ) -> Result<RunId> {
        self.start(json!({"op":"chat_task","source":source,"text":text.as_str(),"fingerprint":fingerprint}))
            .await
    }

    pub(crate) async fn google_task(
        &self,
        source: SourceId,
        text: UserText,
        fingerprint: String,
    ) -> Result<RunId> {
        self.start(json!({"op":"google_task","source":source,"text":text.as_str(),"fingerprint":fingerprint}))
            .await
    }

    pub(crate) async fn page_text(
        &self,
        source: SourceId,
        protocol: ProviderProtocol,
        fingerprint: String,
        prompt: PageTextPrompt,
    ) -> Result<RunId> {
        if self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .and_then(PageView::source)
            != Some(&source)
        {
            return Err(FlowError::NoSource);
        }
        let prompt = match prompt {
            PageTextPrompt::Summary => json!({"kind":"summary"}),
            PageTextPrompt::Answer(text) => json!({"kind":"answer","text":text.as_str()}),
        };
        self.start(json!({"op":"page_text","source":source,"protocol":protocol.name(),"fingerprint":fingerprint,"prompt":prompt}))
            .await
    }

    pub(crate) async fn pi_prompt(&self, source: SourceId, text: UserText) -> Result<RunId> {
        let latest = self.inner.protocol.borrow();
        if latest.latest.as_ref().and_then(PageView::source) != Some(&source) {
            return Err(FlowError::NoSource);
        }
        if latest
            .latest
            .as_ref()
            .and_then(|view| view.pi_profile.as_ref())
            .is_none()
        {
            return Err(FlowError::InvalidInput("Pi profile is unavailable.".into()));
        }
        drop(latest);
        self.start(json!({"op":"pi_prompt","source":source,"text":text.as_str()}))
            .await
    }

    pub(crate) async fn codex_prompt(
        &self,
        source: SourceId,
        text: UserText,
        mode: &str,
    ) -> Result<RunId> {
        let latest = self.inner.protocol.borrow();
        if latest.latest.as_ref().and_then(PageView::source) != Some(&source) {
            return Err(FlowError::NoSource);
        }
        if latest
            .latest
            .as_ref()
            .and_then(|view| view.codex_profile.as_ref())
            .is_none()
        {
            return Err(FlowError::InvalidInput(
                "Codex configuration is unavailable.".into(),
            ));
        }
        drop(latest);
        self.start(json!({"op":"codex_prompt","source":source,"text":text.as_str(),"mode":mode}))
            .await
    }

    pub(crate) async fn opencode_prompt(&self, source: SourceId, text: UserText) -> Result<RunId> {
        let latest = self.inner.protocol.borrow();
        if latest.latest.as_ref().and_then(PageView::source) != Some(&source) {
            return Err(FlowError::NoSource);
        }
        if latest
            .latest
            .as_ref()
            .and_then(|view| view.opencode_profile.as_ref())
            .is_none()
        {
            return Err(FlowError::InvalidInput(
                "OpenCode configuration is unavailable.".into(),
            ));
        }
        drop(latest);
        self.start(json!({"op":"opencode_prompt","source":source,"text":text.as_str()}))
            .await
    }

    pub(crate) async fn pi_follow_up(
        &self,
        workflow: RunId,
        revision: u64,
        text: UserText,
    ) -> Result<RunId> {
        let available = self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .is_some_and(|view| {
                view.pi_profile.as_ref().is_some_and(|profile| {
                    view.agent_sessions.iter().any(|session| {
                        session.workflow == workflow
                            && session.revision == revision
                            && session.ready
                            && session.profile == profile.fingerprint
                            && Some(&session.source) == view.source()
                    })
                })
            });
        if !available {
            return Err(FlowError::InvalidInput(
                "The selected Pi session is unavailable.".into(),
            ));
        }
        self.start(json!({"op":"pi_follow_up","workflow":workflow,"revision":revision,"text":text.as_str()}))
            .await
    }

    pub(crate) async fn codex_follow_up(
        &self,
        workflow: RunId,
        revision: u64,
        text: UserText,
        mode: &str,
    ) -> Result<RunId> {
        let available = self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .is_some_and(|view| {
                view.codex_profile.as_ref().is_some_and(|profile| {
                    view.codex_sessions.iter().any(|session| {
                        session.workflow == workflow
                            && session.revision == revision
                            && session.ready
                            && session.profile == profile.fingerprint
                            && Some(&session.source) == view.source()
                    })
                })
            });
        if !available {
            return Err(FlowError::InvalidInput(
                "The selected Codex session is unavailable.".into(),
            ));
        }
        self.start(json!({"op":"codex_follow_up","workflow":workflow,"revision":revision,"text":text.as_str(),"mode":mode})).await
    }

    pub(crate) async fn opencode_follow_up(
        &self,
        workflow: RunId,
        revision: u64,
        text: UserText,
    ) -> Result<RunId> {
        let available = self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .is_some_and(|view| {
                view.opencode_profile.as_ref().is_some_and(|profile| {
                    view.opencode_sessions.iter().any(|session| {
                        session.workflow == workflow
                            && session.revision == revision
                            && session.ready
                            && session.profile == profile.fingerprint
                            && Some(&session.source) == view.source()
                    })
                })
            });
        if !available {
            return Err(FlowError::InvalidInput(
                "The selected OpenCode session is unavailable.".into(),
            ));
        }
        self.start(json!({"op":"opencode_follow_up","workflow":workflow,"revision":revision,"text":text.as_str()})).await
    }

    pub(crate) async fn native_permission(
        &self,
        run: RunId,
        request: RunId,
        option: Option<String>,
    ) -> Result<PageView> {
        let offered = self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .is_some_and(|view| {
                view.native_permissions.iter().any(|permission| {
                    permission.run == run
                        && permission.request == request
                        && option.as_ref().is_none_or(|selected| {
                            permission.options.iter().any(|item| &item.id == selected)
                        })
                })
            });
        if !self.is_connected() || !offered {
            return Err(FlowError::InvalidInput(
                "This native permission is no longer pending.".into(),
            ));
        }
        match self
            .request(
                json!({"op":"native_permission","run":run,"request":request,"option":option}),
                ReplyKind::Page,
            )
            .await?
        {
            Reply::Page(view) => Ok(self.check_source(view).await),
            _ => Err(FlowError::Protocol(
                "Expected the updated native permission view.".into(),
            )),
        }
    }

    pub(crate) async fn pi_ui_reply(
        &self,
        run: RunId,
        request: RunId,
        kind: &str,
        text: Option<String>,
        confirm: Option<bool>,
    ) -> Result<PageView> {
        let answer = match kind {
            "select" | "text" => {
                json!({"kind":kind,"value":text.ok_or_else(|| FlowError::InvalidInput("Pi UI answer is missing.".into()))?})
            }
            "confirm" => {
                json!({"kind":kind,"value":confirm.ok_or_else(|| FlowError::InvalidInput("Pi UI answer is missing.".into()))?})
            }
            "cancel" => json!({"kind":"cancel"}),
            _ => {
                return Err(FlowError::InvalidInput(
                    "Pi UI answer kind is invalid.".into(),
                ));
            }
        };
        match self
            .request(
                json!({"op":"pi_ui_reply","run":run,"request":request,"answer":answer}),
                ReplyKind::Page,
            )
            .await?
        {
            Reply::Page(view) => Ok(self.check_source(view).await),
            _ => Err(FlowError::Protocol("Expected a Pi UI reply view.".into())),
        }
    }

    pub(crate) async fn codex_input_reply(
        &self,
        run: RunId,
        request: RunId,
        answer: Value,
    ) -> Result<PageView> {
        match self
            .request(
                json!({"op":"codex_input_reply","run":run,"request":request,"answer":answer}),
                ReplyKind::Page,
            )
            .await?
        {
            Reply::Page(view) => Ok(self.check_source(view).await),
            _ => Err(FlowError::Protocol(
                "Expected a Codex input reply view.".into(),
            )),
        }
    }

    async fn start(&self, command: Value) -> Result<RunId> {
        match self.request(command, ReplyKind::Start).await? {
            Reply::Started(run) => Ok(run),
            _ => Err(FlowError::Protocol("Expected a run receipt.".into())),
        }
    }

    pub(crate) async fn watch(&self, run: RunId) -> Result<Views> {
        let id = self.inner.request_id()?;
        let cell = self
            .inner
            .protocol
            .borrow_mut()
            .register_watch(id.clone(), run.clone())?;
        let views = Views {
            connection: Rc::downgrade(&self.inner),
            protocol: Rc::downgrade(&self.inner.protocol),
            cell,
            request: Some(id.clone()),
        };
        self.inner.post(&id, json!({"op":"watch","run":run}))?;
        Ok(views)
    }

    pub(crate) async fn request_stop(&self, run: RunId) -> Result<StopReceipt> {
        match self
            .request(json!({"op":"stop","run":run}), ReplyKind::Stop(run.clone()))
            .await?
        {
            Reply::Stop(receipt) => Ok(receipt),
            _ => Err(FlowError::Protocol("Expected a Stop receipt.".into())),
        }
    }

    pub(crate) async fn decide_browser_action(
        &self,
        action: BrowserActionId,
        revision: u64,
        approve: bool,
    ) -> Result<PageView> {
        let available = self
            .inner
            .protocol
            .borrow()
            .latest
            .as_ref()
            .is_some_and(|view| {
                self.is_connected()
                    && view.browser_actions.iter().any(|item| {
                        item.id == action
                            && item.revision == revision
                            && matches!(item.phase, crate::model::BrowserActionPhase::Pending)
                    })
            });
        if !available {
            return Err(FlowError::InvalidInput(
                "This browser action is no longer pending.".into(),
            ));
        }
        if approve {
            let latest = self
                .inner
                .protocol
                .borrow()
                .latest
                .clone()
                .ok_or(FlowError::NoSource)?;
            let checked = self.check_source(latest).await;
            if !checked.browser_actions.iter().any(|item| {
                item.id == action
                    && item.revision == revision
                    && checked.can_approve(item, self.source_epoch())
            }) {
                return Err(FlowError::InvalidInput(
                    "The browser action document is no longer current.".into(),
                ));
            }
        }
        match self.request(
            json!({"op":"browser_decision","action":action,"revision":revision,"approve":approve}),
            ReplyKind::Page,
        ).await? {
            Reply::Page(view) => Ok(self.check_source(view).await),
            _ => Err(FlowError::Protocol("Expected the updated browser action view.".into())),
        }
    }

    async fn request(&self, command: Value, kind: ReplyKind) -> Result<Reply> {
        if self.is_closed() || (command["op"] != "view" && !self.inner.protocol.borrow().connected)
        {
            return Err(FlowError::Disconnected);
        }
        let id = self.inner.request_id()?;
        let cell = self
            .inner
            .protocol
            .borrow_mut()
            .register_request(id.clone(), kind)?;
        let mut wait = RequestWait {
            protocol: Rc::downgrade(&self.inner.protocol),
            id: id.clone(),
            cell,
            timer: None,
        };
        wait.timer = Some(Timer::new(&wait.cell)?);
        self.inner.post(&id, command)?;
        wait.await
    }

    pub(super) fn observe(&self) -> Result<Views> {
        let cell = self.inner.protocol.borrow_mut().observe()?;
        Ok(Views {
            connection: Rc::downgrade(&self.inner),
            protocol: Rc::downgrade(&self.inner.protocol),
            cell,
            request: None,
        })
    }
    pub(super) fn is_closed(&self) -> bool {
        self.inner.protocol.borrow().closed
    }
    pub(super) fn is_connected(&self) -> bool {
        self.inner.protocol.borrow().connected
    }
    pub(super) fn source_epoch(&self) -> u64 {
        self.inner.epoch.get()
    }
    pub(super) fn on_source_change(&self, callback: impl Fn() + 'static) {
        *self.inner.source_changed.borrow_mut() = Some(Box::new(callback));
    }
    pub(super) async fn check_source(&self, mut view: PageView) -> PageView {
        let epoch = self.inner.epoch.get();
        let status = self
            .inner
            .source_status(&view)
            .await
            .unwrap_or(SourceStatus::Saved);
        for action in &mut view.browser_actions {
            action.target_current = action.phase == crate::model::BrowserActionPhase::Pending
                && self.inner.action_is_current(action).await.unwrap_or(false);
        }
        view.source_epoch = epoch;
        let stable = epoch == self.inner.epoch.get();
        if !stable {
            for action in &mut view.browser_actions {
                action.target_current = false;
            }
        }
        view.source_status = if !stable {
            SourceStatus::Checking
        } else {
            status
        };
        view
    }
}

pub(crate) struct Views {
    connection: Weak<Connection>,
    protocol: Weak<RefCell<Protocol>>,
    cell: Rc<RefCell<WatchCell>>,
    request: Option<String>,
}
impl Views {
    pub(crate) async fn changed(&mut self) -> Result<Option<PageView>> {
        let view = poll_fn(|cx| self.cell.borrow_mut().poll(cx)).await?;
        match (view, self.connection.upgrade()) {
            (Some(view), Some(inner)) => Ok(Some(FlowClient { inner }.check_source(view).await)),
            (view, _) => Ok(view),
        }
    }
}
impl Drop for Views {
    fn drop(&mut self) {
        if let Some(protocol) = self.protocol.upgrade() {
            if let Some(id) = &self.request {
                protocol.borrow_mut().remove_watch(id);
            } else {
                protocol.borrow_mut().remove_observer(&self.cell);
            }
        }
    }
}

struct RequestWait {
    protocol: Weak<RefCell<Protocol>>,
    id: String,
    cell: Rc<RefCell<ReplyCell>>,
    timer: Option<Timer>,
}
impl Future for RequestWait {
    type Output = Result<Reply>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.cell.borrow_mut().poll(cx)
    }
}
impl Drop for RequestWait {
    fn drop(&mut self) {
        if let Some(protocol) = self.protocol.upgrade() {
            protocol.borrow_mut().remove_request(&self.id);
        }
    }
}
struct Timer {
    id: JsValue,
    _callback: Closure<dyn FnMut()>,
}
impl Timer {
    fn new(cell: &Rc<RefCell<ReplyCell>>) -> Result<Self> {
        let weak = Rc::downgrade(cell);
        let callback = Closure::<dyn FnMut()>::new(move || {
            if let Some(cell) = weak.upgrade() {
                let waker = cell.borrow_mut().fail(FlowError::Timeout);
                if let Some(waker) = waker {
                    waker.wake();
                }
            }
        });
        let id = call(
            &js_sys::global(),
            "setTimeout",
            &[callback.as_ref().clone(), 5_000.into()],
        )
        .map_err(js_error)?;
        Ok(Self {
            id,
            _callback: callback,
        })
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        let _ = call(&js_sys::global(), "clearTimeout", &[self.id.clone()]);
    }
}

impl Connection {
    fn request_id(&self) -> Result<String> {
        let n = self
            .next
            .get()
            .checked_add(1)
            .ok_or_else(|| FlowError::Protocol("Request identity capacity reached.".into()))?;
        self.next.set(n);
        Ok(format!("panel-{}-{n}", self.nonce))
    }
    fn post(&self, id: &str, command: Value) -> Result<()> {
        if self.protocol.borrow().closed {
            return Err(FlowError::Disconnected);
        }
        let value = json_js(&json!({"v":crate::WIRE_VERSION,"request":id,"command":command}))?;
        call(&self.port, "postMessage", &[value]).map_err(js_error)?;
        Ok(())
    }
    async fn active_tab(&self) -> Result<JsValue> {
        let query = json_js(&json!({"active":true,"lastFocusedWindow":true}))?;
        let tabs = await_call(&chrome_api("tabs").map_err(js_error)?, "query", &[query])
            .await
            .map_err(js_error)?;
        Ok(Array::from(&tabs).get(0))
    }
    async fn inject(&self, tab: i32, document: Option<&str>, url: &str) -> Result<JsValue> {
        capture::check_access(url).await?;
        let mut target = json!({"tabId":tab});
        if let Some(document) = document {
            target["documentIds"] = json!([document]);
        }
        let details = json_js(&json!({"target":target}))?;
        Reflect::set(&details, &"func".into(), self.capture.as_ref()).map_err(js_error)?;
        let values = await_call(
            &chrome_api("scripting").map_err(js_error)?,
            "executeScript",
            &[details],
        )
        .await
        .map_err(js_error)?;
        capture::check_access(url).await?;
        let item = Array::from(&values).get(0);
        if get(&item, "frameId").map_err(js_error)?.as_f64() != Some(0.0) {
            return Err(FlowError::Host(
                "The selected main frame is unavailable.".into(),
            ));
        }
        Ok(item)
    }
    async fn capture_selected(&self) -> Result<Value> {
        let tab = self.active_tab().await?;
        let tab_id = get(&tab, "id")
            .map_err(js_error)?
            .as_f64()
            .filter(|id| *id >= 0.0 && *id <= f64::from(i32::MAX) && id.fract() == 0.0)
            .ok_or_else(|| FlowError::Host("The selected tab has no valid ID.".into()))?
            as i32;
        let selected_url = string_property(&tab, "url").map_err(js_error)?;
        validate_url(&selected_url)?;
        let item = self.inject(tab_id, None, &selected_url).await?;
        let document = string_property(&item, "documentId").map_err(js_error)?;
        let observed = get(&item, "result").map_err(js_error)?;
        let url = string_property(&observed, "url").map_err(js_error)?;
        validate_url(&url)?;
        let title = string_property(&observed, "title").map_err(js_error)?;
        let text = string_property(&observed, "text").map_err(js_error)?;
        if document.is_empty()
            || document.len() > 128
            || title.is_empty()
            || title.len() > 256
            || url.len() > 2048
            || text.is_empty()
            || text.len() > MAX_PAGE
        {
            return Err(FlowError::InvalidInput(
                "The page has empty or oversized content. Nothing was sent.".into(),
            ));
        }
        let confirmed = self.inject(tab_id, Some(&document), &url).await?;
        let current = self.active_tab().await?;
        let result = get(&confirmed, "result").map_err(js_error)?;
        if selected_url != url
            || get(&current, "id").map_err(js_error)?.as_f64() != Some(f64::from(tab_id))
            || string_property(&current, "url").map_err(js_error)? != url
            || string_property(&confirmed, "documentId").map_err(js_error)? != document
            || string_property(&result, "url").map_err(js_error)? != url
            || string_property(&result, "title").map_err(js_error)? != title
            || string_property(&result, "text").map_err(js_error)? != text
        {
            return Err(FlowError::Host(
                "The selected page changed during capture. Nothing was sent.".into(),
            ));
        }
        Ok(json!({"tab":tab_id,"document":document,"url":url,"title":title,"text":text}))
    }
    async fn source_status(&self, view: &PageView) -> Result<SourceStatus> {
        let Some(source) = &view.source else {
            return Ok(SourceStatus::Saved);
        };
        let tab = self.active_tab().await?;
        if get(&tab, "id").map_err(js_error)?.as_f64() != Some(f64::from(source.tab)) {
            return Ok(SourceStatus::Saved);
        }
        let item = self
            .inject(source.tab, Some(&source.document), &source.url)
            .await?;
        let observed = get(&item, "result").map_err(js_error)?;
        let current = self.active_tab().await?;
        let same_document = get(&current, "id").map_err(js_error)?.as_f64()
            == Some(f64::from(source.tab))
            && string_property(&current, "url").map_err(js_error)? == source.url
            && string_property(&item, "documentId").map_err(js_error)? == source.document
            && string_property(&observed, "url").map_err(js_error)? == source.url;
        Ok(if !same_document {
            SourceStatus::Saved
        } else if string_property(&observed, "title").map_err(js_error)? == source.title
            && string_property(&observed, "text").map_err(js_error)? == source.text
        {
            SourceStatus::Current
        } else {
            SourceStatus::Changed
        })
    }
    async fn action_is_current(&self, action: &crate::model::BrowserActionView) -> Result<bool> {
        let tabs = chrome_api("tabs").map_err(js_error)?;
        let tab = await_call(&tabs, "get", &[action.tab.into()])
            .await
            .map_err(js_error)?;
        if get(&tab, "id").map_err(js_error)?.as_f64() != Some(f64::from(action.tab))
            || string_property(&tab, "url").map_err(js_error)? != action.url
        {
            return Ok(false);
        }
        let item = self
            .inject(action.tab, Some(&action.document), &action.url)
            .await?;
        let observed = get(&item, "result").map_err(js_error)?;
        let current = await_call(&tabs, "get", &[action.tab.into()])
            .await
            .map_err(js_error)?;
        Ok(
            get(&current, "id").map_err(js_error)?.as_f64() == Some(f64::from(action.tab))
                && string_property(&current, "url").map_err(js_error)? == action.url
                && string_property(&item, "documentId").map_err(js_error)? == action.document
                && string_property(&observed, "url").map_err(js_error)? == action.url,
        )
    }
}

pub(super) fn validate_url(url: &str) -> Result<()> {
    crate::site::permission(url).map(|_| ())
}
fn json_js(value: &Value) -> Result<JsValue> {
    let raw = serde_json::to_string(value).map_err(|e| FlowError::Protocol(e.to_string()))?;
    if raw.len() > MAX_FRAME {
        return Err(FlowError::Protocol(
            "Request exceeds the 65536 byte frame limit.".into(),
        ));
    }
    js_sys::JSON::parse(&raw).map_err(js_error)
}
fn decode_frame(value: JsValue) -> Result<Value> {
    let raw = js_sys::JSON::stringify(&value)
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| FlowError::Protocol("Native response was not JSON.".into()))?;
    if raw.len() > MAX_FRAME {
        return Err(FlowError::Protocol(
            "Native response exceeds the 65536 byte frame limit.".into(),
        ));
    }
    serde_json::from_str(&raw).map_err(|e| FlowError::Protocol(e.to_string()))
}
fn js_error(error: JsValue) -> FlowError {
    FlowError::Host(js_message(&error))
}
fn wake(wakers: Vec<std::task::Waker>) {
    for waker in wakers {
        waker.wake();
    }
}
