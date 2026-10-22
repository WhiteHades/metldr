use super::{Cursor, History, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status},
};
use metldr_history_pages::{Assembler, Counter, DocumentKind, Identity, Snapshot};
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
                "history-read-{}-{}",
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

fn profile(history: &History) -> Result<Profile> {
    let read = history.db.begin_read()?;
    decode(read.open_table(META)?.get("profile")?.unwrap().value())
}

fn run_id(store: &str, number: u64) -> String {
    format!("{store}:{number}")
}
fn legacy_id(store: &str, number: u64) -> String {
    format!("{store}:{number:02}")
}

#[test]
fn saved_reads_page_details_and_fail_closed() -> Result<()> {
    let directory = Directory::new();
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let mut legacy = fixture::legacy();
    crate::recovery::interrupt(&mut legacy)?;
    legacy.providers = Default::default();
    let mut history = History::import_new(
        &directory.0.join("history.redb"),
        &serde_json::to_vec(&legacy)?,
        disk.lease(),
    )?;
    let selection = history.selection()?;
    for number in 61..=80 {
        let id = history.edit(|edit| {
            let id = edit.id()?;
            assert_eq!(id, run_id(&selection.store, number));
            let run = Run {
                id: id.clone(),
                source: legacy_id(&selection.store, 1),
                kind: "answer".into(),
                question: Some(format!("quoted \"question\" 🦀 {}", "q".repeat(300))),
                status: Status::Running,
                output: Default::default(),
                error: None,
            };
            edit.admit(
                &selection,
                &format!("read-seed-{number}"),
                run,
                Launch::Fixture {
                    input_digest: format!("{number:064x}"),
                },
            )?;
            let mut completed = edit.run(&selection, &id)?;
            completed
                .output
                .change(metldr_conversation::Change::FlatAppend(format!(
                    "Escaped \"text\" 🦀 {}",
                    "x".repeat(7_450)
                )))?;
            completed.status = Status::Complete;
            edit.put_run(&selection, completed)?;
            Ok(id)
        })?;
        assert_eq!(id, run_id(&selection.store, number));
    }
    let before = profile(&history)?;
    let expected_sequence = before.sequence;
    let expected_next = before.next;
    assert!(history.db.begin_read()?.open_table(RUNS)?.len()? > 16);
    let retained_bytes = history.edit(|edit| {
        (61..=80).try_fold(0usize, |total, number| {
            Ok(total
                + edit
                    .run(&selection, &run_id(&selection.store, number))?
                    .output
                    .text()
                    .len())
        })
    })?;
    assert!(retained_bytes > 49_152);

    let first = history.edit(|edit| edit.runs_page(&selection, None))?;
    assert_eq!(first.anchor, Counter(80));
    assert_eq!(first.runs.len(), 16);
    assert_eq!(
        first
            .runs
            .iter()
            .map(|run| run.id.clone())
            .collect::<Vec<_>>(),
        (65..=80)
            .rev()
            .map(|n| run_id(&selection.store, n))
            .collect::<Vec<_>>()
    );
    assert!(serde_json::to_vec(&first)?.len() <= 32_768);
    let preview_prefix = "Escaped \"text\" 🦀 ";
    assert_eq!(
        first.runs[0].output.text,
        format!("{preview_prefix}{}", "x".repeat(512 - preview_prefix.len()))
    );
    assert!(first.runs[0].output.truncated);
    assert_eq!(first.runs[0].revision, Counter(2));
    assert!(first.runs[0].question.as_ref().unwrap().truncated);
    assert_eq!(serde_json::to_value(&first)?["sequence"], "63");
    assert_eq!(
        serde_json::to_value(Counter(u64::MAX))?,
        serde_json::json!("18446744073709551615")
    );
    assert!(serde_json::from_value::<Counter>(serde_json::json!(81)).is_err());
    assert!(serde_json::from_value::<Counter>(serde_json::json!("081")).is_err());
    let cursor = first.next.as_ref().unwrap().clone();
    assert_eq!(profile(&history)?.next, expected_next);
    assert_eq!(profile(&history)?.sequence, expected_sequence);

    // A later allocation is outside the captured anchor and cannot enter this traversal.
    history.edit(|edit| {
        let id = edit.id()?;
        let run = Run {
            id: id.clone(),
            source: legacy_id(&selection.store, 1),
            kind: "summary".into(),
            question: None,
            status: Status::Running,
            output: Default::default(),
            error: None,
        };
        edit.admit(
            &selection,
            "read-seed-81",
            run,
            Launch::Fixture {
                input_digest: "f".repeat(64),
            },
        )?;
        let mut complete = edit.run(&selection, &id)?;
        complete
            .output
            .change(metldr_conversation::Change::FlatAppend("new run".into()))?;
        complete.status = Status::Complete;
        edit.put_run(&selection, complete)
    })?;
    let page_profile = serde_json::to_value(profile(&history)?)?;
    let second = history.edit(|edit| edit.runs_page(&selection, Some(&cursor)))?;
    let second_ids: Vec<_> = second.runs.iter().map(|run| run.id.clone()).collect();
    let expected_tail = (61..=64)
        .rev()
        .map(|n| run_id(&selection.store, n))
        .chain((1..=11).rev().map(|n| legacy_id(&selection.store, n)))
        .collect::<Vec<_>>();
    assert_eq!(second_ids, expected_tail);
    assert!(second.next.is_none());
    assert!(first.runs.iter().all(|run| !second_ids.contains(&run.id)));
    assert!(
        history
            .edit(|edit| edit.runs_page(
                &selection,
                Some(&Cursor {
                    anchor: Counter(82),
                    before: Counter(0)
                })
            ))
            .is_err()
    );
    assert_eq!(serde_json::to_value(profile(&history)?)?, page_profile);
    assert!(
        history
            .edit(|edit| edit.runs_page(
                &selection,
                Some(&Cursor {
                    anchor: Counter(81),
                    before: Counter(82)
                })
            ))
            .is_err()
    );

    let known =
        history.edit(|edit| edit.source_detail(&selection, &legacy_id(&selection.store, 1)))?;
    assert_eq!(known.revision, Counter(1));
    assert_eq!(known.value.unwrap().id, legacy_id(&selection.store, 1));
    let unavailable =
        history.edit(|edit| edit.source_detail(&selection, &format!("{}:017", selection.store)))?;
    assert!(unavailable.value.is_none());
    assert_eq!(unavailable.revision, Counter(1));
    for (number, source, route, status) in [
        (1, "33", "legacy_unspecified", "complete"),
        (2, "017", "fixture", "complete"),
        (3, "018", "messages", "complete"),
        (4, "019", "https_google_interactions", "complete"),
        (5, "020", "https_chat_completions", "complete"),
        (6, "021", "https_responses", "complete"),
        (7, "022", "https_messages", "complete"),
        (8, "023", "text", "complete"),
        (9, "033", "pi", "interrupted"),
        (10, "034", "codex", "complete"),
        (11, "035", "opencode", "complete"),
        (80, "01", "fixture", "complete"),
    ] {
        let id = legacy_id(&selection.store, number);
        let observed = history.edit(|edit| edit.run_detail(&selection, &id))?;
        assert_eq!(observed.revision, Counter(if number == 80 { 2 } else { 1 }));
        assert_eq!(observed.value["id"], id);
        assert_eq!(
            observed.value["source"],
            format!("{}:{source}", selection.store)
        );
        assert_eq!(observed.value["status"], status);
        assert_eq!(observed.value["route"]["kind"], route);
        assert!(observed.value.get("conversation").is_some());
        if number == 10 {
            assert_eq!(observed.value["output"], "Repeated textRepeated text");
            assert_eq!(observed.value["conversation"][0]["kind"], "message");
            assert_eq!(observed.value["conversation"][1]["kind"], "tool");
            assert_eq!(observed.value["conversation"][2]["kind"], "message");
        }
        if number == 80 {
            assert_eq!(observed.value["kind"], "answer");
            assert_eq!(
                observed.value["output"],
                format!("Escaped \"text\" 🦀 {}", "x".repeat(7_450))
            );
        }
        let identity = Identity {
            store: selection.store.clone(),
            conversation: selection.conversation.clone(),
            epoch: Counter(selection.epoch),
            kind: DocumentKind::Run,
            document: id,
            revision: observed.revision,
            generation: "g1".into(),
            ticket: "read-ticket".into(),
        };
        let snapshot = Snapshot::new(identity.clone(), &observed.value).unwrap();
        let mut assembler = Assembler::new(identity).unwrap();
        let mut offset = 0;
        let mut page_count = 0;
        loop {
            let page = snapshot.page("read-ticket", offset).unwrap();
            page_count += 1;
            let value: serde_json::Value = serde_json::from_slice(&page)?;
            let done = assembler.push(&page, "read-ticket").unwrap();
            if done {
                break;
            }
            offset = value["next"].as_u64().unwrap() as u32;
        }
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&assembler.finish().unwrap())?,
            observed.value
        );
        if number == 80 {
            assert!(
                page_count >= 4,
                "the actual full saved result must require several fragments"
            );
        }
    }
    let action =
        history.edit(|edit| edit.action_detail(&selection, &format!("{}:050", selection.store)))?;
    assert_eq!(action.revision, Counter(1));
    assert_eq!(action.value.id, format!("{}:050", selection.store));
    let alias =
        history.edit(|edit| edit.action_detail(&selection, &format!("{}:50", selection.store)))?;
    assert_eq!(alias.revision, Counter(1));
    assert_eq!(alias.value.id, format!("{}:50", selection.store));
    escaped_pages(&mut history, &selection)?;

    let before_failed_read = profile(&history)?;
    let result = history.edit(|edit| {
        let _staged = edit.id()?;
        let rejected = edit.runs_page(
            &selection,
            Some(&Cursor {
                anchor: Counter(82),
                before: Counter(0),
            }),
        );
        assert!(rejected.is_err());
        Ok(())
    });
    assert!(result.is_err());
    let after_failed_read = profile(&history)?;
    assert_eq!(after_failed_read.next, before_failed_read.next);
    assert_eq!(after_failed_read.sequence, before_failed_read.sequence);

    // Corrupting a stored index makes a read fail as storage corruption and rolls back edits.
    {
        let write = history.db.begin_write()?;
        write
            .open_table(RUN_ORDER)?
            .insert((selection.conversation.as_str(), 89), "wrong:89")?;
        write.commit()?;
    }
    let before_corrupt_read = profile(&history)?;
    let corrupt = history.edit(|edit| {
        let page = edit.runs_page(&selection, None);
        assert!(
            page.as_ref()
                .is_err_and(|error| crate::history::is_failure(error.as_ref()))
        );
        Ok(())
    });
    assert!(corrupt.is_err());
    let after_corrupt_read = profile(&history)?;
    assert_eq!(after_corrupt_read.next, before_corrupt_read.next);
    assert_eq!(after_corrupt_read.sequence, before_corrupt_read.sequence);

    let final_profile = profile(&history)?;
    assert_eq!(final_profile.sequence, expected_sequence + 9);
    assert_eq!(final_profile.next, expected_next + 9);
    Ok(())
}

