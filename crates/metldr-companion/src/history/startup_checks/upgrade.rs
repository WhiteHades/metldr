use super::{Directory, Output, Running};
use crate::{Result, history::schema::*};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableHandle};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn profile(directory: &Path) -> Result<Value> {
    let db = redb::Database::open(directory.join("history.redb"))?;
    let read = db.begin_read()?;
    let table = read.open_table(META)?;
    let profile: Value =
        serde_json::from_slice(table.get("profile")?.ok_or("missing profile")?.value())?;
    let mut tables = serde_json::Map::new();
    for definition in [
        META,
        CONVERSATIONS,
        SOURCES,
        RUNS,
        LAUNCHES,
        STARTS,
        NATIVE_TARGETS,
        PI,
        CODEX,
        OPENCODE,
        ACTIONS,
    ] {
        let mut rows = Vec::new();
        for row in read.open_table(definition)?.iter()? {
            let (key, bytes) = row?;
            if definition.name() != "metadata" || key.value() != "profile" {
                rows.push(json!([key.value(), bytes.value()]));
            }
        }
        tables.insert(definition.name().into(), json!(rows));
    }
    for definition in [NAVIGATION_HEADS, OPEN_ACTIONS, UNCERTAIN_RUNS] {
        let mut rows = Vec::new();
        for row in read.open_table(definition)?.iter()? {
            let (key, value) = row?;
            rows.push(json!([key.value(), value.value()]));
        }
        tables.insert(definition.name().into(), json!(rows));
    }
    for definition in [CONTEXT, WORKFLOW_RUNS, RUN_ORDER, NAVIGATION_GRANTS] {
        let mut rows = Vec::new();
        for row in read.open_table(definition)?.iter()? {
            let (key, value) = row?;
            rows.push(json!([key.value(), value.value()]));
        }
        tables.insert(definition.name().into(), json!(rows));
    }
    let mut action_order = Vec::new();
    for row in read.open_table(ACTION_ORDER)?.iter()? {
        let (key, value) = row?;
        action_order.push(json!([key.value(), value.value()]));
    }
    tables.insert("action_order".into(), json!(action_order));
    let mut by_tool = Vec::new();
    for row in read.open_table(ACTION_BY_TOOL)?.iter()? {
        let (key, value) = row?;
        by_tool.push(json!([key.value(), value.value()]));
    }
    tables.insert("action_by_tool".into(), json!(by_tool));
    let mut active = Vec::new();
    for row in read.open_table(ACTIVE_RUNS)?.iter()? {
        let (key, _) = row?;
        active.push(key.value().to_owned());
    }
    tables.insert("active_runs".into(), json!(active));
    Ok(json!({"profile":profile,"tables":tables}))
}

fn refused(mut host: Running) -> Result<()> {
    assert!(matches!(
        host.output.recv_timeout(Duration::from_secs(5))?,
        Output::Eof
    ));
    assert!(!host.child().wait()?.success());
    host.reader
        .take()
        .unwrap()
        .join()
        .map_err(|_| "refused host reader panicked")?;
    host.child.take();
    Ok(())
}

