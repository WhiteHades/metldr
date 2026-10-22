use super::{ConversationBusy, ConversationChange, History, Selection, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status},
};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn profile(history: &History) -> Result<Value> {
    let read = history.db.begin_read()?;
    let meta = read.open_table(META)?;
    Ok(serde_json::from_slice(
        meta.get("profile")?.ok_or("profile missing")?.value(),
    )?)
}
fn counts(history: &History) -> Result<(u64, u64, u64)> {
    let read = history.db.begin_read()?;
    Ok((
        read.open_table(CONVERSATIONS)?.len()?,
        read.open_table(CONVERSATION_ORDER)?.len()?,
        read.open_table(SELECTIONS)?.len()?,
    ))
}
fn select(
    history: &mut History,
    expected: &Selection,
    request: &str,
    conversation: &str,
) -> Result<Selection> {
    history.edit(|tx| {
        Ok(tx
            .change_conversation(
                expected,
                request,
                ConversationChange::Select {
                    conversation: conversation.into(),
                },
            )?
            .current)
    })
}

#[test]
fn actual_conversations_keep_receipts_scopes_settings_and_native_history() -> Result<()> {
    let directory = Directory(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp").join(
        format!(
            "conversations-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ),
    ));
    fs::create_dir_all(&directory.0)?;
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let mut legacy = fixture::legacy();
    legacy.codex_workflows.values_mut().next().unwrap().binding =
        crate::codex_model::Binding::Bound {
            identity: crate::codex_model::Identity {
                thread: "thread-fixture-1".into(),
                session: "session-fixture-1".into(),
            },
        };
    legacy
        .starts
        .insert("old-run".into(), legacy.view.runs[0].id.clone());
    let mut history = History::import_new(&path, &serde_json::to_vec(&legacy)?, disk.lease())?;
    let original = history.selection()?;
    let before = profile(&history)?;
    let rejected =
        history.edit(|tx| tx.change_conversation(&original, "busy", ConversationChange::Create));
    assert!(rejected.unwrap_err().is::<ConversationBusy>());
    assert_eq!(profile(&history)?, before);
    assert_eq!(counts(&history)?, (1, 1, 0));
    history.edit(|tx| tx.recover(&original))?;
    let settled = profile(&history)?;
    let aborted: Result<()> = history.edit(|tx| {
        tx.change_conversation(&original, "aborted", ConversationChange::Create)?;
        let _ = tx.change_conversation(&original, "caught-stale", ConversationChange::Create);
        Ok(())
    });
    assert!(aborted.is_err());
    assert_eq!(profile(&history)?, settled);
    assert_eq!(counts(&history)?, (1, 1, 0));
    let b = history
        .edit(|tx| tx.change_conversation(&original, "new-b", ConversationChange::Create))?
        .current;
    assert_eq!(b.epoch, original.epoch + 1);
    let empty = history.edit(|tx| tx.project_latest(&b))?;
    assert!(empty.view.source.is_none() && empty.view.runs.is_empty());
    assert_eq!(
        serde_json::to_value(&empty.providers)?,
        settled["providers"]
    );
    assert_eq!(
        serde_json::to_value(&empty.agent_settings)?,
        settled["agents"]
    );

    let digest = "b".repeat(64);
    let run = history.edit(|tx| {
        let mut page = legacy.view.source.as_ref().unwrap().page.clone();
        page.title = "Work in B".into();
        let source = tx.new_source(&b, page)?;
        let run = Run {
            id: tx.id()?,
            source: source.id,
            kind: "summary".into(),
            question: None,
            status: Status::Running,
            output: Default::default(),
            error: None,
        };
        tx.admit(
            &b,
            "b-task",
            run.clone(),
            Launch::Fixture {
                input_digest: digest.clone(),
            },
        )?;
        Ok(run)
    })?;
    let while_active = profile(&history)?;
    assert!(
        select(&mut history, &b, "busy-select", &original.conversation)
            .unwrap_err()
            .is::<ConversationBusy>()
    );
    let retry = history
        .edit(|tx| tx.change_conversation(&original, "new-b", ConversationChange::Create))?;
    assert_eq!(retry.original, b);
    assert_eq!(retry.current, b);
    assert_eq!(profile(&history)?, while_active);
    history.edit(|tx| {
        let mut complete = run.clone();
        complete.status = Status::Complete;
        tx.put_run(&b, complete)
    })?;
    let mut current = select(&mut history, &b, "select-a", &original.conversation)?;
    assert_ne!(current.epoch, original.epoch);
    assert!(history.edit(|tx| tx.runs_page(&original, None)).is_err());
    let a_runs = history.edit(|tx| tx.runs_page(&current, None))?;
    assert_eq!(a_runs.runs.len(), 11);
    assert!(
        a_runs
            .runs
            .iter()
            .any(|row| row.id == legacy.view.runs[0].id)
    );
    current = select(&mut history, &current, "select-b", &b.conversation)?;
    let visible = history.edit(|tx| tx.project_latest(&current))?;
    assert_eq!(visible.view.runs[0].id, run.id);
    assert_eq!(
        visible.view.source.as_ref().unwrap().page.title,
        "Work in B"
    );
    let after_switch = profile(&history)?;
    let retry = history
        .edit(|tx| tx.change_conversation(&original, "new-b", ConversationChange::Create))?;
    assert_eq!(retry.original, b);
    assert_eq!(retry.current, current);
    assert_eq!(profile(&history)?, after_switch);
    assert!(
        history
            .edit(|tx| tx.change_conversation(&current, "new-b", ConversationChange::Create))
            .is_err()
    );
    assert!(select(&mut history, &original, "new-b", &b.conversation).is_err());
    assert!(
        history
            .edit(|tx| tx.change_conversation(&current, "old-run", ConversationChange::Create))
            .is_err()
    );
    assert!(
        history
            .edit(|tx| tx.change_conversation(&current, "b-task", ConversationChange::Create))
            .is_err()
    );
    assert!(
        history
            .edit(|tx| tx.duplicate(&current, "new-b", &digest))
            .is_err()
    );
    assert!(
        history
            .edit(|tx| tx.native_request(&current, "new-b"))
            .is_err()
    );
    let retried_run = history
        .edit(|tx| tx.duplicate(&b, "b-task", &digest))?
        .unwrap();
    assert_eq!(retried_run.run, run.id);
    assert_eq!(retried_run.current, current);
    assert_eq!(profile(&history)?, after_switch);

    let mut ids = BTreeSet::from([original.conversation.clone(), b.conversation.clone()]);
    for n in 0..17 {
        current = history.edit(|tx| {
            Ok(tx
                .change_conversation(&current, &format!("new-{n}"), ConversationChange::Create)?
                .current)
        })?;
        ids.insert(current.conversation.clone());
    }
    let page = history.edit(|tx| tx.conversations_page(&current, None))?;
    assert_eq!(page.conversations.len(), 16);
    assert!(serde_json::to_vec(&page)?.len() <= 32_768);
    let encoded = serde_json::to_value(&page)?;
    assert!(encoded["epoch"].is_string() && encoded["sequence"].is_string());
    assert!(page.conversations.windows(2).all(|rows| {
        rows[0]
            .id
            .rsplit(':')
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > rows[1]
                .id
                .rsplit(':')
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
    }));
    let old_scope = current;
    current = history.edit(|tx| {
        Ok(tx
            .change_conversation(&old_scope, "later", ConversationChange::Create)?
            .current)
    })?;
    assert!(
        history
            .edit(|tx| tx.conversations_page(&old_scope, page.next.as_ref()))
            .is_err()
    );
    let earlier = history.edit(|tx| tx.conversations_page(&current, page.next.as_ref()))?;
    assert_eq!(earlier.conversations.len(), 3);
    assert!(earlier.next.is_none());
    let listed: BTreeSet<_> = page
        .conversations
        .into_iter()
        .chain(earlier.conversations)
        .map(|row| row.id)
        .collect();
    assert_eq!(listed, ids);
    let before_recovery = profile(&history)?;
    history.edit(|tx| tx.recover(&current))?;
    assert_eq!(profile(&history)?, before_recovery);
    drop(history);
    let mut reopened = History::open(&path, disk.lease())?;
    assert_eq!(reopened.selection()?, current);
    assert_eq!(profile(&reopened)?, before_recovery);
    let retry = reopened
        .edit(|tx| tx.change_conversation(&original, "new-b", ConversationChange::Create))?;
    assert_eq!(retry.original, b);
    assert_eq!(retry.current, current);
    current = select(&mut reopened, &current, "reopen-a", &original.conversation)?;
    let task = reopened.edit(|tx| tx.native_task(&current, &legacy.view.runs[9].id))?;
    let super::NativeWorkflow::Codex(workflow) = task.workflow else {
        panic!("wrong native family")
    };
    assert_eq!(
        workflow.binding.resume()?.unwrap().as_str(),
        "thread-fixture-1"
    );
    let unknown = reopened.edit(|tx| tx.native_task(&current, &legacy.view.runs[10].id))?;
    let super::NativeWorkflow::OpenCode(workflow) = unknown.workflow else {
        panic!("wrong native family")
    };
    assert!(workflow.binding.resume().is_err());
    let final_profile = profile(&reopened)?;
    assert_eq!(final_profile["providers"], settled["providers"]);
    assert_eq!(final_profile["agents"], settled["agents"]);
    assert_eq!(counts(&reopened)?.0, 20);
    assert_eq!(counts(&reopened)?.1, 20);
    assert_eq!(final_profile["format"], json!(11));
    Ok(())
}
