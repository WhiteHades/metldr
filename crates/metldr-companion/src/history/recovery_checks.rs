use super::{History, Selection, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status, Store},
};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "history-recovery-{}-{}",
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
    decode(
        table
            .get("profile")?
            .ok_or("missing history profile")?
            .value(),
    )
}

fn run_id(store: &str, number: u64) -> String {
    format!("{store}:{number:02}")
}

fn recovery_fixture() -> Result<Store> {
    let mut state = fixture::legacy();
    crate::recovery::interrupt(&mut state)?;

    for (id, status) in [
        ("0123456789abcdef0123456789abcdef:09", Status::Running),
        ("0123456789abcdef0123456789abcdef:10", Status::Running),
        ("0123456789abcdef0123456789abcdef:11", Status::StopRequested),
    ] {
        state
            .view
            .runs
            .iter_mut()
            .find(|run| run.id == id)
            .unwrap()
            .status = status;
    }
    state
        .workflows
        .get_mut("0123456789abcdef0123456789abcdef:030")
        .unwrap()
        .binding = crate::agent_model::Binding::Pending;
    state
        .codex_workflows
        .get_mut("0123456789abcdef0123456789abcdef:031")
        .unwrap()
        .binding = crate::codex_model::Binding::Bound {
        identity: crate::codex_model::Identity {
            thread: "thread-fixture-1".into(),
            session: "session-fixture-1".into(),
        },
    };
    state
        .opencode_workflows
        .get_mut("0123456789abcdef0123456789abcdef:032")
        .unwrap()
        .binding = crate::opencode_model::Binding::Pending;
    state
        .browser_actions
        .get_mut("0123456789abcdef0123456789abcdef:051")
        .unwrap()
        .state = crate::browser_action::Phase::Dispatching;
    state
        .browser_actions
        .get_mut("0123456789abcdef0123456789abcdef:051")
        .unwrap()
        .revision = 3;

    for (number, status) in [(12, Status::Running), (13, Status::StopRequested)] {
        let id = run_id(&state.view.store, number);
        state.view.runs.push(Run {
            id: id.clone(),
            source: state.view.source.as_ref().unwrap().id.clone(),
            kind: "summary".into(),
            question: None,
            status,
            output: Default::default(),
            error: None,
            corrections: Vec::new(),
        });
        state.launches.insert(
            id.clone(),
            Launch::Fixture {
                input_digest: "e".repeat(64),
            },
        );
        state.starts.insert(format!("recovery-{number}"), id);
    }
    state.validate()?;
    Ok(state)
}

fn counts(history: &History) -> Result<(u64, u64, u64, u64, u64, u64)> {
    let read = history.db.begin_read()?;
    Ok((
        read.open_table(RUNS)?.len()?,
        read.open_table(LAUNCHES)?.len()?,
        read.open_table(STARTS)?.len()?,
        read.open_table(RUN_ORDER)?.len()?,
        read.open_table(ACTIVE_RUNS)?.len()?,
        read.open_table(ACTIONS)?.len()?,
    ))
}

fn run_launch_records(history: &History, id: &str) -> Result<(u64, u64, serde_json::Value)> {
    let read = history.db.begin_read()?;
    let run = read.open_table(RUNS)?;
    let launch = read.open_table(LAUNCHES)?;
    let run: Record<Run> = decode(run.get(id)?.ok_or("missing run")?.value())?;
    let launch: Record<Launch> = decode(launch.get(id)?.ok_or("missing launch")?.value())?;
    Ok((
        run.revision,
        launch.revision,
        serde_json::to_value(launch.value)?,
    ))
}

fn raw_bytes(
    history: &History,
    definition: redb::TableDefinition<&str, &[u8]>,
) -> Result<Vec<(String, Vec<u8>)>> {
    let read = history.db.begin_read()?;
    let table = read.open_table(definition)?;
    let mut records = Vec::new();
    for entry in table.iter()? {
        let (key, value) = entry?;
        records.push((key.value().to_owned(), value.value().to_vec()));
    }
    Ok(records)
}

fn raw_active(history: &History) -> Result<Vec<String>> {
    let read = history.db.begin_read()?;
    let table = read.open_table(ACTIVE_RUNS)?;
    let mut records = Vec::new();
    for entry in table.iter()? {
        let (key, _) = entry?;
        records.push(key.value().to_owned());
    }
    Ok(records)
}

