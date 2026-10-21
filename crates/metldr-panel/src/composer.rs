use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{
    Event, HtmlFormElement, HtmlOptionElement, HtmlSelectElement, HtmlTextAreaElement,
    KeyboardEvent,
};

use crate::model::{
    AgentSessionView, BrowserRouteView, ChatRouteView, FlowError, GoogleRouteView, PageView,
    ResponsesRouteView, RunId, UserText,
};
use crate::{Panel, active_run, begin_start, err, follow_start, js_message, set_panel_status};

pub(super) fn install(panel: &Rc<RefCell<Panel>>) -> Result<(), JsValue> {
    let document = panel.borrow().document.clone();
    let form = document
        .get_element_by_id("composer")
        .ok_or_else(|| err("Composer missing."))?
        .dyn_into::<HtmlFormElement>()?;
    let submit_panel = Rc::clone(panel);
    let listener = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        event.prevent_default();
        if let Err(error) = submit(&submit_panel) {
            set_panel_status(&submit_panel, &format!("Request failed: {error}"));
        }
    });
    form.add_event_listener_with_callback("submit", listener.as_ref().unchecked_ref())?;
    listener.forget();
    let submit_keyboard = Rc::clone(panel);
    let keyboard = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        match crate::composer_ui::keydown(&submit_keyboard, &event) {
            Ok(true) => return,
            Ok(false) => (),
            Err(error) => {
                web_sys::console::error_1(&error);
                return;
            }
        }
        if event.key() == "Enter" && !event.shift_key() && !event.is_composing() {
            event.prevent_default();
            if event
                .target()
                .and_then(|target| target.dyn_into::<HtmlTextAreaElement>().ok())
                .is_some_and(|input| input.value().trim().is_empty())
            {
                return;
            }
            if let Err(error) = form.request_submit() {
                web_sys::console::error_1(&error);
            }
        }
    });
    document
        .get_element_by_id("question")
        .ok_or_else(|| err("Question missing."))?
        .add_event_listener_with_callback("keydown", keyboard.as_ref().unchecked_ref())?;
    keyboard.forget();
    let changed_panel = Rc::clone(panel);
    let changed = Closure::<dyn FnMut(Event)>::new(move |_| {
        if let Err(error) = render(&changed_panel.borrow()) {
            web_sys::console::error_1(&error);
        }
    });
    select(&panel.borrow())?
        .add_event_listener_with_callback("change", changed.as_ref().unchecked_ref())?;
    changed.forget();
    Ok(())
}

enum Destination {
    Model,
    Browser { fingerprint: Option<String> },
    Chat { fingerprint: String },
    Responses { fingerprint: String },
    Google { fingerprint: String },
    Pi,
    Codex,
    OpenCode,
    Continue { workflow: RunId, revision: u64 },
    ContinueCodex { workflow: RunId, revision: u64 },
    ContinueOpenCode { workflow: RunId, revision: u64 },
}

fn codex_mode(panel: &Rc<RefCell<Panel>>) -> &'static str {
    panel
        .borrow()
        .document
        .get_element_by_id("codex-mode")
        .and_then(|node| node.dyn_into::<HtmlSelectElement>().ok())
        .map(|select| {
            if select.value() == "plan" {
                "plan"
            } else {
                "configured"
            }
        })
        .unwrap_or("configured")
}

fn ready(view: &PageView, session: &AgentSessionView) -> bool {
    session.ready
        && view
            .pi_profile
            .as_ref()
            .is_some_and(|profile| session.profile == profile.fingerprint)
        && Some(&session.source) == view.source()
}

fn codex_ready(view: &PageView, session: &AgentSessionView) -> bool {
    session.ready
        && view
            .codex_profile
            .as_ref()
            .is_some_and(|profile| session.profile == profile.fingerprint)
        && Some(&session.source) == view.source()
}

fn opencode_ready(view: &PageView, session: &AgentSessionView) -> bool {
    session.ready
        && view
            .opencode_profile
            .as_ref()
            .is_some_and(|profile| session.profile == profile.fingerprint)
        && Some(&session.source) == view.source()
}

