use std::{cell::RefCell, rc::Rc};

use serde_json::{Map, Value, json};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Document, Element, Event, HtmlInputElement, HtmlSelectElement};

use crate::{
    Panel, err,
    model::{CodexInputPrompt, FlowError, RunId},
};

pub(crate) fn install(doc: &Document, panel: &Rc<RefCell<Panel>>) -> Result<(), JsValue> {
    let weak = Rc::downgrade(panel);
    let click = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        let Some(panel) = weak.upgrade() else { return };
        let Some(target) = event
            .target()
            .and_then(|node| node.dyn_into::<Element>().ok())
        else {
            return;
        };
        let Ok(Some(button)) = target.closest("[data-codex-input-action]") else {
            return;
        };
        if !event.is_trusted() {
            return;
        }
        if let Err(error) = reply(
            &panel,
            &button,
            button
                .get_attribute("data-codex-input-action")
                .as_deref()
                .unwrap_or_default(),
        ) {
            crate::set_panel_status(&panel, &format!("Codex reply failed: {error}"));
        }
    });
    doc.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();
    let weak = Rc::downgrade(panel);
    let submit = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        let Some(panel) = weak.upgrade() else { return };
        let Some(form) = event
            .target()
            .and_then(|node| node.dyn_into::<Element>().ok())
        else {
            return;
        };
        if form.get_attribute("data-codex-input-form").is_none() {
            return;
        }
        event.prevent_default();
        if !event.is_trusted() {
            return;
        }
        if let Err(error) = reply(&panel, &form, "accept") {
            crate::set_panel_status(&panel, &format!("Codex reply failed: {error}"));
        }
    });
    doc.add_event_listener_with_callback("submit", submit.as_ref().unchecked_ref())?;
    submit.forget();
    Ok(())
}

pub(crate) fn render(panel: &Panel) -> Result<(), JsValue> {
    let doc = &panel.document;
    let section = el(doc, "codex-input")?;
    let input = panel.snapshot.as_ref().and_then(|v| v.codex_input.as_ref());
    if input.is_some_and(|i| !i.pending.is_empty()) {
        section.remove_attribute("hidden")?;
    } else {
        section.set_attribute("hidden", "")?;
    }
    let list = el(doc, "codex-input-cards")?;
    let old: Vec<_> = (0..list.children().length())
        .filter_map(|i| list.children().item(i))
        .collect();
    let active = doc.active_element();
    let preserve_live_card_focus = active.as_ref().is_some_and(|active| {
        old.iter().any(|card| {
            card.contains(Some(active.as_ref()))
                && input.is_some_and(|ui| {
                    ui.pending.iter().any(|p| {
                        card.get_attribute("data-codex-request").as_deref()
                            == Some(p.request.as_str())
                    })
                })
        })
    });
    let mut restore_focus = false;
    let mut focus_new_card = false;
    for card in &old {
        if !input.is_some_and(|ui| {
            ui.pending.iter().any(|p| {
                card.get_attribute("data-codex-request").as_deref() == Some(p.request.as_str())
            })
        }) {
            restore_focus |= active
                .as_ref()
                .is_some_and(|node| card.contains(Some(node.as_ref())));
            list.remove_child(card)?;
        }
    }
    if let Some(input) = input {
        for (index, pending) in input.pending.iter().enumerate() {
            let existing = old.iter().find(|c| {
                c.get_attribute("data-codex-request").as_deref() == Some(pending.request.as_str())
            });
            let is_new = existing.is_none();
            let card = match existing {
                Some(card) => card.clone(),
                None => create_card(doc, &input.run, &pending.request, &pending.prompt)?,
            };
            let current = list.children().item(index as u32);
            if !current
                .as_ref()
                .is_some_and(|node| node.is_same_node(Some(&card)))
            {
                list.insert_before(&card, current.as_ref().map(|n| n.as_ref()))?;
            }
            set_disabled(
                &card,
                panel
                    .codex_input_pending
                    .as_ref()
                    .is_some_and(|(run, request)| run == &input.run && request == &pending.request)
                    || panel.codex_input_stopping.as_ref() == Some(&input.run),
            );
            if is_new && !preserve_live_card_focus && !restore_focus && !focus_new_card {
                card.scroll_into_view_with_bool(true);
                if let Ok(card) = card.clone().dyn_into::<web_sys::HtmlElement>() {
                    let _ = card.focus();
                }
                focus_new_card = true;
            }
        }
    }
    if restore_focus {
        if let Some(card) = list
            .children()
            .item(0)
            .and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = card.focus();
        } else if let Some(status) = doc
            .get_element_by_id("status")
            .and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = status.focus();
        }
    }
    Ok(())
}

