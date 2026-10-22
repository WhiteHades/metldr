use super::{History, fixture, schema::*};
use crate::{
    Result,
    flow::{Event, Operation, Owner},
    model::{Command, Request, Status},
    store::Disk,
    transport::Transport,
};
use redb::{ReadableDatabase, ReadableTable};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "native-input-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_owner(engine: &str) -> Result<(Directory, Owner, Arc<std::sync::Mutex<Vec<u8>>>, String)> {
    let directory = Directory::new()?;
    let disk = Disk::lock(directory.0.clone())?;
    let mut original = fixture::legacy();
    crate::recovery::interrupt(&mut original)?;
    let run = match engine {
        "pi" => "0123456789abcdef0123456789abcdef:09",
        "codex" => "0123456789abcdef0123456789abcdef:10",
        _ => "0123456789abcdef0123456789abcdef:11",
    }
    .to_owned();
    let record = original
        .view
        .runs
        .iter_mut()
        .find(|record| record.id == run)
        .ok_or("missing fixture native run")?;
    record.status = Status::Running;
    record.error = None;
    record.output = metldr_conversation::Content::native();
    original.browser_actions.clear();
    original.validate()?;
    disk.save(&original)?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &serde_json::to_vec(&original)?, disk.lease())?;
    let scope = history.selection()?;
    let seeded = history.edit(|edit| edit.native_task(&scope, &run))?;
    assert_eq!(seeded.run.status, Status::Running);
    assert_eq!(seeded.workflow.source().id, seeded.run.source);
    let state = history.edit(|edit| edit.project(&scope, Some(&run)))?;
    let (transport, frames) = Transport::captured();
    let (tx, _) = mpsc::sync_channel::<Event>(8);
    let worker = thread::spawn(|| {});
    let owner = Owner {
        transport,
        disk,
        state,
        browser_view: BTreeMap::new(),
        record_history: Some(history),
        history_pins: Default::default(),
        settings: Default::default(),
        agent_runtime: Default::default(),
        fixture: None,
        subscribers: BTreeMap::new(),
        operation: Some(Operation {
            run: run.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            worker,
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
    };
    Ok((directory, owner, frames, run))
}

fn sequence(owner: &Owner) -> Result<u64> {
    let history = owner.record_history.as_ref().unwrap();
    let read = history.db.begin_read()?;
    let meta = read.open_table(META)?;
    let profile = meta.get("profile")?.unwrap();
    Ok(decode::<Profile>(profile.value())?.sequence)
}

fn stop_owner(owner: &mut Owner) {
    if let Some(operation) = owner.operation.take() {
        operation.worker.join().expect("owned input-check worker");
    }
}

fn pi_offer(
    owner: &mut Owner,
    run: &str,
    key: u64,
) -> Result<mpsc::Receiver<crate::agent::ui::Answer>> {
    let (tx, rx) = mpsc::sync_channel(1);
    owner.pi_ui_change(
        run,
        crate::agent::ui::Change::Offer {
            key,
            dialog: crate::agent::ui::Dialog::Select {
                title: "Choose".into(),
                options: vec!["yes".into()],
            },
            answer: tx,
        },
    )?;
    Ok(rx)
}

#[test]
fn native_input_settlement_publishes_retained_pi_display_without_pending_replies() -> Result<()> {
    for ending in ["complete", "stop", "input-failure"] {
        let (_directory, mut owner, frames, run) = make_owner("pi")?;
        owner.pi_ui_change(
            &run,
            crate::agent::ui::Change::Display(crate::agent::ui::Display::Notify {
                message: "completion notice".into(),
                level: "info".into(),
            }),
        )?;
        let pending = pi_offer(&mut owner, &run, 99)?;
        frames.lock().unwrap().clear();
        match ending {
            "complete" => {
                owner.agent_settled(&run, crate::agent::Outcome::Complete, false, true)?
            }
            "stop" => owner.stop_record("stop-visible-input", &run)?,
            _ => owner.stop_native_record_inputs(&run, "input failed", None, true)?,
        }
        let bytes = frames.lock().unwrap().clone();
        let mut cursor = std::io::Cursor::new(bytes);
        let mut changed = false;
        while let Some(bytes) = crate::wire::read_raw(&mut cursor, crate::FRAME_MAX)? {
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            if value["kind"] == "changed" {
                changed = true;
                assert_eq!(value["view"]["pi_ui"]["run"], run, "{ending}");
                assert_eq!(
                    value["view"]["pi_ui"]["pending"],
                    serde_json::json!([]),
                    "{ending}"
                );
                assert_eq!(
                    value["view"]["pi_ui"]["notices"][0]["message"], "completion notice",
                    "{ending}"
                );
            }
        }
        assert!(changed, "{ending} did not publish its settled display");
        assert!(matches!(
            pending.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        stop_owner(&mut owner);
    }
    Ok(())
}

#[test]
fn native_input_records_reserve_publish_cancel_and_keep_payloads_volatile() -> Result<()> {
    let (directory, mut owner, frames, run) = make_owner("pi")?;
    let original_snapshot = fs::read(directory.0.join("snapshot.json"))?;
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "watch-history".into(),
        command: Command::Conversation {
            scope: crate::history_io::Scope::from(&crate::tests::owner_scope(&owner)),
            action: crate::model::ConversationAction::Watch {
                run: format!("{}:01", owner.state.view.store),
            },
        },
    })?;
    assert!(!owner.state.view.runs.iter().any(|saved| saved.id == run));
    let mut last = 0;
    let first_sequence = sequence(&owner)?;
    for key in 1..=17 {
        let answer = pi_offer(&mut owner, &run, key)?;
        assert!(
            owner.pi_ui.as_ref().is_some_and(|ui| {
                ui.public().unwrap()["pending"].as_array().unwrap().len() == 1
            }),
            "live Pi prompt was lost while viewing history"
        );
        let request = owner.pi_ui.as_ref().unwrap().public()?["pending"][0]["request"]
            .as_str()
            .unwrap()
            .to_owned();
        let before = sequence(&owner)?;
        let next = owner.state.view.sequence;
        if key == 1 {
            assert!(
                owner
                    .pi_ui_reply(
                        "bad",
                        &run,
                        &request,
                        crate::agent::ui::Answer::Select("no".into())
                    )
                    .is_err()
            );
            assert!(matches!(answer.try_recv(), Err(mpsc::TryRecvError::Empty)));
        }
        owner.pi_ui_reply(
            "answer",
            &run,
            &request,
            crate::agent::ui::Answer::Select("yes".into()),
        )?;
        assert!(matches!(
            answer.try_recv().unwrap(),
            crate::agent::ui::Answer::Select(value) if value == "yes"
        ));
        assert_eq!(sequence(&owner)?, before + 1);
        assert_eq!(owner.state.view.sequence, next + 1);
        assert!(frames.lock().unwrap().len() >= 2 * key as usize);
        let id = request.rsplit(':').next().unwrap().parse::<u64>()?;
        assert!(id > last);
        last = id;
    }
    assert_eq!(
        fs::read(directory.0.join("snapshot.json"))?,
        original_snapshot
    );
    assert_eq!(sequence(&owner)?, first_sequence + 34);
    let scope = owner.record_history.as_ref().unwrap().selection()?;
    let saved = owner
        .record_history
        .as_mut()
        .unwrap()
        .edit(|edit| edit.native_task(&scope, &run))?;
    assert!(serde_json::to_string(&saved.run)?.find("yes").is_none());
    assert!(!frames.lock().unwrap().is_empty());
    owner.pi_ui_change(
        &run,
        crate::agent::ui::Change::Display(crate::agent::ui::Display::Title("visible".into())),
    )?;
    assert_eq!(owner.pi_ui.as_ref().unwrap().public()?["title"], "visible");
    let withdrawn = pi_offer(&mut owner, &run, 90)?;
    owner.pi_ui_change(&run, crate::agent::ui::Change::Withdraw { key: 90 })?;
    assert!(matches!(
        withdrawn.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    let disconnected = pi_offer(&mut owner, &run, 91)?;
    owner.disconnect_pi_ui()?;
    assert!(matches!(
        disconnected.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(
        owner
            .operation
            .as_ref()
            .unwrap()
            .stop
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let stopped = owner
        .record_history
        .as_mut()
        .unwrap()
        .edit(|edit| edit.native_task(&scope, &run))?;
    assert_eq!(stopped.run.status, Status::StopRequested);
    let before_display = sequence(&owner)?;
    owner.pi_ui_change(
        &run,
        crate::agent::ui::Change::Display(crate::agent::ui::Display::Title("stopping".into())),
    )?;
    assert_eq!(sequence(&owner)?, before_display + 1);
    assert_eq!(
        owner.state.view.runs.last().unwrap().status,
        Status::StopRequested
    );
    stop_owner(&mut owner);

    let (_directory, mut stale, _, run) = make_owner("pi")?;
    let scope = stale.record_history.as_ref().unwrap().selection()?;
    stale.record_history.as_mut().unwrap().edit(|edit| {
        let mut task = edit.native_task(&scope, &run)?;
        task.run.status = Status::StopRequested;
        edit.put_native(&scope, task)
    })?;
    let stopped_sequence = sequence(&stale)?;
    stale.pi_ui_change(
        &run,
        crate::agent::ui::Change::Display(crate::agent::ui::Display::Title("stale".into())),
    )?;
    assert_eq!(sequence(&stale)?, stopped_sequence);
    assert!(stale.pi_ui.is_none());
    stop_owner(&mut stale);

    // Publication fails after the durable edit. The fatal path cancels waiters and keeps that edit.
    let (directory, mut failed, _, run) = make_owner("pi")?;
    let answer = pi_offer(&mut failed, &run, 1)?;
    let request = failed.pi_ui.as_ref().unwrap().public()?["pending"][0]["request"]
        .as_str()
        .unwrap()
        .to_owned();
    let before_reply = sequence(&failed)?;
    let reserved = failed.state.next;
    failed.transport.fail_next_emit();
    let result = failed.pi_ui_reply(
        "answer",
        &run,
        &request,
        crate::agent::ui::Answer::Select("yes".into()),
    );
    assert!(result.is_err());
    assert!(
        failed
            .operation
            .as_ref()
            .unwrap()
            .stop
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert!(
        failed.pi_ui.as_ref().unwrap().public()?["pending"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(sequence(&failed)?, before_reply + 1);
    assert!(matches!(
        answer.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    drop(failed.record_history.take());
    let mut reopened = History::open(&directory.0.join("history.redb"), failed.disk.lease())?;
    assert_eq!(reopened.selection()?.epoch, 1);
    let scope = reopened.selection()?;
    assert_eq!(
        reopened
            .edit(|edit| edit.native_task(&scope, &run))?
            .run
            .status,
        Status::Running
    );
    let read = reopened.db.begin_read()?;
    let meta = read.open_table(META)?;
    let profile = meta.get("profile")?.unwrap();
    assert_eq!(decode::<Profile>(profile.value())?.next, reserved);
    stop_owner(&mut failed);

    // A stale/missing run row is a fatal history failure before an answer can be delivered.
    let (_directory, mut corrupt, _, run) = make_owner("pi")?;
    let answer = pi_offer(&mut corrupt, &run, 7)?;
    let request = corrupt.pi_ui.as_ref().unwrap().public()?["pending"][0]["request"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = sequence(&corrupt)?;
    let number = run.rsplit(':').next().unwrap().parse::<u64>()?;
    let conversation = corrupt
        .record_history
        .as_ref()
        .unwrap()
        .selection()?
        .conversation;
    let write = corrupt.record_history.as_ref().unwrap().db.begin_write()?;
    assert!(
        write
            .open_table(RUN_ORDER)?
            .remove((conversation.as_str(), number))?
            .is_some()
    );
    write.commit()?;
    let error = corrupt.pi_ui_reply(
        "answer",
        &run,
        &request,
        crate::agent::ui::Answer::Select("yes".into()),
    );
    assert!(error.is_err());
    assert!(crate::history::is_failure(error.unwrap_err().as_ref()));
    assert!(matches!(
        answer.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(sequence(&corrupt)?, before);
    stop_owner(&mut corrupt);

    for engine in ["codex", "opencode"] {
        let (_directory, mut owner, frames, run) = make_owner(engine)?;
        let snapshot = fs::read(owner.disk.directory.join("snapshot.json"))?;
        let details = serde_json::json!({
            "agent": engine,
            "tool": "Command",
            "title": "Run command",
            "input": "synthetic command",
            "options": [{"id":"0", "name":"Allow once"}]
        });
        let (ack, answer) = mpsc::sync_channel(1);
        let before = sequence(&owner)?;
        let before_view = owner.state.view.sequence;
        owner.native_permission(&run, 1, details.clone(), ack)?;
        let request = owner.native_permissions.keys().next().unwrap().clone();
        assert_eq!(sequence(&owner)?, before + 1);
        assert_eq!(owner.state.view.sequence, before_view + 1);
        assert_eq!(
            fs::read(owner.disk.directory.join("snapshot.json"))?,
            snapshot
        );
        assert!(
            owner
                .native_decide("wrong-run", "other-run", &request, Some("0"))
                .is_err()
        );
        assert!(
            owner
                .native_decide("wrong-choice", &run, &request, Some("not-offered"))
                .is_err()
        );
        assert!(matches!(answer.try_recv(), Err(mpsc::TryRecvError::Empty)));
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "permission-history".into(),
            command: Command::Conversation {
                scope: crate::history_io::Scope::from(&crate::tests::owner_scope(&owner)),
                action: crate::model::ConversationAction::Watch {
                    run: format!("{}:01", owner.state.view.store),
                },
            },
        })?;
        assert!(!owner.state.view.runs.iter().any(|saved| saved.id == run));
        let before_reply = frames.lock().unwrap().len();
        let before_reply_view = owner.state.view.sequence;
        owner.native_decide("accepted", &run, &request, Some("0"))?;
        assert!(frames.lock().unwrap().len() > before_reply);
        assert_eq!(owner.state.view.sequence, before_reply_view + 1);
        assert_eq!(answer.try_recv().unwrap().as_deref(), Some("0"));
        assert_eq!(sequence(&owner)?, before + 2);
        assert!(!serde_json::to_string(&owner.state.view.runs)?.contains("synthetic command"));
        assert!(!frames.lock().unwrap().is_empty());
        let (resolved_ack, resolved_reply) = mpsc::sync_channel(1);
        owner.native_permission(&run, 2, details.clone(), resolved_ack)?;
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "resolved-history".into(),
            command: Command::Conversation {
                scope: crate::history_io::Scope::from(&crate::tests::owner_scope(&owner)),
                action: crate::model::ConversationAction::Watch {
                    run: format!("{}:01", owner.state.view.store),
                },
            },
        })?;
        let before_resolution = sequence(&owner)?;
        owner.native_permission_resolved(&run, 2)?;
        assert_eq!(sequence(&owner)?, before_resolution + 1);
        assert!(owner.native_permissions.is_empty());
        assert!(matches!(
            resolved_reply.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        stop_owner(&mut owner);
    }

    let (_directory, mut duplicate, _, run) = make_owner("codex")?;
    let details = serde_json::json!({
        "agent":"codex",
        "tool":"Command",
        "title":"Run command",
        "input":"synthetic command",
        "options":[{"id":"0", "name":"Allow once"}]
    });
    let (first_tx, first_rx) = mpsc::sync_channel(1);
    duplicate.native_permission(&run, 44, details.clone(), first_tx)?;
    let (duplicate_tx, duplicate_rx) = mpsc::sync_channel(1);
    duplicate.native_permission(&run, 44, details, duplicate_tx)?;
    assert_eq!(first_rx.try_recv().unwrap(), None);
    assert_eq!(duplicate_rx.try_recv().unwrap(), None);
    assert!(duplicate.native_permissions.is_empty());
    assert!(
        duplicate
            .operation
            .as_ref()
            .unwrap()
            .stop
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let scope = duplicate.record_history.as_ref().unwrap().selection()?;
    assert_eq!(
        duplicate
            .record_history
            .as_mut()
            .unwrap()
            .edit(|edit| edit.native_task(&scope, &run))?
            .run
            .status,
        Status::StopRequested
    );
    stop_owner(&mut duplicate);
    Ok(())
}
