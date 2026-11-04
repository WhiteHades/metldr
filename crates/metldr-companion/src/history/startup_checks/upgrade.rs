use super::{Directory, Output, Running};
use crate::{Result, history::schema::*};
use metldr_conversation::{ArtifactId, Change, Content, MessageKey};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableHandle};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

mod table;

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
        let table = match read.open_table(definition) {
            Err(redb::TableError::TableDoesNotExist(_)) if definition.name() == "runs" => {
                tables.insert("runs".into(), Value::Null);
                continue;
            }
            result => result?,
        };
        for row in table.iter()? {
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
    let mut artifact_rows = Vec::new();
    if let Ok(table) = read.open_table(ARTIFACTS) {
        for row in table.iter()? {
            let (key, value) = row?;
            let value = serde_json::from_slice::<Value>(value.value())
                .unwrap_or_else(|_| json!(value.value()));
            artifact_rows.push(json!([key.value(), value]));
        }
    }
    tables.insert("artifacts".into(), json!(artifact_rows));
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

fn format17_record_bytes(
    directory: &Path,
) -> Result<(Vec<(String, Vec<u8>)>, Vec<(String, Vec<u8>)>)> {
    let db = redb::Database::open(directory.join("history.redb"))?;
    let read = db.begin_read()?;
    let mut runs = Vec::new();
    for row in read.open_table(RUNS)?.iter()? {
        let (key, bytes) = row?;
        runs.push((key.value().to_owned(), bytes.value().to_vec()));
    }
    let mut artifacts = Vec::new();
    for row in read.open_table(ARTIFACTS)?.iter()? {
        let (key, bytes) = row?;
        artifacts.push((key.value().to_owned(), bytes.value().to_vec()));
    }
    Ok((runs, artifacts))
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY and METLDR_HISTORY_OLD_BINARY with history format 10 through 17"]
fn actual_old_private_host_upgrades_and_cannot_reopen_new_history() -> Result<()> {
    let directory = Directory::new()?;
    let mut legacy = crate::history::fixture::legacy();
    let old_run = format!("{}:08", legacy.view.store);
    let run = format!("{}:8", legacy.view.store);
    let mut launch = legacy
        .launches
        .remove(&old_run)
        .ok_or("missing text launch")?;
    let crate::agent_model::Launch::Text { protocol, .. } = &mut launch else {
        return Err("fixture has no text run".into());
    };
    *protocol = crate::provider_settings::Protocol::Responses;
    legacy
        .view
        .runs
        .iter_mut()
        .find(|run| run.id == old_run)
        .ok_or("fixture has no text run")?
        .id = run.clone();
    legacy.launches.insert(run, launch);
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
    let initial = profile(&directory.0)?;
    let old_format = initial["profile"]["format"]
        .as_u64()
        .ok_or("old format absent")?;
    assert!(matches!(old_format, 10..=17));
    if let Some(expected) = std::env::var_os("METLDR_HISTORY_EXPECT_FORMAT") {
        let expected = expected
            .to_str()
            .ok_or("expected format is not Unicode")?
            .parse::<u64>()?;
        assert_eq!(old_format, expected, "wrong old executable format");
    }
    assert_eq!(
        ready["v"],
        match old_format {
            11 => 12,
            15 => 16,
            16 => 17,
            17 => 18,
            format => format,
        }
    );
    let table = if matches!(old_format, 16 | 17) {
        let table = table::Fixture::insert(&directory.0, old_format)?;
        let mut host = Running::start_binary(&directory.0, &old)?;
        let old_wire = if old_format == 17 { 18 } else { 17 };
        host.ready_version(old_wire)?;
        table.read(&mut host, old_wire, false)?;
        assert!(host.finish()?.success());
        Some(table)
    } else {
        None
    };
    let before = profile(&directory.0)?;
    if let Some(table) = &table {
        assert_eq!(before["tables"]["artifacts"], table.rows(false));
    }
    let format17_bytes = (old_format == 17)
        .then(|| format17_record_bytes(&directory.0))
        .transpose()?;
    assert_eq!(before["tables"]["runs"].as_array().unwrap().len(), 11);
    assert_eq!(before["tables"]["starts"].as_array().unwrap().len(), 11);
    assert_eq!(
        before["tables"]["pi_workflows"].as_array().unwrap().len(),
        1
    );
    let marker = fs::read(directory.0.join("snapshot.json"))?;
    let original_index = if old_format >= 12 {
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
        "artifact table",
        "artifact reference",
        "artifact owner",
        "future chart content",
        "future chart presentation",
    ] {
        if damage == "artifact owner" && !matches!(old_format, 16 | 17) {
            continue;
        }
        if damage.starts_with("future chart") && old_format != 17 {
            continue;
        }
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
            } else if damage == "artifact table" {
                write
                    .open_table(ARTIFACTS)?
                    .insert("unowned", b"invalid".as_slice())?;
            } else if damage == "artifact owner" {
                let fixture = table.as_ref().unwrap();
                let mut artifacts = write.open_table(ARTIFACTS)?;
                let mut record = fixture.rows(false)[0][1].clone();
                record["conversation"] = format!("{}:1", legacy.view.store).into();
                let id = record["value"]["id"].as_str().unwrap();
                artifacts.insert(id, encode(&record)?.as_slice())?;
            } else if damage == "future chart content" {
                let mut artifacts = write.open_table(ARTIFACTS)?;
                let id = table.as_ref().unwrap().rows(false)[0][0]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let mut record: Value = decode(
                    artifacts
                        .get(id.as_str())?
                        .ok_or("missing artifact")?
                        .value(),
                )?;
                record["value"]["content"] = json!({"kind":"chart","chart":{
                    "kind":"bar","title":"Future chart","description":"","data":[{"label":"A","value":1}]
                }});
                artifacts.insert(id.as_str(), encode(&record)?.as_slice())?;
            } else if damage == "future chart presentation" {
                let mut runs = write.open_table(RUNS)?;
                let id = table.as_ref().unwrap().rows(false)[0][1]["value"]["run"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let mut record: Value =
                    decode(runs.get(id.as_str())?.ok_or("missing run")?.value())?;
                let entry = record["value"]["output"]["entries"]
                    .as_array_mut()
                    .ok_or("format 17 run has no native entries")?
                    .iter_mut()
                    .find(|entry| entry["kind"] == "artifact")
                    .ok_or("missing artifact reference")?;
                entry["presentation"] = json!({"kind":"chart"});
                runs.insert(id.as_str(), encode(&record)?.as_slice())?;
            } else if damage == "artifact reference" {
                let mut runs = write.open_table(RUNS)?;
                let (run_key, bytes) = runs.iter()?.next().ok_or("missing run")??;
                let key = run_key.value().to_owned();
                let run_bytes = bytes.value().to_vec();
                drop(bytes);
                drop(run_key);
                let mut record: Record<crate::model::Run> = decode(&run_bytes)?;
                record.value.output = Content::native();
                let message = MessageKey::native("upgrade-artifact-message".into())?;
                record.value.output.change(Change::Begin(message.clone()))?;
                record.value.output.change(Change::Final {
                    key: message,
                    text: "old run".into(),
                })?;
                record
                    .value
                    .output
                    .change(Change::Artifact(ArtifactId::new("old:artifact".into())?))?;
                runs.insert(key.as_str(), encode(&record)?.as_slice())?;
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
                if old_format >= 12 {
                    Some(index_rows(&read)?)
                } else {
                    None
                },
            )
        };
        let before_state = profile(&broken.0)?;
        refused(Running::start(&broken.0)?)?;
        assert_eq!(
            profile(&broken.0)?,
            before_state,
            "refused upgrade changed {damage}"
        );
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
    let mut host = Running::start(&directory.0)?;
    host.ready()?;
    if let Some(table) = &table {
        table.read(&mut host, crate::WIRE_VERSION, true)?;
    }
    assert!(host.finish()?.success());
    let after = profile(&directory.0)?;
    if let Some(bytes) = format17_bytes {
        assert_eq!(
            format17_record_bytes(&directory.0)?,
            bytes,
            "format 17 run or artifact bytes changed during upgrade"
        );
    }
    assert_eq!(after["profile"]["format"], super::super::schema::FORMAT);
    let mut expected = before;
    expected["profile"]["format"] = super::super::schema::FORMAT.into();
    if let Some(table) = &table {
        expected["tables"]["artifacts"] = table.rows(true);
    }
    assert_eq!(after, expected);
    {
        let db = redb::Database::open(directory.0.join("history.redb"))?;
        assert_eq!(db.begin_read()?.open_table(NATIVE_CHOICES)?.len()?, 3);
        if old_format == 15 {
            assert_eq!(db.begin_read()?.open_table(ARTIFACTS)?.len()?, 0);
        }
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
    println!(
        "Old history format {old_format} upgrades to {} with {} artifact records. Old executable refusal, unchanged failed upgrades and repeated current startup pass.",
        super::super::schema::FORMAT,
        table.as_ref().map_or(0, table::Fixture::len)
    );
    Ok(())
}