fn escaped_pages(history: &mut History, selection: &super::Selection) -> Result<()> {
    for number in 82..=89 {
        history.edit(|edit| {
            let id = edit.id()?;
            assert_eq!(id, run_id(&selection.store, number));
            edit.admit(
                selection,
                &format!("escaped-{number}"),
                Run {
                    id: id.clone(),
                    source: legacy_id(&selection.store, 1),
                    kind: "answer".into(),
                    question: Some(format!("Q{}", "\0".repeat(600))),
                    status: Status::Running,
                    output: Default::default(),
                    error: None,
                },
                Launch::Fixture {
                    input_digest: "e".repeat(64),
                },
            )?;
            let mut run = edit.run(selection, &id)?;
            run.output
                .change(metldr_conversation::Change::FlatAppend("\0".repeat(7_000)))?;
            run.status = Status::Complete;
            edit.put_run(selection, run)
        })?;
    }
    let before = serde_json::to_value(profile(history)?)?;
    let mut page = history.edit(|edit| edit.runs_page(selection, None))?;
    assert_eq!(
        page.runs.len(),
        6,
        "escaped JSON reaches the byte bound before the count bound"
    );
    assert_eq!(
        page.runs
            .iter()
            .map(|run| run.id.clone())
            .collect::<Vec<_>>(),
        (84..=89)
            .rev()
            .map(|number| run_id(&selection.store, number))
            .collect::<Vec<_>>()
    );
    let mut ids = std::collections::BTreeSet::new();
    loop {
        assert!(page.runs.len() <= 16);
        assert!(serde_json::to_vec(&page)?.len() <= 32_768);
        for run in &page.runs {
            assert!(ids.insert(run.id.clone()), "a page repeated a saved result");
        }
        let Some(cursor) = page.next else { break };
        page = history.edit(|edit| edit.runs_page(selection, Some(&cursor)))?;
    }
    assert_eq!(ids.len(), 40);
    let mut stale = selection.clone();
    stale.epoch += 1;
    assert!(history.edit(|edit| edit.runs_page(&stale, None)).is_err());
    assert!(
        serde_json::from_value::<super::Cursor>(serde_json::json!({
            "anchor":"89","before":"84","extra":true
        }))
        .is_err()
    );
    assert_eq!(serde_json::to_value(profile(history)?)?, before);
    Ok(())
}
