use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    task::{Poll, Waker},
};

use js_sys::{Function, Promise};
use metldr_history_pages::Scope;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::{
    ag_ui::{AgUiMessage, Event, RunStream},
    composer,
    execution_preference::{ExecutionPreference, MAX_EXECUTION_CHOICE_BYTES},
    flow::{FlowClient, StopReceipt},
    history::Change,
    model::{FlowError, PageView, RunId, UserText},
};

mod activation;
mod active;
mod agents;
mod browser_actions;
mod codex_input;
mod connection;
mod conversations;
mod destinations;
mod history;
mod native_permissions;
mod native_tools;
mod owner;
mod pi_ui;
mod providers;
mod run_details;
mod run_reference;
mod saved_source;
mod settings;
mod settings_updates;
mod source_updates;
pub use active::UiActiveRun;
pub use agents::{UiAgentConfiguration, UiAgentProfile, UiAgentSettings};
pub use browser_actions::{
    UiBrowserAction, UiBrowserActionDetail, UiBrowserActionSnapshot, UiBrowserActionSubscription,
    UiBrowserActions,
};
pub use codex_input::{
    UiCodexAnswer, UiCodexField, UiCodexFieldAnswer, UiCodexInput, UiCodexOption,
    UiCodexPromptKind, UiCodexQuestion, UiCodexQuestionAnswer, UiCodexRequest, UiCodexSnapshot,
    UiCodexSubscription,
};
pub use connection::{UiConnection, UiConnectionSnapshot, UiConnectionSubscription};
pub use destinations::UiDestination;
pub use native_permissions::{
    UiNativePermission, UiNativePermissionOption, UiNativePermissionSnapshot,
    UiNativePermissionSubscription, UiNativePermissions,
};
pub use native_tools::{UiNativeTool, UiNativeToolPage, UiNativeTools, UiToolPreview};
use owner::{NavigationWork, Owner, Stop};
pub use pi_ui::{
    UiPiDialogKind, UiPiNotice, UiPiOption, UiPiRequest, UiPiSnapshot, UiPiStatus,
    UiPiSubscription, UiPiUi, UiPiWidget,
};
pub use providers::{UiProviderConnection, UiProviderSettings};
pub use run_details::{UiRunDetails, UiRunReport, UiRunReportPage};
pub use saved_source::{UiSavedSource, UiSourceSnapshot, UiSourceStatus, UiSourceSubscription};
pub use settings_updates::{UiAgentSnapshot, UiProviderSnapshot, UiSettingsSubscription};

#[wasm_bindgen]
pub struct UiRunRequest {
    thread: String,
    id: String,
    message: String,
    destination: String,
    text: UserText,
    mode: String,
}

#[wasm_bindgen]
impl UiRunRequest {
    #[wasm_bindgen(constructor)]
    pub fn new(
        thread: String,
        id: String,
        message: String,
        destination: String,
        text: String,
        mode: String,
    ) -> Result<UiRunRequest, JsValue> {
        if !crate::ag_ui::valid_id(&thread)
            || !crate::ag_ui::valid_id(&id)
            || !crate::ag_ui::valid_id(&message)
            || destination.len() > 512
            || !matches!(mode.as_str(), "configured" | "plan")
        {
            return Err(crate::err("The UI run request is invalid."));
        }
        Ok(Self {
            thread,
            id,
            message,
            destination,
            text: UserText::try_from(text.as_str()).map_err(js_error)?,
            mode,
        })
    }
}

#[wasm_bindgen]
pub struct UiBridge {
    core: Rc<Core>,
}