fn raw_open_actions(history: &History) -> Result<Vec<(String, String)>> {
    let read = history.db.begin_read()?;
    let table = read.open_table(OPEN_ACTIONS)?;
    let mut records = Vec::new();
    for entry in table.iter()? {
        let (key, value) = entry?;
        records.push((key.value().to_owned(), value.value().to_owned()));
    }
    Ok(records)
}

fn raw_uncertain(history: &History) -> Result<Vec<(String, String)>> {
    let read = history.db.begin_read()?;
    let table = read.open_table(UNCERTAIN_RUNS)?;
    let mut records = Vec::new();
    for entry in table.iter()? {
        let (key, value) = entry?;
        records.push((key.value().to_owned(), value.value().to_owned()));
    }
    Ok(records)
}

fn interrupted_pending_or_bound_head(
    directory: &Directory,
    disk: &crate::store::Disk,
    file: &str,
    binding: crate::agent_model::Binding,
) -> Result<()> {
    let mut state = fixture::legacy();
    crate::recovery::interrupt(&mut state)?;
    state
        .workflows
        .get_mut("0123456789abcdef0123456789abcdef:030")
        .unwrap()
        .binding = binding;
    state.validate()?;
    let path = directory.0.join(file);
    let mut history = History::import_new(&path, &serde_json::to_vec(&state)?, disk.lease())?;
    let scope = history.selection()?;
    let id = "0123456789abcdef0123456789abcdef:09";
    let before_run = history.edit(|edit| edit.run(&scope, id))?;
    let before_records = run_launch_records(&history, id)?;
    history.edit(|edit| edit.recover(&scope))?;
    assert_eq!(
        serde_json::to_value(history.edit(|edit| edit.run(&scope, id))?)?,
        serde_json::to_value(before_run)?
    );
    assert_eq!(run_launch_records(&history, id)?, before_records);
    assert!(matches!(
        history.edit(|edit| edit.native_workflow(&scope, "0123456789abcdef0123456789abcdef:030"))?,
        super::edit::NativeWorkflow::Pi(crate::agent_model::Workflow {
            revision: 3,
            binding: crate::agent_model::Binding::ReleaseUnknown { .. },
            ..
        })
    ));
    Ok(())
}

fn snapshot(history: &mut History, scope: &Selection) -> Result<serde_json::Value> {
    let saved_profile = profile(history)?;
    let saved_counts = counts(history)?;
    let exact_records = [
        ("meta", serde_json::to_value(raw_bytes(history, META)?)?),
        ("runs", serde_json::to_value(raw_bytes(history, RUNS)?)?),
        (
            "launches",
            serde_json::to_value(raw_bytes(history, LAUNCHES)?)?,
        ),
        ("starts", serde_json::to_value(raw_bytes(history, STARTS)?)?),
        ("pi", serde_json::to_value(raw_bytes(history, PI)?)?),
        ("codex", serde_json::to_value(raw_bytes(history, CODEX)?)?),
        (
            "opencode",
            serde_json::to_value(raw_bytes(history, OPENCODE)?)?,
        ),
        (
            "actions",
            serde_json::to_value(raw_bytes(history, ACTIONS)?)?,
        ),
        ("active", serde_json::to_value(raw_active(history)?)?),
        (
            "open_actions",
            serde_json::to_value(raw_open_actions(history)?)?,
        ),
        ("uncertain", serde_json::to_value(raw_uncertain(history)?)?),
        (
            "native_targets",
            serde_json::to_value(raw_bytes(history, NATIVE_TARGETS)?)?,
        ),
    ];
    history.edit(|edit| {
        let runs = [9, 10, 11, 12, 13]
            .into_iter()
            .map(|n| edit.run(scope, &run_id(&scope.store, n)))
            .collect::<Result<Vec<_>>>()?;
        let workflows = ["030", "031", "032"]
            .into_iter()
            .map(|id| edit.native_workflow(scope, &format!("{}:{id}", scope.store)))
            .collect::<Result<Vec<_>>>()?;
        Ok(serde_json::json!({
            "profile": saved_profile,
            "runs": runs,
            "workflows": workflows,
            "dispatch": edit.browser_action(scope, &format!("{}:051", scope.store))?,
            "counts": saved_counts,
            "exact_records": exact_records,
        }))
    })
}

