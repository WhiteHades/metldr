use std::{cell::RefCell, rc::Rc};

use serde_json::json;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Document, Element, Event, HtmlSelectElement, HtmlTextAreaElement};

use crate::{
    Panel, err,
    model::{FlowError, PiUiDialog, PiUiPending, PiUiView, RunRoute, RunStatus},
};

pub(crate) fn install(doc: &Document, panel: &Rc<RefCell<Panel>>) -> Result<(), JsValue> {
    let weak = Rc::downgrade(panel);
    let submit = Closure::<dyn FnMut(Event)>::new(move |event| {
        if let Some(panel) = weak.upgrade() {
            match handle_submit(&panel, &event) {
                Ok(true) => event.prevent_default(),
                Err(error) => {
                    if is_answer_form(&event) {
                        event.prevent_default();
                    }
                    crate::set_panel_status(&panel, &format!("Pi reply failed: {error}"));
                }
                _ => {}
            }
        }
    });
    doc.add_event_listener_with_callback("submit", submit.as_ref().unchecked_ref())?;
    submit.forget();
    for (id, event_name) in [("question", "input"), ("execution", "change")] {
        let target = doc
            .get_element_by_id(id)
            .ok_or_else(|| err("Pi UI editor target is missing"))?;
        let weak = Rc::downgrade(panel);
        let update = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Some(panel) = weak.upgrade() {
                let state = panel.borrow();
                if let Some(ui) = state.snapshot.as_ref().and_then(|view| view.pi_ui.as_ref()) {
                    if let Err(error) = render_editor_suggestion(&state.document, &state, ui) {
                        web_sys::console::error_1(&error);
                    }
                }
            }
        });
        target.add_event_listener_with_callback(event_name, update.as_ref().unchecked_ref())?;
        update.forget();
    }
    Ok(())
}

pub(crate) fn render(panel: &Panel) -> Result<(), JsValue> {
    let doc = &panel.document;
    let section = element(doc, "pi-ui")?;
    let ui = panel.snapshot.as_ref().and_then(|view| view.pi_ui.as_ref());
    let visible = ui.is_some_and(|ui| {
        ui.title.is_some()
            || !ui.pending.is_empty()
            || !ui.statuses.is_empty()
            || !ui.widgets.is_empty()
            || ui.editor.is_some()
            || !ui.notices.is_empty()
            || ui.notices_omitted > 0
    });
    if visible {
        section.remove_attribute("hidden")?;
    } else {
        section.set_attribute("hidden", "")?;
    }
    let title = element(doc, "pi-ui-title")?;
    let safe_title = ui.and_then(|ui| ui.title.as_deref()).map(display_plain);
    title.set_text_content(safe_title.as_deref());
    if ui.and_then(|ui| ui.title.as_ref()).is_some() {
        title.remove_attribute("hidden")?;
    } else {
        title.set_attribute("hidden", "")?;
    }
    let cards = element(doc, "pi-ui-cards")?;
    let statuses = element(doc, "pi-ui-statuses")?;
    let above = element(doc, "pi-ui-widgets-above")?;
    let below = element(doc, "pi-ui-widgets-below")?;
    let notices = element(doc, "pi-ui-notices")?;
    if let Some(ui) = ui {
        render_cards(doc, panel, &cards, ui)?;
        clear(&statuses)?;
        for status in &ui.statuses {
            if let Some(text) = &status.text {
                let row = doc.create_element("p")?;
                row.set_attribute("data-status-key", &status.key)?;
                row.set_text_content(Some(&display_plain(text)));
                statuses.append_child(&row)?;
            }
        }
        clear_widgets(&above)?;
        clear_widgets(&below)?;
        for widget in &ui.widgets {
            if let Some(lines) = &widget.lines {
                let node = doc.create_element("pre")?;
                node.set_attribute("data-part", "widget")?;
                node.set_attribute("data-widget-key", &widget.key)?;
                node.set_text_content(Some(&display_plain(&lines.join("\n"))));
                if widget.placement == "aboveEditor" {
                    above.append_child(&node)?;
                } else {
                    below.append_child(&node)?;
                }
            }
        }
        clear(&notices)?;
        for notice in &ui.notices {
            let node = doc.create_element("p")?;
            node.set_attribute("data-part", "notice")?;
            node.set_attribute("data-level", &notice.level)?;
            node.set_text_content(Some(&display_plain(&notice.message)));
            notices.append_child(&node)?;
        }
        if ui.notices_omitted > 0 {
            let node = doc.create_element("p")?;
            node.set_attribute("data-part", "notice-omission")?;
            node.set_text_content(Some(&format!(
                "{} earlier Pi notices were omitted.",
                ui.notices_omitted
            )));
            notices.append_child(&node)?;
        }
        render_editor_suggestion(doc, panel, ui)?;
    } else {
        clear(&cards)?;
        clear(&statuses)?;
        clear(&above)?;
        clear(&below)?;
        clear(&notices)?;
        if let Some(suggestion) = doc.get_element_by_id("pi-ui-suggestion") {
            suggestion.remove();
        }
    }
    Ok(())
}

