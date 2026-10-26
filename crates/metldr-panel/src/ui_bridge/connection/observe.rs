use super::*;
use wasm_bindgen_futures::spawn_local;

pub(in crate::ui_bridge) struct Observer {
    core: Rc<Core>,
    callback: Function,
    closed: Cell<bool>,
    last: RefCell<Option<UiConnectionSnapshot>>,
}

#[wasm_bindgen]
pub struct UiConnectionSubscription {
    observer: Rc<Observer>,
}

#[wasm_bindgen]
impl UiConnectionSubscription {
    pub fn close(&self) {
        self.observer.close();
    }
}

impl Drop for UiConnectionSubscription {
    fn drop(&mut self) {
        self.observer.close();
    }
}

#[wasm_bindgen]
impl UiConnection {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiConnectionSnapshot) => void")]
        callback: Function,
    ) -> Result<UiConnectionSubscription, JsValue> {
        let observer = Rc::new(Observer {
            core: Rc::clone(&self.core),
            callback,
            closed: Cell::new(false),
            last: RefCell::new(None),
        });
        {
            let mut list = self.core.page.connection_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed.get()));
            if list.len() >= 4 {
                return Err(crate::err("Connection listener capacity reached."));
            }
            list.push(Rc::downgrade(&observer));
        }
        observer.emit().map_err(js_error)?;
        Ok(UiConnectionSubscription { observer })
    }
}

impl Observer {
    fn close(&self) {
        self.closed.set(true);
    }

    fn emit(&self) -> Result<(), FlowError> {
        if self.closed.get() {
            return Ok(());
        }
        let state = UiConnectionSnapshot::read(&self.core);
        if self.last.borrow().as_ref() == Some(&state) {
            return Ok(());
        }
        self.last.replace(Some(state.clone()));
        self.callback
            .call1(&JsValue::UNDEFINED, &state.into())
            .map_err(|_| FlowError::Host("The connection listener disconnected.".into()))?;
        Ok(())
    }
}

impl PageSession {
    pub(in crate::ui_bridge) fn install_connection_updates(
        self: &Rc<Self>,
        flow: FlowClient,
        mut views: crate::flow::Views,
    ) {
        let wake = self.connection_waiter.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
        let weak = Rc::downgrade(self);
        spawn_local(async move {
            loop {
                let next = {
                    let mut changed = std::pin::pin!(views.changed());
                    std::future::poll_fn(|context| {
                        let Some(page) = weak.upgrade() else {
                            return Poll::Ready(None);
                        };
                        if !flow.same_connection(&page.flow.borrow()) {
                            return Poll::Ready(None);
                        }
                        page.connection_waiter
                            .replace(Some(context.waker().clone()));
                        drop(page);
                        changed.as_mut().poll(context).map(Some)
                    })
                    .await
                };
                let Some(page) = weak.upgrade() else {
                    break;
                };
                if !flow.same_connection(&page.flow.borrow()) {
                    break;
                }
                match next {
                    Some(Ok(Some(view))) => {
                        if page.accept(view).is_err() {
                            break;
                        }
                    }
                    Some(Err(_)) => {
                        page.notify_settings();
                        if flow.is_closed() {
                            break;
                        }
                    }
                    None | Some(Ok(None)) => break,
                }
            }
        });
    }

    pub(in crate::ui_bridge) fn notify_connection(&self) {
        let observers: Vec<_> = {
            let mut list = self.connection_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|item| !item.closed.get()));
            list.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for observer in observers {
            if observer.emit().is_err() {
                observer.close();
            }
        }
        if let Some(core) = self.current.borrow().upgrade() {
            let wake = core.stream_waiter.borrow_mut().take();
            if let Some(wake) = wake {
                wake.wake();
            }
        }
    }
}

impl Drop for PageSession {
    fn drop(&mut self) {
        if let Some(wake) = self.connection_waiter.get_mut().take() {
            wake.wake();
        }
    }
}
