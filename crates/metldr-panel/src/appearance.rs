use std::{cell::RefCell, rc::Rc};

use js_sys::Function;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Document, Element, Event, FocusOptions, HtmlElement, HtmlSelectElement, PageTransitionEvent,
};

use crate::err;
use crate::preferences::{
    UiPreferenceSnapshot, UiPreferenceStatus, UiPreferenceSubscription, UiPreferences,
};

const CONTROLS: [&str; 2] = ["appearance-theme", "appearance-size"];

thread_local! {
    static ADAPTER: RefCell<Option<Adapter>> = const { RefCell::new(None) };
}

struct Adapter {
    state: Rc<RefCell<Appearance>>,
    subscription: UiPreferenceSubscription,
    _subscription_callback: Closure<dyn FnMut(UiPreferenceSnapshot)>,
    changes: Vec<Closure<dyn FnMut(Event)>>,
    retry: Closure<dyn FnMut(Event)>,
    pagehide: Closure<dyn FnMut(Event)>,
}

impl Drop for Adapter {
    fn drop(&mut self) {
        self.subscription.close();
        for (id, callback) in CONTROLS.iter().zip(&self.changes) {
            if let Some(control) = self.state.borrow().document.get_element_by_id(id) {
                let _ = control.remove_event_listener_with_callback(
                    "change",
                    callback.as_ref().unchecked_ref(),
                );
            }
        }
        if let Some(retry) = self
            .state
            .borrow()
            .document
            .get_element_by_id("appearance-retry")
        {
            let _ = retry
                .remove_event_listener_with_callback("click", self.retry.as_ref().unchecked_ref());
        }
        if let Some(window) = self.state.borrow().document.default_view() {
            let _ = window.remove_event_listener_with_callback(
                "pagehide",
                self.pagehide.as_ref().unchecked_ref(),
            );
        }
    }
}

struct Appearance {
    document: Document,
    preferences: Rc<UiPreferences>,
    snapshot: UiPreferenceSnapshot,
    restore_focus: Option<Element>,
    last_changed: usize,
}

pub(super) fn install(document: &Document, preferences: Rc<UiPreferences>) -> Result<(), JsValue> {
    if ADAPTER.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let state = Rc::new(RefCell::new(Appearance {
        document: document.clone(),
        snapshot: preferences.snapshot()?,
        preferences,
        restore_focus: None,
        last_changed: 0,
    }));
    render(&state.borrow())?;
    let window = document
        .default_view()
        .ok_or_else(|| err("document has no default window"))?;
    let controls = CONTROLS
        .iter()
        .map(|id| {
            document
                .get_element_by_id(id)
                .ok_or_else(|| err("Appearance control missing."))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let retry_element = document
        .get_element_by_id("appearance-retry")
        .ok_or_else(|| err("Appearance retry missing."))?;

    let weak = Rc::downgrade(&state);
    let subscription_callback = Closure::<dyn FnMut(UiPreferenceSnapshot)>::new(move |snapshot| {
        if let Some(state) = weak.upgrade() {
            let mut appearance = state.borrow_mut();
            appearance.snapshot = snapshot;
            if let Err(error) = render(&appearance) {
                web_sys::console::error_1(&error);
            }
            restore_after_storage(&mut appearance);
        }
    });
    let preferences = Rc::clone(&state.borrow().preferences);
    let subscription = preferences.subscribe(
        subscription_callback
            .as_ref()
            .unchecked_ref::<Function>()
            .clone(),
    )?;

    let mut changes: Vec<Closure<dyn FnMut(Event)>> = Vec::new();
    for (index, control) in controls.iter().enumerate() {
        let weak = Rc::downgrade(&state);
        let change = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            let Some(select) = event
                .target()
                .and_then(|target| target.dyn_into::<HtmlSelectElement>().ok())
            else {
                return;
            };
            let Some(state) = weak.upgrade() else { return };
            let mut appearance = state.borrow_mut();
            if busy(appearance.snapshot) {
                let _ = render(&appearance);
                return;
            }
            appearance.last_changed = index;
            park_focus(&mut appearance);
            let preferences = Rc::clone(&appearance.preferences);
            drop(appearance);
            let result = if index == 0 {
                preferences.set_theme(select.value())
            } else {
                preferences.set_font_size(select.value())
            };
            settle(result);
        });
        if let Err(error) =
            control.add_event_listener_with_callback("change", change.as_ref().unchecked_ref())
        {
            for (id, callback) in CONTROLS.iter().zip(&changes) {
                if let Some(control) = document.get_element_by_id(id) {
                    let _ = control.remove_event_listener_with_callback(
                        "change",
                        callback.as_ref().unchecked_ref(),
                    );
                }
            }
            return Err(error);
        }
        changes.push(change);
    }

    let weak = Rc::downgrade(&state);
    let retry = Closure::<dyn FnMut(Event)>::new(move |_| {
        let Some(state) = weak.upgrade() else { return };
        let mut appearance = state.borrow_mut();
        if busy(appearance.snapshot) {
            return;
        }
        park_focus(&mut appearance);
        let preferences = Rc::clone(&appearance.preferences);
        drop(appearance);
        settle(preferences.retry_appearance());
    });
    if let Err(error) =
        retry_element.add_event_listener_with_callback("click", retry.as_ref().unchecked_ref())
    {
        for (id, callback) in CONTROLS.iter().zip(&changes) {
            if let Some(control) = document.get_element_by_id(id) {
                let _ = control.remove_event_listener_with_callback(
                    "change",
                    callback.as_ref().unchecked_ref(),
                );
            }
        }
        return Err(error);
    }
    let mut cleanup_pending = false;
    let cleanup_window = window.clone();
    let pagehide = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if event
            .dyn_ref::<PageTransitionEvent>()
            .is_some_and(PageTransitionEvent::persisted)
            || cleanup_pending
        {
            return;
        }
        cleanup_pending = true;
        let cleanup = Closure::once_into_js(|| ADAPTER.with(|slot| drop(slot.borrow_mut().take())));
        cleanup_window.queue_microtask(cleanup.unchecked_ref::<Function>());
    });
    if let Err(error) =
        window.add_event_listener_with_callback("pagehide", pagehide.as_ref().unchecked_ref())
    {
        let _ = retry_element
            .remove_event_listener_with_callback("click", retry.as_ref().unchecked_ref());
        for (id, callback) in CONTROLS.iter().zip(&changes) {
            if let Some(control) = document.get_element_by_id(id) {
                let _ = control.remove_event_listener_with_callback(
                    "change",
                    callback.as_ref().unchecked_ref(),
                );
            }
        }
        return Err(error);
    }
    ADAPTER.with(|slot| {
        *slot.borrow_mut() = Some(Adapter {
            state,
            subscription,
            _subscription_callback: subscription_callback,
            changes,
            retry,
            pagehide,
        })
    });
    Ok(())
}