struct Core {
    page: Rc<PageSession>,
    scope: Scope,
    owner: RefCell<Owner>,
    history_loading: Cell<bool>,
    preparing: RefCell<Option<Waker>>,
    stream_waiter: RefCell<Option<Waker>>,
    native_operation: RefCell<native_permissions::Operation>,
    native_subscriptions: RefCell<Vec<std::rc::Weak<native_permissions::Observer>>>,
    browser_operation: RefCell<browser_actions::Operation>,
    browser_subscriptions: RefCell<Vec<std::rc::Weak<browser_actions::Observer>>>,
    pi_operation: RefCell<pi_ui::Operation>,
    pi_subscriptions: RefCell<Vec<std::rc::Weak<pi_ui::Observer>>>,
    codex_operation: RefCell<codex_input::Operation>,
    codex_subscriptions: RefCell<Vec<std::rc::Weak<codex_input::Observer>>>,
}

struct PageSession {
    flow: RefCell<FlowClient>,
    view: RefCell<PageView>,
    execution_preference: RefCell<ExecutionPreference>,
    navigating: Cell<bool>,
    settings: RefCell<settings::Operation>,
    current: RefCell<std::rc::Weak<Core>>,
    settings_subscriptions: RefCell<Vec<std::rc::Weak<settings_updates::Observer>>>,
    invalidated: Cell<bool>,
    source_refreshing: Cell<bool>,
    recovery: RefCell<connection::Slot>,
    connection_subscriptions: RefCell<Vec<std::rc::Weak<connection::Observer>>>,
    source_subscriptions: RefCell<Vec<std::rc::Weak<saved_source::Observer>>>,
    connection_waiter: RefCell<Option<Waker>>,
}

impl PageSession {
    fn accept(&self, view: PageView) -> Result<PageView, FlowError> {
        let mut current = self.view.borrow_mut();
        if self.invalidated.get() || view.store != current.store {
            self.invalidated.set(true);
            drop(current);
            self.notify_settings();
            return Err(FlowError::HistoryStale);
        }
        if !view.is_older_than(&current) {
            *current = view;
        }
        let accepted = current.clone();
        drop(current);
        self.notify_settings();
        Ok(accepted)
    }

    fn settings_blocked(&self) -> bool {
        let view = self.view.borrow();
        self.invalidated.get()
            || self.recovery.borrow().active()
            || self.settings.borrow().blocked()
            || view.provider_settings.busy
            || view.provider_settings.pending.is_some()
            || view.agent_settings.busy
    }

    fn controls_ready(&self) -> bool {
        !self.invalidated.get()
            && !self.recovery.borrow().active()
            && !self.navigating.get()
            && self
                .current
                .borrow()
                .upgrade()
                .is_none_or(|core| core.owner.borrow().settings_ready())
    }
}

struct HistoryRead<'a>(&'a Cell<bool>);

impl Drop for HistoryRead<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    thread_id: String,
    #[serde(flatten)]
    history: history::Page,
    running: bool,
    source_available: bool,
}

#[wasm_bindgen]
pub async fn connect_ui_bridge(capture: Function) -> Result<UiBridge, JsValue> {
    crate::flow::install_capture(capture);
    let flow = FlowClient::connect().await.map_err(js_error)?;
    let view = flow.current_page().await.map_err(js_error)?;
    UiBridge::from_view(flow, view).map_err(js_error)
}

impl UiBridge {
    fn from_view(flow: FlowClient, view: PageView) -> Result<Self, FlowError> {
        let views = flow.observe()?;
        let page = Rc::new(PageSession {
            flow: RefCell::new(flow.clone()),
            view: RefCell::new(view.clone()),
            execution_preference: RefCell::new(ExecutionPreference::load()),
            navigating: Cell::new(false),
            settings: RefCell::new(settings::Operation::default()),
            current: RefCell::new(std::rc::Weak::new()),
            settings_subscriptions: RefCell::new(Vec::new()),
            invalidated: Cell::new(false),
            source_refreshing: Cell::new(false),
            recovery: RefCell::new(connection::Slot::default()),
            connection_subscriptions: RefCell::new(Vec::new()),
            source_subscriptions: RefCell::new(Vec::new()),
            connection_waiter: RefCell::new(None),
        });
        page.install_source_updates();
        page.install_connection_updates(flow, views);
        Ok(Self::from_session(page, &view))
    }