fn create_card(
    doc: &Document,
    run: &RunId,
    request: &RunId,
    prompt: &CodexInputPrompt,
) -> Result<Element, JsValue> {
    let card = doc.create_element("article")?;
    card.set_attribute("data-codex-run", run.as_str())?;
    card.set_attribute("data-codex-request", request.as_str())?;
    card.set_attribute("role", "group")?;
    card.set_attribute("tabindex", "-1")?;
    card.set_attribute("data-part", "codex-input-card")?;
    let title = doc.create_element("h3")?;
    title.set_attribute("data-part", "heading")?;
    title.set_attribute("id", &format!("codex-input-{}", request.as_str()))?;
    title.set_text_content(Some("Codex needs your input"));
    card.set_attribute(
        "aria-labelledby",
        &format!("codex-input-{}", request.as_str()),
    )?;
    card.append_child(&title)?;
    let body = doc.create_element("p")?;
    body.set_attribute("data-part", "message")?;
    card.append_child(&body)?;
    let form = doc.create_element("form")?;
    form.set_attribute("data-codex-input-form", "")?;
    form.set_attribute("aria-label", "Codex input response")?;
    if let CodexInputPrompt::McpForm {
        server, message, ..
    }
    | CodexInputPrompt::McpToolApproval {
        server, message, ..
    } = prompt
    {
        body.set_text_content(Some(message));
        let source = doc.create_element("p")?;
        source.set_attribute("data-part", "description")?;
        source.set_text_content(Some(&format!("MCP server: {server}")));
        card.append_child(&source)?;
    }
    if let CodexInputPrompt::Questions {
        is_blocking: false, ..
    } = prompt
    {
        body.set_text_content(Some("Codex may continue without a response."));
    }
    if let CodexInputPrompt::Questions { questions, .. } = prompt {
        for q in questions {
            let group = doc.create_element("fieldset")?;
            group.set_attribute("data-part", "question")?;
            let legend = doc.create_element("legend")?;
            legend.set_text_content(Some(if q.header.is_empty() {
                "Question"
            } else {
                &q.header
            }));
            group.append_child(&legend)?;
            let question = doc.create_element("p")?;
            question.set_attribute("data-part", "question-text")?;
            question.set_text_content(Some(&q.text));
            group.append_child(&question)?;
            for option in &q.options {
                let label = doc.create_element("label")?;
                label.set_attribute("data-part", "choice")?;
                let input = doc.create_element("input")?;
                input.set_attribute("data-part", "choice-input")?;
                input.set_attribute("type", "checkbox")?;
                input.set_attribute("name", &format!("q:{}", q.id))?;
                input.set_attribute("value", &option.label)?;
                label.append_child(&input)?;
                let copy = doc.create_element("span")?;
                copy.set_text_content(Some(&option.label));
                label.append_child(&copy)?;
                let description = doc.create_element("span")?;
                description.set_attribute("data-part", "choice-description")?;
                description.set_text_content(Some(&option.description));
                label.append_child(&description)?;
                group.append_child(&label)?;
            }
            if q.other {
                let label = doc.create_element("label")?;
                label.set_attribute("data-part", "other")?;
                label.set_text_content(Some("Other"));
                let input = doc.create_element("input")?;
                input.set_attribute("type", "text")?;
                input.set_attribute("data-part", "text-input")?;
                input.set_attribute("name", &format!("other:{}", q.id))?;
                label.append_child(&input)?;
                group.append_child(&label)?;
            }
            form.append_child(&group)?;
        }
    }
    if let CodexInputPrompt::McpForm { fields, .. } = prompt {
        for field in fields {
            let label = doc.create_element("label")?;
            label.set_attribute("data-part", "field")?;
            label.set_text_content(Some(if field.title.is_empty() {
                &field.name
            } else {
                &field.title
            }));
            if let Some(description) = &field.description {
                let d = doc.create_element("span")?;
                d.set_attribute("data-part", "description")?;
                d.set_text_content(Some(description));
                label.append_child(&d)?;
            }
            let control = if let Some(choices) = &field.choices {
                let select = doc.create_element("select")?;
                select.set_attribute("name", &format!("f:{}", field.name))?;
                for choice in choices {
                    let option = doc.create_element("option")?;
                    option.set_attribute("value", choice)?;
                    option.set_text_content(Some(choice));
                    select.append_child(&option)?;
                }
                select
            } else {
                let input = doc.create_element("input")?;
                input.set_attribute("type", "text")?;
                input.set_attribute("data-part", "text-input")?;
                input.set_attribute("name", &format!("f:{}", field.name))?;
                input
            };
            if let Some(default) = &field.default {
                if let Ok(input) = control.clone().dyn_into::<HtmlInputElement>() {
                    input.set_value(default);
                } else if let Ok(select) = control.clone().dyn_into::<HtmlSelectElement>() {
                    select.set_value(default);
                }
            }
            label.append_child(&control)?;
            form.append_child(&label)?;
        }
    }
    if let CodexInputPrompt::McpToolApproval { review, .. } = prompt {
        let pre = doc.create_element("pre")?;
        pre.set_attribute("data-part", "review")?;
        pre.set_text_content(Some(review));
        form.append_child(&pre)?;
    }
    let actions = doc.create_element("div")?;
    actions.set_attribute("data-part", "actions")?;
    let labels: &[(&str, &str)] = match prompt {
        CodexInputPrompt::Questions { .. } => &[("accept", "Submit answers"), ("skip", "Skip")],
        CodexInputPrompt::McpForm { .. } => &[
            ("accept", "Accept"),
            ("decline", "Decline"),
            ("cancel", "Cancel"),
        ],
        CodexInputPrompt::McpToolApproval { .. } => &[
            ("accept", "Allow"),
            ("decline", "Deny"),
            ("cancel", "Cancel"),
        ],
    };
    for (kind, label) in labels {
        let button = doc.create_element("button")?;
        button.set_attribute(
            "type",
            if *kind == "accept" {
                "submit"
            } else {
                "button"
            },
        )?;
        button.set_attribute("data-part", "action")?;
        button.set_attribute("data-codex-input-kind", kind)?;
        if *kind != "accept" {
            button.set_attribute("data-codex-input-action", kind)?;
        }
        button.set_text_content(Some(label));
        actions.append_child(&button)?;
    }
    form.append_child(&actions)?;
    card.append_child(&form)?;
    Ok(card)
}