fn selected(panel: &Panel) -> Result<Destination, FlowError> {
    let value = select(panel)
        .map_err(|error| FlowError::Host(js_message(&error)))?
        .value();
    match value.as_str() {
        "" => Err(FlowError::InvalidInput(
            "Choose a destination before sending.".into(),
        )),
        "model"
            if panel
                .snapshot
                .as_ref()
                .is_some_and(|view| view.fixture_available) =>
        {
            Ok(Destination::Model)
        }
        "model" => Err(FlowError::InvalidInput(
            "The test model is unavailable. Choose a configured destination.".into(),
        )),
        "browser" => match panel
            .snapshot
            .as_ref()
            .and_then(|view| view.browser_route.as_ref())
        {
            Some(BrowserRouteView::Messages { .. }) => {
                Ok(Destination::Browser { fingerprint: None })
            }
            Some(BrowserRouteView::HttpsMessages { fingerprint, .. }) => Ok(Destination::Browser {
                fingerprint: Some(fingerprint.clone()),
            }),
            None => Err(FlowError::InvalidInput(
                "The browser task is unavailable.".into(),
            )),
        },
        "responses" => match panel
            .snapshot
            .as_ref()
            .and_then(|view| view.responses_route.as_ref())
        {
            Some(ResponsesRouteView::HttpsResponses { fingerprint, .. }) => {
                Ok(Destination::Responses {
                    fingerprint: fingerprint.clone(),
                })
            }
            None => Err(FlowError::InvalidInput(
                "The OpenAI Responses destination is unavailable.".into(),
            )),
        },
        "chat" => match panel
            .snapshot
            .as_ref()
            .and_then(|view| view.chat_route.as_ref())
        {
            Some(ChatRouteView::HttpsChatCompletions { fingerprint, .. }) => {
                Ok(Destination::Chat {
                    fingerprint: fingerprint.clone(),
                })
            }
            None => Err(FlowError::InvalidInput(
                "Chat Completions is unavailable.".into(),
            )),
        },
        "google" => match panel
            .snapshot
            .as_ref()
            .and_then(|view| view.google_route.as_ref())
        {
            Some(GoogleRouteView::HttpsGoogleInteractions { fingerprint, .. }) => {
                Ok(Destination::Google {
                    fingerprint: fingerprint.clone(),
                })
            }
            None => Err(FlowError::InvalidInput(
                "Google Interactions is unavailable.".into(),
            )),
        },
        "pi" if panel
            .snapshot
            .as_ref()
            .is_some_and(|view| view.pi_profile.is_some()) =>
        {
            Ok(Destination::Pi)
        }
        "pi" => Err(FlowError::InvalidInput(
            "Pi is unavailable. Choose another destination or configure Pi.".into(),
        )),
        "codex"
            if panel
                .snapshot
                .as_ref()
                .is_some_and(|view| view.codex_profile.is_some()) =>
        {
            Ok(Destination::Codex)
        }
        "codex" => Err(FlowError::InvalidInput(
            "Codex configuration is unavailable. Choose another destination or configure Codex."
                .into(),
        )),
        "opencode"
            if panel
                .snapshot
                .as_ref()
                .is_some_and(|view| view.opencode_profile.is_some()) =>
        {
            Ok(Destination::OpenCode)
        }
        "opencode" => Err(FlowError::InvalidInput(
            "OpenCode is unavailable. Configure an OpenCode profile first.".into(),
        )),
        _ => {
            if let Some(id) = value.strip_prefix("opencode:") {
                let session = panel.snapshot.as_ref().and_then(|view| view.opencode_sessions.iter().find(|session| session.workflow.as_str() == id && opencode_ready(view, session)))
                    .ok_or_else(|| FlowError::InvalidInput("The selected OpenCode session is unavailable. Choose a destination before sending.".into()))?;
                return Ok(Destination::ContinueOpenCode {
                    workflow: session.workflow.clone(),
                    revision: session.revision,
                });
            }
            if let Some(id) = value.strip_prefix("codex:") {
                let session = panel.snapshot.as_ref().and_then(|view| {
                    view.codex_sessions.iter().find(|session| session.workflow.as_str() == id && codex_ready(view, session))
                }).ok_or_else(|| FlowError::InvalidInput("The selected Codex session is unavailable. Choose a destination before sending.".into()))?;
                return Ok(Destination::ContinueCodex {
                    workflow: session.workflow.clone(),
                    revision: session.revision,
                });
            }
            let session = panel.snapshot.as_ref().and_then(|view| {
                view.agent_sessions.iter().find(|session| session.workflow.as_str() == value && ready(view, session))
            }).ok_or_else(|| FlowError::InvalidInput("The selected Pi session is unavailable. Choose a destination before sending.".into()))?;
            Ok(Destination::Continue {
                workflow: session.workflow.clone(),
                revision: session.revision,
            })
        }
    }
}

