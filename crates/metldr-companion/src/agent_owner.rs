use crate::{
    Result,
    agent::{self, Job, Outcome},
    agent_model::{Activity, Binding, Launch, Workflow, digest},
    flow::{Operation, Owner},
    model::{Command, Run, Status},
};
use serde_json::json;
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
};

impl Owner {
    pub(super) fn duplicate(&mut self, id: &str, input_digest: &str) -> Result<bool> {
        let Some(run) = self.state.starts.get(id) else {
            return Ok(false);
        };
        let launch = self.state.launches.get(run).ok_or("missing saved launch")?;
        let saved=launch.input_digest().ok_or("legacy start identity has no verifiable input or route; inspect its saved view instead")?;
        if saved != input_digest {
            return Err("request identity already names a different input or route".into());
        }
        self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"started","run":run}))?;
        Ok(true)
    }

    pub(super) fn pi_start(&mut self, id: &str, command: Command) -> Result<()> {
        self.prepare_agent(crate::agent_settings::Kind::Pi)?;
        let profile = self
            .pi_profile
            .clone()
            .ok_or("no trusted Pi profile configured")?;
        let (text, source) =
            match &command {
                Command::PiPrompt { source, text } => {
                    let prior = self.state.starts.get(id).and_then(|run| {
                        match self.state.launches.get(run) {
                            Some(Launch::Pi { workflow, .. }) => self.state.workflows.get(workflow),
                            _ => None,
                        }
                    });
                    let page = prior
                        .map(|w| &w.source)
                        .filter(|s| &s.id == source)
                        .or_else(|| self.state.view.source.as_ref().filter(|s| &s.id == source))
                        .ok_or("source is not current or the original saved Pi source")?;
                    (text, page)
                }
                Command::PiFollowUp { workflow, text, .. } => (
                    text,
                    &self
                        .state
                        .workflows
                        .get(workflow)
                        .ok_or("unknown Pi workflow")?
                        .source,
                ),
                _ => return Err("invalid Pi command".into()),
            };
        let input_digest = digest(
            &json!({"command":&command,"profile":digest(&profile)?,"message":agent::message(text,source)?}),
        )?;
        if self.duplicate(id, &input_digest)? {
            return Ok(());
        }
        if self.operation.is_some() {
            return Err("another run is active".into());
        }
        if self.state.view.runs.len() >= 16 {
            return Err("prototype run capacity reached".into());
        }
        let mut next = self.state.clone();
        let (workflow, text) = match command {
            Command::PiPrompt { source, text } => {
                let source = next
                    .view
                    .source
                    .as_ref()
                    .filter(|s| s.id == source)
                    .ok_or("source is not current saved source")?
                    .clone();
                let workflow = next.id()?;
                next.workflows.insert(
                    workflow.clone(),
                    Workflow {
                        fingerprint: digest(&profile)?,
                        profile: profile.clone(),
                        source,
                        revision: 1,
                        last_run: String::new(),
                        binding: Binding::Pending,
                    },
                );
                (workflow, text)
            }
            Command::PiFollowUp {
                workflow,
                revision,
                text,
            } => {
                let w = next.workflows.get(&workflow).ok_or("unknown Pi workflow")?;
                if w.revision != revision || w.fingerprint != digest(&profile)? {
                    return Err("Pi workflow revision or declared profile changed".into());
                }
                w.binding
                    .resume_path()?
                    .ok_or("Pi native session binding is not ready")?;
                if !next
                    .view
                    .runs
                    .iter()
                    .any(|r| r.id == w.last_run && r.status.terminal())
                {
                    return Err("Pi workflow still has active work".into());
                }
                (workflow, text)
            }
            _ => return Err("invalid Pi command".into()),
        };
        if text.trim().is_empty() || text.len() > 1024 {
            return Err("invalid Pi prompt".into());
        }
        let run = next.id()?;
        let w = next
            .workflows
            .get_mut(&workflow)
            .ok_or("missing Pi workflow")?;
        w.revision = w
            .revision
            .checked_add(1)
            .ok_or("Pi workflow revision exhausted")?;
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
        next.starts.insert(id.into(), run.clone());
        next.launches.insert(
            run.clone(),
            Launch::Pi {
                input_digest,
                workflow: workflow.clone(),
                activity: vec![],
            },
        );
        let w = next.workflows.get(&workflow).ok_or("missing Pi workflow")?;
        let job = Job {
            run: run.clone(),
            workflow,
            profile: w.profile.clone(),
            binding: w.binding.clone(),
            source: w.source.clone(),
            text,
            target: next.native_browser_target(&run)?,
            registration_dir: self.disk.directory.join("pi-browser"),
        };
        self.commit(next)?;
        let (slot, events) = crate::browser_io::Slot::new(run.clone(), job.target.tab);
        self.browser_io = Some(slot);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let tx = self.tx.clone();
        let worker = thread::spawn(move || agent::execute(job, worker_stop, tx, events));
        self.operation = Some(Operation {
            run: run.clone(),
            stop,
            worker,
        });
        self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"started","run":run}))
    }

    pub(super) fn pi_bound(
        &mut self,
        run: &str,
        workflow: &str,
        binding: Binding,
        ack: mpsc::SyncSender<std::result::Result<bool, String>>,
    ) -> Result<()> {
        let result = (|| -> Result<bool> {
            if self.operation.as_ref().is_none_or(|op| op.run != run) {
                return Err("Pi binding came from a stale operation".into());
            }
            let mut next = self.state.clone();
            let record = next
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .ok_or("missing Pi task")?;
            if record.status != Status::Running {
                return Ok(false);
            }
            let w = next
                .workflows
                .get_mut(workflow)
                .filter(|w| w.last_run == run)
                .ok_or("Pi binding workflow mismatch")?;
            if let Binding::Bound {
                id: old_id,
                path: old_path,
            } = &w.binding
            {
                if !matches!(&binding,Binding::Bound{id,path} if id==old_id && path==old_path) {
                    return Err("Pi session identity changed".into());
                }
            }
            if !matches!(binding, Binding::Bound { .. }) {
                return Err("Pi did not return a real binding".into());
            }
            w.binding = binding;
            w.revision = w
                .revision
                .checked_add(1)
                .ok_or("Pi workflow revision exhausted")?;
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

    pub(super) fn pi_activity(&mut self, run: &str, activity: Activity) -> Result<()> {
        if self.operation.as_ref().is_none_or(|op| op.run != run) {
            return Err("Pi activity came from a stale operation".into());
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
        let Some(Launch::Pi {
            activity: events, ..
        }) = next.launches.get_mut(run)
        else {
            return Err("Pi activity has no launch".into());
        };
        if events.len() >= 16 || !activity.valid() {
            return Err("invalid Pi activity".into());
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

    pub(super) fn agent_settled(
        &mut self,
        run: &str,
        result: Outcome,
        disconnected: bool,
        released: bool,
    ) -> Result<()> {
        let operation = self
            .operation
            .take()
            .ok_or("agent settlement without operation")?;
        if operation.run != run {
            return Err("agent settlement run mismatch".into());
        }
        self.browser_io_finish(run);
        operation
            .worker
            .join()
            .map_err(|_| "agent worker panicked")?;
        let mut next = self.state.clone();
        next.finish_browser(run)?;
        let uncertain = next.browser_actions.values().any(|action| {
            action.proposal.run == run
                && matches!(
                    &action.state,
                    crate::browser_action::Phase::Settled {
                        outcome: crate::browser_action::Outcome::Uncertain { .. }
                    }
                )
        });
        if (!released || disconnected || uncertain)
            && let Some(launch) = next.launches.get(run)
        {
            match launch {
                Launch::Pi { workflow, .. } => next
                    .workflows
                    .get_mut(workflow)
                    .ok_or("missing Pi workflow at settlement")?
                    .binding
                    .block_recovery(),
                Launch::Codex { workflow, .. } => next
                    .codex_workflows
                    .get_mut(workflow)
                    .ok_or("missing Codex workflow at settlement")?
                    .binding
                    .block_recovery(),
                Launch::OpenCode { workflow, .. } => next
                    .opencode_workflows
                    .get_mut(workflow)
                    .ok_or("missing OpenCode workflow at settlement")?
                    .binding
                    .block_recovery(),
                _ => (),
            }
        }
        self.cancel_browser_approval(run);
        let permissions = self.take_native_permissions(run);
        let record = next
            .view
            .runs
            .iter_mut()
            .find(|r| r.id == run)
            .ok_or("Pi settlement without run")?;
        record.status = if disconnected {
            Status::Interrupted
        } else {
            match &result {
                Outcome::Complete => Status::Complete,
                Outcome::Cancelled => Status::Cancelled,
                Outcome::Failed(_) => Status::Failed,
            }
        };
        if let Outcome::Failed(error) = result {
            record.error = Some(error.chars().take(200).collect());
        }
        self.commit(next)?;
        for (_, permission) in permissions {
            let _ = permission.ack.send(None);
        }
        Ok(())
    }
}
