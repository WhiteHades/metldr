use super::{History, fixture, schema::*};
use crate::{Result, model::Store};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct OwnedDirectory(PathBuf);
impl OwnedDirectory {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "history-import-{}-{}",
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

fn records<T: DeserializeOwned + Serialize>(
    read: &redb::ReadTransaction,
    definition: TableDefinition<&str, &[u8]>,
    selected: &str,
) -> Result<BTreeMap<String, Value>> {
    let table = read.open_table(definition)?;
    let mut records = BTreeMap::new();
    for row in table.iter()? {
        let (id, bytes) = row?;
        let record: Record<T> = decode(bytes.value())?;
        assert_eq!(record.conversation, selected);
        assert_eq!(record.revision, 1);
        records.insert(id.value().to_owned(), serde_json::to_value(record.value)?);
    }
    Ok(records)
}

#[test]
fn imports_actual_legacy_graph_without_normalizing_or_replaying() -> Result<()> {
    let directory = OwnedDirectory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let mut legacy = fixture::legacy();
    for (index, run) in legacy.view.runs.iter().enumerate() {
        legacy
            .starts
            .insert(format!("request:{index}"), run.id.clone());
    }
    legacy.validate()?;
    let mut original = serde_json::to_vec_pretty(&legacy)?;
    original.extend_from_slice(b"\n \t");
    assert!(original.len() <= crate::STORE_MAX);
    let original_digest = format!("{:x}", Sha256::digest(&original));
    let expected = serde_json::to_value(&legacy)?;
    let selected = format!("{}:60", legacy.view.store);
    let path = directory.0.join("history.redb");
    let history = History::import_new(&path, &original, disk.lease())?;
    drop(history);
    let history = History::open(&path, disk.lease())?;
    let read = history.db.begin_read()?;
    let meta = read.open_table(META)?;
    assert_eq!(meta.get("legacy_snapshot")?.unwrap().value(), original);
    let profile: Profile = decode(meta.get("profile")?.unwrap().value())?;
    assert_eq!(profile.format, FORMAT);
    assert_eq!(profile.store, legacy.view.store);
    assert_eq!(profile.sequence, 42);
    assert_eq!(profile.next, 61);
    assert_eq!(profile.epoch, 1);
    assert_eq!(profile.selected, selected);
    assert_eq!(profile.legacy_sha256, original_digest);
    assert_eq!(
        serde_json::to_value(&profile.providers)?,
        expected["providers"]
    );
    assert_eq!(
        serde_json::to_value(&profile.agents)?,
        expected["agent_settings"]
    );
    let conversations = read.open_table(CONVERSATIONS)?;
    assert_eq!(conversations.len()?, 1);
    let conversation: Conversation =
        decode(conversations.get(selected.as_str())?.unwrap().value())?;
    assert_eq!(conversation.id, selected);
    assert_eq!(conversation.title, "Current source");
    assert_eq!(
        conversation.source.as_deref(),
        Some(legacy.view.source.as_ref().unwrap().id.as_str())
    );

    let runs = records::<crate::model::Run>(&read, RUNS, &selected)?;
    assert_eq!(runs.len(), 11);
    for run in expected["view"]["runs"].as_array().unwrap() {
        assert_eq!(&runs[run["id"].as_str().unwrap()], run);
    }
    for (values, field) in [
        (
            records::<crate::agent_model::Launch>(&read, LAUNCHES, &selected)?,
            "launches",
        ),
        (
            records::<crate::agent_model::Workflow>(&read, PI, &selected)?,
            "workflows",
        ),
        (
            records::<crate::codex_model::Workflow>(&read, CODEX, &selected)?,
            "codex_workflows",
        ),
        (
            records::<crate::opencode_model::Workflow>(&read, OPENCODE, &selected)?,
            "opencode_workflows",
        ),
        (
            records::<crate::browser_action::Action>(&read, ACTIONS, &selected)?,
            "browser_actions",
        ),
    ] {
        assert_eq!(serde_json::to_value(values)?, expected[field]);
    }
    let order = read.open_table(RUN_ORDER)?;
    assert_eq!(order.len()?, 11);
    let indexed: Vec<_> = order
        .iter()?
        .map(|row| {
            row.map(|(key, id)| {
                let (conversation, number) = key.value();
                (conversation.to_owned(), number, id.value().to_owned())
            })
        })
        .collect::<std::result::Result<_, _>>()?;
    for (index, (conversation, number, id)) in indexed.iter().enumerate() {
        assert_eq!(conversation, &selected);
        assert_eq!(*number, index as u64 + 1);
        assert_eq!(id, &legacy.view.runs[index].id);
        assert_eq!(runs[id]["id"], *id);
    }
    let receipts = read.open_table(STARTS)?;
    assert_eq!(receipts.len()?, 11);
    for (request, run) in &legacy.starts {
        let receipt: Start = decode(receipts.get(request.as_str())?.unwrap().value())?;
        assert_eq!(receipt.conversation, selected);
        assert_eq!(&receipt.run, run);
        assert_eq!(receipt.format, ReceiptFormat::LegacyUnscoped);
    }
    let sources = records::<Provenance>(&read, SOURCES, &selected)?;
    assert_eq!(sources.len(), 12);
    let known: Vec<_> = legacy
        .view
        .source
        .iter()
        .chain(legacy.workflows.values().map(|w| &w.source))
        .chain(legacy.codex_workflows.values().map(|w| &w.source))
        .chain(legacy.opencode_workflows.values().map(|w| &w.source))
        .collect();
    for source in known {
        assert_eq!(sources[&source.id], json!({"kind":"known","source":source}));
    }
    for run in legacy.view.runs.iter().take(8) {
        assert_eq!(
            sources[&run.source],
            json!({"kind":"legacy_unavailable","id":run.source})
        );
    }
    let actions = records::<crate::browser_action::Action>(&read, ACTIONS, &selected)?;
    let action_order = read.open_table(ACTION_ORDER)?;
    assert_eq!(action_order.len()?, 3);
    for row in action_order.iter()? {
        let (key, value) = row?;
        let (run, number, exact_id) = key.value();
        let action = &actions[value.value()];
        assert_eq!(action["proposal"]["run"], run);
        assert_eq!(
            number,
            value.value().rsplit(':').next().unwrap().parse::<u64>()?
        );
        assert_eq!(action["id"], value.value());
        assert_eq!(exact_id, value.value());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    }
    let mut bound = legacy.clone();
    for workflow in bound.workflows.values_mut() {
        if let crate::agent_model::Binding::ReleaseUnknown {
            previous: Some(previous),
        } = &workflow.binding
        {
            workflow.binding = crate::agent_model::Binding::Bound {
                id: previous.id.clone(),
                path: previous.path.clone(),
            };
        } else {
            panic!("fixture Pi binding changed");
        }
    }
    for workflow in bound.codex_workflows.values_mut() {
        if let crate::codex_model::Binding::ReleaseUnknown {
            previous: Some(identity),
        } = &workflow.binding
        {
            workflow.binding = crate::codex_model::Binding::Bound {
                identity: identity.clone(),
            };
        } else {
            panic!("fixture Codex binding changed");
        }
    }
    for workflow in bound.opencode_workflows.values_mut() {
        if let crate::opencode_model::Binding::ReleaseUnknown {
            previous: Some(identity),
        } = &workflow.binding
        {
            workflow.binding = crate::opencode_model::Binding::Bound {
                identity: identity.clone(),
            };
        } else {
            panic!("fixture OpenCode binding changed");
        }
    }
    bound.validate()?;
    let bound_path = directory.0.join("bound.redb");
    drop(History::import_new(
        &bound_path,
        &serde_json::to_vec(&bound)?,
        disk.lease(),
    )?);
    let bound_history = History::open(&bound_path, disk.lease())?;
    let bound_read = bound_history.db.begin_read()?;
    for (values, expected) in [
        (
            records::<crate::agent_model::Workflow>(&bound_read, PI, &selected)?,
            serde_json::to_value(&bound.workflows)?,
        ),
        (
            records::<crate::codex_model::Workflow>(&bound_read, CODEX, &selected)?,
            serde_json::to_value(&bound.codex_workflows)?,
        ),
        (
            records::<crate::opencode_model::Workflow>(&bound_read, OPENCODE, &selected)?,
            serde_json::to_value(&bound.opencode_workflows)?,
        ),
    ] {
        assert_eq!(serde_json::to_value(values)?, expected);
    }
    println!(
        "history import passed: {} exact input bytes, 11 runs, 11 receipts, 12 sources, 3 native workflows, 3 actions",
        original.len()
    );
    Ok(())
}

#[test]
fn refuses_invalid_imports_and_existing_destinations_without_overwrite() -> Result<()> {
    let directory = OwnedDirectory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let legacy = fixture::legacy();
    legacy.validate()?;
    let original = serde_json::to_vec(&legacy)?;
    let path = directory.0.join("history.redb");
    for (label, bytes) in [
        ("malformed", b"{bad".to_vec()),
        ("oversized", vec![b' '; crate::STORE_MAX + 1]),
    ] {
        assert!(
            History::import_new(&path, &bytes, disk.lease()).is_err(),
            "{label}"
        );
        assert!(!path.exists());
    }
    let mut conflict = legacy.clone();
    let source = conflict.workflows.values_mut().next().unwrap();
    source.source.id = conflict.view.source.as_ref().unwrap().id.clone();
    let id = source.source.id.clone();
    conflict.view.runs[8].source = id;
    conflict.browser_actions.clear();
    conflict.validate()?;
    let error = History::import_new(&path, &serde_json::to_vec(&conflict)?, disk.lease())
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "legacy source copies conflict");
    assert!(!path.exists());
    let mut exhausted = legacy.clone();
    exhausted.next = u64::MAX;
    exhausted.validate()?;
    assert!(History::import_new(&path, &serde_json::to_vec(&exhausted)?, disk.lease()).is_err());
    assert!(!path.exists());
    fs::write(&path, b"existing destination sentinel")?;
    assert!(History::import_new(&path, &original, disk.lease()).is_err());
    assert_eq!(fs::read(&path)?, b"existing destination sentinel");
    assert!(History::open(&path, disk.lease()).is_err());
    assert_eq!(fs::read(&path)?, b"existing destination sentinel");
    let empty = directory.0.join("empty.redb");
    fs::write(&empty, b"")?;
    assert!(History::open(&empty, disk.lease()).is_err());
    assert!(fs::read(&empty)?.is_empty());
    let missing = directory.0.join("missing.redb");
    assert!(History::open(&missing, disk.lease()).is_err());
    assert!(!missing.exists());
    let malformed: Store = serde_json::from_slice(&original)?;
    assert_eq!(serde_json::to_vec(&malformed)?, original);
    println!("history import refusals passed without destination overwrite or fallback");
    Ok(())
}
