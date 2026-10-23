pub(crate) fn owner_scope(owner: &crate::flow::Owner) -> crate::history::Selection {
    if let Some(history) = &owner.record_history {
        return history.selection().expect("record history selection");
    }
    let store = owner.state.view.store.clone();
    crate::history::Selection {
        store: store.clone(),
        conversation: format!("{store}:1"),
        epoch: 1,
    }
}

#[cfg(test)]
mod tests {
    use crate::{CONFIG_MAX, FRAME_MAX, flow::*, model::*, native::*, store::*};
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        fs,
        path::Path,
        sync::{Arc, atomic::AtomicBool, mpsc},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn codex_input_answers_are_closed_and_prompt_validated() {
        let prompt = metldr_codex_session::InputPrompt::McpForm {
            server: "fixture".into(),
            message: "Choose a value".into(),
            fields: vec![metldr_codex_session::FormField {
                name: "choice".into(),
                title: "Choice".into(),
                description: None,
                min_chars: 0,
                max_chars: 8,
                choices: Some(vec!["one".into(), "two".into()]),
                default: None,
            }],
        };
        let answer: crate::codex_input::Answer =
            serde_json::from_value(json!({"kind":"accept","value":{"choice":"one"}})).unwrap();
        assert!(prompt.accepts(&answer.native()));
        let invalid: crate::codex_input::Answer =
            serde_json::from_value(json!({"kind":"accept","value":{"choice":"three"}})).unwrap();
        assert!(!prompt.accepts(&invalid.native()));
        assert!(
            serde_json::from_value::<crate::codex_input::Answer>(json!({
                "kind":"accept","value":{"choice":"one"},"extra":true
            }))
            .is_err()
        );
    }

    #[test]
    fn queued_output_can_finish_stopping_but_cannot_change_cancelled_run() {
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "terminal-output-check-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&output).unwrap();
        let (disk, mut state) = Disk::open(output.join("state")).unwrap();
        let source = state.id().unwrap();
        let run = state.id().unwrap();
        state.view.source = Some(Source {
            id: source.clone(),
            page: Page {
                tab: 1,
                document: "owned-document".into(),
                url: "http://127.0.0.1:1/article".into(),
                title: "North report".into(),
                text: "North reports 17 units.".into(),
            },
        });
        state.view.runs.push(Run {
            id: run.clone(),
            source,
            kind: "summary".into(),
            question: None,
            status: Status::StopRequested,
            output: metldr_conversation::Content::flat("Pending ".into()).unwrap(),
            error: None,
            corrections: Vec::new(),
        });
        state.starts.insert("fixture:request".into(), run.clone());
        state.launches.insert(
            run.clone(),
            crate::agent_model::Launch::Fixture {
                input_digest: "0".repeat(64),
            },
        );
        state.validate().unwrap();
        disk.save(&state).unwrap();
        let (tx, rx) = mpsc::sync_channel(2);
        let worker_tx = tx.clone();
        let worker_run = run.clone();
        let worker = thread::spawn(move || {
            worker_tx
                .send(Event::Text {
                    run: worker_run.clone(),
                    change: metldr_conversation::Change::FlatAppend("in flight".into()),
                })
                .unwrap();
            worker_tx
                .send(Event::Settled {
                    run: worker_run,
                    result: Err("stopped".into()),
                })
                .unwrap();
        });
        let mut owner = Owner {
            transport: crate::transport::Transport::stdio(),
            disk,
            settings: Default::default(),
            agent_runtime: Default::default(),
            state,
            browser_view: BTreeMap::new(),
            fixture: Some(crate::native::Fixture {
                origin: "http://127.0.0.1:1".into(),
                address: "127.0.0.1:1".parse().unwrap(),
            }),
            subscribers: BTreeMap::new(),
            operation: Some(Operation {
                run: run.clone(),
                stop: Arc::new(AtomicBool::new(true)),
                worker,
                correction: None,
            }),
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: BTreeMap::new(),
            pi_ui: None,
            codex_input: None,
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
            record_history: None,
            history_pins: Default::default(),
        };
        let Event::Text { run: id, change } = rx.recv().unwrap() else {
            panic!("chunk must precede settlement");
        };
        owner.text(&id, change).unwrap();
        assert_eq!(owner.state.view.runs[0].status, Status::StopRequested);
        assert_eq!(owner.state.view.runs[0].output.text(), "Pending in flight");
        let snapshot = owner.disk.directory.join("snapshot.json");
        fs::copy(&snapshot, output.join("stopping-snapshot.json")).unwrap();
        let Event::Settled { run: id, result } = rx.recv().unwrap() else {
            panic!("settlement must follow the queued chunk");
        };
        owner.settled(&id, result, false).unwrap();
        assert!(owner.operation.is_none());
        assert_eq!(owner.state.view.runs[0].status, Status::Cancelled);
        let before = fs::read(&snapshot).unwrap();
        assert_eq!(before, serde_json::to_vec(&owner.state).unwrap());
        fs::copy(&snapshot, output.join("before-late-output.json")).unwrap();
        // Inject directly: this worker cannot send a chunk after its own settlement.
        owner
            .text(
                &run,
                metldr_conversation::Change::FlatAppend("must not appear".into()),
            )
            .unwrap();
        assert_eq!(before, serde_json::to_vec(&owner.state).unwrap());
        assert_eq!(before, fs::read(&snapshot).unwrap());
        fs::copy(&snapshot, output.join("after-late-output.json")).unwrap();
        drop(owner);
        fs::remove_dir_all(output.join("state")).unwrap();
        eprintln!("terminal output evidence: {}", output.display());
    }

