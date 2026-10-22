use super::{History, fixture, schema};
use crate::{Result, model::Page};
use redb::{ReadableDatabase, ReadableTable, TableHandle};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const EXTENSION: &str = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
const ORIGIN: &str = "http://127.0.0.1:4311";

mod conversations;
mod scoped;
mod upgrade;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "history-startup-{}-{}",
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

enum Output {
    Frame(Value),
    Eof,
    Error(String),
}

struct Running {
    child: Option<Child>,
    output: Receiver<Output>,
    reader: Option<JoinHandle<()>>,
}

impl Running {
    fn start(directory: &Path) -> Result<Self> {
        let binary = std::env::var_os("METLDR_STARTUP_BINARY")
            .ok_or("METLDR_STARTUP_BINARY is required for startup acceptance")?;
        Self::start_binary(directory, &binary)
    }

    fn start_binary(directory: &Path, binary: &std::ffi::OsStr) -> Result<Self> {
        let home = directory.join("private-home");
        fs::create_dir_all(&home)?;
        let config = directory.join("bootstrap.json");
        fs::write(
            &config,
            serde_json::to_vec(&json!({
                "state": directory,
                "origin": ORIGIN,
                "extension_origin": EXTENSION
            }))?,
        )?;
        let mut child = Command::new(binary)
            .arg(EXTENSION)
            .env_clear()
            .env("HOME", home)
            .env("METLDR_FIRST_FLOW_CONFIG", config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut stdout = child.stdout.take().ok_or("companion stdout is absent")?;
        let (sender, output) = mpsc::channel();
        let reader = thread::spawn(move || {
            loop {
                match crate::wire::read_raw(&mut stdout, crate::FRAME_MAX) {
                    Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                        Ok(frame) => {
                            if sender.send(Output::Frame(frame)).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Output::Error(error.to_string()));
                            return;
                        }
                    },
                    Ok(None) => {
                        let _ = sender.send(Output::Eof);
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send(Output::Error(error.to_string()));
                        return;
                    }
                }
            }
        });
        Ok(Self {
            child: Some(child),
            output,
            reader: Some(reader),
        })
    }

    fn child(&mut self) -> &mut Child {
        self.child.as_mut().unwrap()
    }

    fn send(&mut self, request: &str, command: Value) -> Result<()> {
        crate::wire::write_frame(
            self.child()
                .stdin
                .as_mut()
                .ok_or("companion stdin is absent")?,
            &json!({"v":crate::WIRE_VERSION,"request":request,"command":command}),
        )?;
        Ok(())
    }

    fn until(&self, timeout: Duration, mut predicate: impl FnMut(&Value) -> bool) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            match self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(Output::Frame(frame)) if predicate(&frame) => return Ok(frame),
                Ok(Output::Frame(_)) => (),
                Ok(Output::Eof) => {
                    return Err("companion closed stdout before expected frame".into());
                }
                Ok(Output::Error(error)) => return Err(error.into()),
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn ready(&self) -> Result<Value> {
        let ready = self.ready_version(crate::WIRE_VERSION)?;
        let view = &ready["view"];
        let selection = view["selection"]
            .as_object()
            .ok_or("ready response has no selected history scope")?;
        assert_eq!(selection["store"], view["store"]);
        let epoch = selection["epoch"]
            .as_str()
            .ok_or("selected history epoch is not a canonical counter")?;
        assert_eq!(epoch.parse::<u64>()?.to_string(), epoch);
        Ok(ready)
    }

    fn ready_version(&self, version: u8) -> Result<Value> {
        let first = self.until(Duration::from_secs(10), |_| true)?;
        assert_eq!(first["v"], version);
        assert_eq!(first["kind"], "ready");
        assert_eq!(first["request"], Value::Null);
        assert_eq!(first["ended"], true);
        Ok(first)
    }

    fn reply(&self, request: &str) -> Result<Value> {
        self.until(Duration::from_secs(10), |frame| frame["request"] == request)
    }

    fn finish(mut self) -> Result<ExitStatus> {
        self.child().stdin.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = self.child().try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err("companion did not exit after stdin EOF".into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| "companion stdout reader panicked")?;
        }
        match self.output.try_recv() {
            Ok(Output::Eof) => (),
            Ok(Output::Frame(frame)) => {
                return Err(format!("unexpected final companion frame: {frame}").into());
            }
            Ok(Output::Error(error)) => return Err(error.into()),
            Err(error) => return Err(format!("companion stdout did not reach EOF: {error}").into()),
        }
        self.child.take();
        Ok(status)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn empty_legacy(directory: &Path, schema: Option<u8>) -> Result<Vec<u8>> {
    let mut store = fixture::legacy();
    store.view.source = None;
    store.view.runs.clear();
    store.starts.clear();
    store.launches.clear();
    store.workflows.clear();
    store.codex_workflows.clear();
    store.opencode_workflows.clear();
    store.browser_actions.clear();
    store.providers = Default::default();
    store.agent_settings = Default::default();
    store.validate()?;
    let mut value = serde_json::to_value(store)?;
    let object = value
        .as_object_mut()
        .ok_or("serialized store is not an object")?;
    match schema {
        None => {
            for key in [
                "schema",
                "launches",
                "workflows",
                "codex_workflows",
                "opencode_workflows",
                "browser_actions",
                "providers",
                "agent_settings",
            ] {
                object.remove(key);
            }
        }
        Some(2) => {
            object.insert("schema".into(), json!(2));
            for key in ["providers", "agent_settings"] {
                object.remove(key);
            }
        }
        Some(3) => {
            object.insert("schema".into(), json!(3));
            object.remove("agent_settings");
        }
        Some(version) => {
            object.insert("schema".into(), json!(version));
        }
    }
    let bytes = serde_json::to_vec(&value)?;
    fs::write(directory.join("snapshot.json"), &bytes)?;
    Ok(bytes)
}

