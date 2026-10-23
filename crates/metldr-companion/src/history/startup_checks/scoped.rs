use super::{Directory, ORIGIN, Running, empty_legacy};
use crate::{
    agent_model::{Launch, digest},
    history::History,
    history_io::Scope,
    model::{Page, Run, Status},
    provider_settings::Protocol,
};
use serde_json::{Value, json};

fn scoped(scope: &Value, action: Value) -> Value {
    json!({"op":"conversation","scope":scope,"action":action})
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY with the current companion"]
fn scoped_controls_and_start_retry_survive_selection_without_replay() -> crate::Result<()> {
    let directory = Directory::new()?;
    empty_legacy(&directory.0, Some(8))?;
    let (disk, mut history) = History::load(directory.0.clone())?;
    let selection = history.selection()?;
    let messages = crate::https_profile::Config::Https {
        endpoint: "https://example.com/v1/messages".into(),
        model: "saved-messages".into(),
        revision: 1,
        credential_env: "METLDR_SAVED_MESSAGES_KEY".into(),
        trust_ca: None,
    }
    .bind(None)?;
    let crate::https_profile::Profile::Https {
        binding: messages, ..
    } = messages
    else {
        unreachable!()
    };
    let responses = crate::https_profile::Config::Https {
        endpoint: "https://example.com/v1/responses".into(),
        model: "saved-responses".into(),
        revision: 1,
        credential_env: "METLDR_SAVED_RESPONSES_KEY".into(),
        trust_ca: None,
    }
    .bind_responses(None)?;
    let crate::https_profile::Profile::Https {
        binding: responses, ..
    } = responses
    else {
        unreachable!()
    };
    let google = crate::https_profile::Config::Https {
        endpoint: "https://example.com/v1/interactions".into(),
        model: "saved-google".into(),
        revision: 1,
        credential_env: "METLDR_SAVED_GOOGLE_KEY".into(),
        trust_ca: None,
    }
    .bind_google(None)?;
    let crate::https_profile::Profile::Https {
        binding: google, ..
    } = google
    else {
        unreachable!()
    };
    let chat = crate::https_profile::Config::Https {
        endpoint: "https://example.com/v1/chat/completions".into(),
        model: "saved-chat".into(),
        revision: 1,
        credential_env: "METLDR_SAVED_CHAT_KEY".into(),
        trust_ca: None,
    }
    .bind_chat_completions(None)?;
    let crate::https_profile::Profile::Https { binding: chat, .. } = chat else {
        unreachable!()
    };
    let (source, run, action, retries) = history.edit(|edit| {
        let source = edit.new_source(
            &selection,
            Page {
                tab: 1,
                document: "scope-owned-document".into(),
                url: format!("{ORIGIN}/article"),
                title: "Scope owned source".into(),
                text: "Source for the exact saved request.".into(),
            },
        )?;
        let action = json!({"op":"ask","source":source.id,"text":"Keep the original request"});
        let input_digest =
            digest(&json!({"command":action,"origin":ORIGIN,"model":"metldr-fixture"}))?;
        let run = edit.id()?;
        edit.admit(
            &selection,
            "saved-scope-start",
            Run {
                id: run.clone(),
                source: source.id.clone(),
                kind: "answer".into(),
                question: Some("Keep the original request".into()),
                status: Status::Running,
                output: Default::default(),
                error: None,
                corrections: Vec::new(),
            },
            Launch::Fixture { input_digest },
        )?;
        let mut record = edit.run(&selection, &run)?;
        record.status = Status::Complete;
        record.output = metldr_conversation::Content::flat("Original saved result".into())?;
        edit.put_run(&selection, record)?;
        let retries = [
            (
                "saved-page-text",
                json!({"op":"page_text","source":source.id,"protocol":Protocol::Messages,
                    "fingerprint":messages.fingerprint,"prompt":{"kind":"answer","text":"Saved text question"}}),
                Launch::Text {
                    input_digest: digest(&json!({"operation":"page_text","source":source.id,
                        "protocol":Protocol::Messages,"binding":messages,"prompt":{"kind":"answer","text":"Saved text question"}}))?,
                    protocol: Protocol::Messages,
                    binding: messages.clone(),
                },
                "answer",
                Some("Saved text question"),
            ),
            (
                "saved-configured-messages",
                json!({"op":"browser_task","source":source.id,"text":"Saved browser task",
                    "fingerprint":messages.fingerprint}),
                Launch::ConfiguredMessages {
                    input_digest: digest(&json!({"source":source.id,"text":"Saved browser task",
                        "origin":ORIGIN,"route":messages.public()}))?,
                    binding: messages.clone(),
                },
                "browser",
                Some("Saved browser task"),
            ),
            (
                "saved-configured-responses",
                json!({"op":"responses_task","source":source.id,"text":"Saved Responses task",
                    "fingerprint":responses.fingerprint}),
                Launch::ConfiguredResponses {
                    input_digest: digest(&json!({"source":source.id,"text":"Saved Responses task",
                        "origin":ORIGIN,"route":responses.public_responses()}))?,
                    binding: responses.clone(),
                },
                "browser",
                Some("Saved Responses task"),
            ),
            (
                "saved-configured-google",
                json!({"op":"google_task","source":source.id,"text":"Saved Google task",
                    "fingerprint":google.fingerprint}),
                Launch::ConfiguredGoogle {
                    input_digest: digest(&json!({"source":source.id,"text":"Saved Google task",
                        "origin":ORIGIN,"route":google.public_google()}))?,
                    binding: google.clone(),
                },
                "browser",
                Some("Saved Google task"),
            ),
            (
                "saved-configured-chat",
                json!({"op":"chat_task","source":source.id,"text":"Saved Chat task",
                    "fingerprint":chat.fingerprint}),
                Launch::ConfiguredChat {
                    input_digest: digest(&json!({"source":source.id,"text":"Saved Chat task",
                        "origin":ORIGIN,"route":chat.public_chat_completions()}))?,
                    binding: chat.clone(),
                },
                "browser",
                Some("Saved Chat task"),
            ),
        ];
        let mut saved = Vec::new();
        for (request, action, launch, kind, question) in retries {
            let run = edit.id()?;
            edit.admit(
                &selection,
                request,
                Run {
                    id: run.clone(),
                    source: source.id.clone(),
                    kind: kind.into(),
                    question: question.map(str::to_owned),
                    status: Status::Running,
                    output: Default::default(),
                    error: None,
                    corrections: Vec::new(),
                },
                launch,
            )?;
            let mut record = edit.run(&selection, &run)?;
            record.status = Status::Complete;
            record.output = metldr_conversation::Content::flat("Original route result".into())?;
            edit.put_run(&selection, record)?;
            saved.push((request, action, run));
        }
        Ok((source.id, run, action, saved))
    })?;
    let a = serde_json::to_value(Scope::from(&selection))?;
    drop(history);
    drop(disk);

    let mut host = Running::start(&directory.0)?;
    let ready = host.ready()?;
    assert_eq!(ready["view"]["selection"], a);
    host.send("watch-current", scoped(&a, json!({"op":"watch","run":run})))?;
    let watched = host.reply("watch-current")?;
    assert_eq!(watched["kind"], "view");
    assert_eq!(watched["view"]["selection"], a);
    assert_eq!(watched["ended"], true);
    host.send("stop-current", scoped(&a, json!({"op":"stop","run":run})))?;
    let stopped = host.reply("stop-current")?;
    assert_eq!(stopped["kind"], "already_terminal");
    assert_eq!(stopped["scope"], a);
    assert_eq!(stopped["run"], run);

    host.send(
        "select-b",
        json!({"op":"conversation_change","scope":a,"change":{"kind":"create"}}),
    )?;
    let created = host.reply("select-b")?;
    assert_eq!(created["kind"], "conversation_changed");
    let b = created["current"].clone();
    let sequence = created["sequence"].clone();
    host.send("saved-scope-start", scoped(&a, action.clone()))?;
    let duplicate = host.reply("saved-scope-start")?;
    assert_eq!(duplicate["kind"], "started");
    assert_eq!(duplicate["scope"], a);
    assert_eq!(duplicate["run"], run);
    for (request, action, run) in &retries {
        host.send(request, scoped(&a, action.clone()))?;
        let duplicate = host.reply(request)?;
        assert_eq!(duplicate["kind"], "started");
        assert_eq!(duplicate["scope"], a);
        assert_eq!(duplicate["run"], run.as_str());
    }
    let mut changed = retries[1].1.clone();
    changed["text"] = json!("Changed browser task");
    host.send(retries[1].0, scoped(&a, changed))?;
    assert_eq!(host.reply(retries[1].0)?["kind"], "error");
    host.send("still-b", json!({"op":"view"}))?;
    let unchanged = host.reply("still-b")?;
    assert_eq!(unchanged["view"]["selection"], b);
    assert!(unchanged["view"]["source"].is_null());
    assert_eq!(unchanged["view"]["runs"], json!([]));
    assert_eq!(
        unchanged["view"]["sequence"].as_u64().unwrap().to_string(),
        sequence.as_str().unwrap()
    );

    host.send("stale-new-start", scoped(&a, action.clone()))?;
    assert_eq!(host.reply("stale-new-start")?["kind"], "error");
    host.send("select-a", json!({"op":"conversation_change","scope":b,"change":{"kind":"select","conversation":a["conversation"]}}))?;
    let returned = host.reply("select-a")?;
    let fresh = returned["current"].clone();
    assert_ne!(fresh["epoch"], a["epoch"]);
    for (index, action) in [
        json!({"op":"watch","run":run}),
        json!({"op":"stop","run":run}),
        json!({"op":"native_permission","run":run,"request":"123","option":null}),
        json!({"op":"browser_decision","action":run,"revision":1,"approve":false}),
        json!({"op":"codex_input_reply","run":run,"request":"123","answer":{"kind":"cancel"}}),
        json!({"op":"pi_ui_reply","run":run,"request":"123","answer":{"kind":"cancel"}}),
    ]
    .into_iter()
    .enumerate()
    {
        let request = format!("stale-control-{index}");
        host.send(&request, scoped(&a, action))?;
        let refused = host.reply(&request)?;
        assert_eq!(refused["kind"], "error");
        assert_eq!(refused["message"], "history selection changed");
    }
    host.send("saved-scope-start", scoped(&fresh, action))?;
    assert_eq!(host.reply("saved-scope-start")?["kind"], "error");
    host.send("final-source", json!({"op":"view"}))?;
    let final_view = host.reply("final-source")?;
    assert_eq!(final_view["view"]["source"]["id"], source);
    assert_eq!(final_view["view"]["selection"], fresh);
    assert!(host.finish()?.success());
    Ok(())
}
