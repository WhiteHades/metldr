use super::{History, NativeWorkflow, schema::*};
use crate::{
    agent_model::{Binding as PiBinding, Launch},
    agent_settings::{Kind, Profile, Receipt, Selection as AgentSelection},
    flow::{Event, Owner},
    model::{Command, Page, Request, Source, Status, Store},
    store::Disk,
    transport::Transport,
};
use redb::{ReadableDatabase, ReadableTable};
use std::{
    collections::BTreeMap,
    fs, io,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> crate::Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "native-start-{}-{}",
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

#[derive(Clone, Copy)]
enum Engine {
    Pi,
    Codex,
    OpenCode,
}
impl Engine {
    fn kind(self) -> Kind {
        match self {
            Self::Pi => Kind::Pi,
            Self::Codex => Kind::Codex,
            Self::OpenCode => Kind::Opencode,
        }
    }
    fn profile(self, dir: &PathBuf, exe: &PathBuf) -> Profile {
        let workspace = dir.join("workspace");
        match self {
            Self::Pi => Profile::Pi {
                executable: exe.clone(),
                workspace,
                session_dir: dir.join("sessions"),
                provider: "owned-provider".into(),
                model: "owned-model".into(),
                offline: true,
            },
            Self::Codex => Profile::Codex {
                executable: exe.clone(),
                workspace,
            },
            Self::OpenCode => Profile::Opencode {
                executable: exe.clone(),
                workspace,
            },
        }
    }
    fn initial(self, source: &str) -> crate::model::ConversationAction {
        match self {
            Self::Pi => crate::model::ConversationAction::PiPrompt {
                source: source.into(),
                text: "first prompt".into(),
            },
            Self::Codex => crate::model::ConversationAction::CodexPrompt {
                source: source.into(),
                text: "first prompt".into(),
                mode: crate::codex_input::TurnMode::Configured,
            },
            Self::OpenCode => crate::model::ConversationAction::OpenCodePrompt {
                source: source.into(),
                text: "first prompt".into(),
            },
        }
    }
    fn followup(self, workflow: &str, revision: u64) -> crate::model::ConversationAction {
        match self {
            Self::Pi => crate::model::ConversationAction::PiFollowUp {
                workflow: workflow.into(),
                revision,
                text: "second prompt".into(),
            },
            Self::Codex => crate::model::ConversationAction::CodexFollowUp {
                workflow: workflow.into(),
                revision,
                text: "second prompt".into(),
                mode: crate::codex_input::TurnMode::Configured,
            },
            Self::OpenCode => crate::model::ConversationAction::OpenCodeFollowUp {
                workflow: workflow.into(),
                revision,
                text: "second prompt".into(),
            },
        }
    }
}

fn scoped(owner: &Owner, action: crate::model::ConversationAction) -> Command {
    Command::Conversation {
        scope: crate::history_io::Scope::from(&crate::tests::owner_scope(owner)),
        action,
    }
}

fn setup(
    engine: Engine,
    directory: &Directory,
) -> crate::Result<(
    Owner,
    mpsc::Receiver<Event>,
    Arc<std::sync::Mutex<Vec<u8>>>,
    Vec<u8>,
)> {
    let workspace = directory.0.join("workspace");
    let sessions = directory.0.join("sessions");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&sessions)?;
    let executable = fs::canonicalize("/usr/bin/true")?;
    let (disk, mut state) = Disk::open(directory.0.clone())?;
    let source = state.id()?;
    state.view.source = Some(Source {
        id: source.clone(),
        page: Page {
            tab: 17,
            document: "owned-document".into(),
            url: "https://owned.invalid/article".into(),
            title: "Owned page".into(),
            text: "Saved source text".into(),
        },
    });
    let slot = state.agent_settings.slots.get_mut(&engine.kind()).unwrap();
    slot.revision = 1;
    slot.selection = AgentSelection::Configured {
        profile: engine.profile(&directory.0, &executable),
    };
    slot.last_change = Some(Receipt {
        request: "owned-profile".into(),
        expected_revision: 0,
        digest: "a".repeat(64),
    });
    disk.save(&state)?;
    let original = fs::read(directory.0.join("snapshot.json"))?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &original, disk.lease())?;
    let selection = history.selection()?;
    let state = history.edit(|edit| edit.project(&selection, None))?;
    let (transport, frames) = Transport::captured();
    let (tx, rx) = mpsc::sync_channel::<Event>(16);
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
    };
    Ok((owner, rx, frames, original))
}