    fn from_session(page: Rc<PageSession>, view: &PageView) -> Self {
        let bridge = Self {
            core: Rc::new(Core {
                scope: view.selection.clone(),
                page: Rc::clone(&page),
                owner: RefCell::new(Owner::Idle),
                history_loading: Cell::new(false),
                preparing: RefCell::new(None),
                stream_waiter: RefCell::new(None),
                native_operation: RefCell::new(native_permissions::Operation::default()),
                native_subscriptions: RefCell::new(Vec::new()),
                browser_operation: RefCell::new(browser_actions::Operation::default()),
                browser_subscriptions: RefCell::new(Vec::new()),
                pi_operation: RefCell::new(pi_ui::Operation::default()),
                pi_subscriptions: RefCell::new(Vec::new()),
                codex_operation: RefCell::new(codex_input::Operation::default()),
                codex_subscriptions: RefCell::new(Vec::new()),
            }),
        };
        page.current.replace(Rc::downgrade(&bridge.core));
        bridge
    }
}

#[wasm_bindgen]
impl UiBridge {
    #[wasm_bindgen(getter)]
    pub fn thread_id(&self) -> String {
        self.core.scope.conversation.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn navigation_pending(&self) -> bool {
        matches!(*self.core.owner.borrow(), Owner::Navigating(_))
    }

    pub fn active_run(&self) -> Result<Option<UiActiveRun>, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        let view = self.core.cached().map_err(js_error)?;
        Ok(view
            .runs
            .iter()
            .find(|run| !run.status.is_terminal())
            .map(|run| UiActiveRun::new(Rc::clone(&self.core), run)))
    }

    pub fn provider_settings(&self) -> UiProviderSettings {
        UiProviderSettings::new(Rc::clone(&self.core.page))
    }

    pub fn agent_settings(&self) -> UiAgentSettings {
        UiAgentSettings::new(Rc::clone(&self.core.page))
    }

    pub async fn execution_choices(&self) -> Result<Vec<UiDestination>, JsValue> {
        let _read = self.core.begin_read().map_err(js_error)?;
        let view = self.core.current().await.map_err(js_error)?;
        Ok(composer::choices::list(Some(&view))
            .into_iter()
            .map(UiDestination::from)
            .collect())
    }

    pub async fn run_details(&self, encoded: String) -> Result<UiRunDetails, JsValue> {
        run_details::open(Rc::clone(&self.core), encoded)
            .await
            .map_err(js_error)
    }

