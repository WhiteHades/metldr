use crate::{
    Result,
    agent_model::{Launch, digest},
    agent_settings::{Kind, NativeProfile},
    browser_action::Target,
    flow::Owner,
    history::NativeWorkflow,
    model::{ConversationAction, Run, Source, Status},
};
use serde_json::json;

pub(super) struct Admission {
    pub run: String,
    pub workflow: String,
    pub value: NativeWorkflow,
    pub text: String,
    pub target: Target,
}

#[derive(Clone, Copy)]
enum Origin<'a> {
    Source(&'a str),
    Workflow(&'a str, u64),
}

struct Input<'a> {
    kind: Kind,
    origin: Origin<'a>,
    text: &'a str,
}

impl<'a> Input<'a> {
    fn parse(command: &'a ConversationAction) -> Result<Self> {
        let (kind, origin, text) = match command {
            ConversationAction::PiPrompt { source, text } => {
                (Kind::Pi, Origin::Source(source), text)
            }
            ConversationAction::CodexPrompt { source, text, .. } => {
                (Kind::Codex, Origin::Source(source), text)
            }
            ConversationAction::OpenCodePrompt { source, text } => {
                (Kind::Opencode, Origin::Source(source), text)
            }
            ConversationAction::PiFollowUp {
                workflow,
                revision,
                text,
            } => (Kind::Pi, Origin::Workflow(workflow, *revision), text),
            ConversationAction::CodexFollowUp {
                workflow,
                revision,
                text,
                ..
            } => (Kind::Codex, Origin::Workflow(workflow, *revision), text),
            ConversationAction::OpenCodeFollowUp {
                workflow,
                revision,
                text,
            } => (Kind::Opencode, Origin::Workflow(workflow, *revision), text),
            _ => return Err("invalid native command".into()),
        };
        if text.trim().is_empty() || text.len() > 1024 {
            return Err("invalid native prompt".into());
        }
        Ok(Self { kind, origin, text })
    }
    fn digest(
        &self,
        command: &ConversationAction,
        fingerprint: &str,
        source: &Source,
    ) -> Result<String> {
        digest(
            &json!({"command":command,"profile":fingerprint,"message":crate::agent::message(self.text,source)?}),
        )
    }
}

impl Owner {
    pub(super) fn replay_native_start(
        &mut self,
        request: &str,
        scope: &crate::history::Selection,
        command: &ConversationAction,
    ) -> Result<bool> {
        let input = Input::parse(command)?;
        let workflow = self
            .record_history
            .as_mut()
            .ok_or("record history is unavailable")?
            .edit(|edit| edit.native_request_workflow(scope, request))?
            .ok_or("saved native request is absent")?;
        let fingerprint = match (&workflow, input.kind) {
            (NativeWorkflow::Pi(value), Kind::Pi) => &value.fingerprint,
            (NativeWorkflow::Codex(value), Kind::Codex) => &value.fingerprint,
            (NativeWorkflow::OpenCode(value), Kind::Opencode) => &value.fingerprint,
            _ => return Err("request identity already names a different task kind".into()),
        };
        if matches!(input.origin,Origin::Source(source) if source != workflow.source().id) {
            return Err("source differs from the original native request".into());
        }
        let digest = input.digest(command, fingerprint, workflow.source())?;
        self.duplicate(scope, request, &digest)
    }