fn stop(owner: &mut Owner) {
    if let Some(operation) = owner.operation.take() {
        operation
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        operation.worker.join().expect("owned native worker");
        owner.browser_io_finish(&operation.run);
    }
}

fn reopen(mut owner: Owner) -> crate::Result<Owner> {
    assert!(owner.operation.is_none());
    drop(owner.record_history.take());
    let mut history = History::open(
        &owner.disk.directory.join("history.redb"),
        owner.disk.lease(),
    )?;
    let scope = history.selection()?;
    let state = history.edit(|edit| edit.project(&scope, None))?;
    Ok(Owner {
        state,
        record_history: Some(history),
        history_pins: Default::default(),
        browser_view: BTreeMap::new(),
        agent_runtime: Default::default(),
        pi_profile: None,
        codex_profile: None,
        opencode_profile: None,
        ..owner
    })
}

fn task(owner: &mut Owner, run: &str) -> crate::Result<super::edit::NativeTask> {
    let history = owner.record_history.as_mut().unwrap();
    let selection = history.selection()?;
    history.edit(|edit| edit.native_task(&selection, run))
}

fn saved_start(history: &History, request: &str) -> crate::Result<Start> {
    let read = history.db.begin_read()?;
    let table = read.open_table(STARTS)?;
    let value = table
        .get(request)?
        .ok_or("native start receipt is absent")?;
    decode(value.value())
}

fn captured_frames(
    bytes: &Arc<std::sync::Mutex<Vec<u8>>>,
) -> crate::Result<Vec<serde_json::Value>> {
    let bytes = bytes.lock().unwrap();
    let mut cursor = io::Cursor::new(bytes.as_slice());
    let mut result = Vec::new();
    while let Some(frame) = crate::wire::read_raw(&mut cursor, crate::FRAME_MAX)? {
        result.push(serde_json::from_slice(&frame)?);
    }
    Ok(result)
}

fn seed_bound_and_terminal(owner: &mut Owner, run: &str, dir: &PathBuf) -> crate::Result<String> {
    let mut task = task(owner, run)?;
    let workflow_id = match &task.launch {
        Launch::Pi { workflow, .. }
        | Launch::Codex { workflow, .. }
        | Launch::OpenCode { workflow, .. } => workflow.clone(),
        _ => unreachable!(),
    };
    task.run.status = Status::Complete;
    match &mut task.workflow {
        NativeWorkflow::Pi(w) => {
            let path = dir.join("sessions").join("session.jsonl");
            fs::write(&path, b"owned session\n")?;
            let path = fs::canonicalize(path)?;
            w.binding = PiBinding::Bound {
                id: "owned-session".into(),
                path,
            };
            w.revision += 1;
        }
        NativeWorkflow::Codex(w) => {
            w.binding = crate::codex_model::Binding::Bound {
                identity: crate::codex_model::Identity {
                    thread: "thread-fixture-1".into(),
                    session: "owned-session".into(),
                },
            };
            w.revision += 1;
        }
        NativeWorkflow::OpenCode(w) => {
            w.binding = crate::opencode_model::Binding::Bound {
                identity: crate::opencode_model::Identity {
                    session: "session_fixture_1".into(),
                    selection: crate::opencode_model::Selection {
                        model: "owned-model".into(),
                        mode: "build".into(),
                    },
                },
            };
            w.revision += 1;
        }
    }
    match &mut task.launch {
        Launch::Codex { selection, .. } => {
            *selection = Some(crate::codex_model::Selection {
                provider: "owned-provider".into(),
                model: "owned-model".into(),
            })
        }
        Launch::OpenCode { selection, .. } => {
            *selection = Some(crate::opencode_model::Selection {
                model: "owned-model".into(),
                mode: "build".into(),
            })
        }
        _ => (),
    }
    let history = owner.record_history.as_mut().unwrap();
    let selection = history.selection()?;
    history.edit(|edit| edit.put_native(&selection, task))?;
    Ok(workflow_id)
}