fn submit(panel: &Rc<RefCell<Panel>>) -> Result<(), FlowError> {
    let state = panel.borrow();
    if !state.connected || state.start_pending || active_run(&state.snapshot).is_some() {
        return Ok(());
    }
    let destination = selected(&state)?;
    let text = state
        .document
        .get_element_by_id("question")
        .and_then(|element| element.dyn_into::<HtmlTextAreaElement>().ok())
        .map(|element| element.value())
        .unwrap_or_default();
    let text = UserText::try_from(text.as_str())?;
    let source = state
        .snapshot
        .as_ref()
        .and_then(PageView::source)
        .ok_or(FlowError::NoSource)?
        .clone();
    let flow = state.flow.clone();
    let generation = state.connection_generation;
    let codex_mode = codex_mode(panel);
    drop(state);
    begin_start(panel);
    let panel = Rc::clone(panel);
    spawn_local(async move {
        let start = match destination {
            Destination::Model => flow.ask(source, text).await,
            Destination::Browser { fingerprint } => {
                flow.browser_task(source, text, fingerprint).await
            }
            Destination::Responses { fingerprint } => {
                flow.responses_task(source, text, fingerprint).await
            }
            Destination::Chat { fingerprint } => flow.chat_task(source, text, fingerprint).await,
            Destination::Google { fingerprint } => {
                flow.google_task(source, text, fingerprint).await
            }
            Destination::Pi => flow.pi_prompt(source, text).await,
            Destination::Codex => flow.codex_prompt(source, text, codex_mode).await,
            Destination::OpenCode => flow.opencode_prompt(source, text).await,
            Destination::Continue { workflow, revision } => {
                flow.pi_follow_up(workflow, revision, text).await
            }
            Destination::ContinueCodex { workflow, revision } => {
                flow.codex_follow_up(workflow, revision, text, codex_mode)
                    .await
            }
            Destination::ContinueOpenCode { workflow, revision } => {
                flow.opencode_follow_up(workflow, revision, text).await
            }
        };
        follow_start(panel, flow, generation, start, "Request failed").await;
    });
    Ok(())
}

