use super::{History, edit::Failure, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status},
};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
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
        corrections: Vec::new(),
    }
}

fn response_run(
    history: &mut History,
    scope: &super::edit::Selection,
    request: &str,
    seed: char,
) -> Result<String> {
    use metldr_conversation::{Change, MessageKey};

    history.edit(|edit| {
        let id = edit.id()?;
        let mut record = run(id.clone());
        record.output = metldr_conversation::Content::native();
        edit.admit(
            scope,
            request,
            record,
            Launch::Text {
                input_digest: seed.to_string().repeat(64),
                protocol: crate::provider_settings::Protocol::Responses,
                binding: crate::https_profile::Binding {
                    endpoint: "https://example.test/v1/responses".into(),
                    model: "model".into(),
                    revision: 1,
                    fingerprint: seed.to_string().repeat(64),
                },
            },
        )?;
        let mut record = edit.run(scope, &id)?;
        record.output.change(Change::Final {
            key: MessageKey::observed(1)?,
            text: "Before".into(),
        })?;
        edit.put_run(scope, record)?;
        Ok(id)
    })
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
fn chart_records_keep_order_scope_and_identity_after_restart() -> Result<()> {
    use metldr_conversation::{
        ArtifactPresentation, Change, Chart, ChartDatum, ChartKind, EntryRef, MessageKey,
    };
    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &empty_snapshot()?, disk.lease())?;
    let scope = history.selection()?;
    let run = response_run(&mut history, &scope, "chart:start", 'd')?;
    let chart = Chart::new(
        ChartKind::Bar,
        "Report".into(),
        "".into(),
        vec![
            ChartDatum {
                label: "North".into(),
                value: 12.into(),
            },
            ChartDatum {
                label: "South".into(),
                value: (-3).into(),
            },
        ],
    )?;
    let before = serde_json::to_value(profile(&history)?)?;
    let aborted: Result<()> = history.edit(|edit| {
        edit.accept_chart(&scope, &run, "aborted", "root", chart.clone())?;
        Err("synthetic rollback".into())
    });
    assert!(aborted.is_err());
    assert_eq!(serde_json::to_value(profile(&history)?)?, before);
    assert_eq!(history.db.begin_read()?.open_table(ARTIFACTS)?.len()?, 0);
    let id =
        history.edit(|edit| edit.accept_chart(&scope, &run, "chart:1", "root", chart.clone()))?;
    assert_eq!(
        id,
        history.edit(|edit| edit.accept_chart(&scope, &run, "chart:1", "root", chart.clone()))?
    );
    assert!(
        history
            .edit(|edit| edit.accept_chart(&scope, &run, "chart:1", "different", chart.clone()))
            .is_err()
    );
    let mut foreign = scope.clone();
    foreign.epoch += 1;
    assert!(
        history
            .edit(|edit| edit.artifact_detail(&foreign, id.as_str()))
            .is_err()
    );
    history.edit(|edit| {
        let mut value = edit.run(&scope, &run)?;
        assert!(value.output.change(Change::Artifact(id.clone())).is_err());
        value.output.change(Change::Final {
            key: MessageKey::observed(2)?,
            text: "After".into(),
        })?;
        value.status = Status::Interrupted;
        edit.put_run(&scope, value)
    })?;
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    let saved = history.edit(|edit| edit.artifact_detail(&scope, id.as_str()))?;
    assert_eq!(saved.revision.0, 1);
    assert_eq!(
        saved.value.content,
        super::artifacts::ArtifactContent::Chart {
            chart: chart.clone()
        }
    );
    assert!(
        history
            .edit(|edit| edit.accept_chart(&scope, &run, "chart:2", "root", chart.clone()))
            .is_err()
    );
    let output = history.edit(|edit| Ok(edit.run(&scope, &run)?.output))?;
    let display = metldr_conversation::Display::new(output.projection())?;
    assert!(matches!(&display.entries().collect::<Vec<_>>()[..], [
        EntryRef::Message { text: "Before", .. },
        EntryRef::Artifact { id: found, presentation: ArtifactPresentation::Chart },
        EntryRef::Message { text: "After", .. },
    ] if *found == &id));
    Ok(())
}