fn render_cards(
    doc: &Document,
    panel: &Panel,
    list: &Element,
    ui: &PiUiView,
) -> Result<(), JsValue> {
    let old: Vec<_> = (0..list.children().length())
        .filter_map(|i| list.children().item(i))
        .collect();
    let active = doc.active_element();
    let preserve_live_card_focus = active.as_ref().is_some_and(|active| {
        old.iter().any(|card| {
            card.contains(Some(active.as_ref()))
                && ui.pending.iter().any(|pending| {
                    card.get_attribute("data-pi-request").as_deref()
                        == Some(pending.request.as_str())
                })
        })
    });
    let mut focus_lost = false;
    for card in &old {
        if !ui.pending.iter().any(|item| {
            card.get_attribute("data-pi-request").as_deref() == Some(item.request.as_str())
        }) {
            focus_lost |= doc
                .active_element()
                .is_some_and(|active| card.contains(Some(active.as_ref())));
            list.remove_child(card)?;
        }
    }
    let mut focused_new_card = false;
    for (index, pending) in ui.pending.iter().enumerate() {
        let existing = old.iter().find(|card| {
            card.get_attribute("data-pi-request").as_deref() == Some(pending.request.as_str())
        });
        let new_card = existing.is_none();
        let card = match existing {
            Some(card) => card.clone(),
            None => create_card(doc, ui, pending)?,
        };
        update_card(panel, ui, &card, pending)?;
        let current = list.children().item(index as u32);
        if !current
            .as_ref()
            .is_some_and(|node| node.is_same_node(Some(&card)))
        {
            list.insert_before(&card, current.as_ref().map(|node| node.as_ref()))?;
        }
        if new_card && !preserve_live_card_focus && !focus_lost && !focused_new_card {
            card.scroll_into_view_with_bool(true);
            focused_new_card = focus_card(&card)?;
        }
    }
    if focus_lost {
        let first = list
            .children()
            .item(0)
            .and_then(|node| node.dyn_into::<Element>().ok());
        let focused = if let Some(first) = first {
            focus_card(&first)?
        } else {
            false
        };
        if !focused {
            if let Some(status) = doc
                .get_element_by_id("status")
                .and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = status.focus();
            }
        }
    }
    Ok(())
}