pub(super) fn render(panel: &Panel) -> Result<(), JsValue> {
    crate::composer_ui::restore_choice(panel)?;
    let select = select(panel)?;
    let value = select.value();
    let view = panel.snapshot.as_ref();
    let sessions: Vec<_> = view
        .into_iter()
        .flat_map(|view| &view.agent_sessions)
        .filter(|session| view.is_some_and(|view| ready(view, session)))
        .collect();
    let codex_sessions: Vec<_> = view
        .into_iter()
        .flat_map(|view| &view.codex_sessions)
        .filter(|session| view.is_some_and(|view| codex_ready(view, session)))
        .collect();
    let opencode_sessions: Vec<_> = view
        .into_iter()
        .flat_map(|view| &view.opencode_sessions)
        .filter(|session| view.is_some_and(|view| opencode_ready(view, session)))
        .collect();
    let options: Vec<_> = (0..select.children().length())
        .filter_map(|index| select.children().item(index))
        .filter(|option| option.has_attribute("data-session"))
        .collect();
    for option in options {
        let id = option.get_attribute("value").unwrap_or_default();
        let provider = option
            .get_attribute("data-provider")
            .unwrap_or_else(|| "pi".into());
        let available = if provider == "opencode" {
            opencode_sessions
                .iter()
                .any(|session| format!("opencode:{}", session.workflow.as_str()) == id)
        } else if provider == "codex" {
            codex_sessions
                .iter()
                .any(|session| format!("codex:{}", session.workflow.as_str()) == id)
        } else {
            sessions
                .iter()
                .any(|session| session.workflow.as_str() == id)
        };
        if !available {
            if id == value {
                option.set_text_content(Some(if provider == "codex" {
                    "Codex session unavailable"
                } else if provider == "opencode" {
                    "OpenCode session unavailable"
                } else {
                    "Pi session unavailable"
                }));
                option.set_attribute("disabled", "")?;
            } else {
                select.remove_child(&option)?;
            }
        }
    }
    for session in sessions {
        let id = session.workflow.as_str();
        let existing = (0..select.children().length())
            .filter_map(|index| select.children().item(index))
            .find(|option| option.get_attribute("value").as_deref() == Some(id));
        let option = match existing {
            Some(option) => option,
            None => {
                let option = panel.document.create_element("option")?;
                option.set_attribute("value", id)?;
                option.set_attribute("data-session", "")?;
                option.set_attribute("data-provider", "pi")?;
                select.append_child(&option)?;
                option
            }
        };
        option.set_text_content(Some(&format!(
            "Continue Pi session {}",
            id.rsplit(':').next().unwrap_or(id)
        )));
        option.set_attribute("data-revision", &session.revision.to_string())?;
        option.remove_attribute("disabled")?;
    }
    for session in codex_sessions {
        let id = format!("codex:{}", session.workflow.as_str());
        let existing = (0..select.children().length())
            .filter_map(|index| select.children().item(index))
            .find(|option| option.get_attribute("value").as_deref() == Some(id.as_str()));
        let option = match existing {
            Some(option) => option,
            None => {
                let option = panel.document.create_element("option")?;
                option.set_attribute("value", &id)?;
                option.set_attribute("data-session", "")?;
                option.set_attribute("data-provider", "codex")?;
                select.append_child(&option)?;
                option
            }
        };
        option.set_text_content(Some(&format!(
            "Continue Codex session {}",
            session
                .workflow
                .as_str()
                .rsplit(':')
                .next()
                .unwrap_or(session.workflow.as_str())
        )));
        option.set_attribute("data-revision", &session.revision.to_string())?;
        option.remove_attribute("disabled")?;
    }
    for session in opencode_sessions {
        let id = format!("opencode:{}", session.workflow.as_str());
        let existing = (0..select.children().length())
            .filter_map(|index| select.children().item(index))
            .find(|option| option.get_attribute("value").as_deref() == Some(id.as_str()));
        let option = match existing {
            Some(option) => option,
            None => {
                let option = panel.document.create_element("option")?;
                option.set_attribute("value", &id)?;
                option.set_attribute("data-session", "")?;
                option.set_attribute("data-provider", "opencode")?;
                select.append_child(&option)?;
                option
            }
        };
        option.set_text_content(Some(&format!(
            "Continue OpenCode session {}",
            session
                .workflow
                .as_str()
                .rsplit(':')
                .next()
                .unwrap_or(session.workflow.as_str())
        )));
        option.set_attribute("data-revision", &session.revision.to_string())?;
        option.remove_attribute("disabled")?;
    }
    let pi = panel
        .document
        .get_element_by_id("execution-pi")
        .ok_or_else(|| err("Pi option missing."))?
        .dyn_into::<HtmlOptionElement>()?;
    pi.set_disabled(view.is_none_or(|view| view.pi_profile.is_none()));
    let codex = panel
        .document
        .get_element_by_id("execution-codex")
        .ok_or_else(|| err("Codex option missing."))?
        .dyn_into::<HtmlOptionElement>()?;
    codex.set_disabled(view.is_none_or(|view| view.codex_profile.is_none()));
    if let Some(option) = panel.document.get_element_by_id("execution-opencode") {
        option
            .dyn_into::<HtmlOptionElement>()?
            .set_disabled(view.is_none_or(|view| view.opencode_profile.is_none()));
    }
    if let Some(option) = panel.document.get_element_by_id("execution-model") {
        option
            .dyn_into::<HtmlOptionElement>()?
            .set_disabled(view.is_none_or(|view| !view.fixture_available));
    }
    if let Some(option) = panel.document.get_element_by_id("execution-browser") {
        option
            .dyn_into::<HtmlOptionElement>()?
            .set_disabled(view.is_none_or(|view| view.browser_route.is_none()));
    }
    if let Some(option) = panel.document.get_element_by_id("execution-responses") {
        let option = option.dyn_into::<HtmlOptionElement>()?;
        match view.and_then(|view| view.responses_route.as_ref()) {
            Some(ResponsesRouteView::HttpsResponses {
                endpoint, model, ..
            }) => option.set_text_content(Some(&format!("OpenAI Responses, {model}, {endpoint}"))),
            None => option.set_text_content(Some("OpenAI Responses")),
        }
        option.set_disabled(view.is_none_or(|view| view.responses_route.is_none()));
    }
    if let Some(option) = panel.document.get_element_by_id("execution-chat") {
        let option = option.dyn_into::<HtmlOptionElement>()?;
        match view.and_then(|view| view.chat_route.as_ref()) {
            Some(ChatRouteView::HttpsChatCompletions {
                endpoint, model, ..
            }) => option.set_text_content(Some(&format!("Chat Completions, {model}, {endpoint}"))),
            None => option.set_text_content(Some("Chat Completions")),
        }
        option.set_disabled(view.is_none_or(|view| view.chat_route.is_none()));
    }
    if let Some(option) = panel.document.get_element_by_id("execution-google") {
        let option = option.dyn_into::<HtmlOptionElement>()?;
        match view.and_then(|view| view.google_route.as_ref()) {
            Some(GoogleRouteView::HttpsGoogleInteractions {
                endpoint, model, ..
            }) => {
                option.set_text_content(Some(&format!("Google Interactions, {model}, {endpoint}")))
            }
            None => option.set_text_content(Some("Google Interactions")),
        }
        option.set_disabled(view.is_none_or(|view| view.google_route.is_none()));
    }
    select.set_value(&value);
    if let Some(mode) = panel.document.get_element_by_id("codex-mode-wrap") {
        if value == "codex" || value.starts_with("codex:") {
            mode.remove_attribute("hidden")?;
        } else {
            mode.set_attribute("hidden", "")?;
        }
    }
    let destination = selected(panel);
    if destination.is_ok() || value.is_empty() {
        crate::composer_ui::remember_choice(panel, &value);
    }
    let busy = panel.start_pending || active_run(&panel.snapshot).is_some();
    let button = panel
        .document
        .query_selector("[data-action='ask']")?
        .ok_or_else(|| err("Send missing."))?;
    if busy || !panel.connected || view.and_then(PageView::source).is_none() || destination.is_err()
    {
        button.set_attribute("disabled", "")?;
    } else {
        button.remove_attribute("disabled")?;
    }
    let description = if busy {
        "Work is running. The selected destination applies to your next request.".to_owned()
    } else {
        match destination {
            Ok(Destination::Browser { .. }) => match view.and_then(|view| view.browser_route.as_ref()) {
                Some(BrowserRouteView::Messages { model }) => format!("Browser task uses the owned model fixture ({model}). Proposed browser actions require your approval."),
                Some(BrowserRouteView::HttpsMessages { endpoint, model, .. }) => format!("Browser task uses configured HTTPS model {model} at {endpoint}. Proposed browser actions require your approval."),
                None => "The browser task is unavailable.".into(),
            },
            Ok(Destination::Responses { .. }) => match view.and_then(|view| view.responses_route.as_ref()) {
                Some(ResponsesRouteView::HttpsResponses { endpoint, model, .. }) => format!("OpenAI Responses uses configured model {model} at {endpoint}."),
                None => "OpenAI Responses is unavailable.".into(),
            },
            Ok(Destination::Chat { .. }) => match view.and_then(|view| view.chat_route.as_ref()) {
                Some(ChatRouteView::HttpsChatCompletions { endpoint, model, .. }) => format!("Chat Completions uses configured model {model} at {endpoint}. Changes require your approval."),
                None => "Chat Completions is unavailable.".into(),
            },
            Ok(Destination::Google { .. }) => match view.and_then(|view| view.google_route.as_ref()) {
                Some(GoogleRouteView::HttpsGoogleInteractions { endpoint, model, .. }) => format!("Google Interactions uses configured model {model} at {endpoint}."),
                None => "Google Interactions is unavailable.".into(),
            },
            Ok(Destination::Model) => "The fixture model receives your request and the saved source. Page summaries also use this model.".into(),
            Ok(Destination::Pi) => "Pi starts a new native session with the saved source. Pi manages its provider, tools and permissions.".into(),
            Ok(Destination::Codex) => "Codex starts a native session with the saved source. Codex manages its provider, tools and permissions.".into(),
            Ok(Destination::OpenCode) => "OpenCode starts a native session with the saved source. OpenCode manages its provider, tools and permissions.".into(),
            Ok(Destination::Continue { workflow, revision }) => format!("Pi continues workflow {workflow}, revision {revision}, with the saved source and its configured profile."),
            Ok(Destination::ContinueCodex { workflow, revision }) => format!("Codex continues workflow {workflow}, revision {revision}, with the saved source and its configured profile."),
            Ok(Destination::ContinueOpenCode { workflow, revision }) => format!("OpenCode continues workflow {workflow}, revision {revision}, with the saved source and its configured profile."),
            Err(error) => error.to_string(),
        }
    };
    let detail = panel
        .document
        .get_element_by_id("execution-detail")
        .ok_or_else(|| err("Execution details missing."))?;
    if detail.text_content().as_deref() != Some(description.as_str()) {
        detail.set_text_content(Some(description.as_str()));
    }
    crate::composer_ui::render(panel)
}

fn select(panel: &Panel) -> Result<HtmlSelectElement, JsValue> {
    Ok(panel
        .document
        .get_element_by_id("execution")
        .ok_or_else(|| err("Execution choice missing."))?
        .dyn_into()?)
}