fn actual_owner_native_starts_are_record_durable_and_snapshot_compatible() -> crate::Result<()> {
    for engine in [Engine::Pi, Engine::Codex, Engine::OpenCode] {
        let directory = Directory::new()?;
        let (mut owner, _rx, frames, original) = setup(engine, &directory)?;
        let source = owner.state.view.source.as_ref().unwrap().id.clone();
        let request = match engine {
            Engine::Pi => "first-pi",
            Engine::Codex => "first-codex",
            Engine::OpenCode => "first-opencode",
        }
        .to_owned();
        let command = engine.initial(&source);
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: request.clone(),
            command: scoped(&owner, engine.initial(&source)),
        })?;
        let run = owner.state.view.runs.last().unwrap().id.clone();
        let prepared_receipt = saved_start(owner.record_history.as_ref().unwrap(), &request)?;
        assert_eq!(
            owner.operation.as_ref().map(|op| op.run.as_str()),
            Some(run.as_str())
        );
        let first = task(&mut owner, &run)?;
        assert_eq!(first.run.status, Status::Running);
        assert!(matches!(
            (&first.launch, engine),
            (Launch::Pi { .. }, Engine::Pi)
                | (
                    Launch::Codex {
                        selection: None,
                        ..
                    },
                    Engine::Codex
                )
                | (
                    Launch::OpenCode {
                        selection: None,
                        ..
                    },
                    Engine::OpenCode
                )
        ));
        let revision = match &first.workflow {
            NativeWorkflow::Pi(w) => w.revision,
            NativeWorkflow::Codex(w) => w.revision,
            NativeWorkflow::OpenCode(w) => w.revision,
        };
        assert_eq!(revision, 2);
        assert_eq!(first.run.source, source);
        let history = owner.record_history.as_mut().unwrap();
        let selection = history.selection()?;
        let initial_target = history.edit(|edit| edit.native_initial_target(&selection, &run))?;
        assert_eq!(
            initial_target,
            crate::browser_action::Target::from(&owner.state.view.source.as_ref().unwrap().page)
        );
        assert_eq!(
            owner.agent_runtime.states.get(&engine.kind()),
            Some(&"ready")
        );
        assert_eq!(fs::read(directory.0.join("snapshot.json"))?, original);
        assert!(
            captured_frames(&frames)?
                .iter()
                .any(|frame| frame["kind"] == "started" && frame["run"] == run)
        );
        stop(&mut owner);

        let workflow = seed_bound_and_terminal(&mut owner, &run, &directory.0)?;
        let follow = engine.followup(&workflow, 3);
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "follow-up".into(),
            command: scoped(&owner, follow),
        })?;
        let next = owner.state.view.runs.last().unwrap().id.clone();
        let next_task = task(&mut owner, &next)?;
        let next_revision = match &next_task.workflow {
            NativeWorkflow::Pi(w) => w.revision,
            NativeWorkflow::Codex(w) => w.revision,
            NativeWorkflow::OpenCode(w) => w.revision,
        };
        assert_eq!(next_revision, 4);
        assert_eq!(next_task.run.source, source);
        let history = owner.record_history.as_mut().unwrap();
        let selected = history.selection()?;
        assert_eq!(
            history.edit(|edit| edit.native_initial_target(&selected, &next))?,
            initial_target
        );
        stop(&mut owner);
        seed_bound_and_terminal(&mut owner, &next, &directory.0)?;

        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "new-capture".into(),
            command: Command::Conversation {
                scope: crate::history_io::Scope::from(
                    &owner.record_history.as_ref().unwrap().selection()?,
                ),
                action: crate::model::ConversationAction::Capture {
                    expected_source: Some(source.clone()),
                    page: Page {
                        tab: 18,
                        document: "new-document".into(),
                        url: "https://owned.invalid/new".into(),
                        title: "New page".into(),
                        text: "New current source".into(),
                    },
                },
            },
        })?;
        assert_ne!(owner.state.view.source.as_ref().unwrap().id, source);

        owner = reopen(owner)?;
        let saved = saved_start(owner.record_history.as_ref().unwrap(), &request)?;
        assert_eq!(saved.run, run);
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: request.clone(),
            command: scoped(&owner, command),
        })?;
        assert!(owner.operation.is_none());
        assert!(
            captured_frames(&frames)?
                .iter()
                .any(|frame| frame["kind"] == "started" && frame["run"] == run)
        );
        let mut changed = engine.initial(&source);
        match &mut changed {
            crate::model::ConversationAction::PiPrompt { text, .. }
            | crate::model::ConversationAction::CodexPrompt { text, .. }
            | crate::model::ConversationAction::OpenCodePrompt { text, .. } => {
                *text = "changed request".into()
            }
            _ => unreachable!(),
        }
        let count = owner.state.view.runs.len();
        frames.lock().unwrap().clear();
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: request.clone(),
            command: scoped(&owner, changed),
        })?;
        assert_eq!(owner.state.view.runs.len(), count);
        assert!(owner.operation.is_none());
        assert!(
            captured_frames(&frames)?
                .iter()
                .all(|frame| frame["kind"] != "started")
        );
        assert!(
            captured_frames(&frames)?
                .iter()
                .any(|frame| frame["kind"] == "error")
        );

        let original_selection = owner.record_history.as_ref().unwrap().selection()?;
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "remove-global-profile".into(),
            command: Command::AgentRemove {
                agent: engine.kind(),
                expected_revision: 1,
            },
        })?;
        assert_eq!(
            owner.state.agent_settings.slots[&engine.kind()].selection,
            AgentSelection::Removed
        );
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "switch-conversation".into(),
            command: Command::ConversationChange {
                scope: crate::history_io::Scope::from(&original_selection),
                change: crate::history::ConversationChange::Create,
            },
        })?;
        assert_ne!(
            owner
                .record_history
                .as_ref()
                .unwrap()
                .selection()?
                .conversation,
            original_selection.conversation
        );

        frames.lock().unwrap().clear();
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: request.clone(),
            command: Command::Conversation {
                scope: crate::history_io::Scope::from(&original_selection),
                action: engine.initial(&source),
            },
        })?;
        let replay = saved_start(owner.record_history.as_ref().unwrap(), &request)?;
        assert_eq!(replay.run, prepared_receipt.run);
        assert_eq!(replay.conversation, prepared_receipt.conversation);
        assert_eq!(replay.format, prepared_receipt.format);
        assert!(owner.operation.is_none());
        assert!(captured_frames(&frames)?.iter().any(|frame| {
            frame["kind"] == "started"
                && frame["run"] == run
                && frame["scope"]
                    == serde_json::to_value(crate::history_io::Scope::from(&original_selection))
                        .unwrap()
        }));
        assert!(
            captured_frames(&frames)?
                .iter()
                .all(|frame| frame["kind"] != "error")
        );

        let count = owner.state.view.runs.len();
        frames.lock().unwrap().clear();
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "new-request-under-old-scope".into(),
            command: Command::Conversation {
                scope: crate::history_io::Scope::from(&original_selection),
                action: engine.initial(&source),
            },
        })?;
        assert_eq!(owner.state.view.runs.len(), count);
        assert!(owner.operation.is_none());
        assert!(
            captured_frames(&frames)?
                .iter()
                .all(|frame| frame["kind"] != "started")
        );
        assert!(
            captured_frames(&frames)?
                .iter()
                .any(|frame| frame["kind"] == "error")
        );
    }
    Ok(())
}