fn source(history: &History, id: &str) -> Result<crate::model::Source> {
    let read = history.db.begin_read()?;
    let table = read.open_table(schema::SOURCES)?;
    let bytes = table.get(id)?.ok_or("captured source is absent")?;
    let record: schema::Record<schema::Provenance> = schema::decode(bytes.value())?;
    match record.value {
        schema::Provenance::Known { source } => Ok(source),
        _ => Err("captured source content is unavailable".into()),
    }
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY with the current companion"]
fn saved_history_native_frames_pin_retry_release_and_restart() -> Result<()> {
    use metldr_history_pages::{Assembler, Identity};
    let directory = Directory::new()?;
    let mut initial = fixture::legacy();
    initial.providers = Default::default();
    let store = initial.view.store.clone();
    let document = format!("{store}:01");
    let text = format!("Saved \"content\" 🦀 {}", "x".repeat(7_000));
    initial.view.runs[0]
        .output
        .change(metldr_conversation::Change::FlatAppend(text.clone()))?;
    initial.validate()?;
    fs::write(
        directory.0.join("snapshot.json"),
        serde_json::to_vec(&initial)?,
    )?;
    let mut host = Running::start(&directory.0)?;
    let ready = host.ready()?;
    let sequence = ready["view"]["sequence"].clone();
    host.send("current", json!({"op":"history_current"}))?;
    let current = host.reply("current")?;
    assert_eq!(current["kind"], "history_current");
    assert_eq!(current["scope"]["store"], store);
    assert_eq!(current["scope"]["epoch"], "1");
    let scope = current["scope"].clone();
    let generation = current["generation"].clone();
    host.send(
        "list",
        json!({"op":"history_runs","scope":scope,"generation":generation,"cursor":null}),
    )?;
    let list = host.reply("list")?;
    assert_eq!(list["kind"], "history_runs");
    let rows = list["page"]["runs"].as_array().unwrap();
    assert_eq!(rows.len(), 11);
    assert_eq!(rows.first().unwrap()["id"], format!("{store}:11"));
    assert_eq!(rows.last().unwrap()["id"], document);
    assert_eq!(rows.last().unwrap()["revision"], "1");
    let pin = json!({"op":"history_pin","scope":scope,"generation":generation,"kind":"run","document":document,"revision":"1"});
    host.send("pin", pin.clone())?;
    let first = host.reply("pin")?;
    assert_eq!(first["kind"], "history_pinned");
    let identity: Identity = serde_json::from_value(first["identity"].clone())?;
    host.send("retry-pin", pin.clone())?;
    assert_eq!(host.reply("retry-pin")?["identity"], first["identity"]);
    let mut assembled = Assembler::new(identity.clone())?;
    let mut fragments = 0;
    loop {
        let offset = assembled.next_offset();
        let request = format!("page-{offset}");
        let page = json!({"op":"history_page","identity":identity,"offset":offset});
        host.send(&request, page.clone())?;
        let frame = host.reply(&request)?;
        assert_eq!(frame["kind"], "detail");
        assert!(serde_json::to_vec(&frame)?.len() <= crate::FRAME_MAX);
        let done = assembled.push(&serde_json::to_vec(&frame)?, &request)?;
        fragments += 1;
        if done {
            host.send(&request, page)?;
            assert_eq!(
                host.reply(&request)?,
                frame,
                "final page must remain retryable"
            );
            break;
        }
    }
    assert!(fragments >= 4);
    let detail: Value = serde_json::from_slice(&assembled.finish()?)?;
    assert_eq!(detail["id"], document);
    assert_eq!(detail["output"], text);
    assert_eq!(detail["route"]["kind"], "legacy_unspecified");

    for suffix in ["02", "03", "10"] {
        let mut other = pin.clone();
        other["document"] = json!(format!("{store}:{suffix}"));
        host.send("other-pin", other)?;
        assert_eq!(host.reply("other-pin")?["kind"], "history_pinned");
    }
    let mut fifth = pin.clone();
    fifth["document"] = json!(format!("{store}:04"));
    host.send("full", fifth)?;
    assert_eq!(host.reply("full")?["kind"], "history_busy");
    host.send(
        "kept",
        json!({"op":"history_page","identity":identity,"offset":0}),
    )?;
    assert_eq!(
        host.reply("kept")?["kind"],
        "detail",
        "capacity refusal cannot evict unfinished work"
    );

    let mut changed = identity.clone();
    changed.revision.0 += 1;
    host.send(
        "changed",
        json!({"op":"history_page","identity":changed,"offset":0}),
    )?;
    assert_eq!(host.reply("changed")?["kind"], "error");
    host.send(
        "release",
        json!({"op":"history_release","identity":identity}),
    )?;
    assert_eq!(host.reply("release")?["released"], true);
    host.send(
        "release-again",
        json!({"op":"history_release","identity":identity}),
    )?;
    assert_eq!(host.reply("release-again")?["released"], false);
    host.send(
        "released-page",
        json!({"op":"history_page","identity":identity,"offset":0}),
    )?;
    assert_eq!(host.reply("released-page")?["kind"], "history_stale");
    let mut stale = pin.clone();
    stale["revision"] = json!("2");
    host.send("stale", stale)?;
    let stale = host.reply("stale")?;
    assert_eq!(stale["kind"], "history_stale");
    assert_eq!(stale["observation"]["revision"], "1");
    assert_eq!(stale["observation"]["status"], "complete");
    assert_eq!(
        stale["observation"]["sequence"],
        sequence.as_u64().unwrap().to_string()
    );
    assert_eq!(stale["observation"]["document"], document);
    assert_eq!(stale["observation"]["scope"], scope);
    let mut unavailable = pin.clone();
    unavailable["kind"] = json!("source");
    unavailable["document"] = json!(format!("{store}:017"));
    host.send("unavailable", unavailable)?;
    assert_eq!(host.reply("unavailable")?["kind"], "history_unavailable");
    host.send("view", json!({"op":"view"}))?;
    assert_eq!(host.reply("view")?["view"]["sequence"], sequence);
    assert!(host.finish()?.success());

    let mut reopened = Running::start(&directory.0)?;
    assert_eq!(reopened.ready()?["view"]["sequence"], sequence);
    reopened.send(
        "old-page",
        json!({"op":"history_page","identity":identity,"offset":0}),
    )?;
    assert_eq!(reopened.reply("old-page")?["kind"], "history_stale");
    reopened.send("new-current", json!({"op":"history_current"}))?;
    let fresh = reopened.reply("new-current")?;
    assert_eq!(fresh["scope"], scope);
    assert_ne!(fresh["generation"], generation);
    reopened.send("after-stale", json!({"op":"view"}))?;
    assert_eq!(reopened.reply("after-stale")?["view"]["sequence"], sequence);
    assert!(reopened.finish()?.success());
    let (_, mut history) = History::load(directory.0.clone())?;
    let selection = history.selection()?;
    let selected = history.edit(|edit| edit.project(&selection, Some(&document)))?;
    assert_eq!(selected.next, 61);
    assert_eq!(selected.view.sequence, sequence.as_u64().unwrap());
    assert_eq!(selected.view.runs[0].output.text(), text);
    Ok(())
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY with the current companion"]
fn startup_capture_lock_and_legacy_reopen() -> Result<()> {
    let _required_binary = std::env::var_os("METLDR_STARTUP_BINARY")
        .ok_or("METLDR_STARTUP_BINARY is required for startup acceptance")?;
    let directory = Directory::new()?;
    assert!(!directory.0.join("snapshot.json").exists());
    let mut first = Running::start(&directory.0)?;
    let ready = first.ready()?;
    let scope = ready["view"]["selection"].clone();
    assert!(
        ready["view"].is_object(),
        "ready response must contain a view"
    );
    let marker = fs::read(directory.0.join("snapshot.json"))?;
    assert_eq!(serde_json::from_slice::<Value>(&marker)?["schema"], 9);
    assert!(directory.0.join("history.redb").is_file());
    let page = Page {
        tab: 1,
        document: "startup-acceptance-document".into(),
        url: format!("{ORIGIN}/capture"),
        title: "Startup acceptance capture".into(),
        text: "Owned synthetic capture text".into(),
    };
    let capture = json!({"op":"conversation","scope":scope,"action":{"op":"capture","expected_source":null,"page":page}});
    first.send("capture:one", capture.clone())?;
    let captured = first.reply("capture:one")?;
    assert_eq!(captured["kind"], "view");
    assert_eq!(
        captured["view"]["source"]["url"],
        format!("{ORIGIN}/capture")
    );
    let after_capture = captured["view"]["sequence"]
        .as_u64()
        .ok_or("capture sequence absent")?;
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);

    first.send("capture:duplicate", capture)?;
    let duplicate = first.reply("capture:duplicate")?;
    assert_eq!(duplicate["view"]["sequence"], after_capture);
    assert_eq!(
        duplicate["view"]["source"]["id"],
        captured["view"]["source"]["id"]
    );
    assert_eq!(
        duplicate["view"]["source"]["url"],
        format!("{ORIGIN}/capture")
    );
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);

    let mut second = Running::start(&directory.0)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if second.child().try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("second companion remained alive without rejecting lock".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let second_status = second.finish()?;
    assert!(
        !second_status.success(),
        "second companion must reject the held writer lock"
    );
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);

    assert!(
        first.finish()?.success(),
        "first companion must shut down cleanly on EOF"
    );
    let (disk, history) = History::load(directory.0.clone())?;
    let captured_id = captured["view"]["source"]["id"]
        .as_str()
        .ok_or("captured source id absent")?;
    assert_eq!(
        source(&history, captured_id)?.page.text,
        "Owned synthetic capture text"
    );
    drop(history);
    drop(disk);
    let reopened = Running::start(&directory.0)?;
    let reopened_ready = reopened.ready()?;
    assert_eq!(reopened_ready["view"]["source"]["id"], captured_id);
    assert!(reopened.finish()?.success());

    for schema in [
        None,
        Some(2),
        Some(3),
        Some(4),
        Some(5),
        Some(6),
        Some(7),
        Some(8),
    ] {
        let legacy_dir = Directory::new()?;
        let original = empty_legacy(&legacy_dir.0, schema)?;
        let host = Running::start(&legacy_dir.0)?;
        let ready = host.ready()?;
        assert_eq!(ready["kind"], "ready");
        assert!(host.finish()?.success());
        let version = schema.unwrap_or(1);
        assert_eq!(
            fs::read(legacy_dir.0.join(format!("snapshot.v{version}.json")))?,
            original
        );
        let (disk, history) = History::load(legacy_dir.0.clone())?;
        let read = history.db.begin_read()?;
        let meta = read.open_table(schema::META)?;
        let profile: schema::Profile = schema::decode(
            meta.get("profile")?
                .ok_or("history profile absent")?
                .value(),
        )?;
        assert_eq!(profile.format, schema::FORMAT);
        drop(meta);
        drop(read);
        drop(history);
        drop(disk);
        let reopened = Running::start(&legacy_dir.0)?;
        assert_eq!(reopened.ready()?["kind"], "ready");
        assert!(reopened.finish()?.success());
    }
    recovered_before_ready()?;
    refuse_broken_history()?;
    Ok(())
}

