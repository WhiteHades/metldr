use super::{History, edit::Failure, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    browser_action::{Binding, Intent, Outcome, Phase, Proposal, Target},
    model::{Status, Store},
};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use std::{
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

fn navigate(
    state: &mut Store,
    run: &str,
    initial: &Target,
    document: &str,
    url: &str,
) -> Result<Target> {
    let binding = state
        .browser_actions
        .values()
        .filter(|action| action.proposal.run == run)
        .filter_map(|action| match &action.state {
            Phase::Settled {
                outcome: Outcome::Navigated { binding, .. },
            } => Some(binding),
            _ => None,
        })
        .max_by_key(|binding| binding.grant_revision)
        .cloned()
        .unwrap_or_else(|| Binding {
            tab: initial.tab,
            document: initial.document.clone(),
            target: "owned-target".into(),
            frame: "owned-frame".into(),
            session: "owned-session".into(),
            unique_context: format!("context-{run}"),
            grant_revision: 1,
        });
    let next = Binding {
        document: document.into(),
        unique_context: format!("next-{run}-{}", binding.grant_revision),
        grant_revision: binding.grant_revision + 1,
        ..binding.clone()
    };
    let id = state.id()?;
    state.browser_actions.insert(
        id.clone(),
        crate::browser_action::Action {
            id,
            revision: 4,
            proposal: Proposal {
                run: run.into(),
                tool_use_id: format!("navigate-{run}-{}", binding.grant_revision),
                binding,
                url: initial.url.clone(),
                intent: Intent::Navigate {
                    destination: url.into(),
                },
            },
            state: Phase::Settled {
                outcome: Outcome::Navigated {
                    binding: next,
                    url: url.into(),
                    loader: "owned-loader".into(),
                },
            },
        },
    );
    Ok(Target {
        tab: initial.tab,
        document: document.into(),
        url: url.into(),
    })
}

#[test]
fn imported_native_destinations_and_latest_runs_survive_reopen_and_reject_wrong_indexes()
-> Result<()> {
    let directory = Directory(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp").join(
        format!(
            "native-target-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ),
    ));
    fs::create_dir_all(&directory.0)?;
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let mut legacy = fixture::legacy();
    legacy.browser_actions.clear();
    let pi = legacy.view.runs[8].id.clone();
    legacy.view.runs[8].status = Status::Complete;
    let Launch::Pi {
        workflow: pi_workflow,
        ..
    } = legacy.launches[&pi].clone()
    else {
        return Err("Pi fixture changed".into());
    };
    let pi_initial = legacy.native_browser_target(&pi)?;
    let origin = reqwest::Url::parse(&pi_initial.url)?
        .origin()
        .ascii_serialization();
    let decoy = format!("{origin}/decoy");
    let redirect = format!("{origin}/redirect");
    let first = navigate(&mut legacy, &pi, &pi_initial, "pi-second-document", &decoy)?;
    let mut last = pi.clone();
    for n in 0..2 {
        let mut run = legacy.view.runs[8].clone();
        run.id = legacy.id()?;
        last = run.id.clone();
        legacy
            .launches
            .insert(last.clone(), legacy.launches[&pi].clone());
        legacy.view.runs.push(run);
        if n == 0 {
            navigate(&mut legacy, &last, &first, "pi-third-document", &redirect)?;
        }
    }
    // The old Pi validator accepts an earlier last_run; preserve it and index actual creation order.
    assert_eq!(legacy.workflows[&pi_workflow].last_run, pi);
    let codex = legacy.view.runs[9].id.clone();
    let codex_initial = legacy.native_browser_target(&codex)?;
    let codex_middle = navigate(
        &mut legacy,
        &codex,
        &codex_initial,
        "codex-second-document",
        &decoy,
    )?;
    let codex_final = navigate(
        &mut legacy,
        &codex,
        &codex_middle,
        "codex-third-document",
        &redirect,
    )?;
    legacy.seal_outputs()?;
    legacy.validate()?;
    let original = serde_json::to_vec(&legacy)?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &original, disk.lease())?;
    let selection = history.selection()?;
    let unchanged = {
        let read = history.db.begin_read()?;
        assert_eq!(read.open_table(WORKFLOW_RUNS)?.len()?, 5);
        assert_eq!(read.open_table(NATIVE_TARGETS)?.len()?, 5);
        assert_eq!(
            read.open_table(META)?
                .get("legacy_snapshot")?
                .unwrap()
                .value(),
            original
        );
        read.open_table(META)?
            .get("profile")?
            .unwrap()
            .value()
            .to_vec()
    };
    for run in legacy.view.runs.iter().filter(|run| run.kind == "agent") {
        assert_eq!(
            history.edit(|edit| edit.native_initial_target(&selection, &run.id))?,
            legacy.native_browser_target(&run.id)?
        );
    }
    let (latest, destination) =
        history.edit(|edit| edit.native_latest(&selection, &pi_workflow))?;
    assert_eq!(latest, last);
    assert_eq!(destination.document, "pi-third-document");
    assert_eq!(destination.url, redirect);
    assert_eq!(
        history.edit(|edit| edit.native_initial_target(&selection, &pi))?,
        pi_initial
    );
    let Launch::Codex { workflow, .. } = &legacy.launches[&codex] else {
        return Err("Codex fixture changed".into());
    };
    assert_eq!(
        history.edit(|edit| edit.native_latest(&selection, workflow))?,
        (codex.clone(), codex_final)
    );
    let codex_workflow = workflow.clone();
    let open = &legacy.view.runs[10].id;
    let Launch::OpenCode { workflow, .. } = &legacy.launches[open] else {
        return Err("OpenCode fixture changed".into());
    };
    assert_eq!(
        history
            .edit(|edit| edit.native_latest(&selection, workflow))?
            .0,
        *open
    );
    let unknown = history
        .edit(|edit| edit.native_latest(&selection, "unknown-workflow"))
        .unwrap_err();
    assert!(unknown.downcast_ref::<Failure>().is_none());
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    assert_eq!(
        history
            .edit(|edit| edit.native_latest(&selection, &pi_workflow))?
            .0,
        last
    );
    for (run, workflow) in [(&codex, &codex_workflow), (&last, &pi_workflow)] {
        let original = {
            let read = history.db.begin_read()?;
            read.open_table(NATIVE_TARGETS)?
                .get(run.as_str())?
                .unwrap()
                .value()
                .to_vec()
        };
        let mut changed: Record<NativeTarget> = decode(&original)?;
        changed.value.current.url = "https://example.test/wrong-current".into();
        {
            let write = history.db.begin_write()?;
            write
                .open_table(NATIVE_TARGETS)?
                .insert(run.as_str(), encode(&changed)?.as_slice())?;
            write.commit()?;
        }
        assert!(
            history
                .edit(|edit| edit.native_latest(&selection, workflow))
                .unwrap_err()
                .downcast_ref::<Failure>()
                .is_some()
        );
        let write = history.db.begin_write()?;
        write
            .open_table(NATIVE_TARGETS)?
            .insert(run.as_str(), original.as_slice())?;
        write.commit()?;
    }
    {
        let original = {
            let read = history.db.begin_read()?;
            read.open_table(NATIVE_TARGETS)?
                .get(codex.as_str())?
                .unwrap()
                .value()
                .to_vec()
        };
        let mut changed: Record<NativeTarget> = decode(&original)?;
        assert!(changed.value.navigation.is_some());
        let earlier = legacy
            .browser_actions
            .iter()
            .find(|(_, action)| {
                action.proposal.run == codex && action.proposal.binding.grant_revision == 1
            })
            .unwrap()
            .0
            .clone();
        for (navigation, current) in [
            (None, changed.value.initial.clone()),
            (Some(earlier), codex_middle),
        ] {
            changed.value.navigation = navigation;
            changed.value.current = current;
            let write = history.db.begin_write()?;
            write
                .open_table(NATIVE_TARGETS)?
                .insert(codex.as_str(), encode(&changed)?.as_slice())?;
            write.commit()?;
            assert!(
                history
                    .edit(|edit| edit.native_latest(&selection, &codex_workflow))
                    .unwrap_err()
                    .downcast_ref::<Failure>()
                    .is_some()
            );
        }
        let write = history.db.begin_write()?;
        write
            .open_table(NATIVE_TARGETS)?
            .insert(codex.as_str(), original.as_slice())?;
        write.commit()?;
    }
    {
        let write = history.db.begin_write()?;
        let head = write
            .open_table(NAVIGATION_HEADS)?
            .remove(codex.as_str())?
            .unwrap()
            .value()
            .to_owned();
        write.commit()?;
        assert!(
            history
                .edit(|edit| edit.native_latest(&selection, &codex_workflow))
                .unwrap_err()
                .downcast_ref::<Failure>()
                .is_some()
        );
        let write = history.db.begin_write()?;
        write
            .open_table(NAVIGATION_HEADS)?
            .insert(codex.as_str(), head.as_str())?;
        write.commit()?;
    }
    {
        let write = history.db.begin_write()?;
        write
            .open_table(WORKFLOW_RUNS)?
            .insert((pi_workflow.as_str(), u64::MAX), last.as_str())?;
        write.commit()?;
    }
    assert!(
        history
            .edit(|edit| edit.native_latest(&selection, &pi_workflow))
            .unwrap_err()
            .downcast_ref::<Failure>()
            .is_some()
    );
    {
        let write = history.db.begin_write()?;
        write
            .open_table(WORKFLOW_RUNS)?
            .remove((pi_workflow.as_str(), u64::MAX))?;
        let mut table = write.open_table(NATIVE_TARGETS)?;
        let mut saved: Record<NativeTarget> = decode(table.get(last.as_str())?.unwrap().value())?;
        saved.value.initial.url = "https://example.test/wrong-target".into();
        table.insert(last.as_str(), encode(&saved)?.as_slice())?;
        drop(table);
        write.commit()?;
    }
    assert!(
        history
            .edit(|edit| edit.native_initial_target(&selection, &last))
            .unwrap_err()
            .downcast_ref::<Failure>()
            .is_some()
    );
    assert_eq!(
        history
            .db
            .begin_read()?
            .open_table(META)?
            .get("profile")?
            .unwrap()
            .value(),
        unchanged
    );
    Ok(())
}
