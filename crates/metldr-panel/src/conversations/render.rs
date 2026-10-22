use wasm_bindgen::JsValue;
use web_sys::Element;

use crate::{Panel, err};

use super::{Operation, blocked};

pub(crate) fn render(panel: &Panel) -> Result<(), JsValue> {
    let doc = &panel.document;
    let state = &panel.conversations;
    let list = doc
        .get_element_by_id("conversation-list")
        .ok_or_else(|| err("Conversation rows are missing."))?;
    list.set_attribute("aria-busy", if state.busy { "true" } else { "false" })?;
    let query = state.query.trim().to_lowercase();
    let rows: Vec<_> = state
        .pages
        .iter()
        .flat_map(|page| page.conversations.iter())
        .filter(|item| item.title.text.to_lowercase().contains(&query))
        .collect();
    let existing: Vec<_> = (0..list.children().length())
        .filter_map(|index| list.children().item(index))
        .collect();
    for old in &existing {
        if !rows
            .iter()
            .any(|item| old.get_attribute("data-conversation").as_deref() == Some(item.id.as_str()))
        {
            list.remove_child(old)?;
        }
    }
    for (index, item) in rows.iter().enumerate() {
        let row = match existing
            .iter()
            .find(|row| row.get_attribute("data-conversation").as_deref() == Some(item.id.as_str()))
        {
            Some(row) => row.clone(),
            None => {
                let row = doc.create_element("button")?;
                row.set_attribute("type", "button")?;
                row.set_attribute("data-action", "conversation")?;
                row.set_attribute("data-conversation", &item.id)?;
                row
            }
        };
        if row.text_content().as_deref() != Some(item.title.text.as_str()) {
            row.set_text_content(Some(&item.title.text));
        }
        row.set_attribute("title", &item.title.text)?;
        let active = panel
            .snapshot
            .as_ref()
            .is_some_and(|view| view.selection.conversation == item.id);
        if active {
            row.set_attribute("aria-current", "page")?;
        } else {
            row.remove_attribute("aria-current")?;
        }
        disabled(&row, blocked(panel))?;
        let current = list.children().item(index as u32);
        if !current
            .as_ref()
            .is_some_and(|node| node.is_same_node(Some(&row)))
        {
            list.insert_before(&row, current.as_ref().map(|node| node.as_ref()))?;
        }
    }
    if let Some(status) = doc.get_element_by_id("conversation-status") {
        let default = if state.busy {
            "Loading conversations."
        } else if state.query.trim().is_empty() {
            "Chats lists saved conversations for the current store."
        } else if rows.is_empty() {
            "No loaded conversations match this search. Load more conversations to search further."
        } else {
            "Search covers the conversations loaded here."
        };
        status.set_text_content(Some(if state.message.is_empty() {
            default
        } else {
            &state.message
        }));
    }
    if let Some(more) = doc.query_selector("[data-action='conversation-more']")? {
        if state.pages.last().is_some_and(|page| page.next.is_some()) {
            more.remove_attribute("hidden")?;
        } else {
            more.set_attribute("hidden", "")?;
        }
        disabled(&more, state.busy || blocked(panel))?;
    }
    if let Some(refresh) = doc.query_selector("[data-action='conversation-refresh']")? {
        disabled(&refresh, state.busy || blocked(panel))?;
    }
    if let Some(clear) = doc.query_selector("[data-action='conversation-search-close']")? {
        if state.query.is_empty() {
            clear.set_attribute("hidden", "")?;
        } else {
            clear.remove_attribute("hidden")?;
        }
    }
    let recovery = matches!(
        state.operation.as_ref(),
        Some(Operation::Uncertain(_) | Operation::Receipt(_))
    );
    if let Some(container) = doc.get_element_by_id("conversation-recovery") {
        if recovery {
            container.remove_attribute("hidden")?;
        } else {
            container.set_attribute("hidden", "")?;
        }
    }
    if let Some(status) = doc.get_element_by_id("conversation-recovery-status") {
        status.set_text_content(Some(&state.message));
    }
    if let Some(retry) =
        doc.query_selector("#conversation-recovery [data-action='conversation-retry']")?
    {
        if matches!(state.operation.as_ref(), Some(Operation::Uncertain(_))) {
            retry.remove_attribute("hidden")?;
            retry.set_text_content(Some("Retry change"));
        } else if matches!(state.operation.as_ref(), Some(Operation::Receipt(_))) {
            retry.remove_attribute("hidden")?;
            retry.set_text_content(Some("Refresh current view"));
        } else {
            retry.set_attribute("hidden", "")?;
        }
        disabled(&retry, !panel.connected || state.busy)?;
    }
    if let Some(new_chat) = doc.query_selector("[data-action='conversation-new']")? {
        disabled(&new_chat, blocked(panel))?;
    }
    if let Some(trigger) = doc.get_element_by_id("conversations-trigger") {
        disabled(&trigger, !panel.connected || state.busy)?;
    }
    Ok(())
}

fn disabled(element: &Element, value: bool) -> Result<(), JsValue> {
    if value {
        element.set_attribute("disabled", "")
    } else {
        element.remove_attribute("disabled")
    }
}
