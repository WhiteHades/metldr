use crate::{
    Result,
    agent_model::{Launch, digest},
    model::{Source, Store},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    pub executable: PathBuf,
    pub workspace: PathBuf,
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        for path in [&self.executable, &self.workspace] {
            if !path.is_absolute()
                || path.to_string_lossy().len() > 2048
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err("invalid Codex profile path".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub thread: String,
    pub session: String,
}
impl Identity {
    fn validate(&self) -> Result<()> {
        metldr_codex_session::ThreadId::saved(&self.thread)?;
        if self.session.is_empty()
            || self.session.len() > 256
            || self.session.chars().any(char::is_control)
        {
            return Err("invalid Codex native session identity".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Binding {
    Pending,
    Bound { identity: Identity },
    ReleaseUnknown { previous: Option<Identity> },
}
impl Binding {
    pub fn block_recovery(&mut self) {
        let previous = match self {
            Self::Pending => None,
            Self::Bound { identity } => Some(identity.clone()),
            Self::ReleaseUnknown { .. } => return,
        };
        *self = Self::ReleaseUnknown { previous };
    }
    pub fn resume(&self) -> Result<Option<metldr_codex_session::ThreadId>> {
        match self {
            Self::Pending => Ok(None),
            Self::Bound { identity } => {
                identity.validate()?;
                Ok(Some(metldr_codex_session::ThreadId::saved(
                    &identity.thread,
                )?))
            }
            Self::ReleaseUnknown { .. } => Err("Codex native session ownership is unknown".into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    pub provider: String,
    pub model: String,
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.provider, &self.model] {
            if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
                return Err("invalid Codex native model selection".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Workflow {
    pub profile: Profile,
    pub fingerprint: String,
    pub source: Source,
    pub revision: u64,
    pub last_run: String,
    pub binding: Binding,
}

pub(super) fn validate(state: &Store) -> Result<()> {
    if state.workflows.len() + state.codex_workflows.len() > 16 {
        return Err("too many native workflows".into());
    }
    let valid_id = |id: &str| {
        id.strip_prefix(&format!("{}:", state.view.store))
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > 0 && n < state.next)
    };
    for run in &state.view.runs {
        if let Some(Launch::Codex {
            workflow,
            selection,
            activity,
            ..
        }) = state.launches.get(&run.id)
        {
            let w = state
                .codex_workflows
                .get(workflow)
                .ok_or("missing Codex workflow")?;
            if run.kind != "agent"
                || run.source != w.source.id
                || (run.status == crate::model::Status::Complete && selection.is_none())
                || activity.len() > 64
                || activity.iter().any(|item| !item.valid())
            {
                return Err("invalid Codex task record".into());
            }
            if let Some(selection) = selection {
                selection.validate()?;
            }
        }
    }
    for (id, w) in &state.codex_workflows {
        w.profile.validate()?;
        crate::model::check_page_fields(&w.source.page)?;
        if !valid_id(id)
            || !valid_id(&w.source.id)
            || state.workflows.contains_key(id)
            || state.opencode_workflows.contains_key(id)
            || state.view.runs.iter().any(|r| &r.id == id)
            || w.revision == 0
            || w.fingerprint != digest(&w.profile)?
            || !matches!(state.launches.get(&w.last_run), Some(Launch::Codex { workflow, .. }) if workflow == id)
            || state.view.runs.iter().rev().find(|run| matches!(state.launches.get(&run.id), Some(Launch::Codex { workflow, .. }) if workflow == id)).is_none_or(|run| run.id != w.last_run)
        {
            return Err("invalid Codex workflow identity".into());
        }
        match &w.binding {
            Binding::Bound { identity } => identity.validate()?,
            Binding::ReleaseUnknown {
                previous: Some(identity),
            } => identity.validate()?,
            _ => (),
        }
    }
    Ok(())
}
