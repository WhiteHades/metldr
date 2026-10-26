use metldr_history_pages::WIRE_VERSION;

use std::{cell::RefCell, rc::Rc};

use js_sys::{Array, Function, Object, Promise, Reflect};
use serde_json::{Value, json};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::*};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Document, Element, Event};

mod ag_ui;
mod agent_settings;
mod appearance;
mod browser_action_display;
mod browser_background;
mod codex_input;
mod composer;
mod composer_ui;
mod conversations;
mod execution_preference;
mod flow;
mod history;
mod history_ui;
mod interaction_audio;
mod model;
mod pi_ui;
mod preferences;
mod profile;
mod provider_settings;
mod readiness;
mod results;
mod run_display;
mod site;
mod summary;
mod ui_bridge;
mod view;
use flow::{FlowClient, StopReceipt};
use model::{
    BrowserActionId, BrowserActionPhase, FlowError, PageView, RunId, RunStatus, SourceStatus,
};

const HOST: &str = "io.metldr.first_flow";
const PANEL_PORT: &str = "metldr-first-flow";
const MAX_FRAME: usize = 65_536;
const MAX_PAGE: usize = 8_192;

thread_local! {
    static BACKGROUND: RefCell<Background> = RefCell::new(Background::default());
}

#[derive(Default)]
struct Background {
    native: Option<JsValue>,
    readiness: readiness::Gate,
    panels: Vec<JsValue>,
    watches: Vec<PanelWatch>,
    next_cleanup: u64,
}

struct PanelWatch {
    panel: JsValue,
    request: String,
    scope: metldr_history_pages::Scope,
    run: RunId,
    queued: bool,
}

struct Panel {
    saved_history: history_ui::State,
    conversations: conversations::State,
    composer: composer_ui::State,
    flow: FlowClient,
    document: Document,
    snapshot: Option<PageView>,
    connected: bool,
    start_pending: bool,
    correction_pending: Option<CorrectionSubmission>,
    status_message: String,
    browser_decision_pending: Option<(BrowserActionId, u64)>,
    native_permission_pending: Option<(RunId, RunId)>,
    pi_ui_pending: Option<(RunId, RunId)>,
    pi_ui_stopping: Option<RunId>,
    codex_input_pending: Option<(RunId, RunId)>,
    codex_input_stopping: Option<RunId>,
    stale_generation: u64,
    connection_generation: u64,
}

#[derive(Clone)]
struct CorrectionSubmission {
    request: String,
    scope: metldr_history_pages::Scope,
    run: RunId,
    text: String,
}

