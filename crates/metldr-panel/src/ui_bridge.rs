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
    ag_ui::{AgUiMessage, MessageRole, RunStream},
    composer,
    flow::{FlowClient, StopReceipt},
    history::Change,
    model::{FlowError, PageView, RunId, UserText},
};

mod active;
mod agents;
mod conversations;
mod destinations;
mod history;
mod owner;
mod providers;
mod settings;
mod settings_updates;
pub use active::UiActiveRun;
pub use agents::{UiAgentConfiguration, UiAgentProfile, UiAgentSettings};
pub use destinations::UiDestination;
use owner::{NavigationWork, Owner, Stop};
pub use providers::{UiProviderConnection, UiProviderSettings};
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
}

struct PageSession {
    flow: RefCell<FlowClient>,
    view: RefCell<PageView>,
    navigating: Cell<bool>,
    settings: RefCell<settings::Operation>,
    current: RefCell<std::rc::Weak<Core>>,
    settings_subscriptions: RefCell<Vec<std::rc::Weak<settings_updates::Observer>>>,
    invalidated: Cell<bool>,
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
            || self.settings.borrow().blocked()
            || view.provider_settings.busy
            || view.provider_settings.pending.is_some()
            || view.agent_settings.busy
    }

    fn controls_ready(&self) -> bool {
        !self.invalidated.get()
            && !self.navigating.get()
            && self
                .current
                .borrow()
                .upgrade()
                .is_none_or(|core| core.owner.borrow().settings_ready())
    }

    fn replace_flow(&self, flow: FlowClient) {
        self.flow.replace(flow);
        self.notify_settings();
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
    Ok(UiBridge::from_view(flow, view))
}

impl UiBridge {
    fn from_view(flow: FlowClient, view: PageView) -> Self {
        let page = Rc::new(PageSession {
            flow: RefCell::new(flow),
            view: RefCell::new(view.clone()),
            navigating: Cell::new(false),
            settings: RefCell::new(settings::Operation::default()),
            current: RefCell::new(std::rc::Weak::new()),
            settings_subscriptions: RefCell::new(Vec::new()),
            invalidated: Cell::new(false),
        });
        Self::from_session(page, &view)
    }

    fn from_session(page: Rc<PageSession>, view: &PageView) -> Self {
        let bridge = Self {
            core: Rc::new(Core {
                scope: view.selection.clone(),
                page: Rc::clone(&page),
                owner: RefCell::new(Owner::Idle),
                history_loading: Cell::new(false),
                preparing: RefCell::new(None),
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

    pub fn active_run(&self) -> Result<Option<UiActiveRun>, JsValue> {
        self.core.owner.borrow().readable().map_err(js_error)?;
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
            let messages = vec![AgUiMessage {
                id: request.message.clone(),
                role: MessageRole::User,
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
        let started = composer::dispatch(
            &self.core.flow(),
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
        let stop = self.core.owner.borrow_mut().accepted(native.clone());
        self.core.page.notify_settings();
        if stop {
            if let Err(error) = self
                .core
                .flow()
                .request_stop(self.core.scope.clone(), native.clone())
                .await
            {
                self.core.owner.borrow_mut().uncertain();
                self.core.page.notify_settings();
                return Err(js_error(error));
            }
        }
        let result = async {
            stream
                .accept_question(&native)
                .map_err(FlowError::Protocol)?;
            self.core.stream(&native, &mut stream, &emit).await
        }
        .await;
        match result {
            Ok(()) => self.core.owner.borrow_mut().finished(&native),
            Err(error) => {
                // Losing a listener is not evidence that native work stopped.
                let _ = self
                    .core
                    .flow()
                    .request_stop(self.core.scope.clone(), native)
                    .await;
                self.core.owner.borrow_mut().uncertain();
                self.core.page.notify_settings();
                return Err(js_error(error));
            }
        }
        self.core.page.notify_settings();
        Ok(())
    }

    pub async fn stop(&self, id: String) -> Result<String, JsValue> {
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
        let flow = if matches!(&work, NavigationWork::Retry(_) | NavigationWork::Refresh(_)) {
            match flow.reconnect().await {
                Ok(flow) => {
                    let verified = flow
                        .current_page()
                        .await
                        .and_then(|view| self.page.accept(view));
                    if let Err(error) = verified {
                        self.owner.borrow_mut().navigation_interrupted();
                        return Err(error);
                    }
                    flow
                }
                Err(error) => {
                    self.owner.borrow_mut().navigation_interrupted();
                    return Err(error);
                }
            }
        } else {
            flow
        };
        self.page.replace_flow(flow.clone());
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
            if view.selection.store != expected.scope.store
                || (view.selection != expected.scope
                    && view.selection.epoch.0 <= expected.scope.epoch.0)
            {
                return Err(FlowError::HistoryStale);
            }
            let view = self.page.accept(view)?;
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
        self.owner.borrow().readable()?;
        if self.history_loading.replace(true) {
            return Err(FlowError::HistoryBusy);
        }
        Ok(HistoryRead(&self.history_loading))
    }

    fn accept(&self, view: PageView) -> Result<PageView, FlowError> {
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
        self.owner.borrow().readable()?;
        self.accept(self.flow().current_page().await?)
    }

    async fn stream(
        &self,
        native: &RunId,
        stream: &mut RunStream,
        emit: &Function,
    ) -> Result<(), FlowError> {
        let mut views = self
            .flow()
            .watch(self.scope.clone(), native.clone())
            .await?;
        while let Some(view) = views.changed().await? {
            if self.emit_view(view, native, stream, emit)? {
                return Ok(());
            }
        }
        Err(FlowError::Disconnected)
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
        for event in stream.push(run).map_err(FlowError::Protocol)? {
            let encoded = bounded_json(&event)?;
            emit.call1(&JsValue::UNDEFINED, &encoded.into())
                .map_err(|_| FlowError::Host("The UI event consumer disconnected.".into()))?;
        }
        Ok(run.status.is_terminal())
    }
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
