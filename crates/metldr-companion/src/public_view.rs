use crate::{
    Result,
    agent_model::{Launch, Profile, digest},
    model::Store,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn browser_actions(actions: &BTreeMap<String, crate::browser_action::Action>) -> Value {
    Value::Array(
        actions
            .values()
            .map(|action| {
                let p = &action.proposal;
                let phase = match &action.state {
                    crate::browser_action::Phase::Pending => "pending",
                    crate::browser_action::Phase::Approved => "approved",
                    crate::browser_action::Phase::Dispatching => "dispatching",
                    crate::browser_action::Phase::Settled { .. } => "settled",
                };
                let mut value = json!({"id":action.id,"revision":action.revision,"run":p.run,
                    "tab":p.binding.tab,"document":p.binding.document,"url":p.url,"phase":phase});
                match &p.intent {
                    crate::browser_action::Intent::Field {
                        field_id,
                        field_label,
                        previous_value,
                        value: field_value,
                    } => {
                        value["kind"] = json!("field");
                        value["field_id"] = json!(field_id);
                        value["field_label"] = json!(field_label);
                        value["previous_value"] = json!(previous_value);
                        value["value"] = json!(field_value);
                    }
                    crate::browser_action::Intent::Navigate { destination } => {
                        value["kind"] = json!("navigate");
                        value["destination"] = json!(destination);
                    }
                    crate::browser_action::Intent::ActivateButton {
                        button_id,
                        button_label,
                    } => {
                        value["kind"] = json!("activate_button");
                        value["button_id"] = json!(button_id);
                        value["button_label"] = json!(button_label);
                    }
                    crate::browser_action::Intent::SelectOption { selection } => {
                        value["kind"] = json!("select_option");
                        value["selection"] = json!(selection);
                    }
                }
                if let crate::browser_action::Phase::Settled { outcome } = &action.state {
                    use crate::browser_action::Outcome;
                    value["outcome"] = json!(match outcome {
                        Outcome::Verified { .. } | Outcome::Navigated { .. } => "verified",
                        Outcome::ButtonClickObserved => "click_observed",
                        Outcome::SelectionObserved { .. } => "selection_observed",
                        Outcome::Prevented { .. } => "prevented",
                        Outcome::Uncertain { .. } => "uncertain",
                    });
                    if let Outcome::Prevented { reason } | Outcome::Uncertain { reason } = outcome {
                        value["reason"] = json!(reason);
                    }
                    if let Outcome::SelectionObserved { selected } = outcome {
                        value["observed_selection"] = json!(selected);
                    }
                }
                value
            })
            .collect(),
    )
}

pub(super) fn render(
    state: &Store,
    profile: Option<&Profile>,
    codex_profile: Option<&crate::codex_model::Profile>,
    opencode_profile: Option<&crate::opencode_model::Profile>,
    native_permissions: &std::collections::BTreeMap<String, crate::flow::PendingPermission>,
    pi_ui: Option<&crate::pi_ui::State>,
    codex_input: Option<&crate::codex_input::State>,
    browser_route: Option<&crate::https_profile::Profile>,
    responses_route: Option<&crate::https_profile::Profile>,
    google_route: Option<&crate::https_profile::Profile>,
    chat_route: Option<&crate::https_profile::Profile>,
    fixture_available: bool,
    native_choices: Option<&crate::history::NativeChoices>,
) -> Result<Value> {
    let mut view = serde_json::to_value(&state.view)?;
    view["pi_ui"] = pi_ui
        .filter(|ui| state.view.runs.iter().any(|r| r.id == ui.run))
        .map(crate::pi_ui::State::public)
        .transpose()?
        .unwrap_or(Value::Null);
    view["codex_input"] = codex_input
        .filter(|input| state.view.runs.iter().any(|r| r.id == input.run))
        .map(crate::codex_input::State::public)
        .transpose()?
        .unwrap_or(Value::Null);
    view["fixture_available"] = json!(fixture_available);
    view["browser_route"] = browser_route
        .map(|route| route.public())
        .unwrap_or(Value::Null);
    view["responses_route"] = match responses_route {
        Some(crate::https_profile::Profile::Https { binding, .. }) => binding.public_responses(),
        _ => Value::Null,
    };
    view["google_route"] = match google_route {
        Some(crate::https_profile::Profile::Https { binding, .. }) => binding.public_google(),
        _ => Value::Null,
    };
    view["chat_route"] = match chat_route {
        Some(crate::https_profile::Profile::Https { binding, .. }) => {
            binding.public_chat_completions()
        }
        _ => Value::Null,
    };
    if !state.browser_actions.is_empty() {
        view["browser_actions"] = browser_actions(&state.browser_actions);
    }
    for run in view["runs"].as_array_mut().ok_or("invalid internal view")? {
        let record = state
            .view
            .runs
            .iter()
            .find(|record| Some(record.id.as_str()) == run["id"].as_str())
            .ok_or("missing run")?;
        *run = run_record(state, record)?;
    }
    let fingerprint = profile.map(digest).transpose()?;
    view["pi_profile"] = match (profile, &fingerprint) {
        (Some(p), Some(f)) => json!({"fingerprint":f,"provider":p.provider,"model":p.model}),
        _ => Value::Null,
    };
    view["agent_sessions"]=Value::Array(state.workflows.iter().map(|(id,w)| {
        let ready=fingerprint.as_deref()==Some(&w.fingerprint) && state.view.runs.iter().any(|r|r.id==w.last_run && r.status.terminal()) && w.binding.resume_path().is_ok_and(|path|path.is_some());
        json!({"workflow":id,"source":w.source.id,"profile":w.fingerprint,"revision":w.revision,"ready":ready})
    }).collect());
    let fingerprint = codex_profile.map(digest).transpose()?;
    view["codex_profile"] = fingerprint
        .as_ref()
        .map(|fingerprint| json!({"fingerprint":fingerprint}))
        .unwrap_or(Value::Null);
    view["codex_sessions"] = Value::Array(state.codex_workflows.iter().map(|(id,w)| {
        let ready = fingerprint.as_deref() == Some(&w.fingerprint) && state.view.runs.iter().any(|r|r.id == w.last_run && r.status.terminal()) && w.binding.resume().is_ok_and(|id|id.is_some());
        json!({"workflow":id,"source":w.source.id,"profile":w.fingerprint,"revision":w.revision,"ready":ready})
    }).collect());
    let fingerprint = opencode_profile.map(digest).transpose()?;
    view["opencode_profile"] = fingerprint
        .as_ref()
        .map(|fingerprint| json!({"fingerprint":fingerprint}))
        .unwrap_or(Value::Null);
    view["opencode_sessions"] = Value::Array(state.opencode_workflows.iter().map(|(id,w)| {
        let ready=fingerprint.as_deref()==Some(&w.fingerprint) && state.view.runs.iter().any(|r|r.id==w.last_run && r.status.terminal()) && w.binding.resume().is_ok_and(|id|id.is_some());
        json!({"workflow":id,"source":w.source.id,"profile":w.fingerprint,"revision":w.revision,"ready":ready})
    }).collect());
    view["native_permissions"] = Value::Array(native_permissions.iter().map(|(request,pending)| {
        let details=&pending.details;
        json!({"run":pending.run,"request":request,"agent":details["agent"],"tool":details["tool"],"title":details["title"],"input":details["input"],"options":details["options"]})
    }).collect());
    if let Some(choices) = native_choices {
        choices.apply(&mut view)?;
    }
    Ok(view)
}

pub(super) fn run_record(state: &Store, record: &crate::model::Run) -> Result<Value> {
    let mut run = serde_json::to_value(record)?;
    let projection = record.output.projection();
    run["output"] = json!(projection.output);
    run["conversation"] = serde_json::to_value(projection.conversation)?;
    let launch = state.launches.get(&record.id).ok_or("missing launch")?;
    run["route"] = match launch {
        Launch::Messages { model, .. } => json!({"kind":"messages","model":model}),
        Launch::ConfiguredMessages { binding, .. } => binding.public(),
        Launch::ConfiguredResponses { binding, .. } => binding.public_responses(),
        Launch::ConfiguredGoogle { binding, .. } => binding.public_google(),
        Launch::ConfiguredChat { binding, .. } => binding.public_chat_completions(),
        Launch::Text {
            protocol, binding, ..
        } => json!({
            "kind":"text",
            "protocol":protocol.name(),
            "endpoint":binding.endpoint,
            "model":binding.model,
            "revision":binding.revision,
            "fingerprint":binding.fingerprint
        }),
        Launch::LegacyUnspecified => json!({"kind":"legacy_unspecified"}),
        Launch::Fixture { .. } => json!({"kind":"fixture"}),
        Launch::Pi { workflow, .. } => {
            let w = state.workflows.get(workflow).ok_or("missing workflow")?;
            json!({"kind":"pi","workflow":workflow,"provider":w.profile.provider,"model":w.profile.model})
        }
        Launch::Codex {
            workflow,
            selection,
            ..
        } => {
            json!({"kind":"codex","workflow":workflow,"provider":selection.as_ref().map(|s|&s.provider),"model":selection.as_ref().map(|s|&s.model)})
        }
        Launch::OpenCode {
            workflow,
            selection,
            ..
        } => {
            json!({"kind":"opencode","workflow":workflow,"model":selection.as_ref().map(|s|&s.model),"mode":selection.as_ref().map(|s|&s.mode)})
        }
    };
    run["activity"] = match launch {
        Launch::Pi { activity, .. }
        | Launch::Codex { activity, .. }
        | Launch::OpenCode { activity, .. } => serde_json::to_value(activity)?,
        _ => json!([]),
    };
    Ok(run)
}