fn index_rows(read: &redb::ReadTransaction) -> Result<Value> {
    let mut rows = Vec::new();
    for row in read.open_table(NATIVE_CHOICES)?.iter()? {
        let (key, value) = row?;
        rows.push(json!([key.value(), value.value()]));
    }
    Ok(json!(rows))
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY and METLDR_HISTORY_OLD_BINARY with history format 10, 11 or 12"]
fn actual_old_private_host_upgrades_and_cannot_reopen_new_history() -> Result<()> {
    let directory = Directory::new()?;
    let mut legacy = crate::history::fixture::legacy();
    legacy.providers = Default::default();
    legacy.agent_settings = Default::default();
    for (number, run) in legacy.view.runs.iter().enumerate() {
        legacy
            .starts
            .insert(format!("upgrade-start-{number}"), run.id.clone());
    }
    legacy.validate()?;
    let original = serde_json::to_vec(&legacy)?;
    fs::write(directory.0.join("snapshot.json"), &original)?;
    let old = std::env::var_os("METLDR_HISTORY_OLD_BINARY")
        .ok_or("METLDR_HISTORY_OLD_BINARY is required")?;
    let host = Running::start_binary(&directory.0, &old)?;
    let ready = host.until(Duration::from_secs(10), |_| true)?;
    assert_eq!(ready["kind"], "ready");
    assert_eq!(ready["request"], Value::Null);
    assert_eq!(ready["ended"], true);
    assert!(host.finish()?.success());
    let before = profile(&directory.0)?;
    let old_format = before["profile"]["format"]
        .as_u64()
        .ok_or("old format absent")?;
    assert!(matches!(old_format, 10 | 11 | 12));
    assert_eq!(ready["v"], if old_format == 10 { 10 } else { 12 });
    assert_eq!(before["tables"]["runs"].as_array().unwrap().len(), 11);
    assert_eq!(before["tables"]["starts"].as_array().unwrap().len(), 11);
    assert_eq!(
        before["tables"]["pi_workflows"].as_array().unwrap().len(),
        1
    );
    let marker = fs::read(directory.0.join("snapshot.json"))?;
    let original_index = if old_format == 12 {
        let db = redb::Database::open(directory.0.join("history.redb"))?;
        Some(index_rows(&db.begin_read()?)?)
    } else {
        None
    };
    for damage in [
        "missing table",
        "origin digest",
        "native head",
        "index collision",
    ] {
        let broken = Directory::new()?;
        for name in ["snapshot.json", "snapshot.v8.json", "history.redb"] {
            fs::copy(directory.0.join(name), broken.0.join(name))?;
        }
        let (before_profile, before_index) = {
            let db = redb::Database::open(broken.0.join("history.redb"))?;
            let write = db.begin_write()?;
            if damage == "missing table" {
                assert!(write.delete_table(RUNS)?);
            } else if damage == "origin digest" {
                let mut meta = write.open_table(META)?;
                let mut saved: Value =
                    serde_json::from_slice(meta.get("profile")?.unwrap().value())?;
                saved["legacy_sha256"] = "0".repeat(64).into();
                meta.insert("profile", serde_json::to_vec(&saved)?.as_slice())?;
            } else if damage == "native head" {
                let mut workflows = write.open_table(OPENCODE)?;
                let id = legacy.opencode_workflows.keys().next().unwrap();
                let mut saved: Value =
                    serde_json::from_slice(workflows.get(id.as_str())?.unwrap().value())?;
                saved["value"]["last_run"] = legacy
                    .workflows
                    .values()
                    .next()
                    .unwrap()
                    .last_run
                    .clone()
                    .into();
                workflows.insert(id.as_str(), serde_json::to_vec(&saved)?.as_slice())?;
            } else {
                write
                    .open_table(NATIVE_CHOICES)?
                    .insert(("conflict", 1), "unowned")?;
            }
            write.commit()?;
            let read = db.begin_read()?;
            let meta = read.open_table(META)?;
            let bytes = meta.get("profile")?.unwrap().value().to_vec();
            (
                bytes,
                if old_format == 12 {
                    Some(index_rows(&read)?)
                } else {
                    None
                },
            )
        };
        refused(Running::start(&broken.0)?)?;
        let db = redb::Database::open(broken.0.join("history.redb"))?;
        let read = db.begin_read()?;
        let meta = read.open_table(META)?;
        let bytes = meta.get("profile")?.unwrap();
        let saved: Value = serde_json::from_slice(bytes.value())?;
        assert_eq!(
            saved["format"], old_format,
            "refused upgrade changed the old profile"
        );
        assert_eq!(
            bytes.value(),
            before_profile,
            "refused upgrade changed {damage}"
        );
        if let Some(before_index) = before_index {
            assert_eq!(
                index_rows(&read)?,
                before_index,
                "refused upgrade changed existing index"
            );
        } else if damage == "index collision" {
            assert_eq!(read.open_table(NATIVE_CHOICES)?.len()?, 1);
        } else {
            assert!(
                read.open_table(NATIVE_CHOICES).is_err(),
                "failed backfill retained a partial index"
            );
        }
    }
    let host = Running::start(&directory.0)?;
    host.ready()?;
    assert!(host.finish()?.success());
    let after = profile(&directory.0)?;
    assert_eq!(after["profile"]["format"], super::super::schema::FORMAT);
    let mut expected = before;
    expected["profile"]["format"] = super::super::schema::FORMAT.into();
    assert_eq!(after, expected);
    {
        let db = redb::Database::open(directory.0.join("history.redb"))?;
        assert_eq!(db.begin_read()?.open_table(NATIVE_CHOICES)?.len()?, 3);
        if let Some(original_index) = original_index {
            assert_eq!(index_rows(&db.begin_read()?)?, original_index);
        }
    }
    assert_eq!(fs::read(directory.0.join("snapshot.v8.json"))?, original);
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);
    refused(Running::start_binary(&directory.0, &old)?)?;
    assert_eq!(profile(&directory.0)?, after);
    let host = Running::start(&directory.0)?;
    host.ready()?;
    assert!(host.finish()?.success());
    assert_eq!(profile(&directory.0)?, after);
    Ok(())
}