#[wasm_bindgen]
pub fn accept_panel(port: JsValue) {
    let result = (|| -> Result<(), JsValue> {
        if string_property(&port, "name")? != PANEL_PORT {
            return Ok(());
        }
        let runtime = chrome_api("runtime")?;
        let extension_id = string_property(&runtime, "id")?;
        let origin = format!("chrome-extension://{extension_id}");
        let sender = get(&port, "sender")?;
        if string_property(&sender, "id")? != extension_id
            || string_property(&sender, "origin")? != origin
            || !panel_url_allowed(&origin, &string_property(&sender, "url")?)
        {
            let _ = call(&port, "disconnect", &[]);
            return Ok(());
        }
        if BACKGROUND.with(|state| state.borrow().native.is_none()) {
            connect_native()?;
        }
        if BACKGROUND.with(|state| state.borrow().panels.len() >= 16) {
            let _ = call(&port, "disconnect", &[]);
            return Ok(());
        }
        BACKGROUND.with(|state| state.borrow_mut().panels.push(port.clone()));
        let reply_port = port.clone();
        listen(&port, "onMessage", move |message| {
            let request = get(&message, "request").ok().and_then(|v| v.as_string());
            if let Err(error) = forward_request(&reply_port, message) {
                if let Some(request) = request {
                    let message: String = js_message(&error).chars().take(64).collect();
                    if let Ok(raw) = serde_json::to_string(
                        &json!({"v":crate::WIRE_VERSION,"request":request,"kind":"error","message":message}),
                    ) {
                        if let Ok(value) = js_sys::JSON::parse(&raw) {
                            let _ = call(&reply_port, "postMessage", &[value]);
                        }
                    }
                }
            }
        })?;
        let disconnected_port = port.clone();
        listen(&port, "onDisconnect", move |_| {
            remove_panel(&disconnected_port)
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        web_sys::console::error_1(&error);
    }
}

#[wasm_bindgen]
pub fn start_panel(capture: Function) {
    flow::install_capture(capture);
    spawn_local(async {
        if let Err(error) = panel_start().await {
            web_sys::console::error_1(&error);
        }
    });
}

async fn panel_start() -> Result<(), JsValue> {
    let document = web_sys::window()
        .ok_or_else(|| err("window unavailable"))?
        .document()
        .ok_or_else(|| err("document unavailable"))?;
    view::mount(&document)?;
    let preferences = Rc::new(preferences::UiPreferences::new()?);
    if let Err(error) = interaction_audio::install(&document, Rc::clone(&preferences)) {
        web_sys::console::error_1(&error);
    }
    if let Err(error) = appearance::install(&document, preferences) {
        web_sys::console::error_1(&error);
        if let Some(status) = document.get_element_by_id("appearance-status") {
            status.set_text_content(Some(
                "Appearance is unavailable. Reload the panel to retry.",
            ));
        }
    }
    let flow = match FlowClient::connect().await {
        Ok(flow) => flow,
        Err(error) => {
            view::startup_failed(&document)?;
            if document.url()?.starts_with("chrome-extension://") {
                web_sys::console::error_1(&err(&error.to_string()));
            }
            return Ok(());
        }
    };
    let panel = Rc::new(RefCell::new(Panel {
        saved_history: Default::default(),
        conversations: Default::default(),
        composer: composer_ui::State::default(),
        flow,
        document: document.clone(),
        snapshot: None,
        connected: false,
        start_pending: false,
        correction_pending: None,
        status_message: "Reconnect loads the saved view without starting work.".into(),
        browser_decision_pending: None,
        native_permission_pending: None,
        pi_ui_pending: None,
        pi_ui_stopping: None,
        codex_input_pending: None,
        codex_input_stopping: None,
        stale_generation: 0,
        connection_generation: 0,
    }));
    provider_settings::install(&document, &panel)?;
    agent_settings::install(&document, &panel)?;
    conversations::install(&document, &panel)?;
    let click = Rc::clone(&panel);
    let listener = Closure::<dyn FnMut(Event)>::new(move |event| {
        if let Err(error) = handle_click(&click, event) {
            set_panel_status(&click, &format!("Request failed: {error}"));
        }
    });
    document.add_event_listener_with_callback("click", listener.as_ref().unchecked_ref())?;
    listener.forget();

    pi_ui::install(&document, &panel)?;
    codex_input::install(&document, &panel)?;

    composer::install(&panel)?;
    composer_ui::install(&panel)?;

    install_flow(&panel).map_err(|e| err(&e.to_string()))?;
    reconnect(Rc::clone(&panel));
    Ok(())
}

fn capture_page(panel: &Rc<RefCell<Panel>>) {
    {
        let state = panel.borrow();
        if !state.connected
            || state.start_pending
            || active_run(&state.snapshot).is_some()
            || conversations::start_blocked(&state)
        {
            return;
        }
    }
    let (flow, generation) = {
        let p = panel.borrow();
        (p.flow.clone(), p.connection_generation)
    };
    begin_start(panel);
    let capture = {
        let p = panel.borrow();
        let Some(view) = p.snapshot.as_ref() else {
            return;
        };
        flow.capture_selected(view)
    };
    let panel = Rc::clone(panel);
    spawn_local(async move {
        let result = capture.await;
        if panel.borrow().connection_generation != generation {
            return;
        }
        panel.borrow_mut().start_pending = false;
        match result {
            Ok(view) => {
                let selection = view.selection.clone();
                apply_view(&panel, view, generation);
                if panel
                    .borrow()
                    .snapshot
                    .as_ref()
                    .is_some_and(|current| current.selection == selection)
                {
                    set_panel_status(
                        &panel,
                        "Page saved. Choose a destination and send your request.",
                    );
                }
            }
            Err(error) => {
                set_panel_status(
                    &panel,
                    &format!(
                        "Capture failed: {error}. Reconnect to check the saved source before trying again."
                    ),
                );
                render(&panel.borrow());
            }
        }
    });
}

fn handle_click(panel: &Rc<RefCell<Panel>>, event: Event) -> Result<(), FlowError> {
    let Some(target) = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
    else {
        return Ok(());
    };
    let Some(button) = target
        .closest("[data-action]")
        .map_err(|e| FlowError::Host(js_message(&e)))?
    else {
        return Ok(());
    };
    if pi_ui::handle_click(panel, &button, &event)? {
        return Ok(());
    }
    if conversations::handle_click(panel, &button, &event)? {
        return Ok(());
    }
    if history_ui::handle_click(panel, &button, &event)? {
        return Ok(());
    }
    let action = button.get_attribute("data-action");
    if matches!(action.as_deref(), Some("summarize")) {
        let p = panel.borrow();
        if !p.connected
            || p.start_pending
            || active_run(&p.snapshot).is_some()
            || conversations::start_blocked(&p)
        {
            return Ok(());
        }
    }
    match action.as_deref() {
        Some("native-permission") if event.is_trusted() => {
            let run: RunId =
                serde_json::from_value(json!(button.get_attribute("data-native-run").ok_or_else(
                    || FlowError::InvalidInput("Native run identity is missing.".into())
                )?))
                .map_err(|_| FlowError::InvalidInput("Native run identity is invalid.".into()))?;
            let request: RunId = serde_json::from_value(json!(
                button.get_attribute("data-native-request").ok_or_else(|| {
                    FlowError::InvalidInput("Native request identity is missing.".into())
                })?
            ))
            .map_err(|_| FlowError::InvalidInput("Native request identity is invalid.".into()))?;
            let option = button.get_attribute("data-native-option");
            let (flow, scope, generation) = {
                let mut p = panel.borrow_mut();
                let pending = p.connected
                    && p.native_permission_pending.is_none()
                    && active_run(&p.snapshot) == Some(&run)
                    && p.snapshot.as_ref().is_some_and(|view| {
                        view.native_permissions.iter().any(|item| {
                            item.run == run
                                && item.request == request
                                && option.as_ref().is_none_or(|id| {
                                    item.options.iter().any(|offered| &offered.id == id)
                                })
                        })
                    });
                if !pending {
                    return Ok(());
                }
                p.native_permission_pending = Some((run.clone(), request.clone()));
                let scope = p
                    .snapshot
                    .as_ref()
                    .ok_or(FlowError::NoSource)?
                    .selection
                    .clone();
                (p.flow.clone(), scope, p.connection_generation)
            };
            disable_native_permission_buttons(panel);
            set_panel_status(
                panel,
                "Sending native permission decision to the local host.",
            );
            let panel = Rc::clone(panel);
            spawn_local(async move {
                let result = flow.native_permission(scope, run, request, option).await;
                let mut p = panel.borrow_mut();
                if p.connection_generation != generation {
                    return;
                }
                match result {
                    Ok(view) => {
                        p.native_permission_pending = None;
                        drop(p);
                        apply_view(&panel, view, generation);
                    }
                    Err(error) => {
                        if !matches!(&error, FlowError::Timeout | FlowError::Disconnected) {
                            p.native_permission_pending = None;
                        }
                        p.status_message = if matches!(
                            &error,
                            FlowError::Timeout | FlowError::Disconnected
                        ) {
                            "No permission receipt arrived. Reconnect to check the saved request before trying again.".into()
                        } else {
                            format!("Permission decision failed: {error}")
                        };
                        render(&p);
                    }
                }
            });
        }
        Some("browser-approve" | "browser-decline") if event.is_trusted() => {
            let approve = action.as_deref() == Some("browser-approve");
            let id = button.get_attribute("data-browser-action").ok_or_else(|| {
                FlowError::InvalidInput("Browser action identity is missing.".into())
            })?;
            let id: BrowserActionId = serde_json::from_value(json!(id)).map_err(|_| {
                FlowError::InvalidInput("Browser action identity is invalid.".into())
            })?;
            let revision = button
                .get_attribute("data-revision")
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|revision| *revision > 0)
                .ok_or_else(|| {
                    FlowError::InvalidInput("Browser action revision is invalid.".into())
                })?;
            let (flow, scope, generation) = {
                let mut p = panel.borrow_mut();
                let pending = p.connected
                    && p.browser_decision_pending.is_none()
                    && p.snapshot.as_ref().is_some_and(|view| {
                        view.browser_actions.iter().any(|item| {
                            item.id == id
                                && item.revision == revision
                                && item.phase == BrowserActionPhase::Pending
                        })
                    });
                let current_document = p.snapshot.as_ref().is_some_and(|view| {
                    view.browser_actions.iter().any(|item| {
                        item.id == id
                            && item.revision == revision
                            && view.can_approve(item, p.flow.source_epoch())
                    })
                });
                if !pending || (approve && !current_document) {
                    return Ok(());
                }
                p.browser_decision_pending = Some((id.clone(), revision));
                let scope = p
                    .snapshot
                    .as_ref()
                    .ok_or(FlowError::NoSource)?
                    .selection
                    .clone();
                (p.flow.clone(), scope, p.connection_generation)
            };
            let document = panel.borrow().document.clone();
            if let Ok(buttons) = document.query_selector_all("[data-action^='browser-']") {
                for index in 0..buttons.length() {
                    if let Some(button) = buttons
                        .item(index)
                        .and_then(|node| node.dyn_into::<Element>().ok())
                    {
                        let _ = button.set_attribute("disabled", "");
                    }
                }
            }
            set_panel_status(panel, "Sending decision to the local host.");
            let panel = Rc::clone(panel);
            spawn_local(async move {
                let result = flow
                    .decide_browser_action(scope, id, revision, approve)
                    .await;
                let mut p = panel.borrow_mut();
                if p.connection_generation != generation {
                    return;
                }
                match result {
                    Ok(view) => {
                        p.browser_decision_pending = None;
                        drop(p);
                        apply_view(&panel, view, generation);
                    }
                    Err(error) => {
                        if !matches!(&error, FlowError::Timeout | FlowError::Disconnected) {
                            p.browser_decision_pending = None;
                        }
                        p.status_message = if matches!(
                            &error,
                            FlowError::Timeout | FlowError::Disconnected
                        ) {
                            "No decision receipt arrived. Reconnect to check the saved action state before trying again.".into()
                        } else {
                            format!("Decision failed: {error}")
                        };
                        render(&p);
                    }
                }
            });
        }
        Some("copy") if event.is_trusted() => {
            results::copy(panel, &button).map_err(|error| FlowError::Host(js_message(&error)))?
        }
        Some("reconnect") => reconnect(Rc::clone(panel)),
        Some("capture") if event.is_trusted() => capture_page(panel),
        Some("summarize") if event.is_trusted() => summary::start(panel)?,
        Some("stop") => {
            let (flow, scope, run, generation) = {
                let p = panel.borrow();
                (
                    p.flow.clone(),
                    p.snapshot
                        .as_ref()
                        .ok_or(FlowError::NoSource)?
                        .selection
                        .clone(),
                    active_run(&p.snapshot)
                        .ok_or_else(|| {
                            FlowError::InvalidInput("There is no active run to stop.".into())
                        })?
                        .clone(),
                    p.connection_generation,
                )
            };
            {
                let mut p = panel.borrow_mut();
                if p.snapshot
                    .as_ref()
                    .is_some_and(|view| view.pi_ui.as_ref().is_some_and(|ui| ui.run == run))
                {
                    p.pi_ui_stopping = Some(run.clone());
                }
                if p.snapshot
                    .as_ref()
                    .is_some_and(|view| view.codex_input.as_ref().is_some_and(|ui| ui.run == run))
                {
                    p.codex_input_stopping = Some(run.clone());
                }
                render(&p);
            }
            let panel = Rc::clone(panel);
            spawn_local(async move {
                let result = flow.request_stop(scope, run.clone()).await;
                if panel.borrow().connection_generation != generation {
                    return;
                }
                let message = match &result {
                    Ok(StopReceipt::Requested) => {
                        set_panel_status(&panel, "Stop requested.");
                        return;
                    }
                    Ok(StopReceipt::AlreadyTerminal) => {
                        "The host reports that work finished.".to_owned()
                    }
                    Err(error) => format!("Stop failed: {error}"),
                };
                let current = flow.current_page().await;
                if panel.borrow().connection_generation != generation {
                    return;
                }
                match current {
                    Ok(view) => {
                        let fresh = panel
                            .borrow()
                            .snapshot
                            .as_ref()
                            .is_none_or(|old| !view.is_older_than(old));
                        let running = view
                            .runs
                            .iter()
                            .any(|item| item.id == run && item.status == RunStatus::Running);
                        apply_view(&panel, view, generation);
                        if result.is_err() && fresh && running && flow.is_connected() {
                            let mut p = panel.borrow_mut();
                            if p.pi_ui_stopping.as_ref() == Some(&run) {
                                p.pi_ui_stopping = None;
                            }
                            if p.codex_input_stopping.as_ref() == Some(&run) {
                                p.codex_input_stopping = None;
                            }
                            render(&p);
                        }
                        set_panel_status(&panel, &message);
                    }
                    Err(error) => set_panel_status(
                        &panel,
                        &format!(
                            "{message} Could not refresh the host state: {error}. Controls remain paused."
                        ),
                    ),
                }
            });
        }
        _ => {}
    }
    Ok(())
}

fn begin_start(panel: &Rc<RefCell<Panel>>) {
    let mut p = panel.borrow_mut();
    if let Some(source) = p
        .document
        .get_element_by_id("source")
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = source.hide_popover();
    }
    p.start_pending = true;
    p.status_message = "Starting work.".into();
    render(&p);
}

fn install_flow(panel: &Rc<RefCell<Panel>>) -> Result<(), FlowError> {
    let (flow, generation) = {
        let p = panel.borrow();
        (p.flow.clone(), p.connection_generation)
    };
    let weak = Rc::downgrade(panel);
    flow.on_source_change(move || {
        if let Some(panel) = weak.upgrade() {
            stale_check(panel);
        }
    });
    let mut views = flow.observe()?;
    let weak = Rc::downgrade(panel);
    spawn_local(async move {
        loop {
            let next = views.changed().await;
            let Some(panel) = weak.upgrade() else {
                break;
            };
            if panel.borrow().connection_generation != generation {
                break;
            }
            match next {
                Ok(Some(view)) => apply_view(&panel, view, generation),
                Ok(None) => break,
                Err(error) => {
                    let mut p = panel.borrow_mut();
                    p.connected = false;
                    conversations::disconnected(&mut p.conversations);
                    p.status_message = error.to_string();
                    p.codex_input_pending = None;
                    p.codex_input_stopping = None;
                    if let Some(view) = &mut p.snapshot {
                        view.source_status = SourceStatus::Checking;
                        view.codex_input = None;
                    }
                    render(&p);
                }
            }
        }
    });
    Ok(())
}

fn reconnect(panel: Rc<RefCell<Panel>>) {
    set_panel_status(&panel, "Loading saved work.");
    spawn_local(async move {
        let mut flow = panel.borrow().flow.clone();
        if flow.is_closed() {
            match FlowClient::connect().await {
                Ok(replacement) => {
                    flow = replacement;
                    {
                        let mut p = panel.borrow_mut();
                        p.flow = flow.clone();
                        p.saved_history = Default::default();
                        conversations::disconnected(&mut p.conversations);
                        p.start_pending = false;
                        p.native_permission_pending = None;
                        p.pi_ui_pending = None;
                        p.pi_ui_stopping = None;
                        p.codex_input_pending = None;
                        p.codex_input_stopping = None;
                        p.connection_generation = p.connection_generation.wrapping_add(1);
                        p.stale_generation = p.stale_generation.wrapping_add(1);
                        if let Some(view) = &mut p.snapshot {
                            view.source_status = SourceStatus::Checking;
                            view.source_epoch = 0;
                            view.pi_ui = None;
                            view.codex_input = None;
                        }
                        render(&p);
                    }
                    if let Err(error) = install_flow(&panel) {
                        set_panel_status(&panel, &error.to_string());
                        return;
                    }
                }
                Err(error) => {
                    set_panel_status(&panel, &error.to_string());
                    return;
                }
            }
        }
        let generation = panel.borrow().connection_generation;
        let read_epoch = provider_settings::mutation_epoch();
        let agent_read_epoch = agent_settings::mutation_epoch();
        match flow.current_page().await {
            Ok(view) => {
                panel.borrow_mut().browser_decision_pending = None;
                provider_settings::reconcile_current_page(&panel, &view, generation, read_epoch);
                agent_settings::reconcile_current_page(&panel, &view, generation, agent_read_epoch);
                apply_view(&panel, view, generation);
            }
            Err(error) => {
                if panel.borrow().connection_generation == generation {
                    set_panel_status(&panel, &error.to_string());
                }
            }
        }
    });
}

async fn follow_start(
    panel: Rc<RefCell<Panel>>,
    flow: FlowClient,
    scope: metldr_history_pages::Scope,
    generation: u64,
    start: Result<RunId, FlowError>,
    label: &str,
) {
    let mut awaiting_view = true;
    let result = async {
        let mut views = flow.watch(scope, start?).await?;
        while let Some(view) = views.changed().await? {
            if awaiting_view && panel.borrow().connection_generation == generation {
                panel.borrow_mut().start_pending = false;
            }
            awaiting_view = false;
            apply_view(&panel, view, generation);
        }
        Ok::<(), FlowError>(())
    }
    .await;
    if awaiting_view && panel.borrow().connection_generation == generation {
        panel.borrow_mut().start_pending =
            result.as_ref().is_err_and(FlowError::may_have_committed);
        render(&panel.borrow());
    }
    if let Err(error) = result {
        if panel.borrow().connection_generation == generation {
            let recovery = if awaiting_view && error.may_have_committed() {
                " Reload to check saved work before starting again."
            } else {
                ""
            };
            set_panel_status(&panel, &format!("{label}: {error}{recovery}"));
            render(&panel.borrow());
        }
    }
}

fn apply_view(panel: &Rc<RefCell<Panel>>, view: PageView, generation: u64) {
    let mut p = panel.borrow_mut();
    if p.connection_generation != generation {
        return;
    }
    if p.snapshot
        .as_ref()
        .is_some_and(|old| view.is_older_than(old))
    {
        return;
    }
    let selection_changed = p
        .snapshot
        .as_ref()
        .is_some_and(|old| old.selection != view.selection);
    history_ui::selection_changed(&mut p.saved_history, selection_changed, generation);
    conversations::reconcile(&mut p.conversations, &view, selection_changed, generation);
    p.snapshot = Some(view);
    let correction_found = if let Some(pending) = p.correction_pending.clone()
        && pending.scope == p.snapshot.as_ref().unwrap().selection
        && p.snapshot.as_ref().unwrap().runs.iter().any(|run| {
            run.id == pending.run
                && run
                    .corrections
                    .entries
                    .iter()
                    .any(|receipt| receipt.request == pending.request)
        }) {
        if let Some(input) = p
            .document
            .get_element_by_id("question")
            .and_then(|node| node.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
            && input.value() == pending.text
        {
            input.set_value("");
        }
        p.correction_pending = None;
        true
    } else {
        false
    };
    if p.native_permission_pending
        .as_ref()
        .is_some_and(|(run, request)| {
            !p.snapshot.as_ref().is_some_and(|view| {
                view.native_permissions
                    .iter()
                    .any(|item| &item.run == run && &item.request == request)
            })
        })
    {
        p.native_permission_pending = None;
    }
    if p.pi_ui_pending.as_ref().is_some_and(|(run, request)| {
        !p.snapshot
            .as_ref()
            .and_then(|view| view.pi_ui.as_ref())
            .is_some_and(|ui| {
                &ui.run == run && ui.pending.iter().any(|pending| &pending.request == request)
            })
    }) {
        p.pi_ui_pending = None;
    }
    if p.pi_ui_stopping
        .as_ref()
        .is_some_and(|run| active_run(&p.snapshot) != Some(run))
    {
        p.pi_ui_stopping = None;
    }
    if p.codex_input_pending
        .as_ref()
        .is_some_and(|(run, request)| {
            !p.snapshot
                .as_ref()
                .and_then(|view| view.codex_input.as_ref())
                .is_some_and(|ui| {
                    &ui.run == run && ui.pending.iter().any(|item| &item.request == request)
                })
        })
    {
        p.codex_input_pending = None;
    }
    if p.codex_input_stopping
        .as_ref()
        .is_some_and(|run| active_run(&p.snapshot) != Some(run))
    {
        p.codex_input_stopping = None;
    }
    p.connected = p.flow.is_connected();
    p.status_message = if correction_found {
        "Correction receipt found in saved run history."
    } else if p.correction_pending.is_some() {
        "Correction submission unresolved. Check saved run history before sending again."
    } else if p.snapshot.as_ref().and_then(PageView::active_run).is_some() {
        "Working."
    } else if p.start_pending {
        "Starting work."
    } else {
        "Ready."
    }
    .into();
    render(&p);
}

fn stale_check(panel: Rc<RefCell<Panel>>) {
    let (flow, view, generation, connection) = {
        let mut p = panel.borrow_mut();
        p.stale_generation = p.stale_generation.wrapping_add(1);
        let Some(view) = &mut p.snapshot else {
            return;
        };
        view.source_status = SourceStatus::Checking;
        let view = view.clone();
        render(&p);
        (
            p.flow.clone(),
            view,
            p.stale_generation,
            p.connection_generation,
        )
    };
    spawn_local(async move {
        let view = flow.check_source(view).await;
        let mut p = panel.borrow_mut();
        if p.connected && p.stale_generation == generation && p.connection_generation == connection
        {
            if let Some(old) = &mut p.snapshot {
                if old.store == view.store
                    && old.selection == view.selection
                    && old.sequence == view.sequence
                {
                    *old = view;
                    render(&p);
                }
            }
        }
    });
}

fn render(panel: &Panel) {
    if let Err(error) = view::render(panel) {
        view::status(
            &panel.document,
            &format!(
                "Saved results could not be displayed: {}",
                js_message(&error)
            ),
        );
    }
}

fn disable_native_permission_buttons(panel: &Rc<RefCell<Panel>>) {
    let document = panel.borrow().document.clone();
    if let Ok(buttons) = document.query_selector_all("[data-action='native-permission']") {
        for index in 0..buttons.length() {
            if let Some(button) = buttons
                .item(index)
                .and_then(|node| node.dyn_into::<Element>().ok())
            {
                let _ = button.set_attribute("disabled", "");
            }
        }
    }
}

fn active_run(snapshot: &Option<PageView>) -> Option<&model::RunId> {
    snapshot.as_ref().and_then(PageView::active_run)
}
fn set_panel_status(panel: &Rc<RefCell<Panel>>, value: &str) {
    panel.borrow_mut().status_message = value.to_string();
    view::status(&panel.borrow().document, value);
}
fn forward_request(panel: &JsValue, message: JsValue) -> Result<(), JsValue> {
    let raw = js_sys::JSON::stringify(&message)?
        .as_string()
        .ok_or_else(|| err("Panel sent a non JSON message."))?;
    if raw.len() > MAX_FRAME {
        return Err(err("Panel request exceeds the 65536 byte frame limit."));
    }
    let parsed = js_sys::JSON::parse(&raw)?;
    let command: Value = serde_json::from_str(&raw).map_err(|_| err("Invalid panel request."))?;
    let watch =
        panel_subscription(&command, "watch").map_err(|_| err("Invalid panel watch request."))?;
    let unwatch = panel_subscription(&command, "unwatch")
        .map_err(|_| err("Invalid panel unwatch request."))?;
    let op = get(&get(&parsed, "command")?, "op")?.as_string();
    if matches!(op.as_deref(), Some("browser_reply" | "browser_event")) {
        return Err(err(
            "Browser transport messages cannot originate in the panel.",
        ));
    }
    if watch.as_ref().is_some_and(|(request, _, _)| {
        BACKGROUND.with(|b| {
            let watches = &b.borrow().watches;
            watches.len() >= 16
                || watch_id_in_use(watches.iter().map(|watch| watch.request.as_str()), request)
        })
    }) {
        return Err(err(
            "Native watch capacity reached or watch ID is already active.",
        ));
    }
    let native = match BACKGROUND.with(|b| b.borrow().native.clone()) {
        Some(native) => native,
        None => {
            let command = Reflect::get(&parsed, &JsValue::from_str("command"))?;
            let op = Reflect::get(&command, &JsValue::from_str("op"))?.as_string();
            if op.as_deref() != Some("view") {
                return Err(err(
                    "Native host is disconnected. Reconnect before starting work.",
                ));
            }
            connect_native()?;
            BACKGROUND
                .with(|b| b.borrow().native.clone())
                .ok_or_else(|| err("Native host did not connect."))?
        }
    };
    let queued = BACKGROUND.with(|b| {
        let mut background = b.borrow_mut();
        if background
            .native
            .as_ref()
            .is_none_or(|current| !Object::is(current, &native))
        {
            return Err(());
        }
        background.readiness.queue(parsed.clone())
    });
    let ready = match queued {
        Ok(ready) => ready,
        Err(()) => {
            return Err(err(
                "Native host is not ready or its request queue is full.",
            ));
        }
    };
    if ready {
        call(&native, "postMessage", &[parsed])?;
    }
    if let Some((request, scope, run)) = watch {
        BACKGROUND.with(|b| {
            let mut background = b.borrow_mut();
            background.watches.push(PanelWatch {
                panel: panel.clone(),
                request,
                scope,
                run,
                queued: !ready,
            });
        });
    }
    if let Some((request, scope, run)) = unwatch {
        BACKGROUND.with(|b| {
            b.borrow_mut().watches.retain(|entry| {
                !(Object::is(&entry.panel, panel)
                    && entry.request == request
                    && entry.scope == scope
                    && entry.run == run)
            })
        });
    }
    Ok(())
}
fn forward_native(message: JsValue) {
    readiness::native_message(message);
}
fn forward_panel(message: &Value) {
    let Ok(raw) = serde_json::to_string(message) else {
        return;
    };
    let Ok(value) = js_sys::JSON::parse(&raw) else {
        return;
    };
    for port in BACKGROUND.with(|b| b.borrow().panels.clone()) {
        let _ = call(&port, "postMessage", &[value.clone()]);
    }
}
fn connect_native() -> Result<(), JsValue> {
    let runtime = chrome_api("runtime")?;
    let native = call(&runtime, "connectNative", &[JsValue::from_str(HOST)])?;
    browser_background::connect(native.clone())?;
    set_background_native(native.clone());
    let sending = native.clone();
    listen(&native, "onMessage", move |message| {
        if BACKGROUND.with(|b| {
            b.borrow()
                .native
                .as_ref()
                .is_some_and(|current| Object::is(current, &sending))
        }) {
            forward_native(message);
        }
    })?;
    let disconnected = native.clone();
    listen(&native, "onDisconnect", move |_| {
        if !BACKGROUND.with(|b| {
            b.borrow()
                .native
                .as_ref()
                .is_some_and(|current| Object::is(current, &disconnected))
        }) {
            return;
        }
        browser_background::disconnect();
        forward_panel(&json!({"v":crate::WIRE_VERSION,"request":null,"kind":"disconnected"}));
        set_background_native(JsValue::NULL);
    })?;
    Ok(())
}
fn set_background_native(native: JsValue) {
    BACKGROUND.with(|b| {
        let mut background = b.borrow_mut();
        background.native = (!native.is_null()).then_some(native);
        if background.native.is_some() {
            background.readiness.connect();
        } else {
            background.readiness.disconnect();
            background.watches.clear();
        }
    });
}
fn remove_panel(port: &JsValue) {
    let (watches, native, ready) = BACKGROUND.with(|b| {
        let mut background = b.borrow_mut();
        background.panels.retain(|p| !Object::is(p, port));
        let watches: Vec<_> = background
            .watches
            .iter()
            .filter(|watch| Object::is(&watch.panel, port))
            .map(|watch| PanelWatch {
                panel: watch.panel.clone(),
                request: watch.request.clone(),
                scope: watch.scope.clone(),
                run: watch.run.clone(),
                queued: watch.queued,
            })
            .collect();
        background
            .watches
            .retain(|watch| !Object::is(&watch.panel, port));
        (
            watches,
            background.native.clone(),
            background.readiness.is_ready(),
        )
    });
    for watch in watches {
        if watch.queued {
            BACKGROUND.with(|b| b.borrow_mut().readiness.discard_request(&watch.request));
        } else if ready {
            if let Some(native) = native.as_ref() {
                let Some(id) = next_cleanup_request() else {
                    continue;
                };
                let scope = serde_json::to_value(watch.scope).unwrap_or(Value::Null);
                let frame = json!({"v":WIRE_VERSION,"request":id,"command":{"op":"conversation","scope":scope,"action":{"op":"unwatch","run":watch.run,"watch":watch.request}}});
                if let Ok(value) = js_sys::JSON::parse(&frame.to_string()) {
                    let _ = call(native, "postMessage", &[value]);
                }
            }
        }
    }
}

fn panel_subscription(
    command: &Value,
    operation: &str,
) -> Result<Option<(String, metldr_history_pages::Scope, RunId)>, ()> {
    let Some(conversation) = command.get("command") else {
        return Ok(None);
    };
    let Some(action) = conversation.get("action") else {
        return Ok(None);
    };
    if action.get("op").and_then(Value::as_str) != Some(operation) {
        return Ok(None);
    }
    let identity = if operation == "watch" {
        command.get("request")
    } else {
        action.get("watch")
    };
    let request = identity
        .and_then(Value::as_str)
        .filter(|id| valid_panel_request_id(id))
        .ok_or(())?;
    if conversation.get("op").and_then(Value::as_str) != Some("conversation") {
        return Err(());
    }
    let scope: metldr_history_pages::Scope =
        serde_json::from_value(conversation.get("scope").ok_or(())?.clone()).map_err(|_| ())?;
    scope.validate().map_err(|_| ())?;
    let run = serde_json::from_value(action.get("run").ok_or(())?.clone()).map_err(|_| ())?;
    Ok(Some((request.to_owned(), scope, run)))
}

fn next_cleanup_request() -> Option<String> {
    BACKGROUND.with(|b| {
        let mut background = b.borrow_mut();
        background.next_cleanup = background.next_cleanup.checked_add(1)?;
        Some(format!("cleanup-{}", background.next_cleanup))
    })
}

fn watch_id_in_use<'a>(requests: impl IntoIterator<Item = &'a str>, request: &str) -> bool {
    requests.into_iter().any(|active| active == request)
}

fn valid_panel_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-'))
}

