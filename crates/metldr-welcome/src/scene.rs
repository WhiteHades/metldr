use std::{cell::Cell, rc::Rc};

use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{Document, Event, IntersectionObserver, IntersectionObserverEntry};

use crate::dom;

pub fn mount(document: &Document) -> Result<(), JsValue> {
    let scene = dom::element(document, "scene")?;
    let visible = Rc::new(Cell::new(false));
    let update = {
        let scene = scene.clone();
        let document = document.clone();
        let visible = visible.clone();
        Rc::new(move || {
            scene.set_attribute(
                "data-active",
                if visible.get() && !document.hidden() {
                    "true"
                } else {
                    "false"
                },
            )
        })
    };
    let intersection_update = update.clone();
    let intersect = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
        for entry in entries
            .iter()
            .filter_map(|entry| entry.dyn_into::<IntersectionObserverEntry>().ok())
        {
            visible.set(entry.is_intersecting());
        }
        let _ = intersection_update();
    });
    let observer = IntersectionObserver::new(intersect.as_ref().unchecked_ref())?;
    observer.observe(&scene);
    intersect.forget();
    let visibility = Closure::<dyn FnMut(Event)>::new(move |_| {
        let _ = update();
    });
    document.add_event_listener_with_callback(
        "visibilitychange",
        visibility.as_ref().unchecked_ref(),
    )?;
    visibility.forget();
    let leaving = Closure::<dyn FnMut(Event)>::new(move |_| observer.disconnect());
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("Window unavailable"))?;
    window.add_event_listener_with_callback("pagehide", leaving.as_ref().unchecked_ref())?;
    leaving.forget();
    Ok(())
}
