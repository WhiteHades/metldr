use crate::{
    Result,
    agent::{self, Job, Outcome},
    agent_model::{Activity, Binding, Launch},
    flow::{Operation, Owner},
    model::{ConversationAction, Status},
};
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
};

impl Owner {
    pub(super) fn duplicate(
        &mut self,
        scope: &crate::history::Selection,
        id: &str,
        input_digest: &str,
    ) -> Result<bool> {
        let Some(started) = self
            .record_history
            .edit(|edit| edit.duplicate(scope, id, input_digest))?
        else {
            return Ok(false);
        };
        let run = started.run;
        self.emit(self.started_reply(id, scope, &run)?)?;
        Ok(true)
    }

    pub(super) fn pi_start(
        &mut self,
        id: &str,
        scope: &crate::history::Selection,
        command: ConversationAction,
    ) -> Result<()> {
        self.prepare_agent(crate::agent_settings::Kind::Pi)?;
        let profile = self
            .pi_profile
            .clone()
            .ok_or("no trusted Pi profile configured")?;
        let Some(admission) = self.admit_native_start(
            id,
            scope,
            &command,
            crate::agent_settings::NativeProfile::Pi(profile),
        )?
        else {
            return Ok(());
        };
        let crate::native_start::Admission {
            run,
            workflow,
            value,
            text,
            target,
        } = admission;
        let crate::history::NativeWorkflow::Pi(w) = value else {
            return Err("Pi admission workflow mismatch".into());
        };
        let job = Job {
            run: run.clone(),
            workflow,
            profile: w.profile.clone(),
            binding: w.binding.clone(),
            source: w.source.clone(),
            text,
            target,
            registration_dir: self.disk.directory.join("pi-browser"),
        };
        let (slot, events) = crate::browser_io::Slot::new(job.target.tab);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let tx = self.tx.clone();
        let (correction, corrections) = crate::native_correction::channel(&run, &tx);
        let worker =
            thread::spawn(move || agent::execute(job, worker_stop, tx, events, corrections));
        self.operations.insert(
            run.clone(),
            Operation {
                stop,
                worker,
                correction: Some(correction),
                browser_io: Some(Box::new(slot)),
            },
        );
        self.emit(self.started_reply(id, scope, &run)?)
    }

    pub(super) fn pi_bound(
        &mut self,
        run: &str,
        workflow: &str,
        binding: Binding,
        ack: mpsc::SyncSender<std::result::Result<bool, String>>,
    ) -> Result<()> {
        let result = self.native_change(run, |_, launch, native| {
            let crate::history::NativeWorkflow::Pi(w) = native else {
                return Err("Pi binding workflow mismatch".into());
            };
            let Launch::Pi { workflow: bound, .. } = launch else {
                return Err("Pi binding has no launch".into());
            };
            if bound.as_str() != workflow || w.last_run != run {
                return Err("Pi binding workflow mismatch".into());
            }
            match &w.binding {
                Binding::Bound { id: old_id, path: old_path }
                    if !matches!(&binding, Binding::Bound { id, path } if id == old_id && path == old_path) => {
                    return Err("Pi session identity changed".into());
                }
                Binding::ReleaseUnknown { .. } => return Err("Pi native ownership is unknown".into()),
                _ => (),
            }
            if !matches!(binding, Binding::Bound { .. }) {
                return Err("Pi did not return a real binding".into());
            }
            w.binding = binding;
            w.revision = w.revision.checked_add(1).ok_or("Pi workflow revision exhausted")?;
            Ok(true)
        });
        crate::flow::acknowledge(ack, result)
    }

    pub(super) fn pi_activity(&mut self, run: &str, activity: Activity) -> Result<()> {
        self.native_change(run, |task, launch, _| {
            let Launch::Pi {
                activity: events, ..
            } = launch
            else {
                return Err("Pi activity has no launch".into());
            };
            if events.len() >= 16 || !activity.valid() {
                return Err("invalid Pi activity".into());
            }
            if let Some(call) = &activity.call {
                task.output
                    .observe_tool(metldr_conversation::ToolId::new(call.id.clone())?)?;
            }
            events.push(activity);
            Ok(true)
        })?;
        Ok(())
    }

    pub(super) fn agent_settled(
        &mut self,
        run: &str,
        result: Outcome,
        disconnected: bool,
        released: bool,
    ) -> Result<()> {
        let mut operation = self.operations.remove(run).ok_or_else(|| {
            if self.operations.is_empty() {
                "agent settlement without operation"
            } else {
                "agent settlement run mismatch"
            }
        })?;
        drop(operation.browser_io.take());
        operation
            .worker
            .join()
            .map_err(|_| "agent worker panicked")?;
        let pending = self.detach_inputs(run);
        let saved = self.record_execution(run, |edit, execution| {
            let uncertain = edit.finish_browser(execution, run)?;
            let mut task = edit.native_task(execution, run)?;
            crate::native_correction::finish(
                &mut task.run,
                "The run ended before the correction was sent.",
                "The run ended without a correction reply. Delivery is unknown.",
            )?;
            let correction_uncertain = task.run.corrections.iter().any(|receipt| {
                matches!(
                    receipt.state,
                    crate::native_correction::Phase::Uncertain { .. }
                )
            });
            if !released || disconnected || uncertain || correction_uncertain {
                match &mut task.workflow {
                    crate::history::NativeWorkflow::Pi(w) => w.binding.block_recovery(),
                    crate::history::NativeWorkflow::Codex(w) => w.binding.block_recovery(),
                    crate::history::NativeWorkflow::OpenCode(w) => w.binding.block_recovery(),
                }
            }
            task.run.status = if disconnected {
                Status::Interrupted
            } else {
                match &result {
                    Outcome::Complete => Status::Complete,
                    Outcome::Cancelled => Status::Cancelled,
                    Outcome::Failed(_) => Status::Failed,
                }
            };
            if let Outcome::Failed(error) = &result {
                task.run.error = Some(error.chars().take(200).collect());
            }
            edit.put_native(execution, task)?;
            Ok(())
        });
        if let Err(error) = saved {
            self.restore_inputs(pending);
            return Err(error);
        }
        self.cancel_browser_approval(run);
        pending.cancel();
        Ok(())
    }
}