#[cfg(test)]
mod panel_watch_tests {
    use super::{panel_subscription, valid_panel_request_id, watch_id_in_use};
    use serde_json::json;

    #[test]
    fn panel_subscription_validates_identity_and_rejects_duplicate_watch_ids() {
        let store = "0123456789abcdef0123456789abcdef";
        let scope = json!({"store":store,"conversation":format!("{store}:1"),"epoch":"1"});
        let request = json!({"request":"panel-abc-1","command":{"op":"conversation","scope":scope,"action":{"op":"watch","run":format!("{store}:2")}}});
        let watch = panel_subscription(&request, "watch").unwrap().unwrap();
        assert_eq!(watch.0, "panel-abc-1");
        assert_eq!(watch.1.store, store);
        assert_eq!(watch.2.to_string(), format!("{store}:2"));
        assert!(watch_id_in_use([watch.0.as_str()], "panel-abc-1"));

        let malformed = json!({"request":"bad id","command":{"op":"conversation","scope":{"store":"bad"},"action":{"op":"watch","run":"bad"}}});
        assert!(panel_subscription(&malformed, "watch").is_err());
        assert!(!valid_panel_request_id("bad id"));

        let malformed_run = json!({"request":"panel-abc-2","command":{"op":"conversation","scope":scope,"action":{"op":"watch","run":"bad"}}});
        assert!(panel_subscription(&malformed_run, "watch").is_err());
    }
}
fn chrome_api(name: &str) -> Result<JsValue, JsValue> {
    let chrome = Reflect::get(&js_sys::global(), &JsValue::from_str("chrome"))?;
    Reflect::get(&chrome, &JsValue::from_str(name))
}
fn get(object: &JsValue, key: &str) -> Result<JsValue, JsValue> {
    Reflect::get(object, &JsValue::from_str(key))
}
fn string_property(object: &JsValue, key: &str) -> Result<String, JsValue> {
    get(object, key)?
        .as_string()
        .ok_or_else(|| err(&format!("Missing string field {key}.")))
}
fn call(object: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function = get(object, method)?.dyn_into::<Function>()?;
    let js_args = Array::new();
    for value in args {
        js_args.push(value);
    }
    function.apply(object, &js_args)
}
fn promise(value: JsValue) -> Result<JsFuture, JsValue> {
    Ok(JsFuture::from(value.dyn_into::<Promise>()?))
}
async fn await_call(object: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    promise(call(object, method, args)?)?.await
}
fn listen(
    target: &JsValue,
    event: &str,
    mut callback: impl FnMut(JsValue) + 'static,
) -> Result<(), JsValue> {
    let event = get(target, event)?;
    let add = get(&event, "addListener")?.dyn_into::<Function>()?;
    let listener = Closure::<dyn FnMut(JsValue)>::new(move |value| callback(value));
    add.call1(&event, listener.as_ref())?;
    listener.forget();
    Ok(())
}
fn err(message: &str) -> JsValue {
    JsValue::from_str(message)
}
fn js_message(error: &JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

fn panel_url_allowed(origin: &str, url: &str) -> bool {
    url.strip_prefix(origin)
        .is_some_and(|path| matches!(path, "/panel.html" | "/chat.html"))
}

#[cfg(test)]
mod panel_url_checks {
    #[test]
    fn only_exact_owned_application_pages_are_allowed() {
        let origin = "chrome-extension://owned";
        for page in ["panel.html", "chat.html"] {
            assert!(super::panel_url_allowed(
                origin,
                &format!("{origin}/{page}")
            ));
        }
        for url in [
            "chrome-extension://other/chat.html",
            "chrome-extension://owned/sandbox.html",
            "chrome-extension://owned/chat.html?route=panel",
            "chrome-extension://owned/chat.html#answer",
            "https://owned/chat.html",
        ] {
            assert!(!super::panel_url_allowed(origin, url));
        }
    }
}
