mod menu;
mod token;

use crate::{Panel, active_run, err};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use token::{Kind, Token};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Document, Element, Event, HtmlSelectElement, HtmlTextAreaElement, KeyboardEvent};

#[derive(Default)]
pub(super) struct State {
    restore_choice: RefCell<Option<String>>,
    remembered_choice: RefCell<Option<String>>,
    plus: Cell<bool>,
    models: Cell<bool>,
    dismissed: Cell<bool>,
    active: Cell<usize>,
    engaged: Cell<bool>,
    model_active: Cell<Option<usize>>,
    query: RefCell<Option<(Kind, String)>>,
}

pub(super) fn install(panel: &Rc<RefCell<Panel>>) -> Result<(), JsValue> {
    let doc = panel.borrow().document.clone();
    if let Some(window) = web_sys::window()
        && let Ok(Some(storage)) = window.local_storage()
        && let Ok(Some(value)) = storage.get_item("metldr.composer.destination")
        && value.len() <= 1024
    {
        *panel.borrow().composer.restore_choice.borrow_mut() = Some(value.clone());
        *panel.borrow().composer.remembered_choice.borrow_mut() = Some(value);
    }
    let form = element(&doc, "composer")?;
    for (name, handler) in [
        (
            "click",
            clicked as fn(&Rc<RefCell<Panel>>, Event) -> Result<(), JsValue>,
        ),
        ("input", input),
        ("pointerover", hovered),
    ] {
        let panel = Rc::clone(panel);
        listen(&form, name, move |event| {
            if let Err(error) = handler(&panel, event) {
                web_sys::console::error_1(&error);
            }
        })?;
    }
    let keyboard_panel = Rc::clone(panel);
    let keyboard = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        if event.default_prevented()
            || event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
                .is_some_and(|target| target.id() == "question")
        {
            return;
        }
        if let Err(error) = keydown(&keyboard_panel, &event) {
            web_sys::console::error_1(&error);
        }
    });
    form.add_event_listener_with_callback("keydown", keyboard.as_ref().unchecked_ref())?;
    keyboard.forget();
    let outside_panel = Rc::clone(panel);
    listen(doc.as_ref(), "pointerdown", move |event| {
        if target(&event)
            .and_then(|target| target.closest("#composer").ok().flatten())
            .is_none()
        {
            close(&outside_panel.borrow());
            if let Err(error) = render(&outside_panel.borrow()) {
                web_sys::console::error_1(&error);
            }
        }
    })?;
    let prevent = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if target(&event)
            .and_then(|target| {
                target
                    .closest("#composer-menu-rows button, #composer-model-rows button")
                    .ok()
                    .flatten()
            })
            .is_some()
        {
            event.prevent_default();
        }
    });
    form.add_event_listener_with_callback("mousedown", prevent.as_ref().unchecked_ref())?;
    prevent.forget();
    for id in ["composer-menu", "composer-model-menu"] {
        let panel = Rc::clone(panel);
        listen(element(&doc, id)?.as_ref(), "pointerleave", move |_| {
            let state = panel.borrow();
            if id == "composer-menu" {
                state.composer.engaged.set(false);
            } else {
                state.composer.model_active.set(None);
            }
            if let Err(error) = render(&state) {
                web_sys::console::error_1(&error);
            }
        })?;
    }
    let resize_panel = Rc::clone(panel);
    listen(
        web_sys::window()
            .ok_or_else(|| err("Window missing."))?
            .as_ref(),
        "resize",
        move |_| {
            if let Err(error) = render(&resize_panel.borrow()) {
                web_sys::console::error_1(&error);
            }
        },
    )?;
    Ok(())
}

