use super::*;

pub(super) struct Observer {
    core: Rc<Core>,
    callback: Function,
    closed: Cell<bool>,
    last: Cell<Option<UiPreferenceSnapshot>>,
}

#[wasm_bindgen]
pub struct UiPreferenceSubscription {
    observer: Rc<Observer>,
}

#[wasm_bindgen]
impl UiPreferenceSubscription {
    pub fn close(&self) {
        self.observer.closed.set(true);
    }
}

impl Drop for UiPreferenceSubscription {
    fn drop(&mut self) {
        self.close();
    }
}

impl Observer {
    fn emit(&self) -> Result<(), JsValue> {
        if self.closed.get() || self.core.closed.get() {
            return Ok(());
        }
        let snapshot = self.core.snapshot();
        if self.last.get() == Some(snapshot) {
            return Ok(());
        }
        self.last.set(Some(snapshot));
        self.callback
            .call1(&JsValue::UNDEFINED, &snapshot.into())
            .map(|_| ())
    }
}

#[wasm_bindgen]
impl UiPreferences {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiPreferenceSnapshot) => void")]
        callback: Function,
    ) -> Result<UiPreferenceSubscription, JsValue> {
        self.core.ensure_open()?;
        let observer = Rc::new(Observer {
            core: Rc::clone(&self.core),
            callback,
            closed: Cell::new(false),
            last: Cell::new(None),
        });
        {
            let mut list = self.core.observers.borrow_mut();
            list.retain(|weak| {
                weak.upgrade()
                    .is_some_and(|observer| !observer.closed.get())
            });
            if list.len() >= 4 {
                return Err(err("Preference listener capacity reached."));
            }
            list.push(Rc::downgrade(&observer));
        }
        observer.emit()?;
        Ok(UiPreferenceSubscription { observer })
    }
}

impl Core {
    pub(super) fn notify(&self) {
        if self.closed.get() {
            return;
        }
        let observers: Vec<_> = {
            let mut list = self.observers.borrow_mut();
            list.retain(|weak| {
                weak.upgrade()
                    .is_some_and(|observer| !observer.closed.get())
            });
            list.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for observer in observers {
            if observer.emit().is_err() {
                observer.closed.set(true);
            }
        }
    }
}