    #[test]
    fn chrome_parent_window_argument_requires_equals_form_decimal() {
        assert!(valid_parent_window_arg(&[]));
        assert!(valid_parent_window_arg(&["--parent-window=123".into()]));
        assert!(!valid_parent_window_arg(&["--parent-window=".into()]));
        assert!(!valid_parent_window_arg(&["--parent-window=12x".into()]));
        assert!(!valid_parent_window_arg(&[
            "--parent-window".into(),
            "123".into()
        ]));
    }

    #[test]
    fn bootstrap_config_is_bounded_and_rejects_unknown_fields() {
        let valid = br#"{"state":"/tmp/state","origin":"http://127.0.0.1:1234","extension_origin":"chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"}"#;
        assert!(parse_bootstrap_config(valid).is_ok());
        assert!(parse_bootstrap_config(
            br#"{"state":"/tmp/state","origin":"http://127.0.0.1:1234","extension_origin":"chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/","extra":true}"#
        )
        .is_err());
        assert!(parse_bootstrap_config(&vec![b' '; CONFIG_MAX + 1]).is_err());
        let without_fixture = br#"{"state":"/tmp/state","extension_origin":"chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"}"#;
        assert!(parse_bootstrap_config(without_fixture).is_ok());
        assert!(crate::https_profile::Config::Fixture.bind(None).is_err());
    }