pub(super) fn restore_choice(panel: &Panel) -> Result<(), JsValue> {
    if !panel.connected || panel.snapshot.is_none() {
        return Ok(());
    }
    let Some(value) = panel.composer.restore_choice.borrow_mut().take() else {
        return Ok(());
    };
    let select = select(&panel.document)?;
    if !menu::models(panel)?.iter().any(|row| row.value == value) {
        let option = panel.document.create_element("option")?;
        option.set_attribute("value", &value)?;
        option.set_attribute("disabled", "")?;
        option.set_attribute("data-session", "")?;
        option.set_attribute(
            "data-provider",
            if value.starts_with("opencode:") {
                "opencode"
            } else if value.starts_with("codex:") {
                "codex"
            } else {
                "pi"
            },
        )?;
        option.set_text_content(Some("Saved destination unavailable"));
        select.append_child(&option)?;
    }
    select.set_value(&value);
    Ok(())
}

pub(super) fn remember_choice(panel: &Panel, value: &str) {
    if panel.composer.restore_choice.borrow().is_some()
        || panel.composer.remembered_choice.borrow().as_deref() == Some(value)
    {
        return;
    }
    if let Some(window) = web_sys::window()
        && let Ok(Some(storage)) = window.local_storage()
        && storage
            .set_item("metldr.composer.destination", value)
            .is_ok()
    {
        *panel.composer.remembered_choice.borrow_mut() = Some(value.into());
    }
}

pub(super) fn render(panel: &Panel) -> Result<(), JsValue> {
    let doc = &panel.document;
    let choices = menu::models(panel)?;
    let select = select(doc)?;
    let selected = choices.iter().position(|row| row.value == select.value());
    let label = selected
        .and_then(|index| choices.get(index))
        .map_or("Choose a destination", |row| row.name.as_str());
    element(doc, "composer-model-label")?
        .set_text_content(Some(label.split(", ").next().unwrap_or(label)));
    element(doc, "composer-model")?.set_attribute("title", label)?;
    element(doc, "composer-model")?.set_attribute(
        "aria-expanded",
        if panel.composer.models.get() {
            "true"
        } else {
            "false"
        },
    )?;
    element(doc, "composer-add")?.set_attribute(
        "aria-expanded",
        if panel.composer.plus.get() {
            "true"
        } else {
            "false"
        },
    )?;
    let was_open = !element(doc, "composer-model-menu")?.has_attribute("hidden");
    visible(doc, "composer-model-menu", panel.composer.models.get())?;
    if panel.composer.models.get() {
        let changed = menu::paint(doc, "composer-model-rows", &choices, true, &select.value())?;
        if changed && was_open {
            close(panel);
            return render(panel);
        }
        let active = panel
            .composer
            .model_active
            .get()
            .filter(|index| *index < choices.len())
            .or(selected);
        menu::highlight(
            doc,
            "composer-model-highlight",
            "composer-model-rows",
            active,
            panel.composer.model_active.get().is_some(),
        )?;
        menu::position(doc)?;
    }
    let draft = question(doc)?.value();
    let parsed = if panel.composer.dismissed.get() {
        None
    } else {
        token::parse(&draft)
    };
    let query = if panel.composer.plus.get() {
        Some((Kind::Source, String::new()))
    } else {
        parsed.map(|token| (token.kind, token.query.to_ascii_lowercase()))
    };
    if *panel.composer.query.borrow() != query {
        *panel.composer.query.borrow_mut() = query.clone();
        panel.composer.active.set(0);
        panel.composer.engaged.set(false);
    }
    visible(doc, "composer-menu", query.is_some())?;
    if let Some((kind, query)) = query {
        let rows = menu::suggestions(panel, kind, &query);
        panel.composer.active.set(
            panel
                .composer
                .active
                .get()
                .min(rows.len().saturating_sub(1)),
        );
        menu::paint(doc, "composer-menu-rows", &rows, false, "")?;
        if rows.is_empty() {
            element(doc, "composer-menu-rows")?
                .set_text_content(Some(&format!("No matches for \"{query}\"")));
        }
        menu::highlight(
            doc,
            "composer-menu-highlight",
            "composer-menu-rows",
            Some(panel.composer.active.get()),
            panel.composer.engaged.get() && !rows.is_empty(),
        )?;
        element(doc, "composer-menu-footer")?.set_text_content(Some(if kind == Kind::Source {
            "Type to search sources & files"
        } else {
            "Type to search commands"
        }));
    }
    let running = active_run(&panel.snapshot).is_some();
    let send = doc
        .query_selector("[data-action='ask']")?
        .ok_or_else(|| err("Send missing."))?;
    if draft.trim().is_empty() {
        send.set_attribute("disabled", "")?;
    }
    if running {
        send.set_attribute("hidden", "")?;
    } else {
        send.remove_attribute("hidden")?;
    }
    let stop = doc
        .query_selector("[data-action='stop']")?
        .ok_or_else(|| err("Stop missing."))?;
    if running {
        stop.remove_attribute("hidden")?;
    } else {
        stop.set_attribute("hidden", "")?;
    }
    Ok(())
}

