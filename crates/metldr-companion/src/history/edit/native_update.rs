use super::{Edit, NativeTask, Selection, native_task::workflow_id, storage};
use crate::{
    Result,
    agent_model::{Activity, Launch},
    history::schema::*,
    model::Status,
};

impl Edit<'_> {
    pub(crate) fn put_native(&mut self, scope: &Selection, mut task: NativeTask) -> Result<()> {
        let result = (|| {
            self.require_selection(scope)?;
            if task.identity != task.run.id || task.scope != *scope {
                return Err("native task draft identity or scope changed".into());
            }
            self.require_browser_finalized(scope, &task.run)?;
            let before = self.native_task(scope, &task.run.id)?;
            crate::native_correction::validate_update(&before.run, &task.run)?;
            crate::native_correction::validate_route(&task.run, &task.launch)?;
            if task.versions != before.versions {
                return Err("native task draft is stale".into());
            }
            if task.run.source != before.run.source
                || task.run.kind != before.run.kind
                || task.run.question != before.run.question
                || workflow_id(&task.launch)? != workflow_id(&before.launch)?
                || task.launch.input_digest() != before.launch.input_digest()
                || std::mem::discriminant(&task.launch) != std::mem::discriminant(&before.launch)
            {
                return Err("native task identity or route changed".into());
            }
            before.workflow.validate_update(&task.workflow)?;
            validate_launch(&before.launch, &task.launch)?;
            let selected = matches!(
                (&before.launch, &task.launch),
                (
                    Launch::Codex {
                        selection: None,
                        ..
                    },
                    Launch::Codex {
                        selection: Some(_),
                        ..
                    }
                ) | (
                    Launch::OpenCode {
                        selection: None,
                        ..
                    },
                    Launch::OpenCode {
                        selection: Some(_),
                        ..
                    }
                )
            );
            if ((before.workflow.pending() && task.workflow.bound()) || selected)
                && (!task.workflow.bound()
                    || before.workflow.revision().checked_add(1) != Some(task.workflow.revision()))
            {
                return Err("native binding acknowledgement has no matching revision".into());
            }
            if before.run.output.is_native() && !task.run.output.is_native() {
                return Err("native output mode changed".into());
            }
            if before.run.status.terminal() {
                if before.run.status != Status::Interrupted
                    || serde_json::to_vec(&before.run)? != serde_json::to_vec(&task.run)?
                    || serde_json::to_vec(&before.launch)? != serde_json::to_vec(&task.launch)?
                    || task.workflow.revision() != before.workflow.revision()
                    || task.workflow.pending()
                    || task.workflow.bound()
                {
                    return Err("completed native task cannot be rewritten".into());
                }
            } else if before.run.status == Status::StopRequested
                && task.run.status == Status::Running
            {
                return Err("stopped native task cannot resume".into());
            }
            if task.run.status.terminal() {
                task.run
                    .output
                    .end(if task.run.status == Status::Complete {
                        metldr_conversation::Ending::Complete
                    } else {
                        metldr_conversation::Ending::Interrupted
                    })?;
            }
            self.native_state(&task)?;
            let run_changed = serde_json::to_vec(&before.run)? != serde_json::to_vec(&task.run)?;
            let launch_changed =
                serde_json::to_vec(&before.launch)? != serde_json::to_vec(&task.launch)?;
            let workflow_changed =
                serde_json::to_vec(&before.workflow)? != serde_json::to_vec(&task.workflow)?;
            if !run_changed && !launch_changed && !workflow_changed {
                return Ok(());
            }
            storage(|| {
                if run_changed {
                    self.write.open_table(RUNS)?.insert(
                        task.run.id.as_str(),
                        encode(&Record {
                            conversation: scope.conversation.clone(),
                            revision: before.versions[0]
                                .checked_add(1)
                                .ok_or("native run revision exhausted")?,
                            value: &task.run,
                        })?
                        .as_slice(),
                    )?;
                }
                if launch_changed {
                    self.write.open_table(LAUNCHES)?.insert(
                        task.run.id.as_str(),
                        encode(&Record {
                            conversation: scope.conversation.clone(),
                            revision: before.versions[1]
                                .checked_add(1)
                                .ok_or("native launch revision exhausted")?,
                            value: &task.launch,
                        })?
                        .as_slice(),
                    )?;
                }
                if workflow_changed {
                    self.save_native_workflow(
                        scope,
                        workflow_id(&task.launch)?,
                        &task.workflow,
                        before.versions[2]
                            .checked_add(1)
                            .ok_or("native workflow revision exhausted")?,
                    )?;
                }
                if task.run.status.terminal() {
                    self.write
                        .open_table(ACTIVE_RUNS)?
                        .remove(task.run.id.as_str())?;
                }
                Ok(())
            })?;
            self.changed = true;
            self.native_task(scope, &task.run.id)?;
            Ok(())
        })();
        self.checked(result)
    }
}

fn validate_launch(before: &Launch, after: &Launch) -> Result<()> {
    fn prefix(before: &[Activity], after: &[Activity]) -> Result<()> {
        if after.len() < before.len() {
            return Err("native activity history changed".into());
        }
        let mut expected = before.to_vec();
        for (old, new) in before.iter().zip(after) {
            if serde_json::to_vec(old)? != serde_json::to_vec(new)?
                && let Some(call) = &new.call
            {
                crate::tool_report::coalesce(&mut expected, call.clone());
            }
        }
        if serde_json::to_vec(&expected)? != serde_json::to_vec(&after[..before.len()])? {
            return Err("native activity history changed".into());
        }
        Ok(())
    }
    match (before, after) {
        (Launch::Pi { activity: a, .. }, Launch::Pi { activity: b, .. }) => prefix(a, b),
        (
            Launch::Codex {
                activity: a,
                selection: old,
                ..
            },
            Launch::Codex {
                activity: b,
                selection: new,
                ..
            },
        ) => {
            if old.is_some() && serde_json::to_vec(old)? != serde_json::to_vec(new)? {
                return Err("Codex launch selection changed".into());
            }
            prefix(a, b)
        }
        (
            Launch::OpenCode {
                activity: a,
                selection: old,
                ..
            },
            Launch::OpenCode {
                activity: b,
                selection: new,
                ..
            },
        ) => {
            if old.is_some() && serde_json::to_vec(old)? != serde_json::to_vec(new)? {
                return Err("OpenCode launch selection changed".into());
            }
            prefix(a, b)
        }
        _ => Err("native launch kind changed".into()),
    }
}
