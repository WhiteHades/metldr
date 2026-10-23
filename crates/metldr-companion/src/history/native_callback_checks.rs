use super::{History, NativeWorkflow, fixture, schema::*};
use crate::{
    Result,
    agent_model::{Activity, Binding as PiBinding, Launch},
    flow::{Event, Operation, Owner},
    model::{Run, Status},
    store::Disk,
    transport::Transport,
};
use metldr_conversation::Content;
use redb::{ReadableDatabase, ReadableTableMetadata};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "native-callback-{}-{}",
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

fn owner(
    disk: Disk,
    state: crate::model::Store,
    history: History,
) -> (Owner, Arc<std::sync::Mutex<Vec<u8>>>) {
    let (transport, frames) = Transport::captured();
    let (tx, _) = mpsc::sync_channel::<Event>(8);
    let worker = thread::spawn(|| {});
    let run = state.view.runs.last().unwrap().id.clone();
    (
        Owner {
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
                run,
                stop: Arc::new(AtomicBool::new(false)),
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
        },
        frames,
    )
}

fn activity(id: &str) -> Activity {
    Activity {
        tool: "browser.search".into(),
        phase: "started".into(),
        outcome: None,
        call: Some(crate::tool_report::Call::new(id).unwrap()),
    }
}

#[test]
fn actual_owner_native_callbacks_bind_persist_and_fail_closed() -> Result<()> {
    for (engine, fault) in [
        ("pi", "none"),
        ("codex", "none"),
        ("codex", "emit"),
        ("opencode", "none"),
        ("opencode", "corrupt"),
        ("pi", "snapshot"),
        ("codex", "snapshot"),
        ("opencode", "snapshot"),
    ] {
        let directory = Directory::new()?;
        let disk = Disk::lock(directory.0.clone())?;
        let mut legacy = fixture::legacy();
        crate::recovery::interrupt(&mut legacy)?;
        legacy.validate()?;
        disk.save(&legacy)?;
        let path = directory.0.join("history.redb");
        let mut history = History::import_new(&path, &serde_json::to_vec(&legacy)?, disk.lease())?;
        let selection = history.selection()?;
        let source = history.edit(|edit| {
            edit.selected_source(&selection)?
                .ok_or("missing source".into())
        })?;
        let workflow = match engine {
            "pi" => NativeWorkflow::Pi(
                legacy
                    .workflows
                    .remove("0123456789abcdef0123456789abcdef:030")
                    .unwrap(),
            ),
            "codex" => NativeWorkflow::Codex(
                legacy
                    .codex_workflows
                    .remove("0123456789abcdef0123456789abcdef:031")
                    .unwrap(),
            ),
            _ => NativeWorkflow::OpenCode(
                legacy
                    .opencode_workflows
                    .remove("0123456789abcdef0123456789abcdef:032")
                    .unwrap(),
            ),
        };
        let mut workflow = match workflow {
            NativeWorkflow::Pi(mut value) => {
                value.source = source.clone();
                value.last_run.clear();
                value.revision = 2;
                value.binding = PiBinding::Pending;
                NativeWorkflow::Pi(value)
            }
            NativeWorkflow::Codex(mut value) => {
                value.source = source.clone();
                value.last_run.clear();
                value.revision = 2;
                value.binding = crate::codex_model::Binding::Pending;
                NativeWorkflow::Codex(value)
            }
            NativeWorkflow::OpenCode(mut value) => {
                value.source = source.clone();
                value.last_run.clear();
                value.revision = 2;
                value.binding = crate::opencode_model::Binding::Pending;
                NativeWorkflow::OpenCode(value)
            }
        };
        let (workflow_id, run) = history.edit(|edit| {
            let workflow_id = edit.id()?;
            let run = edit.id()?;
            match &mut workflow {
                NativeWorkflow::Pi(w) => w.last_run = run.clone(),
                NativeWorkflow::Codex(w) => w.last_run = run.clone(),
                NativeWorkflow::OpenCode(w) => w.last_run = run.clone(),
            }
            let launch = match &workflow {
                NativeWorkflow::Pi(_) => Launch::Pi {
                    input_digest: "a".repeat(64),
                    workflow: workflow_id.clone(),
                    activity: vec![],
                },
                NativeWorkflow::Codex(_) => Launch::Codex {
                    input_digest: "b".repeat(64),
                    workflow: workflow_id.clone(),
                    selection: None,
                    activity: vec![],
                },
                NativeWorkflow::OpenCode(_) => Launch::OpenCode {
                    input_digest: "c".repeat(64),
                    workflow: workflow_id.clone(),
                    selection: None,
                    activity: vec![],
                },
            };
            edit.admit_native(
                &selection,
                &format!("callback-{engine}"),
                Run {
                    id: run.clone(),
                    source: source.id.clone(),
                    kind: "agent".into(),
                    question: Some("native callback check".into()),
                    status: Status::Running,
                    output: Content::native(),
                    error: None,
                    corrections: Vec::new(),
                },
                launch,
                workflow.clone(),
            )?;
            Ok((workflow_id, run))
        })?;
        let state = history.edit(|edit| edit.project(&selection, Some(&run)))?;
        let snapshot = fs::read(directory.0.join("snapshot.json"))?;
        let (mut owner, frames) = owner(disk, state, history);
        let active = owner.operation.as_ref().unwrap().run.clone();
        assert_eq!(active, run);
        let ack = |owner: &mut Owner| {
            let (tx, rx) = mpsc::sync_channel(1);
            let result = match engine {
                "pi" => owner.pi_bound(
                    &run,
                    &workflow_id,
                    PiBinding::Bound {
                        id: "pi-session-1".into(),
                        path: "/owned/sessions/pi-session-1.jsonl".into(),
                    },
                    tx,
                ),
                "codex" => owner.codex_bound(
                    &run,
                    &workflow_id,
                    crate::codex_model::Identity {
                        thread: "thread-fixture-1".into(),
                        session: "codex-session-fixture".into(),
                    },
                    crate::codex_model::Selection {
                        provider: "fixture-provider".into(),
                        model: "fixture-model".into(),
                    },
                    tx,
                ),
                _ => owner.opencode_bound(
                    &run,
                    &workflow_id,
                    crate::opencode_model::Identity {
                        session: "session_fixture_1".into(),
                        selection: crate::opencode_model::Selection {
                            model: "fixture-model".into(),
                            mode: "build".into(),
                        },
                    },
                    tx,
                ),
            };
            (
                result,
                rx.recv_timeout(Duration::from_secs(2))
                    .expect("binding acknowledgement"),
            )
        };
        if fault == "snapshot" {
            let _archive = owner.record_history.take();
            owner.disk.save(&owner.state)?;
            let (result, reply) = ack(&mut owner);
            result?;
            assert_eq!(reply?, true);
            match engine {
                "pi" => owner.pi_activity(&run, activity("snapshot-tool"))?,
                "codex" => owner.codex_activity(&run, activity("snapshot-tool"))?,
                _ => owner.opencode_activity(&run, activity("snapshot-tool"))?,
            }
            let saved: crate::model::Store =
                serde_json::from_slice(&fs::read(directory.0.join("snapshot.json"))?)?;
            saved.validate()?;
            assert!(serde_json::to_string(&saved.view.runs[0].output)?.contains("snapshot-tool"));
            assert_ne!(fs::read(directory.0.join("snapshot.json"))?, snapshot);
            assert!(!frames.lock().unwrap().is_empty());
            owner
                .operation
                .take()
                .unwrap()
                .worker
                .join()
                .expect("owned callback worker");
            continue;
        }
        if fault == "emit" {
            owner.transport.fail_next_emit();
            let (result, ack) = ack(&mut owner);
            assert!(result.is_err());
            assert!(ack.is_err());
            assert!(frames.lock().unwrap().is_empty());
            let saved = owner
                .record_history
                .as_mut()
                .unwrap()
                .edit(|edit| edit.native_task(&selection, &run))?;
            assert!(matches!(
                saved.workflow,
                NativeWorkflow::Codex(crate::codex_model::Workflow {
                    binding: crate::codex_model::Binding::Bound { .. },
                    revision: 3,
                    ..
                })
            ));
            assert_eq!(fs::read(directory.0.join("snapshot.json"))?, snapshot);
            owner
                .operation
                .take()
                .unwrap()
                .worker
                .join()
                .expect("owned callback worker");
            drop(owner.record_history.take());
            let mut reopened = History::open(&path, owner.disk.lease())?;
            let durable = reopened.edit(|edit| edit.native_task(&selection, &run))?;
            assert!(matches!(
                durable.launch,
                Launch::Codex {
                    selection: Some(_),
                    ..
                }
            ));
            continue;
        }
        if fault == "corrupt" {
            let number = run.rsplit(':').next().unwrap().parse::<u64>()?;
            let write = owner.record_history.as_ref().unwrap().db.begin_write()?;
            write
                .open_table(RUN_ORDER)?
                .remove((selection.conversation.as_str(), number))?;
            write.commit()?;
            let before = serde_json::to_value(&owner.state)?;
            let (result, ack) = ack(&mut owner);
            assert!(result.is_err());
            assert!(ack.is_err());
            assert!(crate::history::is_failure(result.unwrap_err().as_ref()));
            assert_eq!(serde_json::to_value(&owner.state)?, before);
            assert!(frames.lock().unwrap().is_empty());
            assert_eq!(fs::read(directory.0.join("snapshot.json"))?, snapshot);
            owner
                .operation
                .take()
                .unwrap()
                .worker
                .join()
                .expect("owned callback worker");
            continue;
        }
        if fault != "emit" {
            let (result, ack) = ack(&mut owner);
            result?;
            assert_eq!(ack?, true);
        }
        let event = activity("callback-tool-1");
        match engine {
            "pi" => owner.pi_activity(&run, event)?,
            "codex" => owner.codex_activity(&run, event)?,
            _ => owner.opencode_activity(&run, event)?,
        }
        let mut progress = crate::tool_report::Call::new("callback-tool-1")?;
        progress.title = Some(crate::tool_report::Preview::text(
            "Preparing browser search",
            256,
        ));
        progress.output = Some(crate::tool_report::Preview::text(
            "Search is pending",
            2_048,
        ));
        owner.tool_progress(&run, progress.clone())?;
        let published = frames.lock().unwrap().len();
        owner.tool_progress(&run, progress.clone())?;
        owner.tool_progress("stale-run", progress.clone())?;
        assert_eq!(frames.lock().unwrap().len(), published);
        let saved = owner
            .record_history
            .as_mut()
            .unwrap()
            .edit(|edit| edit.native_task(&selection, &run))?;
        assert!(serde_json::to_string(&saved.run.output)?.contains("callback-tool-1"));
        let activities = match &saved.launch {
            Launch::Pi { activity, .. }
            | Launch::Codex { activity, .. }
            | Launch::OpenCode { activity, .. } => activity,
            _ => unreachable!(),
        };
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].call.as_ref().unwrap().id, "callback-tool-1");
        assert_eq!(activities[0].call.as_ref(), Some(&progress));
        for invalid in ["tool", "phase", "identity", "clear"] {
            let result = owner.record_history.as_mut().unwrap().edit(|edit| {
                let mut task = edit.native_task(&selection, &run)?;
                let events = match &mut task.launch {
                    Launch::Pi { activity, .. }
                    | Launch::Codex { activity, .. }
                    | Launch::OpenCode { activity, .. } => activity,
                    _ => unreachable!(),
                };
                match invalid {
                    "tool" => events[0].tool = "different-tool".into(),
                    "phase" => events[0].phase = "completed".into(),
                    "identity" => events[0].call.as_mut().unwrap().id = "different-call".into(),
                    _ => events[0].call.as_mut().unwrap().title = None,
                }
                edit.put_native(&selection, task)
            });
            assert!(
                result.is_err(),
                "accepted {engine} activity rewrite: {invalid}"
            );
        }
        let completed = Activity {
            phase: "completed".into(),
            call: Some(progress.clone()),
            ..activity("callback-tool-1")
        };
        match engine {
            "pi" => owner.pi_activity(&run, completed)?,
            "codex" => owner.codex_activity(&run, completed)?,
            _ => owner.opencode_activity(&run, completed)?,
        }
        let count = frames.lock().unwrap().len();
        let mut late = progress.clone();
        late.title = Some(crate::tool_report::Preview::text(
            "After tool completion",
            256,
        ));
        owner.tool_progress(&run, late)?;
        assert_eq!(frames.lock().unwrap().len(), count);
        owner.record_history.as_mut().unwrap().edit(|edit| {
            let mut record = edit.run(&selection, &run)?;
            record.status = Status::StopRequested;
            edit.put_run(&selection, record)
        })?;
        match engine {
            "pi" => owner.pi_activity(&run, activity("ignored-after-stop"))?,
            "codex" => owner.codex_activity(&run, activity("ignored-after-stop"))?,
            _ => owner.opencode_activity(&run, activity("ignored-after-stop"))?,
        }
        progress.title = Some(crate::tool_report::Preview::text("Late update", 256));
        owner.tool_progress(&run, progress)?;
        let (result, stopped_ack) = ack(&mut owner);
        result?;
        assert_eq!(stopped_ack?, false);
        assert_eq!(frames.lock().unwrap().len(), count);
        drop(owner.record_history.take());
        let mut reopened = History::open(&path, owner.disk.lease())?;
        let reopened_task = reopened.edit(|edit| edit.native_task(&selection, &run))?;
        let revision = match &reopened_task.workflow {
            NativeWorkflow::Pi(w) => w.revision,
            NativeWorkflow::Codex(w) => w.revision,
            NativeWorkflow::OpenCode(w) => w.revision,
        };
        assert_eq!(revision, 3);
        match (&reopened_task.workflow, &reopened_task.launch) {
            (NativeWorkflow::Pi(w), Launch::Pi { .. }) => {
                assert!(matches!(&w.binding, PiBinding::Bound { id, .. } if id == "pi-session-1"))
            }
            (
                NativeWorkflow::Codex(w),
                Launch::Codex {
                    selection: Some(s), ..
                },
            ) => {
                assert_eq!(s.model, "fixture-model");
                assert!(
                    matches!(&w.binding, crate::codex_model::Binding::Bound { identity } if identity.thread == "thread-fixture-1")
                );
            }
            (
                NativeWorkflow::OpenCode(w),
                Launch::OpenCode {
                    selection: Some(s), ..
                },
            ) => {
                assert_eq!(s.model, "fixture-model");
                assert!(
                    matches!(&w.binding, crate::opencode_model::Binding::Bound { identity } if identity.session == "session_fixture_1")
                );
            }
            _ => return Err("native callback changed engine identity".into()),
        }
        assert_eq!(fs::read(directory.0.join("snapshot.json"))?, snapshot);
        assert!(!frames.lock().unwrap().is_empty());
        if fault == "none" {
            let (stale, stale_ack) = {
                let (tx, rx) = mpsc::sync_channel(1);
                let result = match engine {
                    "pi" => owner.pi_bound(
                        "stale-run",
                        &workflow_id,
                        PiBinding::Bound {
                            id: "pi-session-1".into(),
                            path: "/owned/sessions/pi-session-1.jsonl".into(),
                        },
                        tx,
                    ),
                    "codex" => owner.codex_bound(
                        "stale-run",
                        &workflow_id,
                        crate::codex_model::Identity {
                            thread: "thread-fixture-1".into(),
                            session: "codex-session-fixture".into(),
                        },
                        crate::codex_model::Selection {
                            provider: "fixture-provider".into(),
                            model: "fixture-model".into(),
                        },
                        tx,
                    ),
                    _ => owner.opencode_bound(
                        "stale-run",
                        &workflow_id,
                        crate::opencode_model::Identity {
                            session: "session_fixture_1".into(),
                            selection: crate::opencode_model::Selection {
                                model: "fixture-model".into(),
                                mode: "build".into(),
                            },
                        },
                        tx,
                    ),
                };
                (
                    result,
                    rx.recv_timeout(Duration::from_secs(2))
                        .expect("binding acknowledgement"),
                )
            };
            stale?;
            assert!(stale_ack.is_err());
        }
        owner
            .operation
            .take()
            .unwrap()
            .worker
            .join()
            .expect("owned callback worker");
    }
    Ok(())
}
