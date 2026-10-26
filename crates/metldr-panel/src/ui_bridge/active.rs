use super::*;
use crate::model::RunView;

#[wasm_bindgen]
pub struct UiActiveRun {
    active: Rc<Active>,
}

struct Active {
    core: Rc<Core>,
    native: RunId,
    question: Option<String>,
    observation: RefCell<Observation>,
    stopping: Cell<bool>,
}

enum Observation {
    Ready,
    Watching(Option<Waker>),
    Detached,
    Terminal,
    Failed,
}

struct Watching(Rc<Active>);

impl Drop for Watching {
    fn drop(&mut self) {
        self.0
            .core
            .owner
            .borrow_mut()
            .observation_finished(&self.0.native);
        self.0.core.page.notify_settings();
    }
}

struct Stopping<'a>(&'a Cell<bool>);

impl Drop for Stopping<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl UiActiveRun {
    pub(super) fn new(core: Rc<Core>, run: &RunView) -> Self {
        Self {
            active: Rc::new(Active {
                core,
                native: run.id.clone(),
                question: run.question.clone(),
                observation: RefCell::new(Observation::Ready),
                stopping: Cell::new(false),
            }),
        }
    }
}

#[wasm_bindgen]
impl UiActiveRun {
    #[wasm_bindgen(getter)]
    pub fn thread_id(&self) -> String {
        self.active.core.scope.conversation.clone()
    }

    pub fn watch(&self, ui_run_id: String, emit: Function) -> Result<Promise, JsValue> {
        self.active.core.ensure_current().map_err(js_error)?;
        if self.active.core.page.recovery.borrow().active() {
            return Err(js_error(FlowError::HistoryBusy));
        }
        if !matches!(*self.active.observation.borrow(), Observation::Ready) {
            return Err(crate::err("Reconnect to observe this run again."));
        }
        let messages = self
            .active
            .question
            .as_ref()
            .map(|question| AgUiMessage::User {
                id: format!("{}:question", self.active.native),
                content: question.clone(),
            })
            .into_iter()
            .collect();
        let stream = RunStream::new(self.thread_id(), ui_run_id, messages)
            .map_err(|error| js_error(FlowError::InvalidInput(error)))?;
        self.active
            .core
            .owner
            .borrow_mut()
            .observe(self.active.native.clone())
            .map_err(js_error)?;
        *self.active.observation.borrow_mut() = Observation::Watching(None);
        self.active.core.page.notify_settings();
        let watching = Watching(Rc::clone(&self.active));
        Ok(future_to_promise(async move {
            let result = watching.0.watch(stream, emit).await;
            if result.is_err() {
                *watching.0.observation.borrow_mut() = Observation::Failed;
            }
            result.map(|()| JsValue::UNDEFINED).map_err(js_error)
        }))
    }

    pub fn detach(&self) {
        let wake = {
            let mut state = self.active.observation.borrow_mut();
            match &mut *state {
                Observation::Ready | Observation::Watching(_) => {
                    match std::mem::replace(&mut *state, Observation::Detached) {
                        Observation::Watching(wake) => wake,
                        _ => None,
                    }
                }
                _ => None,
            }
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    pub fn stop(&self) -> Result<Promise, JsValue> {
        let active = Rc::clone(&self.active);
        active.core.ensure_current().map_err(js_error)?;
        let flow = active.core.flow();
        if active.stopping.replace(true) {
            return Err(js_error(FlowError::HistoryBusy));
        }
        Ok(future_to_promise(async move {
            let _stopping = Stopping(&active.stopping);
            active.core.ensure_current().map_err(js_error)?;
            if active.core.page.recovery.borrow().active()
                || !flow.same_connection(&active.core.flow())
            {
                return Err(js_error(FlowError::HistoryStale));
            }
            match flow
                .request_stop(active.core.scope.clone(), active.native.clone())
                .await
                .map_err(js_error)?
            {
                StopReceipt::Requested => Ok("requested".into()),
                StopReceipt::AlreadyTerminal => Ok("already_terminal".into()),
            }
        }))
    }
}

impl Active {
    async fn watch(&self, mut stream: RunStream, emit: Function) -> Result<(), FlowError> {
        self.core.ensure_current()?;
        let mut observing = std::pin::pin!(self.core.stream(&self.native, &mut stream, &emit));
        std::future::poll_fn(|context| {
            let mut state = self.observation.borrow_mut();
            match &mut *state {
                Observation::Watching(wake) => *wake = Some(context.waker().clone()),
                _ => return Poll::Ready(Ok(())),
            }
            drop(state);
            observing.as_mut().poll(context).map(|result| {
                if result.is_ok() {
                    *self.observation.borrow_mut() = Observation::Terminal;
                }
                result
            })
        })
        .await
    }
}
