use super::{History, schema::*};
use crate::{
    Result,
    flow::{Event, Owner},
    model::{Command, Page, Request, Run, Source, Status, Store},
    store::Disk,
    transport::Transport,
};
use redb::{ReadableDatabase, ReadableTableMetadata};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "owner-records-{}-{}",
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

fn profile(history: &History) -> Result<Profile> {
    let read = history.db.begin_read()?;
    let table = read.open_table(META)?;
    decode(table.get("profile")?.unwrap().value())
}

fn owner(
    disk: Disk,
    state: Store,
    history: History,
    address: SocketAddr,
) -> Result<(Owner, mpsc::Receiver<Event>, Arc<Mutex<Vec<u8>>>)> {
    let (transport, frames) = Transport::captured();
    let (tx, rx) = mpsc::sync_channel(64);
    Ok((
        Owner {
            transport,
            disk,
            state,
            browser_view: BTreeMap::new(),
            record_history: Some(history),
            history_pins: Default::default(),
            settings: Default::default(),
            agent_runtime: Default::default(),
            fixture: Some(crate::native::Fixture {
                origin: format!("http://{address}"),
                address,
            }),
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
        },
        rx,
        frames,
    ))
}

fn setup(
    directory: &Directory,
    address: SocketAddr,
) -> Result<(Owner, mpsc::Receiver<Event>, Arc<Mutex<Vec<u8>>>)> {
    let (disk, mut state) = Disk::open(directory.0.clone())?;
    state.view.source = Some(Source {
        id: state.id()?,
        page: Page {
            tab: 1,
            document: "owned-source".into(),
            url: format!("http://{address}/article"),
            title: "Owned page".into(),
            text: "An independent source for the saved answer.".into(),
        },
    });
    disk.save(&state)?;
    let mut history = History::import_new(
        &directory.0.join("history.redb"),
        &serde_json::to_vec(&state)?,
        disk.lease(),
    )?;
    let selection = history.selection()?;
    let state = history.edit(|edit| edit.project(&selection, None))?;
    owner(disk, state, history, address)
}

fn ask(owner: &Owner, id: &str) -> Request {
    Request {
        v: crate::WIRE_VERSION,
        request: id.into(),
        command: conversation(
            owner,
            crate::model::ConversationAction::Ask {
                source: owner.state.view.source.as_ref().unwrap().id.clone(),
                text: "What is the saved answer?".into(),
            },
        ),
    }
}

fn history(owner: &Owner) -> &History {
    owner.record_history.as_ref().unwrap()
}

fn conversation(owner: &Owner, action: crate::model::ConversationAction) -> Command {
    Command::Conversation {
        scope: crate::history_io::Scope::from(&history(owner).selection().unwrap()),
        action,
    }
}

fn saved(owner: &Owner, run: &str) -> Result<Record<Run>> {
    let read = history(owner).db.begin_read()?;
    let table = read.open_table(RUNS)?;
    decode(table.get(run)?.unwrap().value())
}

fn frames(bytes: &Arc<Mutex<Vec<u8>>>) -> Result<Vec<Value>> {
    let bytes = bytes.lock().unwrap();
    let mut cursor = io::Cursor::new(bytes.as_slice());
    let mut values = Vec::new();
    while let Some(value) = crate::wire::read_raw(&mut cursor, crate::FRAME_MAX)? {
        values.push(serde_json::from_slice(&value)?);
    }
    Ok(values)
}

fn receive_request(listener: &TcpListener) -> Result<(TcpStream, Value)> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut socket = loop {
        match listener.accept() {
            Ok((socket, _)) => break socket,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(e) => return Err(e.into()),
        }
    };
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    socket.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut bytes = Vec::new();
    loop {
        if let Some(header_end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let header = std::str::from_utf8(&bytes[..header_end])?;
            assert!(header.starts_with("POST /flow HTTP/1.1\r\n"));
            let count: usize = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .ok_or("missing content length")?
                .parse()?;
            if bytes.len() == header_end + 4 + count {
                return Ok((socket, serde_json::from_slice(&bytes[header_end + 4..])?));
            }
        }
        if bytes.len() >= 36_864 || Instant::now() >= deadline {
            return Err("owned provider request exceeds bound or deadline".into());
        }
        let mut block = [0; 1024];
        let n = socket.read(&mut block)?;
        if n == 0 {
            return Err("owned provider request ended early".into());
        }
        bytes.extend_from_slice(&block[..n]);
    }
}

