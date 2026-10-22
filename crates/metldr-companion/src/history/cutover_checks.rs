use super::{History, fixture, migrate, schema};
use crate::Result;
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CHILD: &str = "METLDR_CUTOVER_CHILD";

struct Running(Option<Child>);
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct OwnedDirectory(PathBuf);

impl OwnedDirectory {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "history-cutover-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot(directory: &std::path::Path) -> Result<Vec<u8>> {
    let mut store = fixture::legacy();
    for (index, run) in store.view.runs.iter().enumerate() {
        store
            .starts
            .insert(format!("request:{index}"), run.id.clone());
    }
    store.validate()?;
    let bytes = serde_json::to_vec_pretty(&store)?;
    fs::write(directory.join("snapshot.json"), &bytes)?;
    Ok(bytes)
}

fn empty_store_snapshot(directory: &std::path::Path) -> Result<Vec<u8>> {
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
    let bytes = serde_json::to_vec_pretty(&store)?;
    fs::write(directory.join("snapshot.json"), &bytes)?;
    Ok(bytes)
}

fn child_at(directory: &std::path::Path, phase: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "history::cutover_checks::crash_recovery_at_every_phase",
            "--nocapture",
        ])
        .env(CHILD, phase)
        .env("METLDR_CUTOVER_DIRECTORY", directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

fn stop_at_phase(child: Child, phase: &str) {
    let mut owned = Running(Some(child));
    let child = owned.0.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) if line.starts_with("READY ") => {
                    let _ = sender.send(Ok(line));
                    return;
                }
                Ok(_) => (),
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
        }
    });
    let ready = receiver.recv_timeout(Duration::from_secs(10));
    let _ = child.kill();
    let status = child.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(9));
    reader.join().unwrap();
    assert_eq!(ready.unwrap().unwrap().trim(), format!("READY {phase}"));
}

