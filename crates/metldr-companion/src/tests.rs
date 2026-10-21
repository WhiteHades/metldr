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
            fixture: Some(crate::native::Fixture {
                origin: "http://127.0.0.1:1".into(),
                address: "127.0.0.1:1".parse().unwrap(),
            }),
            subscribers: BTreeMap::new(),
            operation: Some(Operation {
                run: run.clone(),
                stop: Arc::new(AtomicBool::new(true)),
                worker,
            }),
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: BTreeMap::new(),
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
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
        let (disk, state) = Disk::open(output.join("state")).unwrap();
        let store = state.view.store.clone();
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
            fixture: None,
            subscribers: BTreeMap::new(),
            operation: None,
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: BTreeMap::new(),
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
        };
        let capture = || Command::Capture {
            expected_store: store.clone(),
            expected_source: None,
            page: page.clone(),
        };
        assert!(
            owner
                .command(
                    "capture:stale",
                    Command::Capture {
                        expected_store: "0".repeat(32),
                        expected_source: None,
                        page: page.clone(),
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
                    Command::Capture {
                        expected_store: store.clone(),
                        expected_source: None,
                        page: Page {
                            title: "Changed page".into(),
                            ..page
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
                    Command::Summarize {
                        page: captured_page
                    }
                )
                .is_err()
        );
        let directory = owner.disk.directory.clone();
        drop(owner);
        let (_disk, saved) = Disk::open(directory).unwrap();
        assert_eq!(saved.view.source.unwrap().id, source_id);
        assert!(saved.view.runs.is_empty());
        let _ = fs::remove_dir_all(output);
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
            "unsupported native wire version 4; expected 5"
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
