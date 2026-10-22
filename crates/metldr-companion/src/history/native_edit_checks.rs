use super::{History, edit::NativeWorkflow, fixture, schema::*};
use crate::{
    Result,
    agent_model::{Activity, Binding as PiBinding, Launch},
    model::{Run, Status},
};
use metldr_conversation::{Change, Content, Ending, MessageKey};
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

#[test]
fn native_admission_binding_continuation_and_terminal_output_persist() -> Result<()> {
    let directory = Directory(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".tmp").join(
        format!(
            "native-edit-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ),
    ));
    fs::create_dir_all(&directory.0)?;
    let disk = crate::store::Disk::lock(directory.0.clone())?;
    let mut legacy = fixture::legacy();
    crate::recovery::interrupt(&mut legacy)?;
    legacy.validate()?;
    let original = serde_json::to_vec(&legacy)?;
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &original, disk.lease())?;
    let selection = history.selection()?;
    let original_source = history.edit(|edit| {
        edit.selected_source(&selection)?
            .ok_or("missing current source".into())
    })?;

    let mut cases = vec![
        NativeWorkflow::Pi(
            legacy
                .workflows
                .remove("0123456789abcdef0123456789abcdef:030")
                .unwrap(),
        ),
        NativeWorkflow::Codex(
            legacy
                .codex_workflows
                .remove("0123456789abcdef0123456789abcdef:031")
                .unwrap(),
        ),
        NativeWorkflow::OpenCode(
            legacy
                .opencode_workflows
                .remove("0123456789abcdef0123456789abcdef:032")
                .unwrap(),
        ),
    ];

    let rollback: Result<()> = history.edit(|edit| {
        let workflow_id = edit.id()?;
        let run_id = edit.id()?;
        let source = edit
            .selected_source(&selection)?
            .ok_or("missing current source")?;
        let mut workflow = match &cases[0] {
            NativeWorkflow::Pi(workflow) => NativeWorkflow::Pi(workflow.clone()),
            _ => unreachable!(),
        };
        if let NativeWorkflow::Pi(workflow) = &mut workflow {
            workflow.last_run = run_id.clone();
            workflow.revision = 2;
            workflow.source = source.clone();
            workflow.binding = PiBinding::Pending;
        }
        edit.admit_native(
            &selection,
            "native-rollback",
            Run {
                id: run_id,
                source: source.id,
                kind: "agent".into(),
                question: Some("acceptance question".into()),
                status: Status::Running,
                output: Content::native(),
                error: None,
            },
            Launch::Pi {
                input_digest: "a".repeat(64),
                workflow: workflow_id,
                activity: Vec::<Activity>::new(),
            },
            workflow,
        )?;
        Err("deliberate late rollback".into())
    });
    assert_eq!(
        rollback.unwrap_err().to_string(),
        "deliberate late rollback"
    );

    let mut saved = Vec::new();
    for (index, mut workflow) in cases.drain(..).enumerate() {
        let (workflow_id, run_id) = history.edit(|edit| {
            let workflow_id = edit.id()?;
            let run_id = edit.id()?;
            let source = edit
                .selected_source(&selection)?
                .ok_or("missing current source")?;
            match &mut workflow {
                NativeWorkflow::Pi(value) => {
                    value.last_run = run_id.clone();
                    value.revision = 2;
                    value.source = source.clone();
                    value.binding = PiBinding::Pending;
                }
                NativeWorkflow::Codex(value) => {
                    value.last_run = run_id.clone();
                    value.revision = 2;
                    value.source = source.clone();
                    value.binding = crate::codex_model::Binding::Pending;
                }
                NativeWorkflow::OpenCode(value) => {
                    value.last_run = run_id.clone();
                    value.revision = 2;
                    value.source = source.clone();
                    value.binding = crate::opencode_model::Binding::Pending;
                }
            }
            let launch = match &workflow {
                NativeWorkflow::Pi(_) => Launch::Pi {
                    input_digest: "a".repeat(64),
                    workflow: workflow_id.clone(),
                    activity: Vec::new(),
                },
                NativeWorkflow::Codex(_) => Launch::Codex {
                    input_digest: "b".repeat(64),
                    workflow: workflow_id.clone(),
                    selection: None,
                    activity: Vec::new(),
                },
                NativeWorkflow::OpenCode(_) => Launch::OpenCode {
                    input_digest: "c".repeat(64),
                    workflow: workflow_id.clone(),
                    selection: None,
                    activity: Vec::new(),
                },
            };
            edit.admit_native(
                &selection,
                &format!("native-acceptance-{index}"),
                Run {
                    id: run_id.clone(),
                    source: source.id,
                    kind: "agent".into(),
                    question: Some("acceptance question".into()),
                    status: Status::Running,
                    output: Content::native(),
                    error: None,
                },
                launch,
                workflow,
            )?;
            Ok((workflow_id, run_id))
        })?;
        let workflow_number = workflow_id.rsplit(':').next().unwrap().parse::<u64>()?;
        let run_number = run_id.rsplit(':').next().unwrap().parse::<u64>()?;
        assert_eq!(run_number, workflow_number + 1);

        let stale = history.edit(|edit| edit.native_task(&selection, &run_id))?;
        history.edit(|edit| {
            let mut task = edit.native_task(&selection, &run_id)?;
            match &mut task.launch {
                Launch::Codex { selection, .. } => {
                    if let Launch::Codex {
                        selection: saved, ..
                    } = &legacy.launches[&legacy.view.runs[9].id]
                    {
                        *selection = saved.clone();
                    }
                }
                Launch::OpenCode { selection, .. } => {
                    if let Launch::OpenCode {
                        selection: saved, ..
                    } = &legacy.launches[&legacy.view.runs[10].id]
                    {
                        *selection = saved.clone();
                    }
                }
                _ => (),
            }
            match &mut task.workflow {
                NativeWorkflow::Pi(value) => {
                    value.binding = PiBinding::Bound {
                        id: "pi-session-1".into(),
                        path: "/owned/sessions/pi-session-1.jsonl".into(),
                    };
                    value.revision += 1;
                }
                NativeWorkflow::Codex(value) => {
                    let Launch::Codex {
                        selection: Some(selection),
                        ..
                    } = &task.launch
                    else {
                        return Err("Codex selection missing".into());
                    };
                    value.binding = crate::codex_model::Binding::Bound {
                        identity: crate::codex_model::Identity {
                            thread: "thread-fixture-1".into(),
                            session: "session-fixture-1".into(),
                        },
                    };
                    assert_eq!(selection.provider, "fixture-provider");
                    value.revision += 1;
                }
                NativeWorkflow::OpenCode(value) => {
                    let Launch::OpenCode {
                        selection: Some(selection),
                        ..
                    } = &task.launch
                    else {
                        return Err("OpenCode selection missing".into());
                    };
                    value.binding = crate::opencode_model::Binding::Bound {
                        identity: crate::opencode_model::Identity {
                            session: "session_fixture_1".into(),
                            selection: selection.clone(),
                        },
                    };
                    value.revision += 1;
                }
            }
            edit.put_native(&selection, task)
        })?;
        assert!(
            history
                .edit(|edit| edit.put_native(&selection, stale))
                .is_err()
        );

        history.edit(|edit| {
            let mut task = edit.native_task(&selection, &run_id)?;
            task.run.output.change(Change::Final {
                key: MessageKey::native(format!("native-{index}")).unwrap(),
                text: format!("native output {index}"),
            })?;
            task.run.output.end(Ending::Complete)?;
            task.run.status = Status::Complete;
            edit.put_native(&selection, task)
        })?;
        let completed = history.edit(|edit| edit.native_task(&selection, &run_id))?;
        saved.push((workflow_id, run_id, completed));
    }

    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    for (index, (workflow_id, run_id, task)) in saved.iter_mut().enumerate() {
        assert_eq!(task.run.status, Status::Complete);
        assert_eq!(task.run.output.text(), format!("native output {index}"));
        assert_eq!(task.run.question.as_deref(), Some("acceptance question"));
        assert_eq!(task.run.source, original_source.id);
        match &task.workflow {
            NativeWorkflow::Pi(value) => {
                assert_eq!(value.revision, 3);
                assert!(matches!(&value.binding, PiBinding::Bound { id, path }
                    if id == "pi-session-1" && path == "/owned/sessions/pi-session-1.jsonl"));
            }
            NativeWorkflow::Codex(value) => {
                assert_eq!(value.revision, 3);
                assert!(
                    matches!(&value.binding, crate::codex_model::Binding::Bound { identity }
                    if identity.thread == "thread-fixture-1" && identity.session == "session-fixture-1")
                );
            }
            NativeWorkflow::OpenCode(value) => {
                assert_eq!(value.revision, 3);
                assert!(
                    matches!(&value.binding, crate::opencode_model::Binding::Bound { identity }
                    if identity.session == "session_fixture_1")
                );
            }
        }
        let (latest, target) = history.edit(|edit| edit.native_latest(&selection, workflow_id))?;
        assert_eq!(&latest, run_id);
        assert_eq!(
            target,
            crate::browser_action::Target::from(&original_source.page)
        );
        let project = history.edit(|edit| edit.project(&selection, Some(run_id)))?;
        assert_eq!(project.view.runs.len(), 1);
        assert_eq!(project.view.runs[0].id, *run_id);
        assert_eq!(
            project.workflows.len()
                + project.codex_workflows.len()
                + project.opencode_workflows.len(),
            1
        );

        let mut rewrite = history.edit(|edit| edit.native_task(&selection, run_id))?;
        rewrite.run.status = Status::Failed;
        assert!(
            history
                .edit(|edit| edit.put_native(&selection, rewrite))
                .is_err()
        );
    }
    for (index, (workflow_id, previous_id, previous)) in saved.iter().enumerate() {
        let next_id = history.edit(|edit| {
            let run_id = edit.id()?;
            let mut workflow = previous.workflow.clone();
            match &mut workflow {
                NativeWorkflow::Pi(w) => {
                    w.last_run = run_id.clone();
                    w.revision += 1;
                }
                NativeWorkflow::Codex(w) => {
                    w.last_run = run_id.clone();
                    w.revision += 1;
                }
                NativeWorkflow::OpenCode(w) => {
                    w.last_run = run_id.clone();
                    w.revision += 1;
                }
            }
            let mut launch = previous.launch.clone();
            match &mut launch {
                Launch::Pi { activity, .. } => activity.clear(),
                Launch::Codex {
                    activity,
                    selection,
                    ..
                } => {
                    activity.clear();
                    *selection = None;
                }
                Launch::OpenCode {
                    activity,
                    selection,
                    ..
                } => {
                    activity.clear();
                    *selection = None;
                }
                _ => unreachable!(),
            }
            edit.admit_native(
                &selection,
                &format!("native-followup-{index}"),
                Run {
                    id: run_id.clone(),
                    source: original_source.id.clone(),
                    kind: "agent".into(),
                    question: Some("continued question".into()),
                    status: Status::Running,
                    output: Content::native(),
                    error: None,
                },
                launch,
                workflow,
            )?;
            Ok(run_id)
        })?;
        history.edit(|edit| {
            let mut task = edit.native_task(&selection, &next_id)?;
            match (&mut task.launch, &previous.launch) {
                (Launch::Codex { selection, .. }, Launch::Codex { selection: old, .. }) => {
                    *selection = old.clone()
                }
                (Launch::OpenCode { selection, .. }, Launch::OpenCode { selection: old, .. }) => {
                    *selection = old.clone()
                }
                _ => (),
            }
            match &mut task.workflow {
                NativeWorkflow::Pi(w) => w.revision += 1,
                NativeWorkflow::Codex(w) => w.revision += 1,
                NativeWorkflow::OpenCode(w) => w.revision += 1,
            }
            edit.put_native(&selection, task)
        })?;
        if index == 0 {
            history.edit(|edit| {
                let mut run = edit.run(&selection, &next_id)?;
                run.status = Status::StopRequested;
                edit.put_run(&selection, run)
            })?;
            assert_eq!(
                history.edit(|edit| edit.run(&selection, &next_id))?.status,
                Status::StopRequested
            );
            assert!(
                history
                    .edit(|edit| {
                        let mut run = edit.run(&selection, &next_id)?;
                        run.status = Status::Running;
                        edit.put_run(&selection, run)
                    })
                    .is_err()
            );
        }
        history.edit(|edit| {
            let mut run = edit.run(&selection, &next_id)?;
            run.output.change(Change::Final {
                key: MessageKey::native("followup".into()).unwrap(),
                text: "continued output".into(),
            })?;
            run.status = if index == 0 {
                Status::Interrupted
            } else {
                Status::Complete
            };
            edit.put_run(&selection, run)
        })?;
        if index == 0 {
            history.edit(|edit| {
                let mut task = edit.native_task(&selection, &next_id)?;
                let NativeWorkflow::Pi(workflow) = &mut task.workflow else {
                    unreachable!()
                };
                assert_eq!(workflow.revision, 5);
                workflow.binding.block_recovery();
                edit.put_native(&selection, task)
            })?;
            let blocked = history.edit(|edit| edit.native_task(&selection, &next_id))?;
            let NativeWorkflow::Pi(workflow) = &blocked.workflow else {
                unreachable!()
            };
            assert_eq!(workflow.revision, 5);
            assert!(matches!(workflow.binding, PiBinding::ReleaseUnknown { .. }));
            assert!(
                history
                    .edit(|edit| {
                        let next = edit.id()?;
                        let mut workflow = workflow.clone();
                        workflow.last_run = next.clone();
                        workflow.revision += 1;
                        edit.admit_native(
                            &selection,
                            "blocked-continuation",
                            Run {
                                id: next,
                                source: original_source.id.clone(),
                                kind: "agent".into(),
                                question: Some("blocked".into()),
                                status: Status::Running,
                                output: Content::native(),
                                error: None,
                            },
                            Launch::Pi {
                                input_digest: "f".repeat(64),
                                workflow: workflow_id.clone(),
                                activity: Vec::new(),
                            },
                            NativeWorkflow::Pi(workflow),
                        )
                    })
                    .is_err()
            );
        }
        let (latest, target) = history.edit(|edit| edit.native_latest(&selection, workflow_id))?;
        assert_eq!(latest, next_id);
        assert_ne!(&latest, previous_id);
        assert_eq!(
            target,
            crate::browser_action::Target::from(&original_source.page)
        );
        assert_eq!(
            history
                .edit(|edit| edit.run(&selection, previous_id))?
                .output
                .text(),
            format!("native output {index}")
        );
    }
    drop(history);
    let mut history = History::open(&path, disk.lease())?;
    for (workflow_id, old_id, _) in &saved {
        let (latest, _) = history.edit(|edit| edit.native_latest(&selection, workflow_id))?;
        assert_ne!(&latest, old_id);
        assert_eq!(
            history
                .edit(|edit| edit.run(&selection, &latest))?
                .output
                .text(),
            "continued output"
        );
    }
    let read = history.db.begin_read()?;
    assert_eq!(read.open_table(RUNS)?.len()?, 17);
    assert_eq!(
        read.open_table(META)?
            .get("legacy_snapshot")?
            .unwrap()
            .value(),
        original
    );
    Ok(())
}
