use crate::{
    err,
    model::{PageView, RunRoute, RunView},
};
use std::{cell::RefCell, rc::Rc};

use crate::{
    Panel,
    model::{RunId, SourceId},
};

mod assistant;
mod tools;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Document, Element};

pub(super) fn render(doc: &Document, view: Option<&PageView>) -> Result<(), JsValue> {
    let history = doc
        .get_element_by_id("history")
        .ok_or_else(|| err("Result list is missing."))?;
    let store = view.map(|view| view.store.as_str()).unwrap_or("");
    if history.get_attribute("data-store").as_deref() != Some(store) {
        history.set_text_content(None);
        history.set_attribute("data-store", store)?;
    }
    let source = view.and_then(|view| view.source.as_ref());
    let runs: Vec<_> = view
        .into_iter()
        .flat_map(|view| &view.runs)
        .filter(|run| Some(&run.source) == source.map(|source| &source.id))
        .collect();
    let children = history.children();
    let cards: Vec<_> = (0..children.length())
        .filter_map(|i| children.item(i))
        .collect();
    for card in &cards {
        if !runs
            .iter()
            .any(|run| card.get_attribute("data-run").as_deref() == Some(run.id.as_str()))
        {
            history.remove_child(card)?;
        }
    }
    for (position, run) in runs.into_iter().enumerate() {
        let card = match cards
            .iter()
            .find(|card| card.get_attribute("data-run").as_deref() == Some(run.id.as_str()))
        {
            Some(card) => card.clone(),
            None => {
                let card = doc.create_element("article")?;
                card.set_class_name("run");
                card.set_attribute("data-run", run.id.as_str())?;
                card.set_inner_html(include_str!("message.html"));
                card
            }
        };
        update_card(doc, &card, run)?;
        let current = history.children().item(position as u32);
        if !current
            .as_ref()
            .is_some_and(|current| current.is_same_node(Some(&card)))
        {
            history.insert_before(&card, current.as_ref().map(|node| node.as_ref()))?;
        }
    }
    Ok(())
}

fn update_card(doc: &Document, card: &Element, run: &RunView) -> Result<(), JsValue> {
    card.set_attribute("data-kind", &run.kind.to_string())?;
    let route = match &run.route {
        RunRoute::LegacyUnspecified => "legacy route unspecified".to_string(),
        RunRoute::Fixture => "fixture".to_string(),
        RunRoute::Messages { model } => format!("Owned model fixture, model {model}"),
        RunRoute::HttpsMessages { model, .. } => format!("Configured HTTPS model, model {model}"),
        RunRoute::HttpsChatCompletions { endpoint, model, .. } => format!("Chat Completions, model {model}, HTTPS endpoint {endpoint}"),
        RunRoute::HttpsResponses { endpoint, model, .. } => format!("OpenAI Responses, model {model}, HTTPS endpoint {endpoint}"),
        RunRoute::HttpsGoogleInteractions { endpoint, model, .. } => format!("Google Interactions, model {model}, HTTPS endpoint {endpoint}"),
        RunRoute::Pi {
            provider, model, ..
        } => format!(
            "Pi configured provider {provider}, model {model}. Pi manages its endpoint, account, tools and permissions."
        ),
        RunRoute::Codex { provider, model, .. } => match (provider, model) {
            (Some(provider), Some(model)) => format!("Codex configured provider {provider}, model {model}. Codex manages its endpoint, account, tools and permissions."),
            _ => "Codex native session. Codex manages its provider, endpoint, account, tools and permissions.".to_string(),
        },
        RunRoute::OpenCode { model, mode, .. } => format!(
            "OpenCode native session{}{}.",
            model.as_ref().map(|value| format!(" using model {value}")).unwrap_or_default(),
            mode.as_ref().map(|value| format!(" in {value} mode")).unwrap_or_default(),
        ),
    };
    let agent_name = if matches!(&run.route, RunRoute::OpenCode { .. }) {
        "OpenCode"
    } else if matches!(&run.route, RunRoute::Codex { .. }) {
        "Codex"
    } else {
        "Pi"
    };
    update_part(
        card,
        "state",
        Some(&format!("{} | {} | ", run.kind, run.status)),
    )?;
    update_part(card, "route", Some(&route))?;
    update_part(card, "question", run.question.as_deref())?;
    update_part(card, "error", run.error.as_deref())?;
    if assistant::render(doc, card, run)? {
        write_text(&part(card, "copy-status")?, "");
    }
    let actions = part(card, "copy-actions")?;
    if run.output.text().is_empty() {
        actions.set_attribute("hidden", "")?;
    } else {
        actions.remove_attribute("hidden")?;
    }
    let copy = card
        .query_selector("[data-action='copy']")?
        .ok_or_else(|| err("Copy action missing."))?;
    copy.set_attribute("aria-label", &format!("Copy {} result", run.kind))?;
    let activity_group = part(card, "activity")?;
    let activity_count = part(card, "activity-count")?;
    let mut call_ids: Vec<_> = run
        .activity
        .iter()
        .filter_map(|activity| activity.call.as_ref().map(|call| call.id.as_str()))
        .collect();
    call_ids.sort_unstable();
    call_ids.dedup();
    let retained_calls = call_ids.len();
    let unavailable = run.activity.iter().any(|activity| activity.call.is_none());
    write_text(
        &activity_count,
        &format!(
            "{agent_name}, {retained_calls} retained calls, {} reports{}",
            run.activity.len(),
            if unavailable {
                "; details may be unavailable"
            } else {
                ""
            },
        ),
    );
    if run.activity.is_empty() {
        activity_group.set_attribute("hidden", "")?;
    } else {
        activity_group.remove_attribute("hidden")?;
    }
    let report_history = part(&activity_group, "report-history")?;
    let report_count = part(&activity_group, "report-count")?;
    write_text(&report_count, &format!("{} reports", run.activity.len()));
    if retained_calls > 0 && report_history.get_attribute("data-mode").is_none() {
        report_history.remove_attribute("open")?;
        report_history.set_attribute("data-mode", "calls")?;
    }
    let rows = part(card, "activity-rows")?;
    let activities = rows.query_selector_all("[data-activity]")?;
    for (index, activity) in run.activity.iter().enumerate() {
        let paragraph = match activities.item(index as u32) {
            Some(node) => node
                .dyn_into::<Element>()
                .map_err(|_| err("Result activity is not an element."))?,
            None => {
                let paragraph = doc.create_element("p")?;
                paragraph.set_attribute("data-activity", "")?;
                let tool = doc.create_element("span")?;
                tool.set_attribute("data-part", "activity-tool")?;
                let phase = doc.create_element("span")?;
                phase.set_attribute("data-part", "activity-phase")?;
                paragraph.append_child(&tool)?;
                paragraph.append_child(&phase)?;
                rows.append_child(&paragraph)?;
                paragraph
            }
        };
        let tool = paragraph
            .query_selector("[data-part='activity-tool']")?
            .ok_or_else(|| err("Result activity tool is missing."))?;
        let phase = paragraph
            .query_selector("[data-part='activity-phase']")?
            .ok_or_else(|| err("Result activity phase is missing."))?;
        write_text(&tool, &activity.tool);
        write_text(
            &phase,
            &activity.outcome.map_or_else(
                || activity.phase.to_string(),
                |outcome| format!("{}, {outcome}", activity.phase),
            ),
        );
    }
    for index in run.activity.len() as u32..activities.length() {
        if let Some(paragraph) = activities.item(index) {
            rows.remove_child(&paragraph)?;
        }
    }
    Ok(())
}

