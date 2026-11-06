use crate::{
    Result,
    agent_model::{Activity, Launch},
    flow::{Operation, Owner},
    model::ConversationAction,
};
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
};

impl Owner {
    pub(super) fn opencode_start(
        &mut self,
        request: &str,
        scope: &crate::history::Selection,
        command: ConversationAction,
    ) -> Result<()> {
        self.prepare_agent(crate::agent_settings::Kind::Opencode)?;
        let profile = self
            .opencode_profile
            .clone()
            .ok_or("no trusted OpenCode profile configured")?;
        let Some(admission) = self.admit_native_start(
            request,
            scope,
            &command,
            crate::agent_settings::NativeProfile::Opencode(profile),
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
        let crate::history::NativeWorkflow::OpenCode(w) = value else {
            return Err("OpenCode admission workflow mismatch".into());
        };
        let job = crate::opencode::Job {
            run: run.clone(),
            workflow,
            profile: w.profile.clone(),
            binding: w.binding.clone(),
            source: w.source.clone(),
            target,
            text,
        };
        let (slot, events) = crate::browser_io::Slot::new(job.target.tab);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let tx = self.tx.clone();
        let worker = thread::spawn(move || crate::opencode::execute(job, worker_stop, tx, events));
        self.operations.insert(
            run.clone(),
            Operation {
                stop,
                worker,
                correction: None,
                browser_io: Some(Box::new(slot)),
            },
        );
        self.emit(self.started_reply(request, scope, &run)?)
    }

    pub(super) fn opencode_bound(
        &mut self,
        run: &str,
        workflow: &str,
        identity: crate::opencode_model::Identity,
        ack: mpsc::SyncSender<std::result::Result<bool, String>>,
    ) -> Result<()> {
        let result = self.native_change(run, move |_, launch, native| {
            let crate::history::NativeWorkflow::OpenCode(w) = native else {
                return Err("OpenCode binding workflow mismatch".into());
            };
            if w.last_run != run {
                return Err("OpenCode binding workflow mismatch".into());
            }
            match &w.binding {
                crate::opencode_model::Binding::Bound { identity: old } if old != &identity => {
                    return Err("OpenCode native session identity or selection changed".into());
                }
                crate::opencode_model::Binding::ReleaseUnknown { .. } => {
                    return Err("OpenCode native ownership is unknown".into());
                }
                _ => (),
            }
            let Launch::OpenCode {
                workflow: bound,
                selection,
                ..
            } = launch
            else {
                return Err("OpenCode binding has no launch".into());
            };
            if bound.as_str() != workflow || selection.is_some() {
                return Err("OpenCode launch binding changed or repeated".into());
            }
            *selection = Some(identity.selection.clone());
            w.binding = crate::opencode_model::Binding::Bound { identity };
            w.revision = w
                .revision
                .checked_add(1)
                .ok_or("OpenCode workflow revision exhausted")?;
            Ok(true)
        });
        crate::flow::acknowledge(ack, result)
    }

    pub(super) fn opencode_activity(&mut self, run: &str, activity: Activity) -> Result<()> {
        self.native_change(run, move |task, launch, _| {
            let Launch::OpenCode {
                activity: events, ..
            } = launch
            else {
                return Err("OpenCode activity has no launch".into());
            };
            if events.len() >= 64 || !activity.valid() {
                return Err("invalid OpenCode activity".into());
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
}
