use super::{History, edit::Failure, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status},
};
use redb::{ReadableDatabase, ReadableTableMetadata};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "history-edit-{}-{}",
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
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn empty_snapshot() -> Result<Vec<u8>> {
    let mut state = fixture::legacy();
    state.view.runs.clear();
    state.starts.clear();
    state.launches.clear();
    state.workflows.clear();
    state.codex_workflows.clear();
    state.opencode_workflows.clear();
    state.browser_actions.clear();
    state.validate()?;
    Ok(serde_json::to_vec(&state)?)
}

fn run(id: String) -> Run {
    Run {
        id,
        source: fixture::legacy().view.source.unwrap().id,
        kind: "summary".into(),
        question: None,
        status: Status::Running,
        output: Default::default(),
        error: None,
    }
}

fn profile(history: &History) -> Result<Profile> {
    let read = history.db.begin_read()?;
    let table = read.open_table(META)?;
    decode(table.get("profile")?.unwrap().value())
}

fn counts(history: &History) -> Result<(u64, u64, u64, u64, u64)> {
    let read = history.db.begin_read()?;
    Ok((
        read.open_table(RUNS)?.len()?,
        read.open_table(LAUNCHES)?.len()?,
        read.open_table(STARTS)?.len()?,
        read.open_table(RUN_ORDER)?.len()?,
        read.open_table(ACTIVE_RUNS)?.len()?,
    ))
}