    pub fn preferred_execution_choice(&self) -> Result<Option<String>, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        Ok(self.core.page.execution_preference.borrow().value())
    }

    pub fn select_execution_choice(&self, value: String) -> Result<(), JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        if self.core.page.recovery.borrow().active() || !self.core.owner.borrow().settings_ready() {
            return Err(js_error(FlowError::HistoryBusy));
        }
        if value.len() > MAX_EXECUTION_CHOICE_BYTES {
            return Err(crate::err("The execution choice exceeds its limit."));
        }
        if !value.is_empty() {
            composer::resolve(&self.core.page.view.borrow(), &value)
                .map_err(FlowError::InvalidInput)
                .map_err(js_error)?;
        }
        self.core
            .page
            .execution_preference
            .borrow_mut()
            .remember(&value);
        Ok(())
    }

    pub async fn snapshot(&self) -> Result<String, JsValue> {
        let view = self.core.current().await.map_err(js_error)?;
        let snapshot = Snapshot {
            thread_id: self.thread_id(),
            history: self.core.history(None).await.map_err(js_error)?,
            running: view.runs.iter().any(|run| !run.status.is_terminal()),
            source_available: view.source().is_some(),
        };
        bounded_json(&snapshot).map_err(js_error)
    }

    pub async fn load_older(&self, cursor: String) -> Result<String, JsValue> {
        let page = self.core.history(Some(&cursor)).await.map_err(js_error)?;
        bounded_json(&page).map_err(js_error)
    }

    pub async fn table(&self, run: String, artifact: String) -> Result<String, JsValue> {
        let _read = self.core.begin_read().map_err(js_error)?;
        let run: RunId = serde_json::from_value(serde_json::Value::String(run))
            .map_err(|_| crate::err("Invalid table run."))?;
        let artifact = metldr_conversation::ArtifactId::new(artifact)
            .map_err(|_| crate::err("Invalid table identity."))?;
        let flow = self.core.flow();
        let context = flow.history_current().await.map_err(js_error)?;
        if context.scope != self.core.scope {
            return Err(js_error(FlowError::HistoryStale));
        }
        let table = flow
            .history_artifact(&context, &run, &artifact)
            .await
            .map_err(js_error)?;
        self.core.owner.borrow().readable().map_err(js_error)?;
        serde_json::to_string(&table).map_err(|error| crate::err(&error.to_string()))
    }

    pub async fn native_tools(&self, reference: String) -> Result<UiNativeTools, JsValue> {
        native_tools::open(Rc::clone(&self.core), reference)
            .await
            .map_err(js_error)
    }

    pub async fn conversations(&self, cursor: Option<String>) -> Result<String, JsValue> {
        let _read = self.core.begin_read().map_err(js_error)?;
        let page = conversations::page(&self.core.flow(), &self.core.scope, cursor.as_deref())
            .await
            .map_err(js_error)?;
        bounded_json(&page).map_err(js_error)
    }

    pub async fn new_conversation(&self) -> Result<UiBridge, JsValue> {
        self.core
            .begin_navigation(Change::Create)
            .map_err(js_error)?;
        self.core.navigate().await.map_err(js_error)
    }

    pub async fn select_conversation(&self, conversation: String) -> Result<UiBridge, JsValue> {
        let selected = Scope {
            conversation: conversation.clone(),
            ..self.core.scope.clone()
        };
        selected
            .validate()
            .map_err(|_| crate::err("Invalid conversation."))?;
        self.core
            .begin_navigation(Change::Select { conversation })
            .map_err(js_error)?;
        self.core.navigate().await.map_err(js_error)
    }

    pub async fn retry_navigation(&self) -> Result<UiBridge, JsValue> {
        self.core.navigate().await.map_err(js_error)
    }

    pub fn capture(&self) -> Result<Promise, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        let view = self.core.cached().map_err(js_error)?;
        if self.core.page.settings_blocked() {
            return Err(js_error(FlowError::HistoryBusy));
        }
        self.core.owner.borrow_mut().capture().map_err(js_error)?;
        // Prepare the permission request in the user's click, before yielding.
        let capture = self.core.flow().capture_selected(&view);
        drop(view);
        self.core.page.notify_settings();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let result = capture.await.and_then(|view| core.accept(view));
            core.owner.borrow_mut().capture_finished();
            core.page.notify_settings();
            result.map(|_| JsValue::UNDEFINED).map_err(js_error)
        }))
    }

    pub async fn run(&self, request: UiRunRequest, emit: Function) -> Result<(), JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        if request.thread != self.core.scope.conversation {
            return Err(crate::err(
                "The UI request belongs to another conversation.",
            ));
        }
        if self.core.page.settings_blocked() {
            return Err(js_error(FlowError::HistoryBusy));
        }
        self.core
            .owner
            .borrow_mut()
            .begin(&request.id)
            .map_err(js_error)?;
        self.core.page.notify_settings();
        let prepared = async {
            let Some(view) = self.core.prepare_current().await? else {
                return Ok(None);
            };
            if view.runs.iter().any(|run| !run.status.is_terminal()) {
                return Err(FlowError::HistoryBusy);
            }
            let source = view.source().cloned().ok_or(FlowError::NoSource)?;
            let destination =
                composer::resolve(&view, &request.destination).map_err(FlowError::InvalidInput)?;
            let messages = vec![AgUiMessage::User {
                id: request.message.clone(),
                content: request.text.as_str().into(),
            }];
            let stream = RunStream::new(request.thread, request.id.clone(), messages)
                .map_err(FlowError::InvalidInput)?;
            Ok::<_, FlowError>(Some((source, destination, stream)))
        }
        .await;
        let (source, destination, mut stream) = match prepared {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.core.owner.borrow_mut().not_sent();
                self.core.page.notify_settings();
                return Ok(());
            }
            Err(error) => {
                self.core.owner.borrow_mut().not_sent();
                self.core.page.notify_settings();
                return Err(js_error(error));
            }
        };
        self.core.ensure_current().map_err(js_error)?;
        let flow = self.core.flow();
        let started = composer::dispatch(
            &flow,
            self.core.scope.clone(),
            source,
            destination,
            request.text,
            &request.mode,
        )
        .await;
        let native = match started {
            Ok(native) => native,
            Err(error) => {
                self.core.owner.borrow_mut().start_failed(&error);
                self.core.page.notify_settings();
                return Err(js_error(error));
            }
        };
        self.core.ensure_current().map_err(js_error)?;
        if !self.core.flow().same_connection(&flow) {
            return Err(js_error(FlowError::HistoryStale));
        }
        let stop = self.core.owner.borrow_mut().accepted(native.clone());
        self.core.page.notify_settings();
        let result = async {
            let events = stream
                .accept_question(&native)
                .map_err(FlowError::Protocol)?;
            emit_events(events, &emit)?;
            if stop {
                flow.request_stop(self.core.scope.clone(), native.clone())
                    .await?;
            }
            self.core.stream(&native, &mut stream, &emit).await
        }
        .await;
        match result {
            Ok(()) => self.core.owner.borrow_mut().finished(&native),
            Err(error) => {
                if self.core.ensure_current().is_ok()
                    && self.core.flow().same_connection(&flow)
                    && !matches!(error, FlowError::Disconnected)
                {
                    let _ = flow
                        .request_stop(self.core.scope.clone(), native.clone())
                        .await;
                }
                if self.core.ensure_current().is_ok() {
                    self.core
                        .owner
                        .borrow_mut()
                        .run_uncertain(&request.id, &native);
                }
                self.core.page.notify_settings();
                return Err(js_error(error));
            }
        }
        self.core.page.notify_settings();
        Ok(())
    }

    pub async fn stop(&self, id: String) -> Result<String, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        if self.core.page.recovery.borrow().active() {
            return Err(js_error(FlowError::HistoryBusy));
        }
        let stop = self.core.owner.borrow_mut().stop(&id).map_err(js_error)?;
        match stop {
            Stop::Pending => {
                let preparing = self.core.preparing.borrow_mut().take();
                if let Some(waker) = preparing {
                    waker.wake();
                }
                Ok("pending".into())
            }
            Stop::Native(native) => {
                match self
                    .core
                    .flow()
                    .request_stop(self.core.scope.clone(), native)
                    .await
                    .map_err(js_error)?
                {
                    StopReceipt::Requested => Ok("requested".into()),
                    StopReceipt::AlreadyTerminal => Ok("already_terminal".into()),
                }
            }
        }
    }
}