pub(super) fn keydown(panel: &Rc<RefCell<Panel>>, event: &KeyboardEvent) -> Result<bool, JsValue> {
    if event.is_composing() {
        return Ok(false);
    }
    if event.key() == "Enter"
        && event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .is_some_and(|target| {
                target.has_attribute("data-execution-value")
                    || target.has_attribute("data-suggestion")
            })
    {
        return Ok(false);
    }
    let state = panel.borrow();
    if event.key() == "Escape" {
        state.composer.dismissed.set(true);
        close(&state);
        render(&state)?;
        event.prevent_default();
        return Ok(true);
    }
    let models = state.composer.models.get();
    let rows = if models {
        menu::models(&state)?
    } else if let Some((kind, query)) = state.composer.query.borrow().as_ref() {
        menu::suggestions(&state, *kind, query)
    } else {
        return Ok(false);
    };
    if rows.is_empty() {
        return Ok(false);
    }
    let current = if models {
        state.composer.model_active.get().unwrap_or_else(|| {
            rows.iter()
                .position(|row| {
                    row.value
                        == select(&state.document)
                            .map(|select| select.value())
                            .unwrap_or_default()
                })
                .unwrap_or(0)
        })
    } else {
        state.composer.active.get()
    }
    .min(rows.len() - 1);
    match event.key().as_str() {
        "ArrowDown" | "ArrowUp" => {
            let delta = if event.key() == "ArrowDown" {
                1
            } else {
                rows.len() - 1
            };
            let next = (1..=rows.len())
                .map(|step| (current + step * delta) % rows.len())
                .find(|index| !rows[*index].disabled)
                .unwrap_or(current);
            if models {
                state.composer.model_active.set(Some(next));
            } else {
                state.composer.active.set(next);
                state.composer.engaged.set(true);
            }
            render(&state)?;
        }
        "Enter" if !event.shift_key() => {
            let row = rows[current].clone();
            drop(state);
            pick(panel, row, event.is_trusted())?;
        }
        "Tab" if !models && matches!(rows[current].action, menu::Action::Insert(_)) => {
            let row = rows[current].clone();
            drop(state);
            pick(panel, row, event.is_trusted())?;
        }
        _ => return Ok(false),
    }
    event.prevent_default();
    Ok(true)
}

fn clicked(panel: &Rc<RefCell<Panel>>, event: Event) -> Result<(), JsValue> {
    let Some(button) = target(&event).and_then(|target| target.closest("button").ok().flatten())
    else {
        return Ok(());
    };
    let state = panel.borrow();
    if button.has_attribute("disabled") {
        return Ok(());
    }
    match button.id().as_str() {
        "composer-add" => {
            state.composer.plus.set(!state.composer.plus.get());
            state.composer.models.set(false);
            question(&state.document)?.focus()?;
        }
        "composer-model" => {
            state.composer.models.set(!state.composer.models.get());
            state.composer.plus.set(false);
            state.composer.model_active.set(None);
        }
        _ => {
            if let Some(value) = button.get_attribute("data-execution-value") {
                let row = menu::models(&state)?
                    .into_iter()
                    .find(|row| row.value == value && !row.disabled);
                drop(state);
                if let Some(row) = row {
                    pick(panel, row, event.is_trusted())?;
                }
                return Ok(());
            }
            if let Some(value) = button.get_attribute("data-suggestion") {
                let row = state
                    .composer
                    .query
                    .borrow()
                    .as_ref()
                    .and_then(|(kind, query)| {
                        menu::suggestions(&state, *kind, query)
                            .into_iter()
                            .find(|row| row.value == value && !row.disabled)
                    });
                drop(state);
                if let Some(row) = row {
                    pick(panel, row, event.is_trusted())?;
                }
                return Ok(());
            }
            return Ok(());
        }
    }
    render(&state)
}

