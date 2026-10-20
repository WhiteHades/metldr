use std::{cell::RefCell, rc::Rc};

use js_sys::{Object, Reflect};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, Element, Event, FocusOptions, HtmlElement, HtmlSelectElement};

use crate::{await_call, call, chrome_api, err, get};

const KEYS: [&str; 2] = ["theme", "fontSize"];
const CONTROLS: [&str; 2] = ["appearance-theme", "appearance-size"];
const VALUES: [&[&str]; 2] = [
    &["catppuccin", "gruvbox", "cyberpunk", "light"],
    &["medium", "small", "large"],
];

#[derive(Clone, Copy)]
enum Status {
    Loading,
    Saving,
    Saved,
    LoadFailed,
    SaveFailed,
}

struct Appearance {
    document: Document,
    storage: JsValue,
    values: [&'static str; 2],
    observed: [bool; 2],
    dirty: [bool; 2],
    status: Status,
}

pub(super) fn install(document: &Document) -> Result<(), JsValue> {
    let storage = chrome_api("storage")?;
    let state = Rc::new(RefCell::new(Appearance {
        document: document.clone(),
        storage: get(&storage, "local")?,
        values: [VALUES[0][0], VALUES[1][0]],
        observed: [false; 2],
        dirty: [false; 2],
        status: Status::Loading,
    }));
    state.borrow().render()?;
    for (index, id) in CONTROLS.iter().enumerate() {
        let state = Rc::clone(&state);
        let change = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            let Some(select) = event
                .target()
                .and_then(|target| target.dyn_into::<HtmlSelectElement>().ok())
            else {
                return;
            };
            let mut appearance = state.borrow_mut();
            if appearance.busy() {
                let _ = appearance.render();
                return;
            }
            let Some(value) = parse(index, &JsValue::from_str(&select.value())) else {
                let _ = appearance.render();
                return;
            };
            appearance.values[index] = value;
            appearance.dirty[index] = true;
            drop(appearance);
            save(Rc::clone(&state));
        });
        document
            .get_element_by_id(id)
            .ok_or_else(|| err("Appearance control missing."))?
            .add_event_listener_with_callback("change", change.as_ref().unchecked_ref())?;
        change.forget();
    }
    let retry_state = Rc::clone(&state);
    let retry = Closure::<dyn FnMut(Event)>::new(move |_| {
        let appearance = retry_state.borrow();
        if appearance.busy() {
            return;
        }
        let dirty = appearance.dirty.iter().any(|dirty| *dirty);
        drop(appearance);
        if dirty {
            save(Rc::clone(&retry_state));
        } else {
            load(Rc::clone(&retry_state));
        }
    });
    document
        .get_element_by_id("appearance-retry")
        .ok_or_else(|| err("Appearance retry missing."))?
        .add_event_listener_with_callback("click", retry.as_ref().unchecked_ref())?;
    retry.forget();

    let changed_state = Rc::clone(&state);
    let changed = Closure::<dyn FnMut(JsValue, JsValue)>::new(move |changes, area: JsValue| {
        if area.as_string().as_deref() != Some("local") {
            return;
        }
        let mut appearance = changed_state.borrow_mut();
        for (index, key) in KEYS.iter().enumerate() {
            let Ok(change) = get(&changes, key) else {
                continue;
            };
            if change.is_undefined() {
                continue;
            }
            let raw = get(&change, "newValue").unwrap_or(JsValue::UNDEFINED);
            appearance.values[index] = parse(index, &raw).unwrap_or(VALUES[index][0]);
            appearance.observed[index] = true;
            if !matches!(appearance.status, Status::Saving) {
                appearance.dirty[index] = false;
            }
        }
        if matches!(appearance.status, Status::SaveFailed)
            && !appearance.dirty.iter().any(|dirty| *dirty)
        {
            appearance.status = Status::Saved;
        }
        if let Err(error) = appearance.render() {
            web_sys::console::error_1(&error);
        }
    });
    call(
        &get(&storage, "onChanged")?,
        "addListener",
        &[changed.as_ref().clone()],
    )?;
    changed.forget();
    load(state);
    Ok(())
}

fn parse(index: usize, raw: &JsValue) -> Option<&'static str> {
    let raw = raw.as_string()?;
    VALUES[index].iter().copied().find(|value| *value == raw)
}

impl Appearance {
    fn park_focus(&self) -> Option<Element> {
        let restore_focus = self
            .document
            .active_element()
            .filter(|element| {
                CONTROLS.contains(&element.id().as_str()) || element.id() == "appearance-retry"
            })
            .and_then(|element| {
                if element.id() == "appearance-retry" {
                    let index = self.dirty.iter().position(|dirty| *dirty).unwrap_or(0);
                    self.document.get_element_by_id(CONTROLS[index])
                } else {
                    Some(element)
                }
            });
        if restore_focus.is_some() {
            if let Some(status) = self.document.get_element_by_id("appearance-status") {
                if let Some(status) = status.dyn_ref::<HtmlElement>() {
                    let options = FocusOptions::new();
                    options.set_prevent_scroll(true);
                    let _ = status.focus_with_options(&options);
                }
            }
        }
        restore_focus
    }

