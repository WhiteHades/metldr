use crate::{
    Panel, err,
    flow::FlowClient,
    history::{Context, Cursor, RunPage, RunReference},
    model::{FlowError, RunView},
    results,
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Element, Event};

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    pub(super) context: Option<Context>,
    page: Option<RunPage>,
    pub(super) detail: Option<RunView>,
    busy: bool,
    serial: u64,
    exhausted: bool,
    message: String,
}

pub(super) fn busy(state: &State) -> bool {
    state.busy
}

pub(super) fn selection_changed(state: &mut State, changed: bool, generation: u64) {
    if (changed || state.generation != generation)
        && let Some(serial) = state.serial.checked_add(1)
    {
        state.serial = serial;
        state.busy = false;
        state.context = None;
        state.page = None;
        state.detail = None;
        state.message = "Saved results follow the selected conversation.".into();
    } else if state.serial == u64::MAX {
        state.exhausted = true;
        state.busy = false;
    }
    state.generation = generation;
}

pub(super) fn handle_click(
    panel: &Rc<RefCell<Panel>>,
    button: &Element,
    event: &Event,
) -> Result<bool, FlowError> {
    let action = button.get_attribute("data-action");
    if !matches!(
        action.as_deref(),
        Some("history-latest" | "history-earlier" | "history-open")
    ) {
        return Ok(false);
    }
    if !event.is_trusted() {
        return Ok(true);
    }
    let (flow, selection, generation, serial) = {
        let mut p = panel.borrow_mut();
        if p.saved_history.busy || p.saved_history.exhausted || !p.connected {
            return Ok(true);
        }
        let Some(serial) = p.saved_history.serial.checked_add(1) else {
            p.saved_history.exhausted = true;
            p.saved_history.message = "History request capacity reached.".into();
            return Ok(true);
        };
        let selection = p.snapshot.as_ref().map(|view| view.selection.clone());
        let Some(selection) = selection else {
            return Ok(true);
        };
        p.saved_history.serial = serial;
        p.saved_history.busy = true;
        p.saved_history.message = "Loading saved work.".into();
        (p.flow.clone(), selection, p.connection_generation, serial)
    };
    let request = match action.as_deref() {
        Some("history-open") => {
            let p = panel.borrow();
            let id = button.get_attribute("data-history-run");
            p.saved_history
                .context
                .clone()
                .zip(p.saved_history.page.as_ref().and_then(|page| {
                    page.runs
                        .iter()
                        .find(|run| Some(run.id.as_str()) == id.as_deref())
                        .cloned()
                }))
                .map(|(context, summary)| Request::Run(context, summary.reference()))
        }
        Some("history-earlier") => {
            let p = panel.borrow();
            p.saved_history
                .context
                .clone()
                .zip(
                    p.saved_history
                        .page
                        .as_ref()
                        .and_then(|page| page.next.clone()),
                )
                .map(|(context, cursor)| Request::Page(Some((context, cursor))))
        }
        _ => Some(Request::Page(None)),
    };
    let Some(request) = request else {
        let mut p = panel.borrow_mut();
        p.saved_history.busy = false;
        return Ok(true);
    };
    crate::render(&panel.borrow());
    let panel = Rc::clone(panel);
    spawn_local(async move {
        let result = read(&flow, request, &selection).await;
        let mut p = panel.borrow_mut();
        if p.connection_generation != generation
            || p.saved_history.serial != serial
            || p.snapshot
                .as_ref()
                .is_none_or(|view| view.selection != selection)
        {
            return;
        }
        p.saved_history.busy = false;
        match result {
            Ok(Read::Page(context, page)) => {
                if context.scope != selection {
                    return;
                }
                if p.saved_history.context.as_ref() != Some(&context) {
                    p.saved_history.detail = None;
                }
                p.saved_history.context = Some(context);
                p.saved_history.message = if page.runs.is_empty() {
                    "No saved results.".into()
                } else {
                    "Choose a result to read. Your active task stays unchanged.".into()
                };
                p.saved_history.page = Some(page);
            }
            Ok(Read::Run(run)) => {
                p.saved_history.detail = Some(run);
                p.saved_history.message =
                    "Saved result opened. Your active task stays unchanged.".into();
            }
            Err(error) => p.saved_history.message = error.to_string(),
        }
        crate::render(&p);
    });
    Ok(true)
}

