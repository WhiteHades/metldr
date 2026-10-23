use crate::{
    Result,
    agent_model::Launch,
    codex,
    codex_model::{Binding, Identity, Selection},
    flow::{Operation, Owner},
    model::ConversationAction,
};
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
};

impl Owner {
    pub(super) fn codex_activity(
        &mut self,
        run: &str,
        activity: crate::agent_model::Activity,
    ) -> Result<()> {
        self.native_change(run, |task, launch, _workflow| {
            let Launch::Codex {
                activity: events, ..
            } = launch
            else {
                return Err("Codex activity has no launch".into());
            };
            if events.len() >= 64 || !activity.valid() {
                return Err("invalid Codex activity".into());
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

    pub(super) fn codex_start(
        &mut self,
        id: &str,
        scope: &crate::history::Selection,
        command: ConversationAction,
    ) -> Result<()> {
        self.prepare_agent(crate::agent_settings::Kind::Codex)?;
        let profile = self
            .codex_profile
            .clone()
            .ok_or("no trusted Codex profile configured")?;
        let mode = match &command {
            ConversationAction::CodexPrompt { mode, .. }
            | ConversationAction::CodexFollowUp { mode, .. } => *mode,
            _ => return Err("invalid Codex command".into()),
        };
        let Some(admission) = self.admit_native_start(
            id,
            scope,
            &command,
            crate::agent_settings::NativeProfile::Codex(profile),
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
        let crate::history::NativeWorkflow::Codex(w) = value else {
            return Err("Codex admission workflow mismatch".into());
        };
        let job = codex::Job {
            run: run.clone(),
            workflow,
            profile: w.profile.clone(),
            binding: w.binding.clone(),
            source: w.source.clone(),
            target,
            text,
            mode,
        };
        let (slot, events) = crate::browser_io::Slot::new(run.clone(), job.target.tab);
        self.browser_io = Some(slot);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let tx = self.tx.clone();
        let (correction, corrections) = crate::native_correction::channel(&run, &tx);
        let worker =
            thread::spawn(move || codex::execute(job, worker_stop, tx, events, corrections));
        self.operation = Some(Operation {
            run: run.clone(),
            stop,
            worker,
            correction: Some(correction),
        });
        self.emit(self.started_reply(id, scope, &run)?)
    }

    pub(super) fn codex_bound(
        &mut self,
        run: &str,
        workflow: &str,
        identity: Identity,
        selection: Selection,
        ack: mpsc::SyncSender<std::result::Result<bool, String>>,
    ) -> Result<()> {
        let result = self.native_change(run, |_, launch, workflow_state| {
            let crate::history::NativeWorkflow::Codex(w) = workflow_state else {
                return Err("Codex binding has no workflow".into());
            };
            if w.last_run != run {
                return Err("Codex binding workflow mismatch".into());
            }
            match &w.binding {
                Binding::Bound { identity: old } if old != &identity => {
                    return Err("Codex native thread identity changed".into());
                }
                Binding::ReleaseUnknown { .. } => {
                    return Err("Codex native ownership is unknown".into());
                }
                _ => (),
            }
            let Launch::Codex {
                workflow: bound,
                selection: selected,
                ..
            } = launch
            else {
                return Err("Codex binding has no launch".into());
            };
            if bound != workflow || selected.is_some() {
                return Err("Codex launch binding changed or repeated".into());
            }
            *selected = Some(selection);
            w.binding = Binding::Bound { identity };
            w.revision = w
                .revision
                .checked_add(1)
                .ok_or("Codex workflow revision exhausted")?;
            Ok(true)
        });
        match result {
            Ok(ready) => {
                let _ = ack.send(Ok(ready));
                Ok(())
            }
            Err(error) => {
                let _ = ack.send(Err(error.to_string()));
                if error.downcast_ref::<std::io::Error>().is_some()
                    || crate::history::is_failure(error.as_ref())
                {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }
}