fn native_start_publication_failure_and_corrupt_followup_never_dispatch() -> crate::Result<()> {
    for engine in [Engine::Codex, Engine::OpenCode] {
        let directory = Directory::new()?;
        let (mut owner, _rx, frames, _) = setup(engine, &directory)?;
        let source = owner.state.view.source.as_ref().unwrap().id.clone();
        owner.prepare_agent(engine.kind())?;
        frames.lock().unwrap().clear();
        owner.transport.fail_next_emit();
        let command = engine.initial(&source);
        assert!(
            owner
                .request(Request {
                    v: crate::WIRE_VERSION,
                    request: "publish-fails".into(),
                    command: scoped(&owner, command)
                })
                .is_err()
        );
        assert!(owner.operation.is_none());
        assert!(frames.lock().unwrap().is_empty());
        let history = owner.record_history.as_mut().unwrap();
        let selection = history.selection()?;
        let run = saved_start(history, "publish-fails")?.run;
        assert_eq!(
            history.edit(|edit| Ok(edit.native_task(&selection, &run)?.run.status))?,
            Status::Running
        );
        owner = reopen(owner)?;
        let command = engine.initial(&source);
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: "publish-fails".into(),
            command: scoped(&owner, command),
        })?;
        assert!(owner.operation.is_none());

        let bound_dir = Directory::new()?;
        let (mut corrupt, _rx, corrupt_frames, _) = setup(engine, &bound_dir)?;
        let source = corrupt.state.view.source.as_ref().unwrap().id.clone();
        let command = engine.initial(&source);
        corrupt.request(Request {
            v: crate::WIRE_VERSION,
            request: "before-corrupt".into(),
            command: scoped(&corrupt, command),
        })?;
        let run = corrupt.state.view.runs.last().unwrap().id.clone();
        stop(&mut corrupt);
        let workflow = seed_bound_and_terminal(&mut corrupt, &run, &bound_dir.0)?;
        let history = corrupt.record_history.as_ref().unwrap();
        let number = run.rsplit(':').next().unwrap().parse::<u64>()?;
        let history = corrupt.record_history.as_ref().unwrap();
        let write = history.db.begin_write()?;
        assert!(
            write
                .open_table(WORKFLOW_RUNS)?
                .remove((workflow.as_str(), number))?
                .is_some()
        );
        write.commit()?;
        let count = corrupt.state.view.runs.len();
        corrupt_frames.lock().unwrap().clear();
        assert!(
            corrupt
                .request(Request {
                    v: crate::WIRE_VERSION,
                    request: "corrupt-followup".into(),
                    command: scoped(&corrupt, engine.followup(&workflow, 3))
                })
                .is_err()
        );
        assert!(corrupt.operation.is_none());
        assert!(corrupt_frames.lock().unwrap().is_empty());
        assert_eq!(corrupt.state.view.runs.len(), count);
        let read = corrupt.record_history.as_ref().unwrap().db.begin_read()?;
        assert!(read.open_table(STARTS)?.get("corrupt-followup")?.is_none());
    }
    Ok(())
}

