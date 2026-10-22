use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, Element, Event, HtmlInputElement};

use crate::{
    Panel, active_run, err,
    flow::ConversationRequest,
    history::{Change, Context, ConversationPage, Cursor, Scope},
    model::FlowError,
};

mod render;
pub(super) use render::render;

#[derive(Default)]
pub(super) struct State {
    context: Option<Context>,
    pages: Vec<ConversationPage>,
    query: String,
    busy: bool,
    serial: u64,
    generation: u64,
    exhausted: bool,
    message: String,
    operation: Option<Operation>,
}

enum Operation {
    Changing(ConversationRequest),
    Uncertain(ConversationRequest),
    Receipt(Context),
}

pub(super) fn install(doc: &Document, panel: &Rc<RefCell<Panel>>) -> Result<(), JsValue> {
    let query = doc
        .get_element_by_id("conversation-query")
        .ok_or_else(|| err("Conversation search is missing."))?;
    let changed_panel = Rc::clone(panel);
    let changed = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        let query = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlInputElement>().ok())
            .map(|input| input.value())
            .unwrap_or_default();
        changed_panel.borrow_mut().conversations.query = query;
        crate::render(&changed_panel.borrow());
    });
    query.add_event_listener_with_callback("input", changed.as_ref().unchecked_ref())?;
    changed.forget();

    let popover = doc
        .get_element_by_id("conversations")
        .ok_or_else(|| err("Conversation list is missing."))?;
    let focus_doc = doc.clone();
    let focus_panel = Rc::clone(panel);
    let toggle = Closure::<dyn FnMut(Event)>::new(move |_| {
        if focus_doc
            .get_element_by_id("conversations")
            .is_some_and(|popover| popover.matches(":popover-open").unwrap_or(false))
        {
            if let Some(search) = focus_doc
                .get_element_by_id("conversation-query")
                .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
            {
                let _ = search.focus();
            }
            if focus_panel.borrow().conversations.pages.is_empty() {
                begin_read(&focus_panel, None);
            }
        }
    });
    popover.add_event_listener_with_callback("toggle", toggle.as_ref().unchecked_ref())?;
    toggle.forget();

    Ok(())
}

