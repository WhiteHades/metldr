use super::*;
use wasm_bindgen_futures::spawn_local;

enum Lifecycle {
    Open(Option<Waker>),
    Closed,
}
enum Next<T> {
    Value(T),
    Replaced,
    Closed,
}
pub(in crate::ui_bridge) struct Observer {
    core: Rc<Core>,
    callback: Function,
    lifecycle: RefCell<Lifecycle>,
    last: RefCell<Option<State>>,
}
#[wasm_bindgen]
pub struct UiCodexSubscription {
    observer: Rc<Observer>,
}
#[wasm_bindgen]
impl UiCodexSubscription {
    pub fn close(&self) {
        self.observer.close();
    }
}
impl Drop for UiCodexSubscription {
    fn drop(&mut self) {
        self.observer.close();
    }
}
impl Observer {
    pub(super) fn subscribe(
        core: Rc<Core>,
        callback: Function,
    ) -> Result<UiCodexSubscription, JsValue> {
        let observer = Rc::new(Self {
            core: Rc::clone(&core),
            callback,
            lifecycle: RefCell::new(Lifecycle::Open(None)),
            last: RefCell::new(None),
        });
        {
            let mut list = core.codex_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed()));
            if list.len() >= 4 {
                return Err(crate::err("Codex input listener capacity reached."));
            }
            list.push(Rc::downgrade(&observer));
        }
        observer.emit().map_err(js_error)?;
        let watch = Rc::clone(&observer);
        spawn_local(async move {
            watch.watch().await;
        });
        Ok(UiCodexSubscription { observer })
    }
    pub(in crate::ui_bridge) fn closed(&self) -> bool {
        matches!(*self.lifecycle.borrow(), Lifecycle::Closed)
    }
    pub(in crate::ui_bridge) fn close(&self) {
        if let Lifecycle::Open(Some(wake)) =
            std::mem::replace(&mut *self.lifecycle.borrow_mut(), Lifecycle::Closed)
        {
            wake.wake();
        }
    }
    pub(in crate::ui_bridge) fn wake(&self) {
        let wake = match &mut *self.lifecycle.borrow_mut() {
            Lifecycle::Open(wake) => wake.take(),
            Lifecycle::Closed => None,
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub(in crate::ui_bridge) fn emit(&self) -> Result<(), FlowError> {
        if self.closed() {
            return Ok(());
        }
        let state = State::read(&self.core);
        if self.last.borrow().as_ref() == Some(&state) {
            return Ok(());
        }
        self.last.replace(Some(state.clone()));
        self.callback
            .call1(&JsValue::UNDEFINED, &UiCodexSnapshot { state }.into())
            .map(|_| ())
            .map_err(|_| FlowError::Host("The Codex input listener disconnected.".into()))
    }
    fn relevant(&self, view: &PageView) -> bool {
        let current = self.core.page.view.borrow();
        view.selection != current.selection
            || view.codex_input != current.codex_input
            || view
                .runs
                .iter()
                .map(|r| (&r.id, &r.status, &r.route))
                .ne(current.runs.iter().map(|r| (&r.id, &r.status, &r.route)))
    }
    async fn wait<F: Future>(&self, flow: &FlowClient, future: F) -> Next<F::Output> {
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(|cx| {
            match &mut *self.lifecycle.borrow_mut() {
                Lifecycle::Closed => return Poll::Ready(Next::Closed),
                Lifecycle::Open(wake) => *wake = Some(cx.waker().clone()),
            }
            if !self.core.flow().same_connection(flow) {
                return Poll::Ready(Next::Replaced);
            }
            future.as_mut().poll(cx).map(Next::Value)
        })
        .await
    }
    async fn replacement(&self, flow: &FlowClient) -> bool {
        matches!(
            self.wait(flow, std::future::pending::<()>()).await,
            Next::Replaced
        )
    }
    async fn watch(self: Rc<Self>) {
        while !self.closed() {
            let flow = self.core.flow();
            let mut views = match flow.observe() {
                Ok(views) => views,
                Err(_) => {
                    self.core.notify_codex_input();
                    if !self.replacement(&flow).await {
                        return;
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
                        if self.core.accept(view).is_err() {
                            self.core.notify_codex_input();
                            return;
                        }
                    }
                    Next::Value(Ok(None) | Err(_)) => {
                        self.core.notify_codex_input();
                        if !self.replacement(&flow).await {
                            return;
                        }
                        break;
                    }
                }
            }
        }
    }
}