fn no_dispatch(owner: &Owner, listener: &TcpListener, rx: &mpsc::Receiver<Event>) {
    assert!(owner.operation.is_none());
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(matches!(listener.accept(), Err(e) if e.kind() == io::ErrorKind::WouldBlock));
}

fn complete(
    owner: &mut Owner,
    rx: &mpsc::Receiver<Event>,
    mut socket: TcpStream,
    expected: &str,
) -> Result<()> {
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n{}\n{{\"done\":true}}\n",
        json!({"delta": expected})
    )?;
    drop(socket);
    loop {
        match rx.recv_timeout(Duration::from_secs(3))? {
            Event::Text { run, change } => owner.text(&run, change)?,
            Event::Settled { run, result } => {
                assert!(result.is_ok());
                owner.settled(&run, result, false)?;
                return Ok(());
            }
            _ => return Err("unexpected actual Owner event".into()),
        }
    }
}

// This fixture reopens one exact projection; normal native startup recovery is still separate.
fn reopen(
    mut owner_before: Owner,
    address: SocketAddr,
    run: &str,
) -> Result<(Owner, mpsc::Receiver<Event>, Arc<Mutex<Vec<u8>>>)> {
    assert!(owner_before.operation.is_none());
    owner_before.record_history.take();
    let mut history = History::open(
        &owner_before.disk.directory.join("history.redb"),
        owner_before.disk.lease(),
    )?;
    let selection = history.selection()?;
    let state = history.edit(|edit| edit.project(&selection, Some(run)))?;
    owner(owner_before.disk, state, history, address)
}

#[test]
fn native_choices_survive_a_flat_result_without_loading_native_runs() -> Result<()> {
    use crate::agent_settings::Kind;
    for kind in [Kind::Pi, Kind::Codex, Kind::Opencode] {
        let directory = Directory::new()?;
        let disk = Disk::lock(directory.0.clone())?;
        let mut legacy = super::fixture::legacy();
        crate::recovery::interrupt(&mut legacy)?;
        let (workflow, source, field) = match kind {
            Kind::Pi => legacy
                .workflows
                .iter()
                .next()
                .map(|(id, w)| (id.clone(), w.source.clone(), "agent_sessions")),
            Kind::Codex => legacy
                .codex_workflows
                .iter()
                .next()
                .map(|(id, w)| (id.clone(), w.source.clone(), "codex_sessions")),
            Kind::Opencode => legacy
                .opencode_workflows
                .iter()
                .next()
                .map(|(id, w)| (id.clone(), w.source.clone(), "opencode_sessions")),
        }
        .ok_or("fixture workflow absent")?;
        legacy.view.source = Some(source.clone());
        let run = legacy.view.runs[0].id.clone();
        let mut history = History::import_new(
            &directory.0.join("history.redb"),
            &serde_json::to_vec(&legacy)?,
            disk.lease(),
        )?;
        let scope = history.selection()?;
        let state = history.edit(|edit| edit.project(&scope, Some(&run)))?;
        assert_eq!(state.view.runs.len(), 1);
        assert!(
            state.workflows.is_empty()
                && state.codex_workflows.is_empty()
                && state.opencode_workflows.is_empty()
        );
        let (mut owner, _, wire) = owner(disk, state, history, "127.0.0.1:1".parse()?)?;
        match kind {
            Kind::Pi => owner.pi_profile = Some(legacy.workflows[&workflow].profile.clone()),
            Kind::Codex => {
                owner.codex_profile = Some(legacy.codex_workflows[&workflow].profile.clone())
            }
            Kind::Opencode => {
                owner.opencode_profile = Some(legacy.opencode_workflows[&workflow].profile.clone())
            }
        }
        owner.reply_view(Some("native-choices"), "view", true)?;
        let view = frames(&wire)?.pop().ok_or("view reply absent")?["view"].clone();
        assert_eq!(view[field].as_array().map(Vec::len), Some(1));
        assert_eq!(view[field][0]["workflow"], workflow);
        assert_eq!(view[field][0]["source"], source.id);
        assert_eq!(view["runs"].as_array().map(Vec::len), Some(1));
        assert_eq!(view["runs"][0]["id"], run);
        assert!(!serde_json::to_string(&view)?.contains("/synthetic/"));
    }
    Ok(())
}

#[test]
fn actual_owner_admission_stream_and_durable_retry() -> Result<()> {
    let directory = Directory::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let (mut owner, rx, wire) = setup(&directory, address)?;
    let old_snapshot = fs::read(directory.0.join("snapshot.json"))?;
    let request = ask(&owner, "owner-stream");
    owner.request(request)?;
    let run = owner.state.view.runs[0].id.clone();
    assert_eq!(saved(&owner, &run)?.value.status, Status::Running);
    let (socket, body) = receive_request(&listener)?;
    assert_eq!(body["question"], "What is the saved answer?");
    assert_eq!(
        body["source"]["text"],
        "An independent source for the saved answer."
    );
    assert_eq!(body["history"], json!([]));
    let expected = "Exact saved answer, repeated repeated. 🦀";
    complete(&mut owner, &rx, socket, expected)?;
    let record = saved(&owner, &run)?;
    assert_eq!(record.revision, 3);
    assert_eq!(record.value.status, Status::Complete);
    assert_eq!(record.value.output.text(), expected);
    assert_eq!(
        history(&owner)
            .db
            .begin_read()?
            .open_table(ACTIVE_RUNS)?
            .len()?,
        0
    );
    assert_eq!(
        profile(history(&owner))?.sequence,
        owner.state.view.sequence
    );
    assert!(
        frames(&wire)?
            .iter()
            .any(|frame| frame["kind"] == "started" && frame["run"] == run)
    );
    let updates: Vec<_> = frames(&wire)?
        .into_iter()
        .filter(|frame| frame["kind"] == "changed")
        .collect();
    assert_eq!(updates.len(), 3);
    for (index, (status, output)) in [
        ("running", ""),
        ("running", expected),
        ("complete", expected),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            updates[index]["view"]["sequence"],
            owner.state.view.sequence - 2 + index as u64
        );
        assert_eq!(updates[index]["view"]["runs"][0]["id"], run);
        assert_eq!(updates[index]["view"]["runs"][0]["status"], status);
        assert_eq!(updates[index]["view"]["runs"][0]["output"], output);
    }
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, old_snapshot);
    let mut retained = vec![(run.clone(), expected.to_owned())];
    for index in 1..=20 {
        wire.lock().unwrap().clear();
        owner.request(ask(&owner, &format!("archive-{index}")))?;
        let id = owner.state.view.runs[0].id.clone();
        let (socket, body) = receive_request(&listener)?;
        let context = body["history"].as_array().unwrap();
        assert!(!context.is_empty() && context.len() <= 16);
        assert!(serde_json::to_vec(&body)?.len() <= 32768);
        let expected_context = &retained[retained.len() - context.len()..];
        for (value, (_, text)) in context.iter().zip(expected_context) {
            assert_eq!(value["output"], text.as_str());
        }
        let output = format!("Result {index}. {}", "x".repeat(3000));
        complete(&mut owner, &rx, socket, &output)?;
        assert_eq!(owner.state.view.runs.len(), 1);
        assert!(owner.state.starts.is_empty());
        retained.push((id, output));
    }
    assert!(retained.iter().map(|(_, text)| text.len()).sum::<usize>() > crate::STORE_MAX);
    for (id, text) in &retained {
        assert_eq!(saved(&owner, id)?.value.output.text(), text.as_str());
    }
    {
        let read = history(&owner).db.begin_read()?;
        for table in [RUNS, LAUNCHES, STARTS] {
            assert_eq!(read.open_table(table)?.len()?, 21);
        }
        assert_eq!(read.open_table(RUN_ORDER)?.len()?, 21);
        assert_eq!(read.open_table(CONTEXT)?.len()?, 21);
    }
    let last = retained.last().unwrap().0.clone();
    let (mut owner, rx, wire) = reopen(owner, address, &last)?;
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "open-earliest".into(),
        command: conversation(
            &owner,
            crate::model::ConversationAction::Watch { run: run.clone() },
        ),
    })?;
    let opened = frames(&wire)?;
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0]["kind"], "view");
    assert_eq!(opened[0]["ended"], true);
    assert_eq!(opened[0]["view"]["runs"][0]["id"], run);
    assert_eq!(opened[0]["view"]["runs"][0]["output"], expected);
    no_dispatch(&owner, &listener, &rx);
    wire.lock().unwrap().clear();
    assert!(owner.state.starts.is_empty());
    assert_eq!(saved(&owner, &run)?.value.output.text(), expected);
    let before = serde_json::to_value(profile(history(&owner))?)?;
    owner.request(ask(&owner, "owner-stream"))?;
    assert_eq!(
        frames(&wire)?,
        vec![
            json!({"v":crate::WIRE_VERSION,"request":"owner-stream","kind":"started","scope":crate::history_io::Scope::from(&crate::tests::owner_scope(&owner)),"run":run})
        ]
    );
    assert_eq!(serde_json::to_value(profile(history(&owner))?)?, before);
    no_dispatch(&owner, &listener, &rx);

    owner.request(ask(&owner, "stop-record"))?;
    let stopped = owner.state.view.runs[0].id.clone();
    let (socket, _) = receive_request(&listener)?;
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "stop-now".into(),
        command: conversation(
            &owner,
            crate::model::ConversationAction::Stop {
                run: stopped.clone(),
            },
        ),
    })?;
    assert_eq!(saved(&owner, &stopped)?.value.status, Status::StopRequested);
    let Event::Settled {
        run: settled,
        result,
    } = rx.recv_timeout(Duration::from_secs(3))?
    else {
        return Err("Stop returned an unexpected event".into());
    };
    assert_eq!(settled, stopped);
    assert!(result.is_err());
    owner.settled(&settled, result, false)?;
    assert_eq!(saved(&owner, &stopped)?.value.status, Status::Cancelled);
    drop(socket);

    let source = owner.state.view.source.as_ref().unwrap().clone();
    let mut page = source.page;
    page.document = "second-owned-source".into();
    page.text = "A second independent source.".into();
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "capture-second".into(),
        command: Command::Conversation {
            scope: crate::history_io::Scope::from(&history(&owner).selection()?),
            action: crate::model::ConversationAction::Capture {
                expected_source: Some(source.id.clone()),
                page: page.clone(),
            },
        },
    })?;
    let captured = owner.state.view.source.as_ref().unwrap().id.clone();
    assert_ne!(captured, source.id);
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "summary-second".into(),
        command: conversation(&owner, crate::model::ConversationAction::Summarize { page }),
    })?;
    let summary = owner.state.view.runs[0].id.clone();
    let (socket, body) = receive_request(&listener)?;
    assert_eq!(body["instruction"], "summarize");
    assert_eq!(body["source"]["text"], "A second independent source.");
    assert_eq!(body["history"], json!([]));
    complete(&mut owner, &rx, socket, "Exact second source summary.")?;
    assert_eq!(
        saved(&owner, &summary)?.value.output.text(),
        "Exact second source summary."
    );
    assert_eq!(saved(&owner, &run)?.value.output.text(), expected);
    assert_eq!(
        history(&owner)
            .db
            .begin_read()?
            .open_table(SOURCES)?
            .len()?,
        3
    );
    assert_eq!(
        history(&owner).db.begin_read()?.open_table(RUNS)?.len()?,
        23
    );
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, old_snapshot);
    no_dispatch(&owner, &listener, &rx);
    Ok(())
}

