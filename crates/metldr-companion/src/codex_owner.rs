use crate::{
    Result, agent,
    agent_model::{Launch, digest},
    codex,
    codex_model::{Binding, Identity, Selection, Workflow},
    flow::{Operation, Owner},
    model::{Command, Run, Status},
};
use serde_json::json;
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
        if self.operation.as_ref().is_none_or(|op| op.run != run) {
            return Err("Codex activity came from a stale operation".into());
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
        let Some(Launch::Codex {
            activity: events, ..
        }) = next.launches.get_mut(run)
        else {
            return Err("Codex activity has no launch".into());
        };
        if events.len() >= 64 || !activity.valid() {
            return Err("invalid Codex activity".into());
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

    pub(super) fn codex_start(&mut self, id: &str, command: Command) -> Result<()> {
        self.prepare_agent(crate::agent_settings::Kind::Codex)?;
        let profile = self
            .codex_profile
            .clone()
            .ok_or("no trusted Codex profile configured")?;
        let (text, source) =
            match &command {
                Command::CodexPrompt { source, text } => {
                    let prior = self.state.starts.get(id).and_then(|run| {
                        match self.state.launches.get(run) {
                            Some(Launch::Codex { workflow, .. }) => {
                                self.state.codex_workflows.get(workflow)
                            }
                            _ => None,
                        }
                    });
                    let saved = prior
                        .map(|w| &w.source)
                        .filter(|s| &s.id == source)
                        .or_else(|| self.state.view.source.as_ref().filter(|s| &s.id == source))
                        .ok_or("source is not current or the original saved Codex source")?;
                    (text, saved)
                }
                Command::CodexFollowUp { workflow, text, .. } => (
                    text,
                    &self
                        .state
                        .codex_workflows
                        .get(workflow)
                        .ok_or("unknown Codex workflow")?
                        .source,
                ),
                _ => return Err("invalid Codex command".into()),
            };
        let input_digest = digest(
            &json!({"command":&command,"profile":digest(&profile)?,"message":agent::message(text,source)?}),
        )?;
        if self.duplicate(id, &input_digest)? {
            return Ok(());
        }
        if self.operation.is_some() || self.state.view.runs.len() >= 16 {
            return Err("another run is active or prototype capacity reached".into());
        }
        if text.trim().is_empty() || text.len() > 1024 {
            return Err("invalid Codex prompt".into());
        }
        let mut next = self.state.clone();
        let (workflow, text) = match command {
            Command::CodexPrompt { source, text } => {
                let source = next
                    .view
                    .source
                    .as_ref()
                    .filter(|s| s.id == source)
                    .ok_or("source is not current saved source")?
                    .clone();
                let workflow = next.id()?;
                next.codex_workflows.insert(
                    workflow.clone(),
                    Workflow {
                        profile: profile.clone(),
                        fingerprint: digest(&profile)?,
                        source,
                        revision: 1,
                        last_run: String::new(),
                        binding: Binding::Pending,
                    },
                );
                (workflow, text)
            }
            Command::CodexFollowUp {
                workflow,
                revision,
                text,
            } => {
                let w = next
                    .codex_workflows
                    .get(&workflow)
                    .ok_or("unknown Codex workflow")?;
                if w.revision != revision || w.fingerprint != digest(&profile)? {
                    return Err("Codex workflow revision or profile changed".into());
                }
                w.binding
                    .resume()?
                    .ok_or("Codex native thread is not ready")?;
                if !next
                    .view
                    .runs
                    .iter()
                    .any(|r| r.id == w.last_run && r.status.terminal())
                {
                    return Err("Codex workflow still has active work".into());
                }
                (workflow, text)
            }
            _ => return Err("invalid Codex command".into()),
        };
        let run = next.id()?;
        let w = next
            .codex_workflows
            .get_mut(&workflow)
            .ok_or("missing Codex workflow")?;
        w.revision = w
            .revision
            .checked_add(1)
            .ok_or("Codex workflow revision exhausted")?;
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
            Launch::Codex {
                input_digest,
                workflow: workflow.clone(),
                selection: None,
                activity: vec![],
            },
        );
        let w = next
            .codex_workflows
            .get(&workflow)
            .ok_or("missing Codex workflow")?;
        let job = codex::Job {
            run: run.clone(),
            workflow,
            profile: w.profile.clone(),
            binding: w.binding.clone(),
            source: w.source.clone(),
            target: next.codex_browser_target(&run)?,
            text,
        };
        self.commit(next)?;
        let (slot, events) = crate::browser_io::Slot::new(run.clone(), job.target.tab);
        self.browser_io = Some(slot);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let tx = self.tx.clone();
        let worker = thread::spawn(move || codex::execute(job, worker_stop, tx, events));
        self.operation = Some(Operation {
            run: run.clone(),
            stop,
            worker,
        });
        self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"started","run":run}))
    }

    pub(super) fn codex_bound(
        &mut self,
        run: &str,
        workflow: &str,
        identity: Identity,
        selection: Selection,
        ack: mpsc::SyncSender<std::result::Result<bool, String>>,
    ) -> Result<()> {
        let result = (|| -> Result<bool> {
            if self.operation.as_ref().is_none_or(|op| op.run != run) {
                return Err("Codex binding came from a stale operation".into());
            }
            let mut next = self.state.clone();
            if next
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .ok_or("missing Codex task")?
                .status
                != Status::Running
            {
                return Ok(false);
            }
            let w = next
                .codex_workflows
                .get_mut(workflow)
                .filter(|w| w.last_run == run)
                .ok_or("Codex binding workflow mismatch")?;
            match &w.binding {
                Binding::Bound { identity: old } if old != &identity => {
                    return Err("Codex native thread identity changed".into());
                }
                Binding::ReleaseUnknown { .. } => {
                    return Err("Codex native ownership is unknown".into());
                }
                _ => (),
            }
            w.binding = Binding::Bound { identity };
            w.revision = w
                .revision
                .checked_add(1)
                .ok_or("Codex workflow revision exhausted")?;
            let Some(Launch::Codex {
                workflow: bound,
                selection: selected,
                ..
            }) = next.launches.get_mut(run)
            else {
                return Err("Codex binding has no launch".into());
            };
            if bound != workflow || selected.is_some() {
                return Err("Codex launch binding changed or repeated".into());
            }
            *selected = Some(selection);
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
}