#[test]
fn saved_recovery_is_atomic_idempotent_and_preserves_workflow_heads() -> Result<()> {
    let directory = Directory::new()?;
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let path = directory.0.join("history.redb");
    let original = recovery_fixture()?;
    let original_bytes = serde_json::to_vec(&original)?;
    let mut oracle = original.clone();
    crate::recovery::interrupt(&mut oracle)?;
    let mut history = History::import_new(&path, &original_bytes, disk.lease())?;
    let scope = history.selection()?;
    let before = profile(&history)?;
    let before_settings = (before.providers.clone(), before.agents.clone());
    let before_counts = counts(&history)?;

    history.edit(|edit| edit.recover(&scope))?;
    let recovered_profile = profile(&history)?;
    assert_eq!(recovered_profile.sequence, before.sequence + 1);
    assert_eq!(recovered_profile.next, before.next);
    assert!(recovered_profile.providers.pending.is_none());
    assert!(
        recovered_profile
            .providers
            .retired
            .contains(&"b".repeat(32))
    );
    assert_eq!(
        serde_json::to_value(&recovered_profile.providers.slots)?,
        serde_json::to_value(&before_settings.0.slots)?
    );
    assert_eq!(
        serde_json::to_value(&recovered_profile.agents)?,
        serde_json::to_value(&before_settings.1)?
    );
    for (id, expected_revision, kind) in [
        ("030", 3, "Pi"),
        ("031", 2, "Codex"),
        ("032", 4, "OpenCode"),
    ] {
        let workflow =
            history.edit(|edit| edit.native_workflow(&scope, &format!("{}:{id}", scope.store)))?;
        let wire = serde_json::to_value(workflow)?;
        assert_eq!(wire[kind]["revision"], expected_revision);
        assert!(wire[kind]["binding"]["kind"] == "release_unknown");
    }
    assert_eq!(
        counts(&history)?,
        (
            before_counts.0,
            before_counts.1,
            before_counts.2,
            before_counts.3,
            0,
            before_counts.5
        )
    );
    for number in 9..=13 {
        let actual = history.edit(|edit| edit.run(&scope, &run_id(&scope.store, number)))?;
        assert_eq!(actual.status, Status::Interrupted);
        assert_eq!(
            serde_json::to_value(actual)?,
            serde_json::to_value(
                oracle
                    .view
                    .runs
                    .iter()
                    .find(|run| run.id.ends_with(&format!(":{number:02}")))
                    .unwrap()
                    .clone()
            )?
        );
    }
    for number in 1..=8 {
        assert_eq!(
            serde_json::to_value(
                history.edit(|edit| edit.run(&scope, &run_id(&scope.store, number)))?
            )?,
            serde_json::to_value(
                oracle
                    .view
                    .runs
                    .iter()
                    .find(|run| run.id.ends_with(&format!(":{number:02}")))
                    .unwrap()
                    .clone()
            )?
        );
    }
    assert_eq!(
        history
            .edit(|edit| edit.run(&scope, &run_id(&scope.store, 2)))?
            .source,
        "0123456789abcdef0123456789abcdef:017"
    );
    let dispatch =
        history.edit(|edit| edit.browser_action(&scope, &format!("{}:051", scope.store)))?;
    assert!(matches!(
        dispatch.state,
        crate::browser_action::Phase::Settled {
            outcome: crate::browser_action::Outcome::Uncertain { ref reason }
        } if reason == "native host restarted after dispatch"
    ));
    let after_first = profile(&history)?;
    history.edit(|edit| edit.recover(&scope))?;
    assert_eq!(profile(&history)?.sequence, after_first.sequence);
    assert_eq!(profile(&history)?.next, before.next);

    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    assert_eq!(profile(&history)?.sequence, after_first.sequence);
    let recovered = history.edit(|edit| Ok(edit.run(&scope, &run_id(&scope.store, 9))?))?;
    assert_eq!(recovered.status, Status::Interrupted);
    assert!(matches!(
        history.edit(|edit| edit.native_workflow(&scope, &format!("{}:031", scope.store)))?,
        super::edit::NativeWorkflow::Codex(crate::codex_model::Workflow {
            binding: crate::codex_model::Binding::ReleaseUnknown { .. },
            ..
        })
    ));

    // A pending action follows the same recovery path but records that dispatch never began.
    let mut predispatch = fixture::legacy();
    crate::recovery::interrupt(&mut predispatch)?;
    predispatch
        .view
        .runs
        .iter_mut()
        .find(|run| run.id.ends_with(":09"))
        .unwrap()
        .status = Status::Running;
    predispatch
        .workflows
        .get_mut("0123456789abcdef0123456789abcdef:030")
        .unwrap()
        .binding = crate::agent_model::Binding::Pending;
    predispatch
        .browser_actions
        .get_mut("0123456789abcdef0123456789abcdef:051")
        .unwrap()
        .state = crate::browser_action::Phase::Pending;
    predispatch
        .browser_actions
        .get_mut("0123456789abcdef0123456789abcdef:051")
        .unwrap()
        .revision = 1;
    predispatch.validate()?;
    let pre_path = directory.0.join("predispatch.redb");
    let mut pre_history =
        History::import_new(&pre_path, &serde_json::to_vec(&predispatch)?, disk.lease())?;
    let pre_scope = pre_history.selection()?;
    pre_history.edit(|edit| edit.recover(&pre_scope))?;
    let prevented = pre_history
        .edit(|edit| edit.browser_action(&pre_scope, "0123456789abcdef0123456789abcdef:051"))?;
    assert!(matches!(
        prevented.state,
        crate::browser_action::Phase::Settled {
            outcome: crate::browser_action::Outcome::Prevented { ref reason }
        } if reason == "native host restarted before dispatch"
    ));

    interrupted_pending_or_bound_head(
        &directory,
        &disk,
        "interrupted-pending.redb",
        crate::agent_model::Binding::Pending,
    )?;
    interrupted_pending_or_bound_head(
        &directory,
        &disk,
        "interrupted-bound.redb",
        crate::agent_model::Binding::Bound {
            id: "pi-session-1".into(),
            path: "/owned/sessions/pi-session-1.jsonl".into(),
        },
    )?;

    // An interrupted task remains unchanged when its already-completed newer workflow head exists.
    let mut headed = fixture::legacy();
    crate::recovery::interrupt(&mut headed)?;
    let workflow = "0123456789abcdef0123456789abcdef:030";
    let latest = "0123456789abcdef0123456789abcdef:12";
    headed.view.runs.push(Run {
        id: latest.into(),
        source: headed.workflows[workflow].source.id.clone(),
        kind: "agent".into(),
        question: None,
        status: Status::Complete,
        output: Default::default(),
        error: None,
        corrections: Vec::new(),
    });
    headed.launches.insert(
        latest.into(),
        Launch::Pi {
            input_digest: "f".repeat(64),
            workflow: workflow.into(),
            activity: Vec::new(),
        },
    );
    let pi = headed.workflows.get_mut(workflow).unwrap();
    pi.last_run = latest.into();
    pi.binding = crate::agent_model::Binding::Bound {
        id: "pi-session-1".into(),
        path: "/owned/sessions/pi-session-1.jsonl".into(),
    };
    headed.validate()?;
    let head_path = directory.0.join("completed-head.redb");
    let mut head_history =
        History::import_new(&head_path, &serde_json::to_vec(&headed)?, disk.lease())?;
    let head_scope = head_history.selection()?;
    head_history.edit(|edit| edit.recover(&head_scope))?;
    assert_eq!(
        head_history
            .edit(|edit| edit.run(&head_scope, latest))?
            .status,
        Status::Complete
    );
    assert_eq!(
        serde_json::to_value(
            head_history.edit(|edit| edit.native_workflow(&head_scope, workflow))?
        )?["Pi"]["last_run"],
        latest
    );
    assert_eq!(
        serde_json::to_value(
            head_history.edit(|edit| edit.native_workflow(&head_scope, workflow))?
        )?["Pi"]["binding"]["kind"],
        "bound"
    );

    // Sequence exhaustion occurs after staged writes and must roll the whole edit back.
    let rollback_path = directory.0.join("rollback.redb");
    let mut rollback = History::import_new(&rollback_path, &original_bytes, disk.lease())?;
    let rollback_scope = rollback.selection()?;
    let mut maxed = profile(&rollback)?;
    maxed.sequence = u64::MAX;
    {
        let mut write = rollback.db.begin_write()?;
        let mut metadata = write.open_table(META)?;
        metadata.insert("profile", encode(&maxed)?.as_slice())?;
        drop(metadata);
        write.commit()?;
    }
    let before_failure = snapshot(&mut rollback, &rollback_scope)?;
    assert!(rollback.edit(|edit| edit.recover(&rollback_scope)).is_err());
    assert_eq!(snapshot(&mut rollback, &rollback_scope)?, before_failure);
    Ok(())
}