#[test]
fn actual_owner_commit_failure_and_restart_before_dispatch() -> Result<()> {
    for before_commit in [true, false] {
        let directory = Directory::new()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let (mut owner, rx, wire) = setup(&directory, address)?;
        let initial = serde_json::to_value(&owner.state)?;
        let old_snapshot = fs::read(directory.0.join("snapshot.json"))?;
        let before = serde_json::to_value(profile(history(&owner))?)?;
        let run = format!("{}:{}", owner.state.view.store, owner.state.next);
        if before_commit {
            let selected = history(&owner).selection()?;
            let write = history(&owner).db.begin_write()?;
            write.open_table(RUN_ORDER)?.insert(
                (selected.conversation.as_str(), owner.state.next),
                "retained-collision",
            )?;
            write.commit()?;
        } else {
            owner.transport.fail_next_emit();
        }
        let request = ask(&owner, "owner-failure");
        let error = owner.request(request).unwrap_err();
        assert!(wire.lock().unwrap().is_empty());
        no_dispatch(&owner, &listener, &rx);
        assert_eq!(fs::read(directory.0.join("snapshot.json"))?, old_snapshot);
        if before_commit {
            assert!(super::is_failure(error.as_ref()));
            assert_eq!(serde_json::to_value(&owner.state)?, initial);
            assert_eq!(serde_json::to_value(profile(history(&owner))?)?, before);
            let read = history(&owner).db.begin_read()?;
            for table in [RUNS, LAUNCHES, STARTS] {
                assert_eq!(read.open_table(table)?.len()?, 0);
            }
            assert_eq!(read.open_table(ACTIVE_RUNS)?.len()?, 0);
            let selected = history(&owner).selection()?;
            assert_eq!(
                read.open_table(RUN_ORDER)?
                    .get((selected.conversation.as_str(), owner.state.next))?
                    .unwrap()
                    .value(),
                "retained-collision"
            );
        } else {
            assert!(error.downcast_ref::<io::Error>().is_some());
            assert_eq!(saved(&owner, &run)?.value.status, Status::Running);
            assert_eq!(
                profile(history(&owner))?.next,
                before["next"].as_u64().unwrap() + 1
            );
            let (mut owner, rx, wire) = reopen(owner, address, &run)?;
            let mut recovered = owner.state.clone();
            crate::recovery::interrupt(&mut recovered)?;
            assert_eq!(recovered.view.runs[0].status, Status::Interrupted);
            owner.persist_records(|edit, selection| {
                edit.put_run(selection, recovered.view.runs[0].clone())?;
                Ok(((), Some(edit.project(selection, Some(&run))?)))
            })?;
            assert_eq!(saved(&owner, &run)?.value.status, Status::Interrupted);
            assert_eq!(
                history(&owner)
                    .db
                    .begin_read()?
                    .open_table(ACTIVE_RUNS)?
                    .len()?,
                0
            );
            owner.reply_view(None, "ready", false)?;
            assert!(owner.state.starts.is_empty());
            let before_retry = serde_json::to_value(profile(history(&owner))?)?;
            owner.request(ask(&owner, "owner-failure"))?;
            assert_eq!(
                serde_json::to_value(profile(history(&owner))?)?,
                before_retry
            );
            let frames = frames(&wire)?;
            assert_eq!(frames.len(), 2);
            assert_eq!(frames[0]["kind"], "ready");
            assert_eq!(frames[0]["view"]["runs"][0]["status"], "interrupted");
            assert_eq!(
                frames[1],
                json!({"v":crate::WIRE_VERSION,"request":"owner-failure","kind":"started","scope":crate::history_io::Scope::from(&crate::tests::owner_scope(&owner)),"run":run})
            );
            no_dispatch(&owner, &listener, &rx);
        }
    }
    Ok(())
}