fn create_card(doc: &Document, ui: &PiUiView, pending: &PiUiPending) -> Result<Element, JsValue> {
    let card = doc.create_element("article")?;
    card.set_attribute("data-pi-run", ui.run.as_str())?;
    card.set_attribute("data-pi-request", pending.request.as_str())?;
    let heading = doc.create_element("h3")?;
    heading.set_attribute("data-part", "title")?;
    let heading_id = format!("pi-dialog-{}", pending.request.as_str().replace(':', "-"));
    heading.set_attribute("id", &heading_id)?;
    card.set_attribute("role", "group")?;
    card.set_attribute("tabindex", "-1")?;
    card.set_attribute("aria-labelledby", &heading_id)?;
    card.append_child(&heading)?;
    let body = doc.create_element("p")?;
    body.set_attribute("data-part", "body")?;
    card.append_child(&body)?;
    let choices = doc.create_element("div")?;
    choices.set_attribute("data-part", "choices")?;
    let form = doc.create_element("form")?;
    form.set_attribute("data-part", "answer-form")?;
    form.set_attribute("data-pi-answer-kind", "text")?;
    card.append_child(&form)?;
    form.append_child(&choices)?;
    let actions = doc.create_element("div")?;
    actions.set_attribute("data-part", "actions")?;
    form.append_child(&actions)?;
    match &pending.dialog {
        PiUiDialog::Select { title, options } => {
            heading.set_text_content(Some(&display_plain(title)));
            body.set_attribute("hidden", "")?;
            form.set_attribute("data-pi-answer-kind", "select")?;
            choices.set_attribute("role", "group")?;
            choices.set_attribute("aria-label", &display_plain(title))?;
            for option in options {
                let button = answer_button(doc, "select", Some(option), None)?;
                button.set_attribute("aria-pressed", "false")?;
                button.set_attribute("data-part", "choice")?;
                if option.is_empty() {
                    button.set_attribute("aria-label", "Empty option")?;
                }
                let mark = doc.create_element("span")?;
                mark.set_attribute("data-part", "radio-mark")?;
                mark.set_attribute("aria-hidden", "true")?;
                button.append_child(&mark)?;
                let label = doc.create_element("span")?;
                label.set_attribute("data-part", "choice-label")?;
                label.set_text_content(Some(&display_plain(option)));
                button.append_child(&label)?;
                choices.append_child(&button)?;
            }
            append_action(&actions, doc, "cancel", None, None, "Cancel")?;
            let submit = submit_action(doc)?;
            submit.set_attribute("data-requires-selection", "")?;
            submit.set_attribute("disabled", "")?;
            actions.append_child(&submit)?;
        }
        PiUiDialog::Confirm { title, message } => {
            heading.set_text_content(Some(&display_plain(title)));
            body.set_text_content(Some(&display_plain(message)));
            append_action(&choices, doc, "confirm", None, Some(true), "Yes")?;
            append_action(&choices, doc, "confirm", None, Some(false), "No")?;
            append_action(&actions, doc, "cancel", None, None, "Cancel")?;
        }
        PiUiDialog::Input {
            title, placeholder, ..
        }
        | PiUiDialog::Editor {
            title,
            prefill: placeholder,
        } => {
            heading.set_text_content(Some(&display_plain(title)));
            body.set_attribute("hidden", "")?;
            let textarea = doc
                .create_element("textarea")?
                .dyn_into::<HtmlTextAreaElement>()?;
            textarea.set_attribute("data-part", "draft")?;
            textarea.set_attribute("aria-label", &display_plain(title))?;
            if let PiUiDialog::Input { placeholder, .. } = &pending.dialog {
                if let Some(placeholder) = placeholder {
                    textarea.set_attribute("placeholder", &display_plain(placeholder))?;
                }
            } else if let Some(prefill) = placeholder {
                textarea.set_value(prefill);
            }
            choices.append_child(&textarea)?;
            append_action(&actions, doc, "cancel", None, None, "Cancel")?;
            let submit = submit_action(doc)?;
            actions.append_child(&submit)?;
        }
    }
    Ok(card)
}

fn update_card(
    panel: &Panel,
    ui: &PiUiView,
    card: &Element,
    pending: &PiUiPending,
) -> Result<(), JsValue> {
    card.set_attribute("data-pi-run", ui.run.as_str())?;
    card.set_attribute("data-pi-request", pending.request.as_str())?;
    card.set_attribute(
        "data-pi-connection",
        &panel.connection_generation.to_string(),
    )?;
    let disabled = !is_running_pi(panel, &ui.run)
        || panel.pi_ui_stopping.as_ref() == Some(&ui.run)
        || !panel.connected
        || !panel.flow.is_connected()
        || panel.pi_ui_pending.is_some();
    let controls = card.query_selector_all("button, textarea")?;
    for index in 0..controls.length() {
        if let Some(control) = controls.item(index) {
            let element: Element = control.dyn_into()?;
            if disabled {
                element.set_attribute("disabled", "")?;
            } else if element.has_attribute("data-requires-selection")
                && card
                    .query_selector("[data-pi-kind=select][aria-pressed=true]")?
                    .is_none()
            {
                element.set_attribute("disabled", "")?;
            } else {
                element.remove_attribute("disabled")?;
            }
        }
    }
    Ok(())
}