#[test]
fn actual_records_commit_update_retry_abort_and_reopen() -> Result<()> {
    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let original = empty_snapshot()?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &original, disk.lease())?;
    let selection = history.selection()?;
    let before = serde_json::to_value(profile(&history)?)?;
    let launch = || Launch::Fixture {
        input_digest: "b".repeat(64),
    };
    let aborted: Result<()> = history.edit(|tx| {
        let id = tx.id()?;
        tx.admit(&selection, "abort:1", run(id), launch())?;
        Err("synthetic failure before commit".into())
    });
    assert_eq!(
        aborted.unwrap_err().to_string(),
        "synthetic failure before commit"
    );
    assert_eq!(counts(&history)?, (0, 0, 0, 0, 0));
    assert_eq!(serde_json::to_value(profile(&history)?)?, before);

    let id = history.edit(|tx| {
        let id = tx.id()?;
        tx.admit(&selection, "start:1", run(id.clone()), launch())?;
        Ok(id)
    })?;
    assert_eq!(id, format!("{}:61", selection.store));
    assert_eq!(counts(&history)?, (1, 1, 1, 1, 1));
    assert_eq!(
        (profile(&history)?.sequence, profile(&history)?.next),
        (43, 62)
    );
    let duplicate = history.edit(|tx| {
        Ok(tx
            .duplicate(&selection, "start:1", &"b".repeat(64))?
            .unwrap())
    })?;
    assert_eq!(duplicate.run, id);
    assert_eq!(duplicate.current, selection);
    assert_eq!(
        (profile(&history)?.sequence, profile(&history)?.next),
        (43, 62)
    );
    let mut wrong_scope = selection.clone();
    wrong_scope.epoch += 1;
    assert!(
        history
            .edit(|tx| tx.duplicate(&wrong_scope, "start:1", &"b".repeat(64)))
            .is_err()
    );
    assert!(
        history
            .edit(|tx| {
                let id = tx.id()?;
                tx.admit(&selection, "start:2", run(id), launch())
            })
            .is_err()
    );
    assert_eq!(
        (profile(&history)?.sequence, profile(&history)?.next),
        (43, 62)
    );
    history.edit(|tx| {
        let mut changed = tx.run(&selection, &id)?;
        changed
            .output
            .change(metldr_conversation::Change::FlatAppend(
                "Exact saved text, repeated repeated. 🦀".into(),
            ))?;
        changed.status = Status::Complete;
        tx.put_run(&selection, changed)
    })?;
    assert_eq!(counts(&history)?, (1, 1, 1, 1, 0));
    assert_eq!(
        (profile(&history)?.sequence, profile(&history)?.next),
        (44, 62)
    );
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    let saved = history.edit(|tx| tx.run(&selection, &id))?;
    assert_eq!(
        saved.output.text(),
        "Exact saved text, repeated repeated. 🦀"
    );
    assert_eq!(saved.status, Status::Complete);
    {
        let read = history.db.begin_read()?;
        let table = read.open_table(RUNS)?;
        let saved: Record<Run> = decode(table.get(id.as_str())?.unwrap().value())?;
        assert_eq!(saved.revision, 2);
        assert_eq!(
            read.open_table(META)?
                .get("legacy_snapshot")?
                .unwrap()
                .value(),
            original
        );
    }
    let completed_profile = serde_json::to_value(profile(&history)?)?;
    assert!(
        history
            .edit(|tx| {
                let mut completed = tx.run(&selection, &id)?;
                completed.status = Status::Failed;
                completed.error = Some("must not replace a completed result".into());
                completed
                    .output
                    .change(metldr_conversation::Change::FlatAppend(
                        " unwanted overwrite".into(),
                    ))?;
                tx.put_run(&selection, completed)
            })
            .is_err()
    );
    assert_eq!(serde_json::to_value(profile(&history)?)?, completed_profile);
    let preserved = history.edit(|tx| tx.run(&selection, &id))?;
    assert_eq!(preserved.status, Status::Complete);
    assert_eq!(
        preserved.output.text(),
        "Exact saved text, repeated repeated. 🦀"
    );
    for n in 2..=21 {
        let request = format!("start:{n}");
        let id = history.edit(|tx| {
            let id = tx.id()?;
            tx.admit(&selection, &request, run(id.clone()), launch())?;
            Ok(id)
        })?;
        history.edit(|tx| {
            let mut saved = tx.run(&selection, &id)?;
            saved.status = Status::Complete;
            tx.put_run(&selection, saved)
        })?;
    }
    assert_eq!(counts(&history)?, (21, 21, 21, 21, 0));
    let source = fixture::legacy().view.source.unwrap().id;
    let context = history.edit(|tx| tx.fixture_context(&selection, &source))?;
    assert_eq!(context.len(), 16);
    assert_eq!(context[0].id, format!("{}:66", selection.store));
    let fingerprint = "a".repeat(64);
    for n in 0..12 {
        let protocol = if n == 9 {
            crate::provider_settings::Protocol::Responses
        } else {
            crate::provider_settings::Protocol::Messages
        };
        let id = history.edit(|tx| {
            let id = tx.id()?;
            tx.admit(
                &selection,
                &format!("text-{n}"),
                run(id.clone()),
                Launch::Text {
                    input_digest: "b".repeat(64),
                    protocol,
                    binding: crate::https_profile::Binding {
                        endpoint: "https://example.test/v1".into(),
                        model: "fixture".into(),
                        revision: 1,
                        fingerprint: if n == 10 {
                            "b".repeat(64)
                        } else {
                            fingerprint.clone()
                        },
                    },
                },
            )?;
            Ok(id)
        })?;
        history.edit(|tx| {
            let mut saved = tx.run(&selection, &id)?;
            saved.output = metldr_conversation::Content::flat(format!("route{n}"))?;
            saved.status = if n == 11 {
                Status::Failed
            } else {
                Status::Complete
            };
            tx.put_run(&selection, saved)
        })?;
    }
    let context = history.edit(|tx| {
        tx.text_context(
            &selection,
            &source,
            crate::provider_settings::Protocol::Messages,
            &fingerprint,
        )
    })?;
    assert_eq!(
        context
            .iter()
            .map(|run| run.output.text().into_owned())
            .collect::<Vec<_>>(),
        ["route3", "route4", "route5", "route6", "route7", "route8"]
    );
    assert_eq!(
        history.edit(|tx| tx.run(&selection, &id))?.output.text(),
        "Exact saved text, repeated repeated. 🦀"
    );
    let legacy_directory = Directory::new();
    let legacy_disk = crate::store::Disk::lock(legacy_directory.0.clone())?;
    let mut legacy = fixture::legacy();
    let legacy_run = legacy.view.runs[1].id.clone();
    let digest = legacy.launches[&legacy_run]
        .input_digest()
        .unwrap()
        .to_owned();
    legacy.starts.insert("legacy:1".into(), legacy_run.clone());
    let mut imported = History::import_new(
        &legacy_directory.0.join("history.redb"),
        &serde_json::to_vec(&legacy)?,
        legacy_disk.lease(),
    )?;
    let old_selection = imported.selection()?;
    assert_eq!(imported.db.begin_read()?.open_table(CONTEXT)?.len()?, 2);
    let context =
        imported.edit(|tx| tx.fixture_context(&old_selection, &legacy.view.runs[1].source))?;
    assert_eq!(
        context
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        [legacy_run.as_str()]
    );
    if let Launch::Text {
        protocol, binding, ..
    } = &legacy.launches[&legacy.view.runs[7].id]
    {
        let context = imported.edit(|tx| {
            tx.text_context(
                &old_selection,
                &legacy.view.runs[7].source,
                *protocol,
                &binding.fingerprint,
            )
        })?;
        assert_eq!(context[0].id, legacy.view.runs[7].id);
    } else {
        return Err("legacy text fixture changed".into());
    }
    {
        let write = imported.db.begin_write()?;
        let mut next = profile(&imported)?;
        next.epoch = 3;
        write
            .open_table(META)?
            .insert("profile", encode(&next)?.as_slice())?;
        write.commit()?;
    }
    let current = imported.selection()?;
    let receipt =
        imported.edit(|tx| Ok(tx.duplicate(&old_selection, "legacy:1", &digest)?.unwrap()))?;
    assert_eq!(receipt.run, legacy_run);
    assert_eq!(receipt.current, current);
    assert_ne!(current, old_selection);
    assert!(
        imported
            .edit(|tx| tx.run(&old_selection, &legacy_run))
            .is_err()
    );
    assert!(
        imported
            .edit(|tx| {
                assert!(tx.duplicate(&old_selection, "unseen:1", &digest)?.is_none());
                tx.require_selection(&old_selection)
            })
            .is_err()
    );
    Ok(())
}