    pub(super) fn admit_native_start(
        &mut self,
        request: &str,
        scope: &crate::history::Selection,
        command: &ConversationAction,
        profile: NativeProfile,
    ) -> Result<Option<Admission>> {
        let input = Input::parse(command)?;
        let fingerprint = match (&profile, input.kind) {
            (NativeProfile::Pi(profile), Kind::Pi) => digest(profile)?,
            (NativeProfile::Codex(profile), Kind::Codex) => digest(profile)?,
            (NativeProfile::Opencode(profile), Kind::Opencode) => digest(profile)?,
            _ => return Err("native command and profile differ".into()),
        };
        let (source, prior) = if let Some(history) = &mut self.record_history {
            history.edit(|edit| {
                if let Some(workflow) = edit.native_request_workflow(scope, request)? {
                    let source = workflow.source().clone();
                    if matches!(input.origin, Origin::Source(id) if id != source.id) {
                        return Err("source differs from the original native request".into());
                    }
                    return Ok((source, None));
                }
                edit.require_selection(scope)?;
                match input.origin {
                    Origin::Source(source) => Ok((
                        edit.selected_source(scope)?
                            .filter(|saved| saved.id == source)
                            .ok_or("source is not the current saved native source")?,
                        None,
                    )),
                    Origin::Workflow(workflow, _) => {
                        let value = edit.native_workflow(scope, workflow)?;
                        Ok((value.source().clone(), Some(value)))
                    }
                }
            })?
        } else {
            match input.origin {
                Origin::Source(source) => {
                    let saved = self
                        .state
                        .starts
                        .get(request)
                        .and_then(|run| self.state.launches.get(run))
                        .map(|launch| NativeWorkflow::from_store(&self.state, launch))
                        .transpose()?;
                    let source = saved
                        .as_ref()
                        .map(NativeWorkflow::source)
                        .filter(|s| s.id == source)
                        .or_else(|| self.state.view.source.as_ref().filter(|s| s.id == source))
                        .ok_or("source is not current or the original saved native source")?;
                    (source.clone(), None)
                }
                Origin::Workflow(workflow, _) => {
                    let value = match input.kind {
                        Kind::Pi => self
                            .state
                            .workflows
                            .get(workflow)
                            .cloned()
                            .map(NativeWorkflow::Pi),
                        Kind::Codex => self
                            .state
                            .codex_workflows
                            .get(workflow)
                            .cloned()
                            .map(NativeWorkflow::Codex),
                        Kind::Opencode => self
                            .state
                            .opencode_workflows
                            .get(workflow)
                            .cloned()
                            .map(NativeWorkflow::OpenCode),
                    }
                    .ok_or("unknown native workflow")?;
                    (value.source().clone(), Some(value))
                }
            }
        };
        let input_digest = input.digest(command, &fingerprint, &source)?;
        if self.duplicate(scope, request, &input_digest)? {
            return Ok(None);
        }
        if self.record_history.is_some() {
            self.require_conversation(scope)?;
        }
        if self.operation.is_some() {
            return Err("another run is active".into());
        }
        let value = match prior {
            Some(value) => {
                let Origin::Workflow(_, revision) = input.origin else {
                    unreachable!()
                };
                validate_followup(&value, &profile, revision, &fingerprint)?;
                value
            }
            None => new_workflow(profile, source, fingerprint),
        };
        let existing = match input.origin {
            Origin::Workflow(workflow, _) => Some(workflow),
            Origin::Source(_) => None,
        };
        if self.record_history.is_some() {
            self.record_change(|edit, _| {
                let workflow = match existing {
                    Some(workflow) => workflow.to_owned(),
                    None => edit.id()?,
                };
                let run = edit.id()?;
                let (value, record, launch) =
                    records(value, &workflow, &run, input.text, input_digest)?;
                edit.admit_native(scope, request, record, launch, value.clone())?;
                let target = edit.native_initial_target(scope, &run)?;
                let projection = edit.project(scope, Some(&run))?;
                Ok((
                    Some(Admission {
                        run,
                        workflow,
                        value,
                        text: input.text.into(),
                        target,
                    }),
                    Some(projection),
                ))
            })
        } else {
            // shortcut: remove snapshot admission with the remaining snapshot callers at record cutover.
            if self.state.view.runs.len() >= 16 {
                return Err("prototype run capacity reached".into());
            }
            let mut next = self.state.clone();
            if existing.is_some() {
                let previous = match &value {
                    NativeWorkflow::Pi(w) => &w.last_run,
                    NativeWorkflow::Codex(w) => &w.last_run,
                    NativeWorkflow::OpenCode(w) => &w.last_run,
                };
                if !next
                    .view
                    .runs
                    .iter()
                    .any(|run| run.id == *previous && run.status.terminal())
                {
                    return Err("native workflow still has active work".into());
                }
            }
            let workflow = match existing {
                Some(workflow) => workflow.to_owned(),
                None => next.id()?,
            };
            let run = next.id()?;
            let (value, record, launch) =
                records(value, &workflow, &run, input.text, input_digest)?;
            value.install(&workflow, &mut next);
            next.view.runs.push(record);
            next.starts.insert(request.into(), run.clone());
            next.launches.insert(run.clone(), launch);
            let target = next.native_browser_target(&run)?;
            self.commit(next)?;
            Ok(Some(Admission {
                run,
                workflow,
                value,
                text: input.text.into(),
                target,
            }))
        }
    }
}