enum Request {
    Page(Option<(Context, Cursor)>),
    Run(Context, RunReference),
}
enum Read {
    Page(Context, RunPage),
    Run(RunView),
}
async fn read(
    flow: &FlowClient,
    request: Request,
    selection: &crate::history::Scope,
) -> Result<Read, FlowError> {
    match request {
        Request::Page(previous) => {
            let (context, cursor) = match previous {
                Some((context, cursor)) => (context, Some(cursor)),
                None => {
                    let context = flow.history_current().await?;
                    if &context.scope != selection {
                        return Err(FlowError::HistoryStale);
                    }
                    (context, None)
                }
            };
            let page = flow.history_runs(&context, cursor).await?;
            Ok(Read::Page(context, page))
        }
        Request::Run(context, summary) => {
            if &context.scope != selection {
                return Err(FlowError::HistoryStale);
            }
            Ok(Read::Run(flow.history_run(&context, &summary).await?))
        }
    }
}

pub(super) fn render(panel: &Panel) -> Result<(), JsValue> {
    let doc = &panel.document;
    let state = &panel.saved_history;
    let list = doc
        .get_element_by_id("saved-history-list")
        .ok_or_else(|| err("Saved result list missing."))?;
    let rows = state
        .page
        .as_ref()
        .map(|page| page.runs.as_slice())
        .unwrap_or(&[]);
    let existing: Vec<_> = (0..list.children().length())
        .filter_map(|i| list.children().item(i))
        .collect();
    for old in &existing {
        if !rows
            .iter()
            .any(|run| old.get_attribute("data-history-run").as_deref() == Some(run.id.as_str()))
        {
            list.remove_child(old)?;
        }
    }
    for (i, summary) in rows.iter().enumerate() {
        let row = match existing.iter().find(|row| {
            row.get_attribute("data-history-run").as_deref() == Some(summary.id.as_str())
        }) {
            Some(row) => row.clone(),
            None => {
                let row = doc.create_element("button")?;
                row.set_attribute("type", "button")?;
                row.set_attribute("data-action", "history-open")?;
                row.set_attribute("data-history-run", summary.id.as_str())?;
                row
            }
        };
        let title = summary
            .question
            .as_ref()
            .map(|preview| preview.text.as_str())
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| summary.output.text.as_str());
        let title = if title.is_empty() {
            summary.kind.to_string()
        } else {
            title.to_owned()
        };
        let label = format!("{}, {}: {}", summary.kind, summary.status, title);
        if row.text_content().as_deref() != Some(&label) {
            row.set_text_content(Some(&label));
        }
        row.set_attribute(
            "aria-pressed",
            if state
                .detail
                .as_ref()
                .is_some_and(|run| run.id == summary.id)
            {
                "true"
            } else {
                "false"
            },
        )?;
        disabled(&row, state.busy || !panel.connected)?;
        let current = list.children().item(i as u32);
        if !current
            .as_ref()
            .is_some_and(|item| item.is_same_node(Some(&row)))
        {
            list.insert_before(&row, current.as_ref().map(|node| node.as_ref()))?;
        }
    }
    if let Some(status) = doc.get_element_by_id("saved-history-status") {
        let text = if state.message.is_empty() {
            "Open saved results without starting a task."
        } else {
            &state.message
        };
        if status.text_content().as_deref() != Some(text) {
            status.set_text_content(Some(text));
        }
    }
    for action in ["history-latest", "history-earlier"] {
        if let Some(button) = doc.query_selector(&format!("[data-action='{action}']"))? {
            disabled(&button, state.busy || !panel.connected)?;
            if action == "history-earlier" {
                if state.page.as_ref().is_some_and(|page| page.next.is_some()) {
                    button.remove_attribute("hidden")?;
                } else {
                    button.set_attribute("hidden", "")?;
                }
            }
        }
    }
    results::render_saved(doc, state.detail.as_ref())
}

fn disabled(element: &Element, value: bool) -> Result<(), JsValue> {
    if value {
        element.set_attribute("disabled", "")
    } else {
        element.remove_attribute("disabled")
    }
}