fn answer_button(
    doc: &Document,
    kind: &str,
    text: Option<&str>,
    boolean: Option<bool>,
) -> Result<Element, JsValue> {
    let button = doc.create_element("button")?;
    button.set_attribute("type", "button")?;
    button.set_attribute("data-action", "pi-ui-answer")?;
    button.set_attribute("data-pi-kind", kind)?;
    if let Some(text) = text {
        button.set_attribute("data-pi-value", text)?;
    }
    if let Some(boolean) = boolean {
        button.set_attribute("data-pi-bool", if boolean { "true" } else { "false" })?;
    }
    Ok(button)
}

fn append_action(
    parent: &Element,
    doc: &Document,
    kind: &str,
    text: Option<&str>,
    boolean: Option<bool>,
    label: &str,
) -> Result<(), JsValue> {
    let button = answer_button(doc, kind, text, boolean)?;
    button.set_text_content(Some(label));
    if kind == "cancel" {
        button.set_attribute("data-variant", "quiet")?;
    }
    parent.append_child(&button)?;
    Ok(())
}

fn submit_action(doc: &Document) -> Result<Element, JsValue> {
    let button = doc.create_element("button")?;
    button.set_attribute("type", "submit")?;
    button.set_attribute("data-variant", "primary")?;
    button.set_text_content(Some("Send"));
    Ok(button)
}

