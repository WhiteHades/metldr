use super::*;
use crate::model::{SourceId, SourceStatus};

#[wasm_bindgen]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiSourceStatus {
    Empty,
    Checking,
    Current,
    Changed,
    Saved,
    Unavailable,
}

impl UiSourceStatus {
    fn read(source: bool, connected: bool, fresh: bool, status: SourceStatus) -> Self {
        if !connected {
            Self::Unavailable
        } else if !source {
            Self::Empty
        } else if !fresh {
            Self::Checking
        } else {
            match status {
                SourceStatus::Checking => Self::Checking,
                SourceStatus::Current => Self::Current,
                SourceStatus::Changed => Self::Changed,
                SourceStatus::Saved => Self::Saved,
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct State {
    source: Option<SourceId>,
    title: Option<String>,
    url: Option<String>,
    status: UiSourceStatus,
}

impl State {
    fn read(core: &Core) -> Self {
        let view = core.page.view.borrow();
        let source = core
            .ensure_current()
            .is_ok()
            .then_some(view.source.as_ref())
            .flatten();
        let flow = core.flow();
        Self {
            source: source.map(|source| source.id.clone()),
            title: source.map(|source| source.title.clone()),
            url: source.map(|source| source.url.clone()),
            status: UiSourceStatus::read(
                source.is_some(),
                core.ensure_current().is_ok() && flow.is_connected(),
                view.source_epoch == flow.source_epoch(),
                view.source_status,
            ),
        }
    }
}

#[wasm_bindgen]
pub struct UiSavedSource {
    core: Rc<Core>,
}

#[wasm_bindgen]
pub struct UiSourceSnapshot {
    core: Rc<Core>,
    state: State,
}

#[wasm_bindgen]
impl UiSourceSnapshot {
    #[wasm_bindgen(getter)]
    pub fn source_id(&self) -> Option<String> {
        self.state.source.as_ref().map(ToString::to_string)
    }
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> Option<String> {
        self.state.title.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn url(&self) -> Option<String> {
        self.state.url.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> UiSourceStatus {
        self.state.status
    }

    pub fn text(&self) -> Result<Option<String>, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        let view = self.core.cached().map_err(js_error)?;
        if view.source.as_ref().map(|source| &source.id) != self.state.source.as_ref() {
            return Err(js_error(FlowError::HistoryStale));
        }
        Ok(view.source.as_ref().map(|source| source.text.clone()))
    }
}

#[wasm_bindgen]
impl UiBridge {
    pub fn saved_source(&self) -> Result<UiSavedSource, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        Ok(UiSavedSource {
            core: Rc::clone(&self.core),
        })
    }
}

#[wasm_bindgen]
impl UiSavedSource {
    pub fn snapshot(&self) -> Result<UiSourceSnapshot, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        Ok(UiSourceSnapshot {
            core: Rc::clone(&self.core),
            state: State::read(&self.core),
        })
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn refresh(&self) -> Result<Promise, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let _read = core.begin_read().map_err(js_error)?;
            core.current().await.map_err(js_error)?;
            Ok(JsValue::UNDEFINED)
        }))
    }

    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiSourceSnapshot) => void")]
        callback: Function,
    ) -> Result<UiSourceSubscription, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        let observer = Rc::new(Observer {
            core: Rc::clone(&self.core),
            callback,
            closed: Cell::new(false),
            last: RefCell::new(None),
        });
        {
            let mut list = self.core.page.source_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed.get()));
            if list.len() >= 4 {
                return Err(crate::err("Source listener capacity reached."));
            }
            list.push(Rc::downgrade(&observer));
        }
        observer.emit().map_err(js_error)?;
        self.core.page.refresh_action_source();
        Ok(UiSourceSubscription { observer })
    }
}

pub(super) struct Observer {
    core: Rc<Core>,
    callback: Function,
    closed: Cell<bool>,
    last: RefCell<Option<State>>,
}

#[wasm_bindgen]
pub struct UiSourceSubscription {
    observer: Rc<Observer>,
}

#[wasm_bindgen]
impl UiSourceSubscription {
    pub fn close(&self) {
        self.observer.closed.set(true);
    }
}

impl Drop for UiSourceSubscription {
    fn drop(&mut self) {
        self.close();
    }
}

impl Observer {
    fn emit(&self) -> Result<(), FlowError> {
        if self.closed.get() {
            return Ok(());
        }
        let state = State::read(&self.core);
        if self.last.borrow().as_ref() == Some(&state) {
            return Ok(());
        }
        self.last.replace(Some(state.clone()));
        let snapshot = UiSourceSnapshot {
            core: Rc::clone(&self.core),
            state,
        };
        self.callback
            .call1(&JsValue::UNDEFINED, &snapshot.into())
            .map_err(|_| FlowError::Host("The source listener disconnected.".into()))?;
        Ok(())
    }
}

impl PageSession {
    pub(super) fn has_source_observers(&self) -> bool {
        let mut list = self.source_subscriptions.borrow_mut();
        list.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed.get()));
        !list.is_empty()
    }

    pub(super) fn notify_source(&self) {
        let observers: Vec<_> = self
            .source_subscriptions
            .borrow()
            .iter()
            .filter_map(std::rc::Weak::upgrade)
            .collect();
        for observer in observers {
            if observer.emit().is_err() {
                observer.closed.set(true);
            }
        }
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn invalidated_source_never_claims_current() {
        assert!(matches!(
            UiSourceStatus::read(true, true, false, SourceStatus::Current),
            UiSourceStatus::Checking
        ));
        assert!(matches!(
            UiSourceStatus::read(true, false, true, SourceStatus::Current),
            UiSourceStatus::Unavailable
        ));
        assert!(matches!(
            UiSourceStatus::read(false, true, true, SourceStatus::Current),
            UiSourceStatus::Empty
        ));
        assert!(matches!(
            UiSourceStatus::read(true, true, true, SourceStatus::Current),
            UiSourceStatus::Current
        ));
    }
}