fn native_start_keeps_the_snapshot_execution_path_working() -> crate::Result<()> {
    for engine in [Engine::Pi, Engine::Codex, Engine::OpenCode] {
        let directory = Directory::new()?;
        let (mut owner, _rx, _, _) = setup(engine, &directory)?;
        drop(owner.record_history.take());
        owner.disk.save(&owner.state)?;
        let source = owner.state.view.source.as_ref().unwrap().id.clone();
        let scope = crate::tests::owner_scope(&owner);
        let action = engine.initial(&source);
        match engine {
            Engine::Pi => owner.pi_start("snapshot-start", &scope, action)?,
            Engine::Codex => owner.codex_start("snapshot-start", &scope, action)?,
            Engine::OpenCode => owner.opencode_start("snapshot-start", &scope, action)?,
        }
        assert!(owner.operation.is_some());
        let run = owner.operation.as_ref().unwrap().run.clone();
        assert_eq!(
            owner
                .state
                .launches
                .get(&run)
                .unwrap()
                .input_digest()
                .is_some(),
            true
        );
        let saved: Store = serde_json::from_slice(&fs::read(directory.0.join("snapshot.json"))?)?;
        saved.validate()?;
        assert!(saved.launches.contains_key(&run));
        stop(&mut owner);
    }
    Ok(())
}

#[test]
fn actual_owner_native_start_admission_and_failure_boundaries() -> crate::Result<()> {
    for engine in [Engine::Pi, Engine::Codex, Engine::OpenCode] {
        let directory = Directory::new()?;
        let (mut owner, _rx, _, _) = setup(engine, &directory)?;
        let scope = crate::tests::owner_scope(&owner);
        let source = owner.state.view.source.as_ref().unwrap().id.clone();
        owner.record_change(|edit, _| {
            let run = edit.id()?;
            edit.admit(
                &scope,
                "different-start-family",
                crate::model::Run {
                    id: run.clone(),
                    source: source.clone(),
                    kind: "answer".into(),
                    question: Some("Original fixture request".into()),
                    status: Status::Running,
                    output: Default::default(),
                    error: None,
                    corrections: Vec::new(),
                },
                Launch::Fixture {
                    input_digest: "f".repeat(64),
                },
            )?;
            let mut record = edit.run(&scope, &run)?;
            record.status = Status::Complete;
            edit.put_run(&scope, record)?;
            Ok(((), Some(edit.project(&scope, Some(&run))?)))
        })?;
        let error = owner
            .command(
                "different-start-family",
                scoped(&owner, engine.initial(&source)),
            )
            .unwrap_err();
        assert!(
            !super::is_failure(error.as_ref()),
            "request reuse must be a refusal, not a storage failure: {error}"
        );
        assert!(owner.operation.is_none());
    }
    actual_owner_native_starts_are_record_durable_and_snapshot_compatible()?;
    native_start_publication_failure_and_corrupt_followup_never_dispatch()?;
    native_start_keeps_the_snapshot_execution_path_working()
}