#[test]
fn table_records_are_atomic_scoped_idempotent_and_survive_reopen() -> Result<()> {
    use metldr_conversation::{Change, Content, MessageKey, Table};
    use metldr_history_pages::{Assembler, DocumentKind, Identity, Snapshot};
    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &empty_snapshot()?, disk.lease())?;
    let scope = history.selection()?;
    let table = || {
        Table::new(
            "Quarterly sales".into(),
            vec!["Quarter 🐈".into(), "Sales\\\"".into()],
            vec![
                vec![format!("Q1\n{}", "🐈".repeat(700)), "$100".into()],
                vec!["Q2".into(), "$250".into()],
            ],
            "User-provided data".into(),
        )
    };
    let run_id = history.edit(|edit| {
        let id = edit.id()?;
        let mut record = run(id.clone());
        record.output = Content::native();
        edit.admit(
            &scope,
            "table:start",
            record,
            Launch::Text {
                input_digest: "a".repeat(64),
                protocol: crate::provider_settings::Protocol::Responses,
                binding: crate::https_profile::Binding {
                    endpoint: "https://example.test/v1/responses".into(),
                    model: "model".into(),
                    revision: 1,
                    fingerprint: "a".repeat(64),
                },
            },
        )?;
        let mut record = edit.run(&scope, &id)?;
        record.output.change(Change::Final {
            key: MessageKey::observed(1)?,
            text: "Before".into(),
        })?;
        edit.put_run(&scope, record)?;
        Ok(id)
    })?;
    let before = serde_json::to_value(profile(&history)?)?;
    let aborted: Result<()> = history.edit(|edit| {
        edit.accept_table(&scope, &run_id, "call:aborted", "surface", table()?)?;
        Err("synthetic rollback".into())
    });
    assert!(aborted.is_err());
    assert_eq!(serde_json::to_value(profile(&history)?)?, before);
    assert_eq!(history.db.begin_read()?.open_table(ARTIFACTS)?.len()?, 0);
    let id =
        history.edit(|edit| edit.accept_table(&scope, &run_id, "call:1", "surface", table()?))?;
    assert_eq!(
        id,
        history.edit(|edit| edit.accept_table(&scope, &run_id, "call:1", "surface", table()?))?
    );
    assert_eq!(history.db.begin_read()?.open_table(ARTIFACTS)?.len()?, 1);
    let before = serde_json::to_value(profile(&history)?)?;
    assert!(
        history
            .edit(|edit| edit.accept_table(&scope, &run_id, "call:1", "changed", table()?))
            .is_err()
    );
    assert_eq!(serde_json::to_value(profile(&history)?)?, before);
    let mut foreign = scope.clone();
    foreign.epoch += 1;
    assert!(
        history
            .edit(|edit| edit.artifact_detail(&foreign, id.as_str()))
            .is_err()
    );
    let mut foreign_conversation = scope.clone();
    foreign_conversation.conversation = format!("{}:61", scope.store);
    assert!(
        history
            .edit(|edit| edit.artifact_detail(&foreign_conversation, id.as_str()))
            .is_err()
    );
    history.edit(|edit| {
        let mut record = edit.run(&scope, &run_id)?;
        assert!(!serde_json::to_string(&record)?.contains("Quarterly sales"));
        record.output.change(Change::Final {
            key: MessageKey::observed(2)?,
            text: "After".into(),
        })?;
        record.status = Status::StopRequested;
        edit.put_run(&scope, record)
    })?;
    assert!(
        history
            .edit(|edit| edit.accept_table(&scope, &run_id, "call:2", "surface", table()?))
            .is_err()
    );
    assert_eq!(
        id,
        history.edit(|edit| edit.accept_table(&scope, &run_id, "call:1", "surface", table()?))?
    );
    history.edit(|edit| {
        let mut record = edit.run(&scope, &run_id)?;
        record.status = Status::Interrupted;
        edit.put_run(&scope, record)
    })?;
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    let saved = history.edit(|edit| edit.artifact_detail(&scope, id.as_str()))?;
    assert_eq!(saved.revision.0, 1);
    assert!(matches!(
        &saved.value.content,
        super::artifacts::ArtifactContent::Table { table: saved_table } if *saved_table == table()?
    ));
    let identity = Identity {
        store: scope.store.clone(),
        conversation: scope.conversation.clone(),
        epoch: metldr_history_pages::Counter(scope.epoch),
        kind: DocumentKind::Artifact,
        document: id.as_str().to_owned(),
        revision: saved.revision,
        generation: "g1".into(),
        ticket: "artifact-ticket".into(),
    };
    let snapshot = Snapshot::new(identity.clone(), &saved.value)?;
    let mut assembler = Assembler::new(identity)?;
    let mut pages = 0;
    loop {
        let page = snapshot.page("artifact-ticket", assembler.next_offset())?;
        pages += 1;
        if assembler.push(&page, "artifact-ticket")? {
            break;
        }
    }
    assert!(pages > 1);
    let assembled = assembler.finish()?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&assembled)?,
        serde_json::to_value(&saved.value)?
    );
    let serialized = std::str::from_utf8(&assembled)?;
    assert!(serialized.contains("🐈"));
    assert!(serialized.contains("\\n"));
    let content = history.edit(|edit| Ok(edit.run(&scope, &run_id)?.output))?;
    let display = metldr_conversation::Display::new(content.projection())?;
    assert!(
        matches!(&display.entries().collect::<Vec<_>>()[..], [metldr_conversation::EntryRef::Message {text:"Before",..}, metldr_conversation::EntryRef::Artifact { id: observed, .. }, metldr_conversation::EntryRef::Message {text:"After",..}] if *observed == &id)
    );
    let orphan_id = history.edit(|edit| edit.id())?;
    let mut orphan = saved.value;
    orphan.id = metldr_conversation::ArtifactId::new(orphan_id.clone())?;
    orphan.call = "orphan".into();
    let write = history.db.begin_write()?;
    write.open_table(ARTIFACTS)?.insert(
        orphan_id.as_str(),
        encode(&Record {
            conversation: scope.conversation,
            revision: 1,
            value: orphan,
        })?
        .as_slice(),
    )?;
    write.commit()?;
    drop(history);
    assert!(History::open(&path, disk.lease()).is_err());
    Ok(())
}