fn new_workflow(profile: NativeProfile, source: Source, fingerprint: String) -> NativeWorkflow {
    match profile {
        NativeProfile::Pi(profile) => NativeWorkflow::Pi(crate::agent_model::Workflow {
            profile,
            fingerprint,
            source,
            revision: 1,
            last_run: String::new(),
            binding: crate::agent_model::Binding::Pending,
        }),
        NativeProfile::Codex(profile) => NativeWorkflow::Codex(crate::codex_model::Workflow {
            profile,
            fingerprint,
            source,
            revision: 1,
            last_run: String::new(),
            binding: crate::codex_model::Binding::Pending,
        }),
        NativeProfile::Opencode(profile) => {
            NativeWorkflow::OpenCode(crate::opencode_model::Workflow {
                profile,
                fingerprint,
                source,
                revision: 1,
                last_run: String::new(),
                binding: crate::opencode_model::Binding::Pending,
            })
        }
    }
}

fn validate_followup(
    value: &NativeWorkflow,
    profile: &NativeProfile,
    revision: u64,
    fingerprint: &str,
) -> Result<()> {
    let ready = match (value, profile) {
        (NativeWorkflow::Pi(w), NativeProfile::Pi(_))
            if w.revision == revision && w.fingerprint == fingerprint =>
        {
            w.binding.resume_path()?.is_some()
        }
        (NativeWorkflow::Codex(w), NativeProfile::Codex(_))
            if w.revision == revision && w.fingerprint == fingerprint =>
        {
            w.binding.resume()?.is_some()
        }
        (NativeWorkflow::OpenCode(w), NativeProfile::Opencode(_))
            if w.revision == revision && w.fingerprint == fingerprint =>
        {
            w.binding.resume()?.is_some()
        }
        _ => return Err("native workflow revision profile or kind changed".into()),
    };
    if !ready {
        return Err("native workflow revision profile or binding changed".into());
    }
    Ok(())
}

fn records(
    mut value: NativeWorkflow,
    workflow: &str,
    run: &str,
    text: &str,
    input_digest: String,
) -> Result<(NativeWorkflow, Run, Launch)> {
    macro_rules! advance {
        ($w:expr) => {{
            $w.revision = $w
                .revision
                .checked_add(1)
                .ok_or("native workflow revision exhausted")?;
            $w.last_run = run.into();
        }};
    }
    let launch = match &mut value {
        NativeWorkflow::Pi(w) => {
            advance!(w);
            Launch::Pi {
                input_digest,
                workflow: workflow.into(),
                activity: vec![],
            }
        }
        NativeWorkflow::Codex(w) => {
            advance!(w);
            Launch::Codex {
                input_digest,
                workflow: workflow.into(),
                selection: None,
                activity: vec![],
            }
        }
        NativeWorkflow::OpenCode(w) => {
            advance!(w);
            Launch::OpenCode {
                input_digest,
                workflow: workflow.into(),
                selection: None,
                activity: vec![],
            }
        }
    };
    let record = Run {
        id: run.into(),
        source: value.source().id.clone(),
        kind: "agent".into(),
        question: Some(text.into()),
        status: Status::Running,
        output: metldr_conversation::Content::native(),
        error: None,
        corrections: Vec::new(),
    };
    Ok((value, record, launch))
}