fn old_capture(
    directory: &std::path::Path,
    old_binary: &std::path::Path,
) -> Result<std::process::Output> {
    let home = directory.join("private-home");
    fs::create_dir_all(&home)?;
    let child = Command::new(old_binary)
        .args([
            "--state",
            directory.to_str().ok_or("state path is not UTF-8")?,
            "--origin",
            "http://127.0.0.1:4311",
            "--extension-origin",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
        ])
        .env_clear()
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut owned = Running(Some(child));
    let child = owned.0.as_mut().unwrap();
    let request = serde_json::to_vec(&json!({
        "v": 9,
        "request": "capture:1",
        "command": {
            "op": "capture",
            "expected_store": fixture::legacy().view.store,
            "expected_source": null,
            "page": {
                "tab": 1,
                "document": "synthetic-document",
                "url": "http://127.0.0.1:4311/capture",
                "title": "Synthetic capture",
                "text": "Synthetic captured text"
            }
        }
    }))?;
    let mut input = child.stdin.take().ok_or("old binary stdin is absent")?;
    if let Err(error) = input
        .write_all(&(request.len() as u32).to_ne_bytes())
        .and_then(|()| input.write_all(&request))
    {
        if error.kind() != std::io::ErrorKind::BrokenPipe {
            return Err(error.into());
        }
    }
    drop(input);

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(owned.0.take().unwrap().wait_with_output()?);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("old companion timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn crash_recovery_at_every_phase() -> Result<()> {
    if let Ok(phase) = std::env::var(CHILD) {
        let directory = PathBuf::from(std::env::var_os("METLDR_CUTOVER_DIRECTORY").unwrap());
        let _history = migrate::run(directory, |at| {
            if format!("{at:?}") == phase {
                println!("\nREADY {phase}");
                use std::io::Write;
                std::io::stdout().flush()?;
                loop {
                    thread::park();
                }
            }
            Ok(())
        })?;
        panic!("migration passed requested stop phase {phase}");
    }

    let phases = [
        migrate::Phase::Backup,
        migrate::Phase::Imported,
        migrate::Phase::DatabaseSynced,
        migrate::Phase::DatabaseRenamed,
        migrate::Phase::DatabaseInstalled,
        migrate::Phase::MarkerSynced,
        migrate::Phase::MarkerRenamed,
        migrate::Phase::Complete,
    ];
    for phase in phases {
        let directory = OwnedDirectory::new();
        let legacy = snapshot(&directory.0)?;
        let digest = format!("{:x}", Sha256::digest(&legacy));
        stop_at_phase(
            child_at(&directory.0, &format!("{phase:?}")),
            &format!("{phase:?}"),
        );

        let backup = fs::read(directory.0.join("snapshot.v8.json"))?;
        assert_eq!(backup, legacy);
        let history = History::migrate(directory.0.clone())?;
        drop(history);
        let history = History::migrate(directory.0.clone())?;
        let read = history.db.begin_read()?;
        let meta = read.open_table(schema::META)?;
        assert_eq!(meta.get("legacy_snapshot")?.unwrap().value(), legacy);
        let profile: schema::Profile = schema::decode(meta.get("profile")?.unwrap().value())?;
        assert_eq!(profile.store, fixture::legacy().view.store);
        assert_eq!(profile.legacy_sha256, digest);
        assert_eq!(profile.next, 61);
        assert_eq!(profile.sequence, 42);
        assert_eq!(profile.epoch, 1);
        assert_eq!(profile.selected, format!("{}:60", profile.store));
        let conversations = read.open_table(schema::CONVERSATIONS)?;
        assert_eq!(conversations.len()?, 1);
        drop(conversations);
        assert_eq!(read.open_table(schema::SOURCES)?.len()?, 12);
        assert_eq!(read.open_table(schema::RUNS)?.len()?, 11);
        assert_eq!(read.open_table(schema::STARTS)?.len()?, 11);
        drop(meta);
        drop(read);

        let mut write = history.db.begin_write()?;
        {
            let mut meta = write.open_table(schema::META)?;
            meta.insert("cutover_sentinel", b"preserved".as_slice())?;
        }
        write.commit()?;
        drop(history);
        let reopened = History::migrate(directory.0.clone())?;
        let read = reopened.db.begin_read()?;
        let meta = read.open_table(schema::META)?;
        assert_eq!(meta.get("cutover_sentinel")?.unwrap().value(), b"preserved");
    }
    Ok(())
}

#[test]
#[ignore = "requires METLDR_CUTOVER_OLD_BINARY with saved schema 8"]
fn migration_refuses_ambiguous_state_and_old_writer_conflict() -> Result<()> {
    let old_binary = std::env::var_os("METLDR_CUTOVER_OLD_BINARY")
        .map(PathBuf::from)
        .ok_or("METLDR_CUTOVER_OLD_BINARY must name the jailed old companion")?;

    let mismatch = OwnedDirectory::new();
    let old = snapshot(&mismatch.0)?;
    fs::write(mismatch.0.join("snapshot.v8.json"), b"different backup")?;
    assert!(History::migrate(mismatch.0.clone()).is_err());
    assert!(!mismatch.0.join("history.redb").exists());
    assert_eq!(fs::read(mismatch.0.join("snapshot.json"))?, old);
    assert_eq!(
        fs::read(mismatch.0.join("snapshot.v8.json"))?,
        b"different backup"
    );

    let marker_only = OwnedDirectory::new();
    let old = snapshot(&marker_only.0)?;
    fs::write(
        marker_only.0.join("snapshot.json"),
        serde_json::to_vec(&json!({
            "schema": 9,
            "store": fixture::legacy().view.store,
            "legacy_sha256": format!("{:x}", Sha256::digest(&old))
        }))?,
    )?;
    assert!(History::migrate(marker_only.0.clone()).is_err());
    assert!(!marker_only.0.join("history.redb").exists());

    let corrupt = OwnedDirectory::new();
    let original = snapshot(&corrupt.0)?;
    drop(History::migrate(corrupt.0.clone())?);
    let marker = fs::read(corrupt.0.join("snapshot.json"))?;
    fs::write(corrupt.0.join("history.redb"), b"not a redb database")?;
    assert!(History::migrate(corrupt.0.clone()).is_err());
    assert_eq!(
        fs::read(corrupt.0.join("history.redb"))?,
        b"not a redb database"
    );
    assert_eq!(fs::read(corrupt.0.join("snapshot.json"))?, marker);
    assert_eq!(fs::read(corrupt.0.join("snapshot.v8.json"))?, original);

    let conflict = OwnedDirectory::new();
    let original = empty_store_snapshot(&conflict.0)?;
    stop_at_phase(
        child_at(&conflict.0, "DatabaseInstalled"),
        "DatabaseInstalled",
    );
    let captured = old_capture(&conflict.0, &old_binary)?;
    assert!(
        captured.status.success(),
        "old capture failed: {:?}",
        captured.stderr
    );
    let changed = fs::read(conflict.0.join("snapshot.json"))?;
    assert_ne!(changed, original);
    let stored: crate::model::Store = serde_json::from_slice(&changed)?;
    assert_eq!(stored.view.sequence, 43);
    assert_eq!(stored.next, 61);
    assert_eq!(
        stored.view.source.as_ref().unwrap().id,
        format!("{}:60", stored.view.store)
    );
    assert!(History::migrate(conflict.0.clone()).is_err());
    assert!(conflict.0.join("history.redb").exists());
    assert!(conflict.0.join("snapshot.v8.json").exists());
    assert_eq!(fs::read(conflict.0.join("snapshot.v8.json"))?, original);
    assert_eq!(fs::read(conflict.0.join("snapshot.json"))?, changed);
    {
        let disk = crate::store::Disk::lock(conflict.0.clone())?;
        let history = History::open(&conflict.0.join("history.redb"), disk.lease())?;
        let read = history.db.begin_read()?;
        let meta = read.open_table(schema::META)?;
        assert_eq!(meta.get("legacy_snapshot")?.unwrap().value(), original);
        let profile: schema::Profile = schema::decode(meta.get("profile")?.unwrap().value())?;
        assert_eq!(profile.next, 61);
        assert_eq!(profile.sequence, 42);
        assert_eq!(profile.selected, format!("{}:60", profile.store));
    }

    let live = OwnedDirectory::new();
    let original = empty_store_snapshot(&live.0)?;
    let history = migrate::run(live.0.clone(), |phase| {
        if phase == migrate::Phase::Backup {
            let denied = old_capture(&live.0, &old_binary)?;
            assert_eq!(denied.status.code(), Some(1));
            assert!(denied.stdout.is_empty());
            assert_eq!(fs::read(live.0.join("snapshot.json"))?, original);
        }
        Ok(())
    })?;
    let current = fs::read(live.0.join("snapshot.json"))?;
    let lock_error = crate::store::Disk::lock(live.0.clone())
        .err()
        .expect("returned History released its writer lease");
    let denied = old_capture(&live.0, &old_binary)?;
    assert_eq!(denied.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(denied.stderr)?,
        format!("metldr companion: {lock_error}\n")
    );
    assert!(denied.stdout.is_empty());
    assert_eq!(fs::read(live.0.join("snapshot.json"))?, current);
    drop(history);
    let refused_marker = old_capture(&live.0, &old_binary)?;
    assert_eq!(refused_marker.status.code(), Some(1));
    assert!(refused_marker.stdout.is_empty());
    assert_eq!(fs::read(live.0.join("snapshot.json"))?, current);
    Ok(())
}