#[test]
fn actual_owner_conversation_switch_waits_for_work_and_preserves_retry() -> Result<()> {
    use super::{ConversationChange, Selection};
    fn change(
        owner: &mut Owner,
        wire: &Arc<Mutex<Vec<u8>>>,
        request: &str,
        scope: &Selection,
        change: ConversationChange,
    ) -> Result<Value> {
        wire.lock().unwrap().clear();
        owner.request(Request {
            v: crate::WIRE_VERSION,
            request: request.into(),
            command: Command::ConversationChange {
                scope: crate::history_io::Scope::from(scope),
                change,
            },
        })?;
        frames(wire)?
            .pop()
            .ok_or_else(|| "change reply absent".into())
    }

    let directory = Directory::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let (mut owner, rx, wire) = setup(&directory, address)?;
    let a = history(&owner).selection()?;
    let page = owner.state.view.source.as_ref().unwrap().page.clone();
    let settings = serde_json::to_value((&owner.state.providers, &owner.state.agent_settings))?;
    owner.history_current("old-context")?;
    let context = frames(&wire)?.pop().unwrap();
    let created = change(
        &mut owner,
        &wire,
        "create-b",
        &a,
        ConversationChange::Create,
    )?;
    assert_eq!(created["kind"], "conversation_changed");
    let b = history(&owner).selection()?;
    assert_ne!(a.conversation, b.conversation);
    assert_eq!(b.epoch, a.epoch + 1);
    assert!(owner.state.view.source.is_none());
    assert!(owner.state.view.runs.is_empty());
    assert_eq!(
        serde_json::to_value((&owner.state.providers, &owner.state.agent_settings))?,
        settings
    );
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "held-b".into(),
        command: conversation(&owner, crate::model::ConversationAction::Summarize { page }),
    })?;
    let run = owner.state.view.runs[0].id.clone();
    let (socket, _) = receive_request(&listener)?;
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "watch-b".into(),
        command: conversation(
            &owner,
            crate::model::ConversationAction::Watch { run: run.clone() },
        ),
    })?;
    let before = serde_json::to_value(profile(history(&owner))?)?;
    let live = serde_json::to_value(&owner.state)?;
    let retry = change(
        &mut owner,
        &wire,
        "create-b",
        &a,
        ConversationChange::Create,
    )?;
    assert_eq!(retry["original"], created["original"]);
    assert_eq!(retry["current"], created["current"]);
    assert_eq!(frames(&wire)?.len(), 1);
    assert_eq!(serde_json::to_value(&owner.state)?, live);
    assert!(owner.subscribers.contains_key("watch-b"));
    for (request, action) in [
        ("busy-create", ConversationChange::Create),
        (
            "select-a",
            ConversationChange::Select {
                conversation: a.conversation.clone(),
            },
        ),
    ] {
        let reply = change(&mut owner, &wire, request, &b, action)?;
        assert_eq!(reply["kind"], "conversation_busy");
        assert_eq!(serde_json::to_value(profile(history(&owner))?)?, before);
        assert_eq!(serde_json::to_value(&owner.state)?, live);
    }
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "stop-b".into(),
        command: conversation(
            &owner,
            crate::model::ConversationAction::Stop { run: run.clone() },
        ),
    })?;
    let Event::Settled {
        run: settled,
        result,
    } = rx.recv_timeout(Duration::from_secs(3))?
    else {
        return Err("held conversation did not settle after Stop".into());
    };
    assert_eq!(settled, run);
    assert!(result.is_err());
    owner.settled(&settled, result, false)?;
    drop(socket);
    assert_eq!(saved(&owner, &run)?.value.status, Status::Cancelled);
    assert!(owner.subscribers.is_empty());

    let before = serde_json::to_value(profile(history(&owner))?)?;
    let scope = crate::tests::owner_scope(&owner);
    owner.subscribers.insert(
        "unsettled-watch".into(),
        crate::flow::Subscription { scope, run },
    );
    let select = ConversationChange::Select {
        conversation: a.conversation.clone(),
    };
    let refused = change(&mut owner, &wire, "select-a", &b, select.clone())?;
    assert_eq!(refused["kind"], "conversation_busy");
    assert_eq!(serde_json::to_value(profile(history(&owner))?)?, before);
    assert_eq!(
        history(&owner)
            .db
            .begin_read()?
            .open_table(SELECTIONS)?
            .len()?,
        1
    );
    owner.subscribers.clear();
    let selected = change(&mut owner, &wire, "select-a", &b, select)?;
    assert_eq!(selected["kind"], "conversation_changed");
    let current = history(&owner).selection()?;
    assert_eq!(current.conversation, a.conversation);
    assert_eq!(current.epoch, b.epoch + 1);
    assert_eq!(
        owner.state.view.source.as_ref().unwrap().page.title,
        "Owned page"
    );
    assert!(owner.state.view.runs.is_empty());
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "stale-a".into(),
        command: Command::HistoryRuns {
            scope: serde_json::from_value(context["scope"].clone())?,
            generation: context["generation"].as_str().unwrap().into(),
            cursor: None,
        },
    })?;
    assert_eq!(frames(&wire)?.pop().unwrap()["kind"], "history_stale");
    assert_eq!(
        history(&owner)
            .db
            .begin_read()?
            .open_table(CONVERSATIONS)?
            .len()?,
        2
    );
    assert_eq!(
        history(&owner)
            .db
            .begin_read()?
            .open_table(SELECTIONS)?
            .len()?,
        2
    );
    no_dispatch(&owner, &listener, &rx);
    Ok(())
}