fn settle(result: Result<js_sys::Promise, JsValue>) {
    match result {
        Ok(promise) => spawn_local(async move {
            if let Err(error) = JsFuture::from(promise).await {
                web_sys::console::error_1(&error);
            }
        }),
        Err(error) => web_sys::console::error_1(&error),
    }
}

fn busy(snapshot: UiPreferenceSnapshot) -> bool {
    matches!(
        snapshot.appearance_status(),
        UiPreferenceStatus::Loading | UiPreferenceStatus::Saving
    )
}

fn park_focus(appearance: &mut Appearance) {
    appearance.restore_focus = appearance
        .document
        .active_element()
        .filter(|element| {
            CONTROLS.contains(&element.id().as_str()) || element.id() == "appearance-retry"
        })
        .and_then(|element| {
            if element.id() == "appearance-retry" {
                appearance
                    .document
                    .get_element_by_id(CONTROLS[appearance.last_changed])
            } else {
                Some(element)
            }
        });
    if appearance.restore_focus.is_some() {
        if let Some(status) = appearance
            .document
            .get_element_by_id("appearance-status")
            .and_then(|e| e.dyn_into::<HtmlElement>().ok())
        {
            let options = FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = status.focus_with_options(&options);
        }
    }
}

fn render(appearance: &Appearance) -> Result<(), JsValue> {
    let root = appearance
        .document
        .document_element()
        .ok_or_else(|| err("Root missing."))?;
    root.set_attribute("data-theme", &appearance.snapshot.theme())?;
    let size = appearance.snapshot.font_size();
    if size == "medium" {
        root.remove_attribute("data-font-size")?;
    } else {
        root.set_attribute("data-font-size", &size)?;
    }
    for (id, value) in CONTROLS.iter().zip([appearance.snapshot.theme(), size]) {
        let select = appearance
            .document
            .get_element_by_id(id)
            .ok_or_else(|| err("Appearance control missing."))?
            .dyn_into::<HtmlSelectElement>()?;
        select.set_value(&value);
        select.set_disabled(busy(appearance.snapshot));
    }
    let message = match appearance.snapshot.appearance_status() {
        UiPreferenceStatus::Loading => "Loading appearance.",
        UiPreferenceStatus::Saving => "Saving appearance.",
        UiPreferenceStatus::Saved => "Appearance saved on this device.",
        UiPreferenceStatus::LoadFailed => "Couldn't load appearance. Choose a setting or retry.",
        UiPreferenceStatus::SaveFailed => {
            "Couldn't confirm the save. Your preview is still shown. Retry to save it."
        }
    };
    appearance
        .document
        .get_element_by_id("appearance-status")
        .ok_or_else(|| err("Appearance status missing."))?
        .set_text_content(Some(message));
    let retry = appearance
        .document
        .get_element_by_id("appearance-retry")
        .ok_or_else(|| err("Appearance retry missing."))?;
    if matches!(
        appearance.snapshot.appearance_status(),
        UiPreferenceStatus::LoadFailed | UiPreferenceStatus::SaveFailed
    ) {
        retry.remove_attribute("hidden")?;
    } else {
        retry.set_attribute("hidden", "")?;
    }
    Ok(())
}

fn restore_after_storage(appearance: &mut Appearance) {
    if busy(appearance.snapshot) {
        return;
    }
    let status_focused = appearance
        .document
        .active_element()
        .is_some_and(|element| element.id() == "appearance-status");
    let open = appearance
        .document
        .get_element_by_id("appearance-settings")
        .is_some_and(|popover| popover.matches(":popover-open").unwrap_or(false));
    let target = appearance.restore_focus.take();
    if status_focused && open {
        if let Some(target) = target.and_then(|element| element.dyn_into::<HtmlElement>().ok()) {
            let _ = target.focus();
        }
    }
}