impl Core {
    fn ensure_current(&self) -> Result<(), FlowError> {
        self.owner.borrow().readable()?;
        if self.page.invalidated.get()
            || self.page.view.borrow().selection != self.scope
            || !self
                .page
                .current
                .borrow()
                .upgrade()
                .is_some_and(|owner| std::ptr::eq(owner.as_ref(), self))
        {
            return Err(FlowError::HistoryStale);
        }
        Ok(())
    }

    fn flow(&self) -> FlowClient {
        self.page.flow.borrow().clone()
    }

    fn cached(&self) -> Result<std::cell::Ref<'_, PageView>, FlowError> {
        let view = self.page.view.borrow();
        if view.selection != self.scope {
            return Err(FlowError::HistoryStale);
        }
        Ok(view)
    }

    fn begin_navigation(&self, change: Change) -> Result<(), FlowError> {
        self.ensure_current()?;
        if self.history_loading.get() || self.page.settings_blocked() {
            return Err(FlowError::HistoryBusy);
        }
        let request = self
            .flow()
            .conversation_request(self.scope.clone(), change)?;
        self.owner.borrow_mut().navigate(request)?;
        self.page.navigating.set(true);
        self.page.notify_settings();
        Ok(())
    }

    async fn navigate(&self) -> Result<UiBridge, FlowError> {
        let work = self.owner.borrow_mut().navigation_step()?;
        let flow = self.flow();
        let mut recovery = None;
        let flow = if matches!(&work, NavigationWork::Retry(_) | NavigationWork::Refresh(_)) {
            let reconnect = async {
                let guard = connection::Recovery::begin(Rc::clone(&self.page))?;
                let flow = guard.connect().await?;
                let view = flow.current_page().await?;
                if view.selection.store != self.scope.store
                    || (view.selection != self.scope
                        && view.selection.epoch.0 <= self.scope.epoch.0)
                {
                    return Err(FlowError::HistoryStale);
                }
                recovery = Some(guard);
                Ok::<_, FlowError>(flow)
            }
            .await;
            match reconnect {
                Ok(flow) => flow,
                Err(error) => {
                    self.owner.borrow_mut().navigation_interrupted();
                    return Err(error);
                }
            }
        } else {
            flow
        };
        let expected = match work {
            NavigationWork::Send(request) | NavigationWork::Retry(request) => {
                match flow.change_conversation(&request).await {
                    Ok(receipt) => {
                        self.owner
                            .borrow_mut()
                            .navigation_confirmed(receipt.current.clone());
                        receipt.current
                    }
                    Err(error) => {
                        self.navigation_failed(&error);
                        return Err(error);
                    }
                }
            }
            NavigationWork::Refresh(expected) => expected,
        };
        let result = flow.current_page().await.and_then(|view| {
            if view.selection != expected.scope {
                return Err(FlowError::HistoryStale);
            }
            let view = match &recovery {
                Some(recovery) => recovery.navigation(flow.clone(), view, &expected)?,
                None => self.page.accept(view)?,
            };
            Ok(UiBridge::from_session(Rc::clone(&self.page), &view))
        });
        match &result {
            Ok(_) => {
                *self.owner.borrow_mut() = Owner::Retired;
                self.page.navigating.set(false);
                self.page.notify_settings();
            }
            Err(error) => self.navigation_failed(error),
        }
        drop(recovery);
        result
    }

    fn navigation_failed(&self, error: &FlowError) {
        let mut owner = self.owner.borrow_mut();
        owner.navigation_failed(error);
        self.page
            .navigating
            .set(matches!(*owner, Owner::Navigating(_)));
        drop(owner);
        self.page.notify_settings();
    }

    async fn prepare_current(&self) -> Result<Option<PageView>, FlowError> {
        let mut current = std::pin::pin!(self.current());
        let result = std::future::poll_fn(|context| {
            if self.owner.borrow().stop_pending() {
                return Poll::Ready(Ok(None));
            }
            self.preparing.replace(Some(context.waker().clone()));
            current
                .as_mut()
                .poll(context)
                .map(|result| result.map(Some))
        })
        .await;
        self.preparing.borrow_mut().take();
        result
    }

    async fn history(&self, cursor: Option<&str>) -> Result<history::Page, FlowError> {
        let _read = self.begin_read()?;
        history::page(&self.flow(), &self.scope, cursor).await
    }

    fn begin_read(&self) -> Result<HistoryRead<'_>, FlowError> {
        self.ensure_current()?;
        if self.history_loading.replace(true) {
            return Err(FlowError::HistoryBusy);
        }
        Ok(HistoryRead(&self.history_loading))
    }

    fn accept(&self, view: PageView) -> Result<PageView, FlowError> {
        self.ensure_current()?;
        if view.selection != self.scope {
            return Err(FlowError::HistoryStale);
        }
        let view = self.page.accept(view)?;
        if view.selection != self.scope {
            return Err(FlowError::HistoryStale);
        }
        Ok(view)
    }

    async fn current(&self) -> Result<PageView, FlowError> {
        self.ensure_current()?;
        let flow = self.flow();
        let view = flow.current_page().await?;
        if !flow.same_connection(&self.flow()) {
            return Err(FlowError::HistoryStale);
        }
        self.accept(view)
    }

    async fn stream(
        &self,
        native: &RunId,
        stream: &mut RunStream,
        emit: &Function,
    ) -> Result<(), FlowError> {
        loop {
            self.ensure_current()?;
            let flow = self.flow();
            let result = async {
                let mut views = flow.watch(self.scope.clone(), native.clone()).await?;
                while let Some(view) = views.changed().await? {
                    self.ensure_current()?;
                    if !flow.same_connection(&self.flow()) {
                        return Err(FlowError::Disconnected);
                    }
                    if self.emit_view(view, native, stream, emit)? {
                        return Ok(());
                    }
                }
                Err(FlowError::Disconnected)
            }
            .await;
            if !matches!(result, Err(FlowError::Disconnected)) {
                return result;
            }
            self.page.notify_settings();
            self.wait_for_connection(&flow).await?;
        }
    }

    async fn wait_for_connection(&self, flow: &FlowClient) -> Result<(), FlowError> {
        struct Waiting<'a>(&'a RefCell<Option<Waker>>);
        impl Drop for Waiting<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().take();
            }
        }
        let _waiting = Waiting(&self.stream_waiter);
        std::future::poll_fn(|context| {
            if let Err(error) = self.ensure_current() {
                return Poll::Ready(Err(error));
            }
            if !self.page.recovery.borrow().active()
                && (flow.is_connected() || !flow.same_connection(&self.flow()))
            {
                return Poll::Ready(Ok(()));
            }
            self.stream_waiter.replace(Some(context.waker().clone()));
            Poll::Pending
        })
        .await
    }

    fn emit_view(
        &self,
        view: PageView,
        native: &RunId,
        stream: &mut RunStream,
        emit: &Function,
    ) -> Result<bool, FlowError> {
        let view = self.accept(view)?;
        let run = view
            .runs
            .iter()
            .find(|run| run.id == *native)
            .ok_or_else(|| FlowError::Protocol("The watched UI run is missing.".into()))?;
        emit_events(stream.push(run).map_err(FlowError::Protocol)?, emit)?;
        Ok(run.status.is_terminal())
    }
}

fn emit_events(events: Vec<Event>, emit: &Function) -> Result<(), FlowError> {
    for event in events {
        let encoded = bounded_json(&event)?;
        emit.call1(&JsValue::UNDEFINED, &encoded.into())
            .map_err(|_| FlowError::Host("The UI event consumer disconnected.".into()))?;
    }
    Ok(())
}

fn bounded_json(value: &impl Serialize) -> Result<String, FlowError> {
    let encoded =
        serde_json::to_string(value).map_err(|error| FlowError::Protocol(error.to_string()))?;
    if encoded.len() > crate::MAX_FRAME {
        return Err(FlowError::Protocol(
            "The UI event exceeds its frame limit.".into(),
        ));
    }
    Ok(encoded)
}

fn js_error(error: FlowError) -> JsValue {
    crate::err(&error.to_string())
}