fn input(panel: &Rc<RefCell<Panel>>, event: Event) -> Result<(), JsValue> {
    if target(&event).is_some_and(|target| target.id() == "question") {
        panel.borrow().composer.dismissed.set(false);
        panel.borrow().composer.plus.set(false);
        crate::composer::render(&panel.borrow())?;
    }
    Ok(())
}

fn hovered(panel: &Rc<RefCell<Panel>>, event: Event) -> Result<(), JsValue> {
    let Some(button) =
        target(&event).and_then(|target| target.closest("[data-menu-index]").ok().flatten())
    else {
        return Ok(());
    };
    let Some(index) = button
        .get_attribute("data-menu-index")
        .and_then(|v| v.parse().ok())
    else {
        return Ok(());
    };
    let state = panel.borrow();
    if button.has_attribute("data-execution-value") {
        state.composer.model_active.set(Some(index));
    } else {
        state.composer.active.set(index);
        state.composer.engaged.set(true);
    }
    render(&state)
}

fn pick(panel: &Rc<RefCell<Panel>>, row: menu::Row, trusted: bool) -> Result<(), JsValue> {
    if row.disabled {
        return Ok(());
    }
    let state = panel.borrow();
    close(&state);
    match row.action {
        menu::Action::Model => {
            let select = select(&state.document)?;
            if !menu::models(&state)?
                .iter()
                .any(|current| current.value == row.value && !current.disabled)
            {
                return Ok(());
            }
            select.set_value(&row.value);
            drop(state);
            select.dispatch_event(&Event::new("change")?)?;
        }
        menu::Action::Capture => {
            drop(state);
            if trusted {
                crate::capture_page(panel);
            }
        }
        menu::Action::Insert(kind) => {
            let input = question(&state.document)?;
            let draft = input.value();
            let token = token::parse(&draft);
            let prefix = token
                .as_ref()
                .map_or(draft.as_str(), |Token { start, .. }| &draft[..*start]);
            input.set_value(&format!(
                "{prefix}{}{name} ",
                if kind == Kind::Source { "@" } else { "" },
                name = row.name
            ));
            state.composer.dismissed.set(false);
        }
    }
    let state = panel.borrow();
    question(&state.document)?.focus()?;
    crate::composer::render(&state)
}

fn close(panel: &Panel) {
    panel.composer.plus.set(false);
    panel.composer.models.set(false);
    panel.composer.model_active.set(None);
}
fn target(event: &Event) -> Option<Element> {
    event.target()?.dyn_into().ok()
}
fn question(doc: &Document) -> Result<HtmlTextAreaElement, JsValue> {
    element(doc, "question")?.dyn_into().map_err(Into::into)
}
fn select(doc: &Document) -> Result<HtmlSelectElement, JsValue> {
    element(doc, "execution")?.dyn_into().map_err(Into::into)
}
fn element(doc: &Document, id: &str) -> Result<Element, JsValue> {
    doc.get_element_by_id(id)
        .ok_or_else(|| err(&format!("Composer {id} missing.")))
}
fn visible(doc: &Document, id: &str, show: bool) -> Result<(), JsValue> {
    let el = element(doc, id)?;
    if show {
        el.remove_attribute("hidden")
    } else {
        el.set_attribute("hidden", "")
    }
}
fn listen(
    target: &web_sys::EventTarget,
    name: &str,
    handler: impl FnMut(Event) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(Event)>::new(handler);
    target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}