    fn busy(&self) -> bool {
        matches!(self.status, Status::Loading | Status::Saving)
    }

    fn render(&self) -> Result<(), JsValue> {
        let root = self
            .document
            .document_element()
            .ok_or_else(|| err("Root missing."))?;
        root.set_attribute("data-theme", self.values[0])?;
        if self.values[1] == "medium" {
            root.remove_attribute("data-font-size")?;
        } else {
            root.set_attribute("data-font-size", self.values[1])?;
        }
        for (index, id) in CONTROLS.iter().enumerate() {
            let select = self
                .document
                .get_element_by_id(id)
                .ok_or_else(|| err("Appearance control missing."))?
                .dyn_into::<HtmlSelectElement>()?;
            select.set_value(self.values[index]);
            select.set_disabled(self.busy());
        }
        let message = match self.status {
            Status::Loading => "Loading appearance.",
            Status::Saving => "Saving appearance.",
            Status::Saved => "Appearance saved on this device.",
            Status::LoadFailed => "Couldn't load appearance. Choose a setting or retry.",
            Status::SaveFailed => {
                "Couldn't confirm the save. Your preview is still shown. Retry to save it."
            }
        };
        self.document
            .get_element_by_id("appearance-status")
            .ok_or_else(|| err("Appearance status missing."))?
            .set_text_content(Some(message));
        let retry = self
            .document
            .get_element_by_id("appearance-retry")
            .ok_or_else(|| err("Appearance retry missing."))?;
        if matches!(self.status, Status::LoadFailed | Status::SaveFailed) {
            retry.remove_attribute("hidden")?;
        } else {
            retry.set_attribute("hidden", "")?;
        }
        Ok(())
    }
}

fn load(state: Rc<RefCell<Appearance>>) {
    let (storage, restore_focus) = {
        let mut appearance = state.borrow_mut();
        let restore_focus = appearance.park_focus();
        appearance.status = Status::Loading;
        appearance.observed = [false; 2];
        let _ = appearance.render();
        (appearance.storage.clone(), restore_focus)
    };
    spawn_local(async move {
        let keys = js_sys::Array::new();
        for key in KEYS {
            keys.push(&JsValue::from_str(key));
        }
        let result = await_call(&storage, "get", &[keys.into()]).await;
        let mut appearance = state.borrow_mut();
        match result {
            Ok(stored) => {
                for (index, key) in KEYS.iter().enumerate() {
                    if !appearance.observed[index] {
                        let raw = get(&stored, key).unwrap_or(JsValue::UNDEFINED);
                        appearance.values[index] = parse(index, &raw).unwrap_or(VALUES[index][0]);
                    }
                }
                appearance.status = Status::Saved;
            }
            Err(_) => appearance.status = Status::LoadFailed,
        }
        if let Err(error) = appearance.render() {
            web_sys::console::error_1(&error);
        }
        restore_after_storage(&appearance.document, restore_focus);
    });
}

fn save(state: Rc<RefCell<Appearance>>) {
    let (storage, patch, restore_focus) = {
        let mut appearance = state.borrow_mut();
        let patch = Object::new();
        for (index, key) in KEYS.iter().enumerate() {
            if appearance.dirty[index] {
                if let Err(error) = Reflect::set(
                    &patch,
                    &JsValue::from_str(key),
                    &JsValue::from_str(appearance.values[index]),
                ) {
                    web_sys::console::error_1(&error);
                    return;
                }
            }
        }
        let restore_focus = appearance.park_focus();
        appearance.status = Status::Saving;
        let _ = appearance.render();
        (appearance.storage.clone(), patch, restore_focus)
    };
    spawn_local(async move {
        let result = await_call(&storage, "set", &[patch.into()]).await;
        let mut appearance = state.borrow_mut();
        match result {
            Ok(_) => {
                appearance.dirty = [false; 2];
                appearance.status = Status::Saved;
            }
            Err(_) => appearance.status = Status::SaveFailed,
        }
        if let Err(error) = appearance.render() {
            web_sys::console::error_1(&error);
        }
        restore_after_storage(&appearance.document, restore_focus);
    });
}

fn restore_after_storage(document: &Document, target: Option<Element>) {
    let status_focused = document
        .active_element()
        .is_some_and(|element| element.id() == "appearance-status");
    let open = document
        .get_element_by_id("appearance-settings")
        .is_some_and(|popover| popover.matches(":popover-open").unwrap_or(false));
    if status_focused && open {
        if let Some(target) = target.and_then(|element| element.dyn_into::<HtmlElement>().ok()) {
            let _ = target.focus();
        }
    }
}