#[test]
fn generated_artifacts_revise_complete_interrupt_and_reopen() -> Result<()> {
    use metldr_conversation::{
        ArtifactCompletion, ArtifactPresentation, GeneratedAnswer, GeneratedUpdate,
    };
    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &empty_snapshot()?, disk.lease())?;
    let scope = history.selection()?;
    let run_id = response_run(&mut history, &scope, "generated:start", 'c')?;
    let partial = GeneratedAnswer::default().updated(GeneratedUpdate::Html("<p>A".into()))?;
    let id = history.edit(|edit| {
        edit.preview_generated(
            &scope,
            &run_id,
            "call:generated",
            "surface",
            partial.clone(),
        )
    })?;
    assert_eq!(
        history
            .edit(|edit| edit.artifact_detail(&scope, id.as_str()))?
            .revision
            .0,
        1
    );
    history.edit(|edit| {
        edit.preview_generated(
            &scope,
            &run_id,
            "call:generated",
            "surface",
            partial.clone(),
        )
    })?;
    let extended = partial.updated(GeneratedUpdate::Html("B</p>".into()))?;
    assert_eq!(
        id,
        history.edit(|edit| {
            edit.preview_generated(
                &scope,
                &run_id,
                "call:generated",
                "surface",
                extended.clone(),
            )
        })?
    );
    let saved = history.edit(|edit| edit.artifact_detail(&scope, id.as_str()))?;
    assert_eq!(saved.revision.0, 2);
    assert_eq!(
        saved.value.presentation(saved.revision.0)?,
        ArtifactPresentation::Generated {
            revision: 2,
            completion: ArtifactCompletion::Streaming,
        }
    );
    let complete = extended.updated(GeneratedUpdate::HtmlComplete)?.finish()?;
    history.edit(|edit| {
        edit.complete_generated(
            &scope,
            &run_id,
            "call:generated",
            "surface",
            complete.clone(),
        )
    })?;
    history.edit(|edit| {
        edit.complete_generated(
            &scope,
            &run_id,
            "call:generated",
            "surface",
            complete.clone(),
        )
    })?;
    assert_eq!(
        history
            .edit(|edit| edit.artifact_detail(&scope, id.as_str()))?
            .revision
            .0,
        3
    );
    let mut record = history.edit(|edit| edit.run(&scope, &run_id))?;
    record.status = Status::Complete;
    history.edit(|edit| edit.put_run(&scope, record))?;

    let interrupted_run = response_run(&mut history, &scope, "generated:interrupt", 'f')?;
    let draft = GeneratedAnswer::default().updated(GeneratedUpdate::Html("<p>draft".into()))?;
    let interrupted_id = history.edit(|edit| {
        edit.preview_generated(&scope, &interrupted_run, "call:draft", "surface", draft)
    })?;
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    history.edit(|edit| edit.recover(&scope))?;
    let interrupted = history.edit(|edit| edit.artifact_detail(&scope, interrupted_id.as_str()))?;
    assert_eq!(interrupted.revision.0, 2);
    assert_eq!(
        interrupted.value.presentation(interrupted.revision.0)?,
        ArtifactPresentation::Generated {
            revision: 2,
            completion: ArtifactCompletion::Interrupted,
        }
    );
    let mut corrupt = interrupted.value.clone();
    if let super::artifacts::ArtifactContent::Generated { completion, .. } = &mut corrupt.content {
        *completion = ArtifactCompletion::Streaming;
    }
    assert!(
        corrupt
            .presentation_for_run(interrupted.revision.0, Status::Interrupted)
            .is_err()
    );
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    assert_eq!(
        history
            .edit(|edit| edit.artifact_detail(&scope, id.as_str()))?
            .revision
            .0,
        3
    );
    assert_eq!(
        history
            .edit(|edit| edit.artifact_detail(&scope, interrupted_id.as_str()))?
            .value
            .presentation(2)?,
        ArtifactPresentation::Generated {
            revision: 2,
            completion: ArtifactCompletion::Interrupted,
        }
    );
    Ok(())
}

