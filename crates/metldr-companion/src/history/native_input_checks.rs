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
    make_owner_with_binding(engine, false)
}

fn make_owner_with_binding(
    engine: &str,
    bound: bool,
) -> Result<(Directory, Owner, Arc<std::sync::Mutex<Vec<u8>>>, String)> {
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
    if bound {
        let workflow = original.workflows.values_mut().next().unwrap();
        let sessions = directory.0.join("sessions");
        fs::create_dir(&sessions)?;
        let path = sessions.join("pi-session-1.jsonl");
        fs::write(&path, b"owned session fixture\n")?;
        workflow.profile.session_dir = sessions;
        workflow.fingerprint = crate::agent_model::digest(&workflow.profile)?;
        workflow.binding = crate::agent_model::Binding::Bound {
            id: "pi-session-1".into(),
            path,
        };
    }
    original.validate()?;
    disk.save(&original)?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &serde_json::to_vec(&original)?, disk.lease())?;
    let scope = history.selection()?;
    let seeded = history.edit(|edit| {
        edit.current_execution(&scope, &run)
            .and_then(|execution| edit.native_task(&execution, &run))
    })?;
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
        operations: BTreeMap::from([(
            run.clone(),
            Operation {
                stop: Arc::new(AtomicBool::new(false)),
                worker,
                correction: None,
            },
        )]),
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
    for (_, operation) in std::mem::take(&mut owner.operations) {
        operation.worker.join().expect("owned input-check worker");
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn codex_input_replies_and_withdrawals_preserve_owned_waiters() -> Result<()> {
    use metldr_codex_session::{CodexSession, Event as NativeEvent, InputAnswer, Thread};
    use std::os::unix::fs::PermissionsExt;

    let (directory, mut owner, _, run) = make_owner("codex")?;
    let executable = directory.0.join("input-peer");
    let send = |value: serde_json::Value| {
        format!(
            "printf '%s\\n' '{}'\n",
            value.to_string().replace('\'', "'\\''")
        )
    };
    let mut script = format!(
        "#!/bin/sh\nread -r line\n{}read -r line\nread -r line\n{}",
        send(serde_json::json!({"id":1,"result":{}})),
        send(serde_json::json!({"id":2,"result":{
            "thread":{"id":"thread-1","sessionId":"owned-session"},
            "model":"fixture","modelProvider":"fixture","cwd":directory.0
        }})),
    );
    for id in 1..=4 {
        script.push_str(&send(serde_json::json!({
            "id":id,"method":"mcpServer/elicitation/request","params":{
                "threadId":"thread-1","mode":"form","serverName":"owned",
                "message":"Owned input","requestedSchema":{"type":"object","properties":{}}
            }
        })));
    }
    script.push_str("while read -r line; do :; done\n");
    fs::write(&executable, script)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    let mut session =
        CodexSession::open(&executable, &directory.0, Thread::New { tools: vec![] }).await?;
    let mut inputs = Vec::new();
    for _ in 0..4 {
        let NativeEvent::Input(input) = session.next_event().await? else {
            panic!("owned input request");
        };
        inputs.push(input);
    }
    let mut receivers = Vec::new();
    for input in &inputs[..2] {
        let (answer, receiver) = mpsc::sync_channel(1);
        owner.codex_input_offer(&run, input.id().clone(), input.prompt().clone(), answer)?;
        receivers.push(receiver);
    }
    let request = owner.codex_input.as_ref().unwrap().public()?["pending"][0]["request"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut stale = owner.record_history.as_ref().unwrap().selection()?;
    stale.epoch += 1;
    let subscription = crate::flow::Subscription {
        scope: stale,
        run: run.clone(),
    };
    owner
        .subscribers
        .insert("stale-input-view".into(), subscription.clone());
    let error = owner
        .codex_input_reply(
            "retry",
            &run,
            &request,
            crate::codex_input::Answer::Accept {
                value: BTreeMap::new(),
            },
        )
        .unwrap_err();
    assert!(!crate::native_input_records::fatal(error.as_ref()));
    assert!(
        receivers
            .iter()
            .all(|r| matches!(r.try_recv(), Err(mpsc::TryRecvError::Empty)))
    );
    owner.subscribers.clear();
    owner.codex_input_reply(
        "reply",
        &run,
        &request,
        crate::codex_input::Answer::Accept {
            value: BTreeMap::new(),
        },
    )?;
    assert!(matches!(receivers[0].try_recv()?, InputAnswer::Accept(value) if value.is_empty()));
    assert!(matches!(
        receivers[1].try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    owner
        .subscribers
        .insert("stale-input-view".into(), subscription);
    let error = owner
        .codex_input_withdraw(&run, inputs[1].id())
        .unwrap_err();
    assert!(!crate::native_input_records::fatal(error.as_ref()));
    assert!(matches!(
        receivers[1].try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    owner.subscribers.clear();
    owner.codex_input_withdraw(&run, inputs[1].id())?;
    assert!(owner.codex_input.is_none());
    assert!(matches!(
        receivers[1].try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));

    for input in &inputs[2..] {
        let (answer, receiver) = mpsc::sync_channel(1);
        owner.codex_input_offer(&run, input.id().clone(), input.prompt().clone(), answer)?;
        receivers.push(receiver);
    }
    owner.codex_input_withdraw(&run, inputs[2].id())?;
    assert!(matches!(
        receivers[2].try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(matches!(
        receivers[3].try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let request = owner.codex_input.as_ref().unwrap().public()?["pending"][0]["request"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = sequence(&owner)?;
    owner.transport.fail_next_emit();
    let error = owner
        .codex_input_reply(
            "failed-publication",
            &run,
            &request,
            crate::codex_input::Answer::Accept {
                value: BTreeMap::new(),
            },
        )
        .unwrap_err();
    assert!(crate::native_input_records::fatal(error.as_ref()));
    assert_eq!(sequence(&owner)?, before + 1);
    assert!(owner.codex_input.is_none());
    assert!(matches!(
        receivers[3].try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    stop_owner(&mut owner);
    let exit = session.close().await?;
    assert!(!exit.forced && exit.code == Some(0));
    Ok(())
}

#[test]
fn native_corrections_fence_duplicates_stop_and_uncertain_session_reuse() -> Result<()> {
    use crate::native_correction::{Phase, Target, channel};
    for (dispatch, restart) in [(true, false), (false, false), (false, true), (true, true)] {
        let (directory, mut owner, _, run) = make_owner_with_binding("pi", true)?;
        let scope = owner.record_history.as_ref().unwrap().selection()?;
        let (slot, receiver) = channel(&run, &owner.tx);
        owner.operations.get_mut(&run).unwrap().correction = Some(slot);
        owner.correction_ready(&run, Target::Pi)?;
        owner.correct_native("correction-1", &scope, &run, "Keep the exact correction.")?;
        assert_eq!(receiver.take()?.unwrap().text, "Keep the exact correction.");
        let admitted = sequence(&owner)?;
        owner.correct_native("correction-1", &scope, &run, "Keep the exact correction.")?;
        assert_eq!(sequence(&owner)?, admitted);
        assert!(receiver.take()?.is_none());
        let admitted_task = owner.record_history.as_mut().unwrap().edit(|edit| {
            edit.current_execution(&scope, &run)
                .and_then(|execution| edit.native_task(&execution, &run))
        })?;
        let super::NativeWorkflow::Pi(workflow) = admitted_task.workflow else {
            panic!("Pi fixture")
        };
        assert!(workflow.binding.resume_path()?.is_some());
        assert!(
            owner
                .correct_native("correction-1", &scope, &run, "Changed")
                .is_err()
        );
        let mut stale = scope.clone();
        stale.epoch += 1;
        assert!(
            owner
                .correct_native("correction-2", &stale, &run, "Stale")
                .is_err()
        );
        if dispatch {
            let (ack, reply) = mpsc::sync_channel(1);
            owner.correction_dispatch(&run, "correction-1", Target::Pi, ack)?;
            assert!(reply.recv()?);
        }
        if restart {
            stop_owner(&mut owner);
            drop(owner.record_history.take());
            let mut reopened =
                History::open(&directory.0.join("history.redb"), owner.disk.lease())?;
            reopened.edit(|edit| edit.recover(&scope))?;
            owner.record_history = Some(reopened);
        } else {
            owner.stop_run("stop-correction", &run)?;
            let (ack, reply) = mpsc::sync_channel(1);
            owner.correction_dispatch(&run, "correction-1", Target::Pi, ack)?;
            assert!(!reply.recv()?);
            owner.agent_settled(&run, crate::agent::Outcome::Cancelled, false, true)?;
        }
        let saved = owner.record_history.as_mut().unwrap().edit(|edit| {
            edit.current_execution(&scope, &run)
                .and_then(|execution| edit.native_task(&execution, &run))
        })?;
        assert_eq!(saved.run.corrections.len(), 1);
        assert_eq!(saved.run.corrections[0].text, "Keep the exact correction.");
        assert!(matches!(
            (&saved.run.corrections[0].state, dispatch),
            (Phase::Uncertain { .. }, true) | (Phase::NotSent { .. }, false)
        ));
        assert!(saved.run.status.terminal());
        let super::NativeWorkflow::Pi(workflow) = saved.workflow else {
            panic!("Pi fixture")
        };
        if dispatch || restart {
            assert!(
                workflow.binding.resume_path().is_err(),
                "uncertain correction must block reuse"
            );
        } else {
            assert!(workflow.binding.resume_path()?.is_some());
        }
        owner.correct_native("correction-1", &scope, &run, "Keep the exact correction.")?;
        assert!(receiver.take().is_err());
    }
    Ok(())
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
    let saved = owner.record_history.as_mut().unwrap().edit(|edit| {
        edit.current_execution(&scope, &run)
            .and_then(|execution| edit.native_task(&execution, &run))
    })?;
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
            .operations
            .get(&run)
            .unwrap()
            .stop
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let stopped = owner.record_history.as_mut().unwrap().edit(|edit| {
        edit.current_execution(&scope, &run)
            .and_then(|execution| edit.native_task(&execution, &run))
    })?;
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
        let mut task = edit
            .current_execution(&scope, &run)
            .and_then(|execution| edit.native_task(&execution, &run))?;
        task.run.status = Status::StopRequested;
        edit.current_execution(&scope, &task.run.id)
            .and_then(|execution| edit.put_native(&execution, task))
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
            .operations
            .get(&run)
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
            .edit(|edit| edit
                .current_execution(&scope, &run)
                .and_then(|execution| edit.native_task(&execution, &run)))?
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
            .operations
            .get(&run)
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
            .edit(|edit| edit
                .current_execution(&scope, &run)
                .and_then(|execution| edit.native_task(&execution, &run)))?
            .run
            .status,
        Status::StopRequested
    );
    stop_owner(&mut duplicate);
    Ok(())
}

fn select_other_fixture(history: &mut History) -> Result<super::Selection> {
    // Seed a different selected conversation without relaxing production admission.
    let write = history.db.begin_write()?;
    let mut profile: Profile = {
        let metadata = write.open_table(META)?;
        decode(metadata.get("profile")?.unwrap().value())?
    };
    let number = profile.next;
    let other = format!("{}:{number}", profile.store);
    profile.next += 1;
    profile.sequence += 1;
    profile.epoch += 1;
    profile.selected = other.clone();
    write.open_table(CONVERSATIONS)?.insert(
        other.as_str(),
        encode(&Conversation {
            id: other.clone(),
            title: "Other conversation".into(),
            source: None,
        })?
        .as_slice(),
    )?;
    write
        .open_table(CONVERSATION_ORDER)?
        .insert(number, other.as_str())?;
    write
        .open_table(META)?
        .insert("profile", encode(&profile)?.as_slice())?;
    write.commit()?;
    history.selection()
}

#[test]
fn offscreen_native_input_keeps_selected_state_and_recovers_its_owner() -> Result<()> {
    let (directory, mut owner, frames, run) = make_owner("codex")?;
    let history = owner.record_history.as_mut().unwrap();
    let original = history.selection()?;
    let execution = history.edit(|edit| edit.current_execution(&original, &run))?;
    assert!(
        history
            .edit(|edit| {
                edit.change_conversation(&original, "still-busy", super::ConversationChange::Create)
            })
            .is_err()
    );

    let selected = select_other_fixture(history)?;
    owner.state = history.edit(|edit| edit.project_latest(&selected))?;
    assert!(
        history
            .edit(|edit| edit.current_execution(&selected, &run))
            .is_err()
    );
    assert!(
        history
            .edit(|edit| edit.run(&execution, &format!("{}:01", execution.store())))
            .is_err()
    );
    assert_eq!(
        history
            .edit(|edit| edit.native_task(&execution, &run))?
            .run
            .id,
        run
    );
    let visible = serde_json::to_vec(&owner.state)?;
    let actions = serde_json::to_vec(&owner.browser_view)?;
    let counter = owner.state.next;
    let mut replies = Vec::new();
    for key in 1..=2 {
        let (ack, reply) = mpsc::sync_channel(1);
        owner.native_permission(
            &run,
            key,
            serde_json::json!({
                "agent":"codex", "tool":"fixture", "title":"Owned permission",
                "input":"fixture", "options":[{"id":"allow","name":"Allow"}]
            }),
            ack,
        )?;
        replies.push(reply);
    }
    assert_eq!(owner.native_permissions.len(), 2);
    assert_eq!(owner.native_input_state(&run)?.next, counter + 2);
    assert_eq!(serde_json::to_vec(&owner.state)?, visible);
    assert_eq!(serde_json::to_vec(&owner.browser_view)?, actions);
    assert!(frames.lock().unwrap().is_empty());
    owner.command("view-other", Command::View)?;
    let mut cursor = std::io::Cursor::new(frames.lock().unwrap().clone());
    let reply: serde_json::Value = serde_json::from_slice(
        &crate::wire::read_raw(&mut cursor, crate::FRAME_MAX)?.ok_or("missing view frame")?,
    )?;
    assert_eq!(reply["view"]["native_permissions"], serde_json::json!([]));
    assert_eq!(
        reply["view"]["selection"]["conversation"],
        selected.conversation
    );
    let request = owner.native_permissions.keys().next().unwrap().clone();
    assert!(
        owner
            .command(
                "wrong-conversation",
                Command::Conversation {
                    scope: crate::history_io::Scope::from(&selected),
                    action: crate::model::ConversationAction::NativePermission {
                        run: run.clone(),
                        request,
                        option: Some("allow".into()),
                    },
                }
            )
            .is_err()
    );
    assert!(
        replies
            .iter()
            .all(|reply| matches!(reply.try_recv(), Err(mpsc::TryRecvError::Empty)))
    );
    let history = owner.record_history.as_mut().unwrap();
    let write = history.db.begin_write()?;
    let conversation = write
        .open_table(CONVERSATIONS)?
        .remove(execution.conversation())?
        .ok_or("missing fixture conversation")?
        .value()
        .to_vec();
    write.commit()?;
    assert!(
        history
            .edit(|edit| edit.refresh_execution(&execution))
            .is_err()
    );
    let write = history.db.begin_write()?;
    write
        .open_table(CONVERSATIONS)?
        .insert(execution.conversation(), conversation.as_slice())?;
    write.commit()?;
    stop_owner(&mut owner);
    drop(owner.record_history.take());
    let mut history = History::open(&directory.0.join("history.redb"), owner.disk.lease())?;
    history.edit(|edit| edit.recover(&selected))?;
    let task = history.edit(|edit| edit.native_task(&execution, &run))?;
    assert_eq!(task.run.status, Status::Interrupted);
    let super::NativeWorkflow::Codex(workflow) = task.workflow else {
        return Err("native recovery changed workflow kind".into());
    };
    assert!(matches!(
        workflow.binding,
        crate::codex_model::Binding::ReleaseUnknown { .. }
    ));
    assert_eq!(history.selection()?, selected);
    let view = history.edit(|edit| edit.project_latest(&selected))?;
    assert!(view.view.runs.is_empty() && view.view.source.is_none());
    assert!(view.view.sequence > owner.state.view.sequence);
    Ok(())
}

#[test]
fn offscreen_pi_disconnect_stops_and_cancels_its_own_input() -> Result<()> {
    let (_directory, mut owner, frames, run) = make_owner("pi")?;
    let history = owner.record_history.as_mut().unwrap();
    let execution = history.edit(|edit| edit.execution(&run))?;
    let selected = select_other_fixture(history)?;
    owner.state = history.edit(|edit| edit.project_latest(&selected))?;
    let visible = serde_json::to_vec(&owner.state)?;
    let pending = pi_offer(&mut owner, &run, 7)?;
    owner.disconnect_pi_ui()?;
    assert!(
        owner
            .operations
            .get(&run)
            .unwrap()
            .stop
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let task = owner
        .record_history
        .as_mut()
        .unwrap()
        .edit(|edit| edit.native_task(&execution, &run))?;
    assert_eq!(task.run.status, Status::StopRequested);
    assert!(matches!(
        pending.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(serde_json::to_vec(&owner.state)?, visible);
    assert!(frames.lock().unwrap().is_empty());
    stop_owner(&mut owner);
    Ok(())
}