pub(super) fn handle_click(
    panel: &Rc<RefCell<Panel>>,
    button: &Element,
    event: &Event,
) -> Result<bool, FlowError> {
    let Some(action) = button.get_attribute("data-action") else {
        return Ok(false);
    };
    match action.as_str() {
        "conversation-search" if event.is_trusted() => {
            if let Some(input) = panel
                .borrow()
                .document
                .get_element_by_id("conversation-query")
                .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
            {
                let _ = input.focus();
            }
            Ok(true)
        }
        "conversation-search-close" if event.is_trusted() => {
            panel.borrow_mut().conversations.query.clear();
            if let Some(input) = panel
                .borrow()
                .document
                .get_element_by_id("conversation-query")
                .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
            {
                input.set_value("");
                let _ = input.focus();
            }
            crate::render(&panel.borrow());
            Ok(true)
        }
        "conversation-new" if event.is_trusted() => {
            close(panel);
            change(panel, Change::Create)?;
            Ok(true)
        }
        "conversation" if event.is_trusted() => {
            let Some(id) = button.get_attribute("data-conversation") else {
                return Ok(true);
            };
            close(panel);
            change(panel, Change::Select { conversation: id })?;
            Ok(true)
        }
        "conversation-more" if event.is_trusted() => {
            load_more(panel);
            Ok(true)
        }
        "conversation-refresh" if event.is_trusted() => {
            begin_read(panel, None);
            Ok(true)
        }
        "conversation-retry" if event.is_trusted() => {
            retry(panel);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn retry_blocked(panel: &Panel) -> bool {
    !panel.connected
        || panel.start_pending
        || active_run(&panel.snapshot).is_some()
        || panel.browser_decision_pending.is_some()
        || panel.native_permission_pending.is_some()
        || panel.pi_ui_pending.is_some()
        || panel.pi_ui_stopping.is_some()
        || panel.codex_input_pending.is_some()
        || panel.codex_input_stopping.is_some()
        || crate::history_ui::busy(&panel.saved_history)
        || panel.conversations.busy
        || crate::provider_settings::interlocked()
        || crate::agent_settings::interlocked()
}

fn blocked(panel: &Panel) -> bool {
    retry_blocked(panel) || panel.conversations.operation.is_some()
}

pub(super) fn start_blocked(panel: &Panel) -> bool {
    panel.conversations.operation.is_some()
}

pub(super) fn settings_blocked(panel: &Panel) -> bool {
    panel.conversations.operation.is_some()
}

pub(super) fn selection_changed(
    state: &mut State,
    changed: bool,
    selection: &Scope,
    generation: u64,
) {
    if changed
        || state.generation != generation
        || state
            .context
            .as_ref()
            .is_some_and(|context| context.scope != *selection)
    {
        if let Some(serial) = state.serial.checked_add(1) {
            state.serial = serial;
        } else {
            state.exhausted = true;
        }
        state.context = None;
        state.pages.clear();
        state.busy = false;
        state.message.clear();
    }
    state.generation = generation;
}

pub(super) fn disconnected(state: &mut State) {
    if let Some(operation) = state.operation.take() {
        state.operation = Some(match operation {
            Operation::Changing(request) => {
                state.message =
                    "The change has no receipt. Reconnect, then retry this exact request.".into();
                Operation::Uncertain(request)
            }
            operation => operation,
        });
    }
}

pub(super) fn reconcile(
    state: &mut State,
    view: &crate::model::PageView,
    scope_changed: bool,
    generation: u64,
) {
    selection_changed(state, scope_changed, &view.selection, generation);
}

fn begin_read(panel: &Rc<RefCell<Panel>>, cursor: Option<Cursor>) {
    let (flow, selection, generation, serial) = {
        let mut state = panel.borrow_mut();
        if state.conversations.busy || blocked(&state) || state.conversations.exhausted {
            return;
        }
        let Some(selection) = state.snapshot.as_ref().map(|view| view.selection.clone()) else {
            return;
        };
        let Some(serial) = state.conversations.serial.checked_add(1) else {
            state.conversations.exhausted = true;
            state.conversations.message = "Conversation list request capacity reached.".into();
            return;
        };
        state.conversations.serial = serial;
        state.conversations.busy = true;
        (
            state.flow.clone(),
            selection,
            state.connection_generation,
            serial,
        )
    };
    crate::render(&panel.borrow());
    let panel = Rc::clone(panel);
    spawn_local(async move {
        let result = async {
            let context = flow.history_current().await?;
            if context.scope != selection {
                return Err(FlowError::HistoryStale);
            }
            let page = flow.history_conversations(&context, cursor.clone()).await?;
            Ok::<_, FlowError>((context, page))
        }
        .await;
        let mut state = panel.borrow_mut();
        if state.connection_generation != generation
            || state.conversations.serial != serial
            || state
                .snapshot
                .as_ref()
                .is_none_or(|view| view.selection != selection)
        {
            return;
        }
        state.conversations.busy = false;
        match result {
            Ok((context, page)) => {
                if cursor.is_none() {
                    state.conversations.pages.clear();
                    state.conversations.context = Some(context.clone());
                }
                if state.conversations.context.as_ref() == Some(&context) {
                    state.conversations.pages.push(page);
                    state.conversations.message.clear();
                } else {
                    state.conversations.message =
                        "Saved conversations changed. Refresh the list.".into();
                    state.conversations.pages.clear();
                    state.conversations.context = None;
                }
            }
            Err(FlowError::HistoryBusy) => {
                state.conversations.message = "Saved history is busy. Try again shortly.".into()
            }
            Err(error) => {
                state.conversations.message = format!("Could not load conversations: {error}")
            }
        }
        drop(state);
        crate::render(&panel.borrow());
    });
}

fn load_more(panel: &Rc<RefCell<Panel>>) {
    let cursor = panel
        .borrow()
        .conversations
        .pages
        .last()
        .and_then(|page| page.next.clone());
    if let Some(cursor) = cursor {
        begin_read(panel, Some(cursor));
    }
}

fn close(panel: &Rc<RefCell<Panel>>) {
    if let Some(popover) = panel
        .borrow()
        .document
        .get_element_by_id("conversations")
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = popover.hide_popover();
    }
}

fn change(panel: &Rc<RefCell<Panel>>, change: Change) -> Result<(), FlowError> {
    let request = {
        let state = panel.borrow();
        if blocked(&state) {
            return Ok(());
        }
        let scope = state
            .snapshot
            .as_ref()
            .ok_or(FlowError::NoSource)?
            .selection
            .clone();
        state.flow.conversation_request(scope, change)?
    };
    start_change(panel, request);
    Ok(())
}

fn retry(panel: &Rc<RefCell<Panel>>) {
    enum Retry {
        Change(ConversationRequest),
        Refresh(Context),
    }
    let action = {
        let state = panel.borrow();
        if retry_blocked(&state) {
            return;
        }
        match state.conversations.operation.as_ref() {
            Some(Operation::Uncertain(request)) => Retry::Change(request.clone()),
            Some(Operation::Receipt(context)) => Retry::Refresh(context.clone()),
            _ => return,
        }
    };
    match action {
        Retry::Change(request) => start_change(panel, request),
        Retry::Refresh(context) => refresh_receipt(panel, context),
    }
}

fn refresh_receipt(panel: &Rc<RefCell<Panel>>, expected: Context) {
    let (flow, generation) = {
        let state = panel.borrow();
        (state.flow.clone(), state.connection_generation)
    };
    panel.borrow_mut().conversations.message = "Refreshing the confirmed conversation view.".into();
    crate::render(&panel.borrow());
    let panel = Rc::clone(panel);
    spawn_local(async move {
        match flow.current_page().await {
            Ok(view) if panel.borrow().connection_generation == generation => {
                let matched = apply_receipt_view(&panel, view, generation, &expected);
                if !matched {
                    panel.borrow_mut().conversations.message = "The current view has not reached the confirmed conversation yet. Refresh to check again.".into();
                    crate::render(&panel.borrow());
                }
            }
            Err(error) if panel.borrow().connection_generation == generation => {
                panel.borrow_mut().conversations.message = format!(
                    "Confirmed change remains saved; current view refresh failed: {error}."
                );
                crate::render(&panel.borrow());
            }
            _ => {}
        }
    });
}

fn apply_receipt_view(
    panel: &Rc<RefCell<Panel>>,
    view: crate::model::PageView,
    generation: u64,
    expected: &Context,
) -> bool {
    let (accepted, exact_scope) = {
        let state = panel.borrow();
        let current = state
            .snapshot
            .as_ref()
            .filter(|old| view.is_older_than(old))
            .unwrap_or(&view);
        let accepted = state.connection_generation == generation
            && view.selection.store == expected.scope.store
            && current.selection.store == expected.scope.store
            && (current.selection == expected.scope
                || current.selection.epoch.0 > expected.scope.epoch.0);
        (accepted, current.selection == expected.scope)
    };
    if !accepted {
        return false;
    }
    crate::apply_view(panel, view, generation);
    let mut state = panel.borrow_mut();
    if state.connection_generation == generation
        && matches!(state.conversations.operation.as_ref(), Some(Operation::Receipt(context)) if context == expected)
    {
        state.conversations.operation = None;
        state.conversations.message = if exact_scope {
            "Conversation changed.".into()
        } else {
            "The confirmed change was superseded. Showing the current conversation.".into()
        };
        drop(state);
        crate::render(&panel.borrow());
        true
    } else {
        false
    }
}

fn start_change(panel: &Rc<RefCell<Panel>>, request: ConversationRequest) {
    let (flow, generation) = {
        let mut state = panel.borrow_mut();
        state.conversations.operation = Some(Operation::Changing(request.clone()));
        state.conversations.message = "Changing conversation.".into();
        (state.flow.clone(), state.connection_generation)
    };
    crate::render(&panel.borrow());
    let panel = Rc::clone(panel);
    spawn_local(async move {
        match flow.change_conversation(&request).await {
            Ok(receipt) => {
                if panel.borrow().connection_generation != generation {
                    return;
                }
                panel.borrow_mut().conversations.operation =
                    Some(Operation::Receipt(receipt.current.clone()));
                panel.borrow_mut().conversations.message =
                    "Loading the saved conversation view.".into();
                crate::render(&panel.borrow());
                match flow.current_page().await {
                    Ok(view) if panel.borrow().connection_generation == generation => {
                        if !apply_receipt_view(&panel, view, generation, &receipt.current) {
                            panel.borrow_mut().conversations.message = "Change confirmed; current view has not reached the requested conversation yet. Retry to check again.".into();
                            crate::render(&panel.borrow());
                        }
                    }
                    Err(error) if panel.borrow().connection_generation == generation => {
                        panel.borrow_mut().conversations.message = format!(
                            "Change confirmed; current view refresh failed: {error}. Reconnect to load it."
                        );
                        crate::render(&panel.borrow());
                    }
                    _ => {}
                }
            }
            Err(error) if panel.borrow().connection_generation == generation => {
                let mut state = panel.borrow_mut();
                if matches!(&error, FlowError::HistoryBusy | FlowError::HistoryStale) {
                    state.conversations.operation = None;
                    state.conversations.message = match &error {
                        FlowError::HistoryBusy => {
                            "Saved history is busy. The conversation did not change.".into()
                        }
                        _ => "The saved conversation list changed. Refresh it before trying again."
                            .into(),
                    };
                } else {
                    state.conversations.operation = Some(Operation::Uncertain(request.clone()));
                    state.conversations.message =
                        "Could not confirm the change. Retry to check whether it was saved.".into();
                }
                drop(state);
                crate::render(&panel.borrow());
                if matches!(&error, FlowError::HistoryBusy | FlowError::HistoryStale) {
                    begin_read(&panel, None);
                }
            }
            _ => {}
        }
    });
}