fn refuse_broken_history() -> Result<()> {
    let missing = Directory::new()?;
    let marker = serde_json::to_vec(
        &json!({"schema":9,"store":"a".repeat(32),"legacy_sha256":"b".repeat(64)}),
    )?;
    fs::write(missing.0.join("snapshot.json"), &marker)?;
    assert!(!Running::start(&missing.0)?.finish()?.success());
    assert_eq!(fs::read(missing.0.join("snapshot.json"))?, marker);
    assert!(!missing.0.join("history.redb").exists());

    let corrupt = Directory::new()?;
    let (disk, history) = History::load(corrupt.0.clone())?;
    drop(history);
    drop(disk);
    let marker = fs::read(corrupt.0.join("snapshot.json"))?;
    let backup = fs::read(corrupt.0.join("snapshot.v8.json"))?;
    fs::write(corrupt.0.join("history.redb"), b"invalid saved database")?;
    assert!(!Running::start(&corrupt.0)?.finish()?.success());
    assert_eq!(
        fs::read(corrupt.0.join("history.redb"))?,
        b"invalid saved database"
    );
    assert_eq!(fs::read(corrupt.0.join("snapshot.json"))?, marker);
    assert_eq!(fs::read(corrupt.0.join("snapshot.v8.json"))?, backup);
    Ok(())
}