#[test]
fn caught_late_failure_cannot_commit_partial_records() -> Result<()> {
    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &empty_snapshot()?, disk.lease())?;
    let selection = history.selection()?;
    let before = serde_json::to_value(profile(&history)?)?;
    {
        let write = history.db.begin_write()?;
        write
            .open_table(RUN_ORDER)?
            .insert((selection.conversation.as_str(), 61), "retained collision")?;
        write.commit()?;
    }
    let result = history.edit(|tx| {
        let id = tx.id()?;
        let error = tx
            .admit(
                &selection,
                "start:1",
                run(id),
                Launch::Fixture {
                    input_digest: "b".repeat(64),
                },
            )
            .unwrap_err();
        assert!(error.downcast_ref::<Failure>().is_some());
        Ok(())
    });
    assert!(result.unwrap_err().downcast_ref::<Failure>().is_some());
    assert_eq!(counts(&history)?, (0, 0, 0, 1, 0));
    assert_eq!(serde_json::to_value(profile(&history)?)?, before);
    assert!(
        history
            .edit(|tx| {
                assert!(
                    tx.require_selection(&super::edit::Selection {
                        epoch: selection.epoch + 1,
                        ..selection.clone()
                    })
                    .is_err()
                );
                tx.id()?;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(serde_json::to_value(profile(&history)?)?, before);
    Ok(())
}