fn reply(panel: &Rc<RefCell<Panel>>, control: &Element, kind: &str) -> Result<(), FlowError> {
    let card = control
        .closest("[data-codex-request]")
        .map_err(js_error)?
        .ok_or_else(|| FlowError::InvalidInput("Codex request disappeared.".into()))?;
    let run = parse_id(card.get_attribute("data-codex-run"))?;
    let request = parse_id(card.get_attribute("data-codex-request"))?;
    let state = panel.borrow();
    let Some(input) = state
        .snapshot
        .as_ref()
        .and_then(|v| v.codex_input.as_ref())
        .filter(|i| i.run == run)
    else {
        return Ok(());
    };
    let Some(pending) = input.pending.iter().find(|p| p.request == request) else {
        return Ok(());
    };
    if !state.connected
        || !state.flow.is_connected()
        || state.codex_input_stopping.as_ref() == Some(&run)
        || state.codex_input_pending.is_some()
    {
        return Ok(());
    }
    let answer = match kind {
        "cancel" | "decline" => json!({"kind":kind}),
        "skip" => json!({"kind":"questions","value":{}}),
        "accept" => match &pending.prompt {
            CodexInputPrompt::Questions { questions, .. } => {
                let mut values = Map::new();
                for q in questions {
                    let mut selected = Vec::new();
                    let nodes = card
                        .query_selector_all("input[type=checkbox]")
                        .map_err(js_error)?;
                    for index in 0..nodes.length() {
                        if let Some(input) = nodes
                            .item(index)
                            .and_then(|n| n.dyn_into::<HtmlInputElement>().ok())
                        {
                            if input.name() == format!("q:{}", q.id) && input.checked() {
                                selected.push(Value::String(input.value()));
                            }
                        }
                    }
                    let other = card.query_selector_all("input[name]").map_err(js_error)?;
                    for index in 0..other.length() {
                        if let Some(input) = other
                            .item(index)
                            .and_then(|node| node.dyn_into::<HtmlInputElement>().ok())
                        {
                            if input.name() == format!("other:{}", q.id)
                                && !input.value().is_empty()
                            {
                                if input.value().chars().count() > 4096 {
                                    return Err(FlowError::InvalidInput(
                                        "Other answer exceeds 4096 characters.".into(),
                                    ));
                                }
                                selected.push(Value::String(input.value()));
                                break;
                            }
                        }
                    }
                    if selected.is_empty() {
                        return Err(FlowError::InvalidInput(
                            "Choose an answer for every question.".into(),
                        ));
                    }
                    values.insert(q.id.clone(), Value::Array(selected));
                }
                json!({"kind":"questions","value":values})
            }
            CodexInputPrompt::McpForm { fields, .. } => {
                let mut values = Map::new();
                for f in fields {
                    let name = format!("f:{}", f.name);
                    let controls = card
                        .query_selector_all("input[name],select[name]")
                        .map_err(js_error)?;
                    let mut field_value = None;
                    for index in 0..controls.length() {
                        if let Some(node) = controls
                            .item(index)
                            .and_then(|node| node.dyn_into::<Element>().ok())
                        {
                            let control_name = if node.tag_name() == "SELECT" {
                                node.clone()
                                    .dyn_into::<HtmlSelectElement>()
                                    .ok()
                                    .map(|s| s.name())
                            } else {
                                node.clone()
                                    .dyn_into::<HtmlInputElement>()
                                    .ok()
                                    .map(|i| i.name())
                            };
                            if control_name.as_deref() == Some(name.as_str()) {
                                field_value = if node.tag_name() == "SELECT" {
                                    node.dyn_into::<HtmlSelectElement>().ok().map(|s| s.value())
                                } else {
                                    node.dyn_into::<HtmlInputElement>().ok().map(|i| i.value())
                                };
                                break;
                            }
                        }
                    }
                    let value = field_value
                        .ok_or_else(|| FlowError::InvalidInput("Form value missing.".into()))?;
                    if value.chars().count() < f.min_chars as usize
                        || value.chars().count() > f.max_chars as usize
                        || f.choices
                            .as_ref()
                            .is_some_and(|choices| !choices.contains(&value))
                    {
                        return Err(FlowError::InvalidInput(
                            "Form value is outside the allowed range.".into(),
                        ));
                    }
                    values.insert(f.name.clone(), Value::String(value));
                }
                json!({"kind":"accept","value":values})
            }
            CodexInputPrompt::McpToolApproval { .. } => json!({"kind":"accept","value":{}}),
        },
        _ => return Ok(()),
    };
    let flow = state.flow.clone();
    let scope = state
        .snapshot
        .as_ref()
        .ok_or(FlowError::NoSource)?
        .selection
        .clone();
    let generation = state.connection_generation;
    let stopped_run = run.clone();
    drop(state);
    panel.borrow_mut().codex_input_pending = Some((run.clone(), request.clone()));
    set_disabled(&card, true);
    let panel = Rc::clone(panel);
    wasm_bindgen_futures::spawn_local(async move {
        match flow.codex_input_reply(scope, run, request, answer).await {
            Ok(view) if panel.borrow().connection_generation == generation => {
                crate::apply_view(&panel, view, generation);
                crate::set_panel_status(&panel, "Reply sent to Codex.");
            }
            Err(error) if panel.borrow().connection_generation == generation => {
                panel.borrow_mut().codex_input_pending = None;
                set_disabled(
                    &card,
                    panel.borrow().codex_input_stopping.as_ref() == Some(&stopped_run),
                );
                crate::set_panel_status(&panel, &format!("Codex reply failed: {error}"));
            }
            _ => (),
        }
    });
    Ok(())
}

fn set_disabled(card: &Element, disabled: bool) {
    if let Ok(nodes) = card.query_selector_all("button,input,select") {
        for index in 0..nodes.length() {
            if let Some(node) = nodes.item(index).and_then(|n| n.dyn_into::<Element>().ok()) {
                if disabled {
                    let _ = node.set_attribute("disabled", "");
                } else {
                    let _ = node.remove_attribute("disabled");
                }
            }
        }
    }
}
fn el(doc: &Document, id: &str) -> Result<Element, JsValue> {
    doc.get_element_by_id(id)
        .ok_or_else(|| err("Codex input UI target is missing"))
}
fn parse_id(value: Option<String>) -> Result<RunId, FlowError> {
    serde_json::from_value(json!(value.ok_or_else(|| FlowError::InvalidInput(
        "Codex request identity is missing.".into()
    ))?))
    .map_err(|_| FlowError::InvalidInput("Codex request identity is invalid.".into()))
}
fn js_error(error: JsValue) -> FlowError {
    FlowError::Host(crate::js_message(&error))
}