#[test]
fn format_sixteen_migrates_tables_and_refuses_generated_records() -> Result<()> {
    use metldr_conversation::Table;
    use serde::Serialize;

    #[derive(Serialize)]
    struct LegacyArtifact {
        id: metldr_conversation::ArtifactId,
        run: String,
        call: String,
        surface_id: String,
        table: Table,
    }

    fn mark_format_sixteen(path: &std::path::Path) -> Result<()> {
        let db = redb::Database::open(path)?;
        let write = db.begin_write()?;
        let mut meta = write.open_table(META)?;
        let mut profile: Profile = decode(meta.get("profile")?.unwrap().value())?;
        profile.format = 16;
        meta.insert("profile", encode(&profile)?.as_slice())?;
        drop(meta);
        write.commit()?;
        Ok(())
    }

    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &empty_snapshot()?, disk.lease())?;
    let scope = history.selection()?;
    let run_id = response_run(&mut history, &scope, "migrate:table", 'd')?;
    let table = Table::new(
        "Saved".into(),
        vec!["A".into()],
        vec![vec!["B".into()]],
        "source".into(),
    )?;
    let id = history
        .edit(|edit| edit.accept_table(&scope, &run_id, "call:table", "surface", table.clone()))?;
    let saved = history.edit(|edit| edit.artifact_detail(&scope, id.as_str()))?;
    drop(history);
    {
        let db = redb::Database::open(&path)?;
        let write = db.begin_write()?;
        write.open_table(ARTIFACTS)?.insert(
            id.as_str(),
            encode(&Record {
                conversation: scope.conversation.clone(),
                revision: 1,
                value: LegacyArtifact {
                    id: id.clone(),
                    run: run_id.clone(),
                    call: "call:table".into(),
                    surface_id: "surface".into(),
                    table,
                },
            })?
            .as_slice(),
        )?;
        write.commit()?;
    }
    mark_format_sixteen(&path)?;
    let mut history = History::open(&path, disk.lease())?;
    let migrated = history.edit(|edit| edit.artifact_detail(&scope, id.as_str()))?;
    assert_eq!(migrated.value, saved.value);
    assert_eq!(migrated.revision, saved.revision);
    assert_eq!(profile(&history)?.format, FORMAT);
    drop(history);

    let generated_path = directory.0.join("generated.redb");
    let mut history = History::import_new(&generated_path, &empty_snapshot()?, disk.lease())?;
    let scope = history.selection()?;
    let run_id = response_run(&mut history, &scope, "migrate:generated", 'e')?;
    let source = metldr_conversation::GeneratedAnswer::default().updated(
        metldr_conversation::GeneratedUpdate::Html("<p>generated".into()),
    )?;
    history.edit(|edit| {
        edit.preview_generated(&scope, &run_id, "call:generated", "surface", source)
    })?;
    drop(history);
    mark_format_sixteen(&generated_path)?;
    assert!(History::open(&generated_path, disk.lease()).is_err());
    Ok(())
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

    let mut populated = metldr_conversation::Content::native();
    populated.change(metldr_conversation::Change::Begin(
        metldr_conversation::MessageKey::observed(1)?,
    ))?;
    assert!(
        history
            .edit(|tx| {
                let mut draft = run(tx.id()?);
                draft.output = populated;
                tx.admit(&selection, "prefilled:1", draft, launch())
            })
            .is_err()
    );
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
            let mut draft = run(id.clone());
            if n == 0 {
                draft.output = metldr_conversation::Content::native();
            }
            tx.admit(
                &selection,
                &format!("text-{n}"),
                draft,
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
            if saved.output.is_native() {
                saved.output.change(metldr_conversation::Change::Append {
                    key: metldr_conversation::MessageKey::observed(1)?,
                    text: format!("route{n}"),
                })?;
            } else {
                saved.output = metldr_conversation::Content::flat(format!("route{n}"))?;
            }
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
