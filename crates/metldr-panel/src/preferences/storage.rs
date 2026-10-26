use super::*;
use js_sys::{Array, Object, Reflect};
use model::{Patch, Values};
use wasm_bindgen::closure::Closure;

pub(super) struct Listener {
    event: JsValue,
    callback: Closure<dyn FnMut(JsValue, JsValue)>,
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

pub(super) fn install(core: &Rc<Core>) -> Result<(), JsValue> {
    if core.chrome.is_none() {
        return Ok(());
    }
    let event = get(&chrome_api("storage")?, "onChanged")?;
    let weak = Rc::downgrade(core);
    let callback = Closure::<dyn FnMut(JsValue, JsValue)>::new(move |changes, area: JsValue| {
        if area.as_string().as_deref() != Some("local") {
            return;
        }
        let Some(core) = weak.upgrade().filter(|core| !core.closed.get()) else {
            return;
        };
        let theme = changed(&changes, "theme").map(|value| parse_theme(&value));
        let size = changed(&changes, "fontSize").map(|value| parse_size(&value));
        if theme.is_none() && size.is_none() {
            return;
        }
        core.appearance.borrow_mut().external(theme, size);
        core.notify();
    });
    call(&event, "addListener", &[callback.as_ref().clone()])?;
    core.chrome_listener
        .replace(Some(Listener { event, callback }));
    Ok(())
}

fn changed(changes: &JsValue, key: &str) -> Option<JsValue> {
    let change = get(changes, key)
        .ok()
        .filter(|value| !value.is_undefined())?;
    Some(get(&change, "newValue").unwrap_or(JsValue::UNDEFINED))
}

fn parse_theme(value: &JsValue) -> Theme {
    value
        .as_string()
        .as_deref()
        .and_then(Theme::parse)
        .unwrap_or_default()
}
fn parse_size(value: &JsValue) -> TextSize {
    value
        .as_string()
        .as_deref()
        .and_then(TextSize::parse)
        .unwrap_or_default()
}

impl Core {
    pub(super) async fn load_appearance(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        let keys = Array::new();
        keys.push(&"theme".into());
        keys.push(&"fontSize".into());
        let result = match &self.chrome {
            Some(storage) => await_call(storage, "get", &[keys.into()]).await,
            None => Err(err("Open the Chrome extension to save appearance.")),
        };
        if self.closed.get() {
            return;
        }
        let values = result.ok().map(|stored| Values {
            theme: parse_theme(&get(&stored, "theme").unwrap_or(JsValue::UNDEFINED)),
            size: parse_size(&get(&stored, "fontSize").unwrap_or(JsValue::UNDEFINED)),
        });
        self.appearance.borrow_mut().loaded(values);
        self.notify();
    }

    pub(super) async fn save_appearance(self: &Rc<Self>, patch: Patch) -> Result<(), JsValue> {
        self.ensure_open()?;
        let result = async {
            let object = Object::new();
            if let Some(theme) = patch.theme {
                Reflect::set(&object, &"theme".into(), &theme.key().into())?;
            }
            if let Some(size) = patch.size {
                Reflect::set(&object, &"fontSize".into(), &size.key().into())?;
            }
            match &self.chrome {
                Some(storage) => await_call(storage, "set", &[object.into()]).await,
                None => Err(err("Open the Chrome extension to save appearance.")),
            }
        }
        .await;
        self.ensure_open()?;
        self.appearance.borrow_mut().saved(result.is_ok());
        self.notify();
        result
            .map(|_| ())
            .map_err(|_| err("Could not confirm appearance was saved. Retry to save it."))
    }
}
