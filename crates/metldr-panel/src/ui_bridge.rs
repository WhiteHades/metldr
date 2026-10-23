use std::{
    cell::RefCell,
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
    model::{FlowError, PageView, RunId, UserText},
};

mod history;
mod owner;
use owner::{Owner, Stop};

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
    flow: FlowClient,
    scope: Scope,
    view: RefCell<PageView>,
    owner: RefCell<Owner>,
    preparing: RefCell<Option<Waker>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    thread_id: String,
    messages: Vec<AgUiMessage>,
    running: bool,
    source_available: bool,
}

#[wasm_bindgen]
pub async fn connect_ui_bridge(capture: Function) -> Result<UiBridge, JsValue> {
    crate::flow::install_capture(capture);
    let flow = FlowClient::connect().await.map_err(js_error)?;
    let view = flow.current_page().await.map_err(js_error)?;
    Ok(UiBridge {
        core: Rc::new(Core {
            scope: view.selection.clone(),
            flow,
            view: RefCell::new(view),
            owner: RefCell::new(Owner::Idle),
            preparing: RefCell::new(None),
        }),
    })
}

#[wasm_bindgen]
impl UiBridge {
    #[wasm_bindgen(getter)]
    pub fn thread_id(&self) -> String {
        self.core.scope.conversation.clone()
    }

    pub async fn snapshot(&self) -> Result<String, JsValue> {
        let view = self.core.current().await.map_err(js_error)?;
        let snapshot = Snapshot {
            thread_id: self.thread_id(),
            messages: history::messages(&self.core.flow, &self.core.scope)
                .await
                .map_err(js_error)?,
            running: view.runs.iter().any(|run| !run.status.is_terminal()),
            source_available: view.source().is_some(),
        };
        bounded_json(&snapshot).map_err(js_error)
    }

    pub fn capture(&self) -> Result<Promise, JsValue> {
        self.core.owner.borrow_mut().capture().map_err(js_error)?;
        // Prepare the permission request in the user's click, before yielding.
        let capture = self.core.flow.capture_selected(&self.core.view.borrow());
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let result = capture.await.and_then(|view| core.accept(view));
            core.owner.borrow_mut().capture_finished();
            result.map(|_| JsValue::UNDEFINED).map_err(js_error)
        }))
    }

    pub async fn run(&self, request: UiRunRequest, emit: Function) -> Result<(), JsValue> {
        if request.thread != self.core.scope.conversation {
            return Err(crate::err(
                "The UI request belongs to another conversation.",
            ));
        }
        self.core
            .owner
            .borrow_mut()
            .begin(&request.id)
            .map_err(js_error)?;
        let prepared = async {
            let view = self.core.current().await?;
            if view.runs.iter().any(|run| !run.status.is_terminal()) {
                return Err(FlowError::HistoryBusy);
            }
            let source = view.source().cloned().ok_or(FlowError::NoSource)?;
            let destination =
                composer::resolve(&view, &request.destination).map_err(FlowError::InvalidInput)?;
            let Some(mut messages) = self.core.prepare_history().await? else {
                return Ok(None);
            };
            messages.push(AgUiMessage {
                id: request.message.clone(),
                role: MessageRole::User,
                content: request.text.as_str().into(),
            });
            let stream = RunStream::new(request.thread, request.id.clone(), messages)
                .map_err(FlowError::InvalidInput)?;
            Ok::<_, FlowError>(Some((source, destination, stream)))
        }
        .await;
        let (source, destination, mut stream) = match prepared {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.core.owner.borrow_mut().not_sent();
                return Ok(());
            }
            Err(error) => {
                self.core.owner.borrow_mut().not_sent();
                return Err(js_error(error));
            }
        };
        let started = composer::dispatch(
            &self.core.flow,
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
                return Err(js_error(error));
            }
        };
        let stop = self.core.owner.borrow_mut().accepted(native.clone());
        if stop {
            if let Err(error) = self
                .core
                .flow
                .request_stop(self.core.scope.clone(), native.clone())
                .await
            {
                self.core.owner.borrow_mut().uncertain();
                return Err(js_error(error));
            }
        }
        let result = self.core.stream(&native, &mut stream, &emit).await;
        match result {
            Ok(()) => self.core.owner.borrow_mut().finished(&native),
            Err(error) => {
                // Losing a listener is not evidence that native work stopped.
                let _ = self
                    .core
                    .flow
                    .request_stop(self.core.scope.clone(), native)
                    .await;
                self.core.owner.borrow_mut().uncertain();
                return Err(js_error(error));
            }
        }
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
                    .flow
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
    async fn prepare_history(&self) -> Result<Option<Vec<AgUiMessage>>, FlowError> {
        let mut history = std::pin::pin!(history::messages(&self.flow, &self.scope));
        let result = std::future::poll_fn(|context| {
            if self.owner.borrow().stop_pending() {
                return Poll::Ready(Ok(None));
            }
            self.preparing.replace(Some(context.waker().clone()));
            history
                .as_mut()
                .poll(context)
                .map(|result| result.map(Some))
        })
        .await;
        self.preparing.borrow_mut().take();
        result
    }

    fn accept(&self, view: PageView) -> Result<PageView, FlowError> {
        if view.selection != self.scope {
            return Err(FlowError::HistoryStale);
        }
        self.view.replace(view.clone());
        Ok(view)
    }

    async fn current(&self) -> Result<PageView, FlowError> {
        self.accept(self.flow.current_page().await?)
    }

    async fn stream(
        &self,
        native: &RunId,
        stream: &mut RunStream,
        emit: &Function,
    ) -> Result<(), FlowError> {
        stream
            .accept_question(native)
            .map_err(FlowError::Protocol)?;
        let mut views = self.flow.watch(self.scope.clone(), native.clone()).await?;
        while let Some(view) = views.changed().await? {
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
            if run.status.is_terminal() {
                return Ok(());
            }
        }
        Err(FlowError::Disconnected)
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