fn raw_recovery(history: &History) -> Result<Value> {
    let read = history.db.begin_read()?;
    let mut saved = serde_json::Map::new();
    for table in [
        schema::META,
        schema::RUNS,
        schema::LAUNCHES,
        schema::STARTS,
        schema::PI,
        schema::CODEX,
        schema::OPENCODE,
        schema::ACTIONS,
        schema::NATIVE_TARGETS,
    ] {
        let rows = read
            .open_table(table)?
            .iter()?
            .map(|row| -> Result<_> {
                let (key, value) = row?;
                Ok((key.value().to_owned(), value.value().to_vec()))
            })
            .collect::<Result<Vec<_>>>()?;
        saved.insert(table.name().to_owned(), serde_json::to_value(rows)?);
    }
    let active = read
        .open_table(schema::ACTIVE_RUNS)?
        .iter()?
        .map(|row| row.map(|(key, _)| key.value().to_owned()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    saved.insert("active".into(), json!(active));
    for table in [schema::OPEN_ACTIONS, schema::UNCERTAIN_RUNS] {
        let rows = read
            .open_table(table)?
            .iter()?
            .map(|row| row.map(|(key, value)| (key.value().to_owned(), value.value().to_owned())))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        saved.insert(table.name().to_owned(), json!(rows));
    }
    Ok(saved.into())
}

fn recovered_before_ready() -> Result<()> {
    use crate::{
        agent_model::Launch,
        model::{Run, Status},
    };
    use redb::ReadableTableMetadata;
    let directory = Directory::new()?;
    let mut initial = fixture::legacy();
    initial.providers = Default::default();
    let store = initial.view.store.clone();
    let source = initial.view.source.as_ref().unwrap().clone();
    let run = format!("{store}:14");
    let command = json!({"op":"summarize","page":source.page});
    let digest = crate::agent_model::digest(
        &json!({"command":command,"origin":ORIGIN,"model":"metldr-fixture"}),
    )?;
    initial.view.runs.push(Run {
        id: run.clone(),
        source: source.id,
        kind: "summary".into(),
        question: None,
        status: Status::Running,
        output: Default::default(),
        error: None,
    });
    initial.launches.insert(
        run.clone(),
        Launch::Fixture {
            input_digest: digest,
        },
    );
    initial
        .starts
        .insert("restart:duplicate".into(), run.clone());
    initial.validate()?;
    let original = serde_json::to_vec(&initial)?;
    fs::write(directory.0.join("snapshot.json"), &original)?;
    let mut host = Running::start(&directory.0)?;
    let ready = host.ready()?;
    assert_eq!(ready["view"]["sequence"], 43);
    assert_eq!(ready["view"]["runs"][0]["id"], run);
    assert_eq!(ready["view"]["runs"][0]["status"], "interrupted");
    let scope = ready["view"]["selection"].clone();
    host.send(
        "restart:duplicate",
        json!({"op":"conversation","scope":scope,"action":command}),
    )?;
    let duplicate = host.reply("restart:duplicate")?;
    assert_eq!(duplicate["kind"], "started");
    assert_eq!(duplicate["run"], run);
    host.send(
        "restart:watch",
        json!({"op":"conversation","scope":scope,"action":{"op":"watch","run":format!("{store}:09")}}),
    )?;
    let recovered = host.reply("restart:watch")?;
    assert_eq!(recovered["view"]["sequence"], 43);
    assert_eq!(recovered["view"]["runs"][0]["status"], "interrupted");
    let action = recovered["view"]["browser_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["id"] == format!("{store}:051"))
        .unwrap();
    assert_eq!(action["outcome"], "uncertain");
    assert_eq!(action["reason"], "native host restarted after dispatch");
    assert!(host.finish()?.success());
    assert_eq!(fs::read(directory.0.join("snapshot.v8.json"))?, original);
    let marker = fs::read(directory.0.join("snapshot.json"))?;
    let (disk, mut history) = History::load(directory.0.clone())?;
    let scope = history.selection()?;
    history.edit(|edit| {
        let task = edit.native_task(&scope, &format!("{store}:09"))?;
        assert_eq!(task.run.status, Status::Interrupted);
        assert!(
            matches!(task.workflow, super::NativeWorkflow::Pi(ref workflow)
            if matches!(workflow.binding, crate::agent_model::Binding::ReleaseUnknown { .. }))
        );
        Ok(())
    })?;
    let read = history.db.begin_read()?;
    assert_eq!(read.open_table(schema::ACTIVE_RUNS)?.len()?, 0);
    assert_eq!(read.open_table(schema::RUNS)?.len()?, 12);
    assert_eq!(read.open_table(schema::STARTS)?.len()?, 1);
    let meta = read.open_table(schema::META)?;
    let profile: schema::Profile = schema::decode(meta.get("profile")?.unwrap().value())?;
    assert_eq!(profile.next, 61);
    drop(meta);
    drop(read);
    drop(history);
    drop(disk);
    let reopened = Running::start(&directory.0)?;
    let again = reopened.ready()?;
    assert_eq!(again["view"]["sequence"], 43);
    assert_eq!(again["view"]["runs"][0]["id"], run);
    assert!(reopened.finish()?.success());
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);

    let failed = Directory::new()?;
    initial.view.sequence = u64::MAX;
    fs::write(
        failed.0.join("snapshot.json"),
        serde_json::to_vec(&initial)?,
    )?;
    let (disk, history) = History::load(failed.0.clone())?;
    let before = raw_recovery(&history)?;
    drop(history);
    drop(disk);
    let rejected = Running::start(&failed.0)?;
    assert!(
        !rejected.finish()?.success(),
        "overflow must fail before ready"
    );
    let (disk, history) = History::load(failed.0.clone())?;
    assert_eq!(raw_recovery(&history)?, before);
    drop(history);
    drop(disk);
    Ok(())
}