pub(crate) fn handle_click(
    panel: &Rc<RefCell<Panel>>,
    button: &Element,
    event: &Event,
) -> Result<bool, FlowError> {
    let action = button.get_attribute("data-action");
    match action.as_deref() {
        Some("pi-ui-answer") => {
            if !event.is_trusted() {
                return Ok(true);
            }
            let Some(card) = button.closest("[data-pi-request]").map_err(js_flow)? else {
                return Ok(true);
            };
            let kind = button.get_attribute("data-pi-kind").unwrap_or_default();
            if kind == "select" {
                let options = card
                    .query_selector_all("[data-pi-kind=select]")
                    .map_err(js_flow)?;
                for index in 0..options.length() {
                    if let Some(option) = options
                        .item(index)
                        .and_then(|node| node.dyn_into::<Element>().ok())
                    {
                        option
                            .set_attribute(
                                "aria-pressed",
                                if option.is_same_node(Some(button)) {
                                    "true"
                                } else {
                                    "false"
                                },
                            )
                            .map_err(js_flow)?;
                    }
                }
                if let Some(send) = card
                    .query_selector("[data-requires-selection]")
                    .map_err(js_flow)?
                {
                    send.remove_attribute("disabled").map_err(js_flow)?;
                }
                return Ok(true);
            }
            let text = button.get_attribute("data-pi-value");
            let confirm = button
                .get_attribute("data-pi-bool")
                .and_then(|value| value.parse().ok());
            send(panel, &card, &kind, text, confirm)?;
            Ok(true)
        }
        Some("pi-ui-replace") => {
            if event.is_trusted() {
                replace_suggestion(panel, button)?;
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_submit(panel: &Rc<RefCell<Panel>>, event: &Event) -> Result<bool, FlowError> {
    let Some(form) = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
    else {
        return Ok(false);
    };
    if form.get_attribute("data-part").as_deref() != Some("answer-form") {
        return Ok(false);
    }
    if !event.is_trusted() {
        return Ok(true);
    }
    let Some(card) = form.closest("[data-pi-request]").map_err(js_flow)? else {
        return Ok(true);
    };
    if form.get_attribute("data-pi-answer-kind").as_deref() == Some("select") {
        let value = form
            .query_selector("[data-pi-kind=select][aria-pressed=true]")
            .map_err(js_flow)?
            .and_then(|node| node.get_attribute("data-pi-value"));
        if let Some(value) = value {
            send(panel, &card, "select", Some(value), None)?;
        }
    } else {
        let value = form
            .query_selector("textarea")
            .map_err(js_flow)?
            .and_then(|node| node.dyn_into::<HtmlTextAreaElement>().ok())
            .map(|area| area.value());
        send(panel, &card, "text", value, None)?;
    }
    Ok(true)
}

fn send(
    panel: &Rc<RefCell<Panel>>,
    card: &Element,
    kind: &str,
    text: Option<String>,
    confirm: Option<bool>,
) -> Result<(), FlowError> {
    let kind = kind.to_owned();
    let run = parse_id(card.get_attribute("data-pi-run"))?;
    let request = parse_id(card.get_attribute("data-pi-request"))?;
    let connection = card
        .get_attribute("data-pi-connection")
        .and_then(|value| value.parse::<u64>().ok());
    let allowed = {
        let mut state = panel.borrow_mut();
        let pending = state
            .snapshot
            .as_ref()
            .and_then(|view| view.pi_ui.as_ref())
            .filter(|ui| ui.run == run)
            .and_then(|ui| ui.pending.iter().find(|pending| pending.request == request));
        let answer_matches =
            pending.is_some_and(|pending| match (&pending.dialog, kind.as_str()) {
                (PiUiDialog::Select { options, .. }, "select") => text
                    .as_ref()
                    .is_some_and(|value| options.iter().any(|option| option == value)),
                (PiUiDialog::Confirm { .. }, "confirm") => confirm.is_some(),
                (PiUiDialog::Input { .. } | PiUiDialog::Editor { .. }, "text") => text.is_some(),
                (_, "cancel") => true,
                _ => false,
            });
        if connection != Some(state.connection_generation)
            || !state.connected
            || !state.flow.is_connected()
            || !is_running_pi(&state, &run)
            || state.pi_ui_stopping.as_ref() == Some(&run)
            || state.pi_ui_pending.is_some()
            || !answer_matches
        {
            return Ok(());
        }
        state.pi_ui_pending = Some((run.clone(), request.clone()));
        let scope = state
            .snapshot
            .as_ref()
            .ok_or(FlowError::NoSource)?
            .selection
            .clone();
        Some((state.flow.clone(), scope, state.connection_generation))
    };
    let Some((flow, scope, generation)) = allowed else {
        return Ok(());
    };
    crate::view::status(&panel.borrow().document, "Sending reply to Pi.");
    let panel = Rc::clone(panel);
    wasm_bindgen_futures::spawn_local(async move {
        let result = flow
            .pi_ui_reply(scope, run, request, &kind, text, confirm)
            .await;
        if panel.borrow().connection_generation != generation {
            return;
        }
        match result {
            Ok(view) => {
                crate::apply_view(&panel, view, generation);
                crate::set_panel_status(&panel, "Reply queued for Pi.");
            }
            Err(error) => {
                panel.borrow_mut().pi_ui_pending = None;
                crate::set_panel_status(&panel, &format!("Pi reply failed: {error}"));
            }
        }
    });
    Ok(())
}

fn render_editor_suggestion(doc: &Document, panel: &Panel, ui: &PiUiView) -> Result<(), JsValue> {
    let Some(editor) = &ui.editor else {
        if let Some(old) = doc.get_element_by_id("pi-ui-suggestion") {
            old.remove();
        }
        return Ok(());
    };
    let Some(run) = panel
        .snapshot
        .as_ref()
        .and_then(|view| view.runs.iter().find(|run| run.id == ui.run))
    else {
        return Ok(());
    };
    let RunRoute::Pi { workflow, .. } = &run.route else {
        if let Some(old) = doc.get_element_by_id("pi-ui-suggestion") {
            old.remove();
        }
        return Ok(());
    };
    let selected = doc
        .get_element_by_id("execution")
        .and_then(|node| node.dyn_into::<HtmlSelectElement>().ok())
        .map(|select| select.value());
    if !selected_matches_pi(selected.as_deref(), workflow.as_str()) {
        if let Some(old) = doc.get_element_by_id("pi-ui-suggestion") {
            old.remove();
        }
        return Ok(());
    }
    let destination = selected.as_deref().unwrap_or_default();
    let current = doc.get_element_by_id("pi-ui-suggestion");
    let same_suggestion = current.as_ref().is_some_and(|node| {
        node.get_attribute("data-pi-run").as_deref() == Some(ui.run.as_str())
            && node
                .get_attribute("data-pi-revision")
                .and_then(|value| value.parse::<u64>().ok())
                == Some(editor.revision)
            && node.get_attribute("data-pi-destination").as_deref() == Some(destination)
    });
    if !same_suggestion {
        if let Some(current) = current {
            current.remove();
        }
    }
    let suggestion = match doc.get_element_by_id("pi-ui-suggestion") {
        Some(suggestion) => suggestion,
        None => {
            let suggestion = doc.create_element("div")?;
            suggestion.set_attribute("id", "pi-ui-suggestion")?;
            suggestion.set_attribute("data-part", "suggestion")?;
            let preview = doc.create_element("details")?;
            preview.set_attribute("data-part", "editor-preview")?;
            let summary = doc.create_element("summary")?;
            summary.set_text_content(Some("Suggested text"));
            preview.append_child(&summary)?;
            let text = doc.create_element("pre")?;
            preview.append_child(&text)?;
            suggestion.append_child(&preview)?;
            let button = doc.create_element("button")?;
            button.set_attribute("type", "button")?;
            button.set_attribute("data-action", "pi-ui-replace")?;
            suggestion.append_child(&button)?;
            suggestion
        }
    };
    let text = suggestion
        .query_selector("pre")?
        .ok_or_else(|| err("Pi suggestion preview is missing"))?;
    text.set_text_content(Some(&display_plain(&editor.text)));
    let button = suggestion
        .query_selector("button")?
        .ok_or_else(|| err("Pi suggestion button is missing"))?;
    button.set_attribute("data-pi-run", ui.run.as_str())?;
    button.set_attribute("data-pi-revision", &editor.revision.to_string())?;
    button.set_attribute(
        "data-pi-connection",
        &panel.connection_generation.to_string(),
    )?;
    suggestion.set_attribute("data-pi-run", ui.run.as_str())?;
    suggestion.set_attribute("data-pi-revision", &editor.revision.to_string())?;
    suggestion.set_attribute("data-pi-destination", destination)?;
    let draft = doc
        .get_element_by_id("question")
        .and_then(|node| node.dyn_into::<HtmlTextAreaElement>().ok());
    button.set_text_content(Some(
        if draft
            .as_ref()
            .is_some_and(|draft| !draft.value().is_empty())
        {
            "Replace draft"
        } else {
            "Use suggestion"
        },
    ));
    let enabled = panel.connected
        && panel.flow.is_connected()
        && panel.pi_ui_stopping.as_ref() != Some(&ui.run);
    if enabled {
        button.remove_attribute("disabled")?;
    } else {
        button.set_attribute("disabled", "")?;
    }
    let below = doc
        .get_element_by_id("pi-ui-widgets-below")
        .ok_or_else(|| err("Pi widgets slot is missing"))?;
    if !suggestion
        .parent_element()
        .is_some_and(|parent| parent.is_same_node(Some(&below)))
    {
        below.append_child(&suggestion)?;
    }
    Ok(())
}

fn replace_suggestion(panel: &Rc<RefCell<Panel>>, button: &Element) -> Result<(), FlowError> {
    let run = parse_id(button.get_attribute("data-pi-run"))?;
    let revision = button
        .get_attribute("data-pi-revision")
        .and_then(|value| value.parse::<u64>().ok());
    let state = panel.borrow();
    if button
        .get_attribute("data-pi-connection")
        .and_then(|value| value.parse::<u64>().ok())
        != Some(state.connection_generation)
    {
        return Ok(());
    }
    if !state.connected || !state.flow.is_connected() || state.pi_ui_stopping.as_ref() == Some(&run)
    {
        return Ok(());
    }
    let Some(ui) = state
        .snapshot
        .as_ref()
        .and_then(|view| view.pi_ui.as_ref())
        .filter(|ui| {
            ui.run == run
                && ui
                    .editor
                    .as_ref()
                    .is_some_and(|editor| Some(editor.revision) == revision)
        })
    else {
        return Ok(());
    };
    let Some(run_view) = state
        .snapshot
        .as_ref()
        .and_then(|view| view.runs.iter().find(|item| item.id == run))
    else {
        return Ok(());
    };
    let RunRoute::Pi { workflow, .. } = &run_view.route else {
        return Ok(());
    };
    let selected = state
        .document
        .get_element_by_id("execution")
        .and_then(|node| node.dyn_into::<HtmlSelectElement>().ok())
        .map(|select| select.value());
    if !selected_matches_pi(selected.as_deref(), workflow.as_str()) {
        return Ok(());
    }
    let Some(draft) = state
        .document
        .get_element_by_id("question")
        .and_then(|node| node.dyn_into::<HtmlTextAreaElement>().ok())
    else {
        return Ok(());
    };
    draft.set_value(&ui.editor.as_ref().unwrap().text);
    draft.focus().map_err(js_flow)?;
    Ok(())
}

fn is_running_pi(panel: &Panel, run: &crate::model::RunId) -> bool {
    panel.snapshot.as_ref().is_some_and(|view| {
        view.runs.iter().any(|item| {
            &item.id == run
                && item.status == RunStatus::Running
                && matches!(&item.route, RunRoute::Pi { .. })
        })
    })
}

fn parse_id(value: Option<String>) -> Result<crate::model::RunId, FlowError> {
    serde_json::from_value(json!(value.ok_or_else(|| FlowError::InvalidInput(
        "Pi request identity is missing.".into()
    ))?))
    .map_err(|_| FlowError::InvalidInput("Pi request identity is invalid.".into()))
}
fn element(doc: &Document, id: &str) -> Result<Element, JsValue> {
    doc.get_element_by_id(id)
        .ok_or_else(|| err("Pi UI panel is incomplete"))
}
fn clear(node: &Element) -> Result<(), JsValue> {
    while let Some(child) = node.first_child() {
        node.remove_child(&child)?;
    }
    Ok(())
}

fn clear_widgets(node: &Element) -> Result<(), JsValue> {
    let widgets = node.query_selector_all("[data-part=widget]")?;
    for index in (0..widgets.length()).rev() {
        if let Some(widget) = widgets.item(index) {
            node.remove_child(&widget)?;
        }
    }
    Ok(())
}

fn focus_card(card: &Element) -> Result<bool, JsValue> {
    if let Some(control) = card.query_selector("button:not(:disabled), textarea:not(:disabled)")? {
        if let Ok(control) = control.dyn_into::<web_sys::HtmlElement>() {
            control.focus()?;
            return Ok(true);
        }
    }
    if let Ok(card) = card.clone().dyn_into::<web_sys::HtmlElement>() {
        card.focus()?;
        return Ok(true);
    }
    Ok(false)
}

fn is_answer_form(event: &Event) -> bool {
    event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
        .is_some_and(|form| form.get_attribute("data-part").as_deref() == Some("answer-form"))
}

fn selected_matches_pi(selected: Option<&str>, workflow: &str) -> bool {
    selected == Some("pi") || selected == Some(workflow)
}

/// Strip terminal control sequences for presentation. Payloads used as selection values remain unchanged.
pub(crate) fn display_plain(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            index += 1;
            match bytes.get(index) {
                Some(b'[') => {
                    index += 1;
                    while index < bytes.len() {
                        let byte = bytes[index];
                        index += 1;
                        if (0x40..=0x7e).contains(&byte) {
                            break;
                        }
                    }
                }
                Some(b']') => {
                    index += 1;
                    while index < bytes.len() {
                        if bytes[index] == 0x07 {
                            index += 1;
                            break;
                        }
                        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                            index += 2;
                            break;
                        }
                        index += 1;
                    }
                }
                Some(byte) if byte.is_ascii() => index += 1,
                _ => {}
            }
            continue;
        }
        let start = index;
        let ch = value[index..].chars().next().expect("valid UTF-8 boundary");
        index += ch.len_utf8();
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push_str(&value[start..index]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::display_plain;

    #[test]
    fn ansi_sequences_are_removed_from_display_text() {
        assert_eq!(display_plain("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(display_plain("a\u{1b}]0;title\u{7}b"), "ab");
        assert_eq!(display_plain("x\u{1b}[31"), "x");
        assert_eq!(display_plain("x\u{1b}]unfinished"), "x");
        assert_eq!(display_plain("\u{1b}雪 🌱"), "雪 🌱");
    }
}
fn js_flow(error: JsValue) -> FlowError {
    FlowError::Host(crate::js_message(&error))
}
