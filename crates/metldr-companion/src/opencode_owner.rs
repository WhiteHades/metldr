use crate::{
    Result, agent,
    agent_model::{Activity, Launch, digest},
    flow::{Operation, Owner},
    model::{Command, Run, Status},
};
use serde_json::json;
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
};

impl Owner {
    pub(super) fn opencode_start(&mut self, request: &str, command: Command) -> Result<()> {
        self.prepare_agent(crate::agent_settings::Kind::Opencode)?;
        let profile = self
            .opencode_profile
            .clone()
            .ok_or("no trusted OpenCode profile configured")?;
        let (text, source) = match &command {
            Command::OpenCodePrompt { source, text } => {
                let saved = self
                    .state
                    .view
                    .source
                    .as_ref()
                    .filter(|s| &s.id == source)
                    .ok_or("source is not current saved source")?;
                (text, saved)
            }
            Command::OpenCodeFollowUp { workflow, text, .. } => (
                text,
                &self
                    .state
                    .opencode_workflows
                    .get(workflow)
                    .ok_or("unknown OpenCode workflow")?
                    .source,
            ),
            _ => return Err("invalid OpenCode command".into()),
        };
        if text.trim().is_empty() || text.len() > 1024 {
            return Err("invalid OpenCode prompt".into());
        }
        let input_digest = digest(
            &json!({"command":&command,"profile":digest(&profile)?,"message":agent::message(text,source)?}),
        )?;
        if self.duplicate(request, &input_digest)? {
            return Ok(());
        }
        if self.operation.is_some() || self.state.view.runs.len() >= 16 {
            return Err("another run is active or prototype capacity reached".into());
        }
        let mut next = self.state.clone();
        let (workflow, text) = match command {
            Command::OpenCodePrompt { source, text } => {
                let source = next
                    .view
                    .source
                    .as_ref()
                    .filter(|s| s.id == source)
                    .ok_or("source is not current saved source")?
                    .clone();
                let workflow = next.id()?;
                next.opencode_workflows.insert(
                    workflow.clone(),
                    crate::opencode_model::Workflow {
                        profile: profile.clone(),
                        fingerprint: digest(&profile)?,
                        source,
                        revision: 1,
                        last_run: String::new(),
                        binding: crate::opencode_model::Binding::Pending,
                    },
                );
                (workflow, text)
            }
            Command::OpenCodeFollowUp {
                workflow,
                revision,
                text,
            } => {
                let w = next
                    .opencode_workflows
                    .get(&workflow)
                    .ok_or("unknown OpenCode workflow")?;
                if w.revision != revision || w.fingerprint != digest(&profile)? {
                    return Err("OpenCode workflow revision or profile changed".into());
                }
                w.binding.resume()?.ok_or("OpenCode session is not ready")?;
                if !next
                    .view
                    .runs
                    .iter()
                    .any(|r| r.id == w.last_run && r.status.terminal())
                {
                    return Err("OpenCode workflow still has active work".into());
                }
                (workflow, text)
            }
            _ => return Err("invalid OpenCode command".into()),
        };
        let run = next.id()?;
        let w = next
            .opencode_workflows
            .get_mut(&workflow)
            .ok_or("missing OpenCode workflow")?;
        w.revision = w
            .revision
            .checked_add(1)
            .ok_or("OpenCode workflow revision exhausted")?;
        w.last_run = run.clone();
        next.view.runs.push(Run {
            id: run.clone(),
            source: w.source.id.clone(),
            kind: "agent".into(),
            question: Some(text.clone()),
            status: Status::Running,
            output: metldr_conversation::Content::native(),
            error: None,
        });
        next.starts.insert(request.into(), run.clone());
        next.launches.insert(
            run.clone(),
            Launch::OpenCode {
                input_digest,
                workflow: workflow.clone(),
                selection: None,
                activity: vec![],
            },
        );
        let w = next
            .opencode_workflows
            .get(&workflow)
            .ok_or("missing OpenCode workflow")?;
        let job = crate::opencode::Job {
            run: run.clone(),
            workflow,
            profile: w.profile.clone(),
            binding: w.binding.clone(),
            source: w.source.clone(),
            target: next.native_browser_target(&run)?,
            text,
        };
        self.commit(next)?;
        let (slot, events) = crate::browser_io::Slot::new(run.clone(), job.target.tab);
        self.browser_io = Some(slot);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let tx = self.tx.clone();
        let worker = thread::spawn(move || crate::opencode::execute(job, worker_stop, tx, events));
        self.operation = Some(Operation {
            run: run.clone(),
            stop,
            worker,
        });
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"started","run":run}))
    }

    pub(super) fn opencode_bound(
        &mut self,
        run: &str,
        workflow: &str,
        identity: crate::opencode_model::Identity,
        ack: mpsc::SyncSender<std::result::Result<bool, String>>,
    ) -> Result<()> {
        let result = (|| -> Result<bool> {
            if self.operation.as_ref().is_none_or(|op| op.run != run) {
                return Err("OpenCode binding came from a stale operation".into());
            }
            let mut next = self.state.clone();
            if next
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .ok_or("missing OpenCode task")?
                .status
                != Status::Running
            {
                return Ok(false);
            }
            let w = next
                .opencode_workflows
                .get_mut(workflow)
                .filter(|w| w.last_run == run)
                .ok_or("OpenCode binding workflow mismatch")?;
            match &w.binding {
                crate::opencode_model::Binding::Bound { identity: old } if old != &identity => {
                    return Err("OpenCode native session identity or selection changed".into());
                }
                crate::opencode_model::Binding::ReleaseUnknown { .. } => {
                    return Err("OpenCode native ownership is unknown".into());
                }
                _ => (),
            }
            if let Some(Launch::OpenCode {
                workflow: bound,
                selection,
                ..
            }) = next.launches.get_mut(run)
            {
                if bound != workflow || selection.is_some() {
                    return Err("OpenCode launch binding changed or repeated".into());
                }
                *selection = Some(identity.selection.clone());
            } else {
                return Err("OpenCode binding has no launch".into());
            }
            w.binding = crate::opencode_model::Binding::Bound { identity };
            w.revision = w
                .revision
                .checked_add(1)
                .ok_or("OpenCode workflow revision exhausted")?;
            self.commit(next)?;
            Ok(true)
        })();
        match result {
            Ok(ready) => {
                let _ = ack.send(Ok(ready));
                Ok(())
            }
            Err(error) => {
                let _ = ack.send(Err(error.to_string()));
                if error.downcast_ref::<std::io::Error>().is_some() {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(super) fn opencode_activity(&mut self, run: &str, activity: Activity) -> Result<()> {
        if self.operation.as_ref().is_none_or(|op| op.run != run) {
            return Err("OpenCode activity came from a stale operation".into());
        }
        if self
            .state
            .view
            .runs
            .iter()
            .find(|r| r.id == run)
            .is_none_or(|r| r.status != Status::Running)
        {
            return Ok(());
        }
        let mut next = self.state.clone();
        let Some(Launch::OpenCode {
            activity: events, ..
        }) = next.launches.get_mut(run)
        else {
            return Err("OpenCode activity has no launch".into());
        };
        if events.len() >= 64 || !activity.valid() {
            return Err("invalid OpenCode activity".into());
        }
        if let Some(call) = &activity.call {
            next.view
                .runs
                .iter_mut()
                .find(|record| record.id == run)
                .ok_or("native activity has no run")?
                .output
                .observe_tool(metldr_conversation::ToolId::new(call.id.clone())?)?;
        }
        events.push(activity);
        self.commit(next)
    }
}