    #[test]
    fn capture_is_cas_guarded_persistent_and_does_not_create_a_run() {
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!("capture-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&output);
        let (disk, mut history) = crate::history::History::load(output.join("state")).unwrap();
        let selection = history.selection().unwrap();
        let state = history.edit(|edit| edit.project(&selection, None)).unwrap();
        let scope = crate::history_io::Scope::from(&selection);
        let page = Page {
            tab: 3,
            document: "document-3".into(),
            url: "https://example.com/article".into(),
            title: "Example page".into(),
            text: "Captured text".into(),
        };
        let (transport, frames) = crate::transport::Transport::captured();
        let (tx, _) = mpsc::sync_channel(1);
        let mut owner = Owner {
            transport,
            disk,
            settings: Default::default(),
            agent_runtime: Default::default(),
            state,
            browser_view: BTreeMap::new(),
            fixture: None,
            subscribers: BTreeMap::new(),
            operation: None,
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: BTreeMap::new(),
            pi_ui: None,
            codex_input: None,
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
            record_history: Some(history),
            history_pins: Default::default(),
        };
        let capture = || Command::Conversation {
            scope: scope.clone(),
            action: crate::model::ConversationAction::Capture {
                expected_source: None,
                page: page.clone(),
            },
        };
        assert!(
            owner
                .command(
                    "capture:stale",
                    Command::Conversation {
                        scope: crate::history_io::Scope {
                            store: "0".repeat(32),
                            ..scope.clone()
                        },
                        action: crate::model::ConversationAction::Capture {
                            expected_source: None,
                            page: page.clone(),
                        },
                    }
                )
                .is_err()
        );
        owner.command("capture:first", capture()).unwrap();
        let output_frames = frames.lock().unwrap().clone();
        let mut reader = output_frames.as_slice();
        let mut replies = Vec::new();
        while let Some(frame) = crate::wire::read_raw(&mut reader, FRAME_MAX).unwrap() {
            replies.push(serde_json::from_slice::<serde_json::Value>(&frame).unwrap());
        }
        let receipt = replies.last().unwrap();
        assert_eq!(receipt["kind"], "view");
        assert_eq!(receipt["view"]["source"]["url"], page.url);
        assert_eq!(receipt["view"]["fixture_available"], false);
        let source_id = owner.state.view.source.as_ref().unwrap().id.clone();
        let next = owner.state.next;
        assert!(owner.state.view.runs.is_empty());
        owner.command("capture:retry", capture()).unwrap();
        let output_frames = frames.lock().unwrap().clone();
        let mut reader = output_frames.as_slice();
        let mut replies = Vec::new();
        while let Some(frame) = crate::wire::read_raw(&mut reader, FRAME_MAX).unwrap() {
            replies.push(serde_json::from_slice::<serde_json::Value>(&frame).unwrap());
        }
        assert_eq!(replies.last().unwrap()["kind"], "view");
        assert_eq!(replies.last().unwrap()["view"]["source"]["id"], source_id);
        assert_eq!(owner.state.view.source.as_ref().unwrap().id, source_id);
        assert_eq!(owner.state.next, next);
        assert!(
            owner
                .command(
                    "capture:cas",
                    Command::Conversation {
                        scope: scope.clone(),
                        action: crate::model::ConversationAction::Capture {
                            expected_source: None,
                            page: Page {
                                title: "Changed page".into(),
                                ..page
                            },
                        },
                    }
                )
                .is_err()
        );
        let captured_page = owner.state.view.source.as_ref().unwrap().page.clone();
        assert!(
            owner
                .command(
                    "fixture:disabled",
                    Command::Conversation {
                        scope: scope.clone(),
                        action: crate::model::ConversationAction::Summarize {
                            page: captured_page,
                        },
                    }
                )
                .is_err()
        );
        let directory = owner.disk.directory.clone();
        drop(owner);
        let (disk, mut history) = crate::history::History::load(directory).unwrap();
        let scope = history.selection().unwrap();
        let saved = history.edit(|edit| edit.project_latest(&scope)).unwrap();
        assert_eq!(saved.view.source.unwrap().id, source_id);
        assert!(saved.view.runs.is_empty());
        drop(history);
        drop(disk);
        let _ = fs::remove_dir_all(output);
    }

    #[test]
    fn pi_ui_offers_are_durable_scoped_transient_and_stopped() {
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "pi-ui-owner-check-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&output).unwrap();
        let (disk, mut state) = Disk::open(output.join("state")).unwrap();
        let source_id = state.id().unwrap();
        let workflow = state.id().unwrap();
        let run = state.id().unwrap();
        let profile = crate::agent_model::Profile {
            executable: std::env::current_exe().unwrap(),
            workspace: output.clone(),
            session_dir: output.clone(),
            provider: "fixture".into(),
            model: "fixture".into(),
            offline: true,
        };
        let source = Source {
            id: source_id.clone(),
            page: Page {
                tab: 1,
                document: "owned-document".into(),
                url: "https://example.com/article".into(),
                title: "Example page".into(),
                text: "Example page text.".into(),
            },
        };
        state.view.source = Some(source.clone());
        state.view.runs.push(Run {
            id: run.clone(),
            source: source_id,
            kind: "agent".into(),
            question: Some("Open the page".into()),
            status: Status::Running,
            output: metldr_conversation::Content::native(),
            error: None,
            corrections: Vec::new(),
        });
        state.workflows.insert(
            workflow.clone(),
            crate::agent_model::Workflow {
                fingerprint: crate::agent_model::digest(&profile).unwrap(),
                profile,
                source,
                revision: 1,
                last_run: run.clone(),
                binding: crate::agent_model::Binding::Pending,
            },
        );
        state.launches.insert(
            run.clone(),
            crate::agent_model::Launch::Pi {
                input_digest: "0".repeat(64),
                workflow,
                activity: vec![],
            },
        );
        state.validate().unwrap();
        disk.save(&state).unwrap();
        let (transport, frames) = crate::transport::Transport::captured();
        let (tx, _) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let mut owner = Owner {
            transport,
            disk,
            settings: Default::default(),
            agent_runtime: Default::default(),
            state,
            browser_view: BTreeMap::new(),
            fixture: None,
            subscribers: BTreeMap::new(),
            operation: Some(Operation {
                run: run.clone(),
                stop: stop.clone(),
                worker: thread::spawn(|| {}),
                correction: None,
            }),
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: BTreeMap::new(),
            pi_ui: None,
            codex_input: None,
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
            record_history: None,
            history_pins: Default::default(),
        };

        let nonce = owner.state.next;
        let (request, received) = offer_pi_ui(
            &mut owner,
            &run,
            1,
            crate::agent::ui::Dialog::Input {
                title: "Exact input".into(),
                placeholder: None,
            },
        );
        assert_eq!(request, format!("{}:{nonce}", owner.state.view.store));
        let snapshot = fs::read_to_string(owner.disk.directory.join("snapshot.json")).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(saved["next"], nonce + 1);
        let output_frames = frames.lock().unwrap().clone();
        let mut reader = output_frames.as_slice();
        let mut replies = Vec::new();
        while let Some(frame) = crate::wire::read_raw(&mut reader, FRAME_MAX).unwrap() {
            replies.push(serde_json::from_slice::<serde_json::Value>(&frame).unwrap());
        }
        assert_eq!(
            replies.last().unwrap()["view"]["pi_ui"]["pending"][0]["request"],
            request
        );
        assert!(
            owner
                .pi_ui_reply(
                    "wrong-run",
                    "other-run",
                    &request,
                    crate::agent::ui::Answer::Text("wrong".into()),
                )
                .is_err()
        );
        assert!(
            owner
                .pi_ui_reply(
                    "wrong-request",
                    &run,
                    "stale-request",
                    crate::agent::ui::Answer::Text("wrong".into()),
                )
                .is_err()
        );
        let exact = "Exact answer with spaces";
        owner
            .pi_ui_reply(
                "answer",
                &run,
                &request,
                crate::agent::ui::Answer::Text(exact.into()),
            )
            .unwrap();
        assert!(
            matches!(received.recv().unwrap(), crate::agent::ui::Answer::Text(value) if value == exact)
        );
        assert!(
            !fs::read_to_string(owner.disk.directory.join("snapshot.json"))
                .unwrap()
                .contains(exact)
        );

        let (disconnect_request, abandoned) = offer_pi_ui(
            &mut owner,
            &run,
            2,
            crate::agent::ui::Dialog::Confirm {
                title: "Disconnect check".into(),
                message: "Confirm?".into(),
            },
        );
        owner.disconnect_pi_ui().unwrap();
        assert!(
            owner
                .pi_ui_reply(
                    "disconnected-reply",
                    &run,
                    &disconnect_request,
                    crate::agent::ui::Answer::Confirm(true),
                )
                .is_err()
        );
        assert!(matches!(
            abandoned.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));

        owner.state.view.runs[0].status = Status::Running;
        stop.store(false, std::sync::atomic::Ordering::Release);
        let (stop_request, queued) = offer_pi_ui(
            &mut owner,
            &run,
            3,
            crate::agent::ui::Dialog::Confirm {
                title: "Stop check".into(),
                message: "Confirm?".into(),
            },
        );
        owner
            .pi_ui_reply(
                "queued-answer",
                &run,
                &stop_request,
                crate::agent::ui::Answer::Confirm(true),
            )
            .unwrap();
        owner.stop_run("stop", &run).unwrap();
        assert!(stop.load(std::sync::atomic::Ordering::Acquire));
        assert!(matches!(
            queued.try_recv(),
            Ok(crate::agent::ui::Answer::Confirm(true))
        ));
        assert!(
            owner
                .pi_ui_reply(
                    "after-stop",
                    &run,
                    &stop_request,
                    crate::agent::ui::Answer::Confirm(true),
                )
                .is_err()
        );
        drop(owner);
        fs::remove_dir_all(output).unwrap();
    }

    fn offer_pi_ui(
        owner: &mut Owner,
        run: &str,
        key: u64,
        dialog: crate::agent::ui::Dialog,
    ) -> (String, mpsc::Receiver<crate::agent::ui::Answer>) {
        let (answer, received) = mpsc::sync_channel(1);
        owner
            .pi_ui_change(
                run,
                crate::agent::ui::Change::Offer {
                    key,
                    dialog,
                    answer,
                },
            )
            .unwrap();
        let request = owner.pi_ui.as_ref().unwrap().public().unwrap()["pending"][0]["request"]
            .as_str()
            .unwrap()
            .to_owned();
        (request, received)
    }

    #[test]
    fn native_boundary_rejects_partial_oversize_and_invalid_utf8() {
        let value = json!({"v":crate::WIRE_VERSION,"request":"check:1","command":{"op":"view"}});
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut old = value.clone();
        old["v"] = json!(4);
        assert_eq!(
            crate::wire::decode_request(&serde_json::to_vec(&old).unwrap())
                .err()
                .unwrap()
                .to_string(),
            format!(
                "unsupported native wire version 4; expected {}",
                crate::WIRE_VERSION
            )
        );
        let mut framed = (bytes.len() as u32).to_ne_bytes().to_vec();
        framed.extend_from_slice(&bytes);
        assert!(matches!(
            read_frame(&mut framed.as_slice()).unwrap().unwrap().command,
            Command::View
        ));
        assert!(read_frame(&mut &framed[..framed.len() - 1]).is_err());
        assert!(read_frame(&mut &(FRAME_MAX as u32 + 1).to_ne_bytes()[..]).is_err());
        assert!(read_frame(&mut &[1, 0, 0, 0, 0xff][..]).is_err());
        assert!(read_frame(&mut &[][..]).unwrap().is_none());
    }
}
