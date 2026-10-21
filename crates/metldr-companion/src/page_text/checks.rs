use super::*;
use crate::{
    agent_model::Launch,
    https_profile::Binding,
    model::{Page, View},
};
use serde_json::Value;
use std::collections::BTreeMap;

fn state() -> Store {
    Store {
        schema: crate::STORE_VERSION,
        view: View {
            sequence: 0,
            store: "0".repeat(32),
            source: Some(Source {
                id: format!("{}:1", "0".repeat(32)),
                page: Page {
                    tab: 1,
                    document: "document-not-for-the-provider".into(),
                    url: "https://example.test/report".into(),
                    title: "Report".into(),
                    text: "North reports 17 units.".into(),
                },
            }),
            runs: vec![],
        },
        next: 100,
        starts: BTreeMap::new(),
        launches: BTreeMap::new(),
        workflows: BTreeMap::new(),
        codex_workflows: BTreeMap::new(),
        opencode_workflows: BTreeMap::new(),
        browser_actions: BTreeMap::new(),
        providers: Default::default(),
        agent_settings: Default::default(),
    }
}

fn add(
    state: &mut Store,
    protocol: Protocol,
    fingerprint: &str,
    status: Status,
    source: &str,
    output: String,
) {
    let id = format!("{}:{}", state.view.store, state.view.runs.len() + 3);
    state.view.runs.push(Run {
        id: id.clone(),
        source: source.into(),
        kind: "answer".into(),
        question: Some("Prior question".into()),
        status,
        output: metldr_conversation::Content::flat(output).unwrap(),
        error: None,
    });
    state.launches.insert(
        id,
        Launch::Text {
            input_digest: "a".repeat(64),
            protocol,
            binding: Binding {
                endpoint: "https://example.test/v1".into(),
                model: "model".into(),
                revision: 1,
                fingerprint: fingerprint.into(),
            },
        },
    );
}

#[test]
fn context_contains_only_completed_same_source_and_provider_selection_answers() {
    let mut state = state();
    let source = state.view.source.as_ref().unwrap().clone();
    let current = "a".repeat(64);
    let different = "b".repeat(64);
    for (protocol, fingerprint, status, page, text) in [
        (
            Protocol::Messages,
            &current,
            Status::Complete,
            source.id.as_str(),
            "Relevant answer",
        ),
        (
            Protocol::Responses,
            &current,
            Status::Complete,
            source.id.as_str(),
            "Other protocol",
        ),
        (
            Protocol::Messages,
            &different,
            Status::Complete,
            source.id.as_str(),
            "Other selection",
        ),
        (
            Protocol::Messages,
            &current,
            Status::Failed,
            source.id.as_str(),
            "Failed output",
        ),
        (
            Protocol::Messages,
            &current,
            Status::Complete,
            "other-page",
            "Other page",
        ),
    ] {
        add(&mut state, protocol, fingerprint, status, page, text.into());
    }
    let prompt = Prompt::Answer {
        text: "How many units?".into(),
    };
    let context: Value = serde_json::from_str(
        &input(&state, &source, Protocol::Messages, &current, &prompt).unwrap(),
    )
    .unwrap();
    assert_eq!(
        context,
        json!({"request":{"kind":"answer","text":"How many units?"},
        "source":{"title":"Report","url":"https://example.test/report","text":"North reports 17 units."},
        "history":[{"kind":"answer","question":"Prior question","output":"Relevant answer"}]})
    );
}

#[test]
fn context_limit_drops_whole_old_turns_and_preserves_latest_order() {
    let mut state = state();
    let source = state.view.source.as_ref().unwrap().clone();
    let fingerprint = "a".repeat(64);
    for at in 0..5 {
        add(
            &mut state,
            Protocol::Messages,
            &fingerprint,
            Status::Complete,
            &source.id,
            format!("turn{at}:{}", "x".repeat(8186)),
        );
    }
    let encoded = input(
        &state,
        &source,
        Protocol::Messages,
        &fingerprint,
        &Prompt::Summary,
    )
    .unwrap();
    assert!(encoded.len() <= INPUT_MAX);
    let context: Value = serde_json::from_str(&encoded).unwrap();
    let history = context["history"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["output"], format!("turn3:{}", "x".repeat(8186)));
    assert_eq!(history[1]["output"], format!("turn4:{}", "x".repeat(8186)));
    assert_eq!(context["source"]["text"], source.page.text);
}

#[test]
fn context_retains_at_most_six_complete_turns() {
    let mut state = state();
    let source = state.view.source.as_ref().unwrap().clone();
    let fingerprint = "a".repeat(64);
    for at in 0..9 {
        add(
            &mut state,
            Protocol::Messages,
            &fingerprint,
            Status::Complete,
            &source.id,
            format!("turn{at}"),
        );
    }
    let encoded = input(
        &state,
        &source,
        Protocol::Messages,
        &fingerprint,
        &Prompt::Summary,
    )
    .unwrap();
    let context: Value = serde_json::from_str(&encoded).unwrap();
    let history = context["history"].as_array().unwrap();
    assert_eq!(history.len(), 6);
    assert_eq!(history[0]["output"], "turn3");
    assert_eq!(history[5]["output"], "turn8");
}
