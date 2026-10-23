use std::{cell::Cell, rc::Rc};

use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Document, Element, Event};

use crate::{
    browser, dom,
    setup_guide::{COMMANDS, ExtensionOrigin},
};

struct CopyView {
    document: Document,
    origin: ExtensionOrigin,
    busy: Cell<bool>,
    alive: Cell<bool>,
}

impl CopyView {
    fn status(&self, message: &str, busy: bool) -> Result<(), JsValue> {
        self.busy.set(busy);
        dom::text(&self.document, "setup-copy-status", message)?;
        for command in COMMANDS {
            let button = self
                .document
                .query_selector(&format!("[data-copy-command=\"{}\"]", command.id()))?
                .ok_or_else(|| JsValue::from_str("Copy control unavailable"))?;
            if busy {
                button.set_attribute("disabled", "")?;
            } else {
                button.remove_attribute("disabled")?;
            }
        }
        Ok(())
    }
}

pub fn mount(document: &Document, preview: bool) -> Result<(), JsValue> {
    if preview {
        return dom::text(
            document,
            "setup-copy-status",
            "Open the installed extension to copy setup commands for your extension ID.",
        );
    }
    let origin = browser::runtime_id()
        .ok()
        .and_then(|id| ExtensionOrigin::from_id(&id));
    let Some(origin) = origin else {
        return dom::text(
            document,
            "setup-copy-status",
            "The extension ID could not be read. Find it at chrome://extensions and replace YOUR_EXTENSION_ID manually.",
        );
    };
    for command in COMMANDS {
        dom::text(
            document,
            &format!("setup-command-{}", command.id()),
            &command.text(Some(&origin)),
        )?;
    }
    let view = Rc::new(CopyView {
        document: document.clone(),
        origin,
        busy: Cell::new(false),
        alive: Cell::new(true),
    });
    view.status(
        "Select Copy to copy a command. Commands are never run by this page.",
        false,
    )?;
    let copies = view.clone();
    let click = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if !event.is_trusted() || copies.busy.get() || !copies.alive.get() {
            return;
        }
        let button = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .and_then(|target| target.closest("button[data-copy-command]").ok().flatten());
        let Some(button) = button.filter(|button| !button.has_attribute("disabled")) else {
            return;
        };
        let id = button.get_attribute("data-copy-command");
        let Some(command) = COMMANDS
            .into_iter()
            .find(|command| Some(command.id()) == id.as_deref())
        else {
            return;
        };
        let text = command.text(Some(&copies.origin));
        // Start the browser call in the trusted event, before yielding to a future.
        let result = browser::start_clipboard_write(&text);
        let _ = copies.status("Copying command.", true);
        let copies = copies.clone();
        spawn_local(async move {
            let success = match result {
                Ok(promise) => JsFuture::from(promise).await.is_ok(),
                Err(_) => false,
            };
            if copies.alive.get() {
                let message = if success {
                    "Copied command."
                } else {
                    "The command could not be copied. Select it and copy it manually."
                };
                let _ = copies.status(message, false);
            }
        });
    });
    document.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();
    let pagehide = Closure::<dyn FnMut(Event)>::new(move |_| view.alive.set(false));
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("Window unavailable"))?;
    window.add_event_listener_with_callback("pagehide", pagehide.as_ref().unchecked_ref())?;
    pagehide.forget();
    Ok(())
}