struct CopyTicket {
    store: String,
    source: SourceId,
    run: RunId,
    text: String,
    attempt: String,
    button: Element,
    status: Element,
}

pub(super) fn copy(panel: &Rc<RefCell<Panel>>, button: &Element) -> Result<(), JsValue> {
    let card = button
        .closest("article[data-run]")?
        .ok_or_else(|| err("Copy result missing."))?;
    let ticket = {
        let p = panel.borrow();
        let view = p
            .snapshot
            .as_ref()
            .ok_or_else(|| err("Result unavailable."))?;
        let source = view
            .source
            .as_ref()
            .ok_or_else(|| err("Source unavailable."))?;
        let run = view
            .runs
            .iter()
            .find(|run| {
                run.source == source.id
                    && card.get_attribute("data-run").as_deref() == Some(run.id.as_str())
            })
            .ok_or_else(|| err("Result unavailable."))?;
        let attempt = button
            .get_attribute("data-copy-attempt")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| err("Copy attempt limit reached."))?
            .to_string();
        button.set_attribute("data-copy-attempt", &attempt)?;
        CopyTicket {
            store: view.store.clone(),
            source: source.id.clone(),
            run: run.id.clone(),
            text: run.output.text().to_owned(),
            attempt,
            button: button.clone(),
            status: part(&card, "copy-status")?,
        }
    };
    let write = (|| {
        let navigator = crate::get(&js_sys::global(), "navigator")?;
        let clipboard = crate::get(&navigator, "clipboard")?;
        crate::call(&clipboard, "writeText", &[JsValue::from_str(&ticket.text)])?
            .dyn_into::<js_sys::Promise>()
    })();
    write_text(&ticket.status, "Copying result.");
    let panel = Rc::downgrade(panel);
    spawn_local(async move {
        let copied = match write {
            Ok(promise) => JsFuture::from(promise).await.is_ok(),
            Err(_) => false,
        };
        let Some(panel) = panel.upgrade() else {
            return;
        };
        let p = panel.borrow();
        let Some(view) = p.snapshot.as_ref() else {
            return;
        };
        if view.store != ticket.store
            || view.source.as_ref().map(|source| &source.id) != Some(&ticket.source)
            || !ticket.status.is_connected()
            || !ticket.button.is_connected()
            || ticket.button.get_attribute("data-copy-attempt").as_deref() != Some(&ticket.attempt)
        {
            return;
        }
        let Some(run) = view
            .runs
            .iter()
            .find(|run| run.id == ticket.run && run.source == ticket.source)
        else {
            return;
        };
        let Ok(Some(card)) = ticket.button.closest("article[data-run]") else {
            return;
        };
        if !part(&card, "copy-status").is_ok_and(|status| status.is_same_node(Some(&ticket.status)))
        {
            return;
        }
        let message = if !copied {
            "Could not copy. Select the result and copy it manually."
        } else if run.output.text() == ticket.text {
            "Copied result."
        } else {
            "Copied earlier text. Copy again for the latest result."
        };
        write_text(&ticket.status, message);
    });
    Ok(())
}

fn part(card: &Element, name: &str) -> Result<Element, JsValue> {
    card.query_selector(&format!("[data-part='{name}']"))?
        .ok_or_else(|| err("Result field is missing."))
}

fn update_part(card: &Element, name: &str, value: Option<&str>) -> Result<(), JsValue> {
    let element = part(card, name)?;
    write_text(&element, value.unwrap_or(""));
    if value.is_some() {
        element.remove_attribute("hidden")?;
    } else {
        element.set_attribute("hidden", "")?;
    }
    Ok(())
}

fn write_text(element: &Element, value: &str) {
    if element.text_content().as_deref() != Some(value) {
        element.set_text_content(Some(value));
    }
}
