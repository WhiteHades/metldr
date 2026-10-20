use crate::{
    Result,
    agent_model::{Launch, digest},
    model::{Source, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
                return Err("invalid OpenCode profile path".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    pub model: String,
    pub mode: String,
}
impl Selection {
    fn checked(model: String, mode: String) -> Result<Self> {
        let value = Self { model, mode };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if [&self.model, &self.mode]
            .into_iter()
            .any(|s| s.trim().is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        {
            return Err("invalid OpenCode model selection".into());
        }
        Ok(())
    }
    pub fn from_options(options: &[Value]) -> Result<Self> {
        let mut model = None;
        let mut mode = None;
        if options.len() > 64 {
            return Err("too many OpenCode config options".into());
        }
        for option in options {
            let Some(id) = option["id"].as_str() else {
                continue;
            };
            match id {
                "model" | "mode" => {
                    let value = option["currentValue"]
                        .as_str()
                        .filter(|s| {
                            !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
                        })
                        .ok_or("OpenCode config option has invalid current value")?;
                    let slot = if id == "model" { &mut model } else { &mut mode };
                    if slot.replace(value.to_owned()).is_some() {
                        return Err("duplicate OpenCode model or mode option".into());
                    }
                }
                _ => (),
            }
        }
        Self::checked(
            model.ok_or("OpenCode model option missing")?,
            mode.ok_or("OpenCode mode option missing")?,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub session: String,
    pub selection: Selection,
}
impl Identity {
    fn validate(&self) -> Result<()> {
        metldr_opencode_session::SessionId::saved(&self.session)?;
        self.selection.validate()
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
    pub fn resume(&self) -> Result<Option<metldr_opencode_session::SessionId>> {
        match self {
            Self::Pending => Ok(None),
            Self::Bound { identity } => {
                identity.validate()?;
                Ok(Some(metldr_opencode_session::SessionId::saved(
                    &identity.session,
                )?))
            }
            Self::ReleaseUnknown { .. } => Err("OpenCode session ownership is unknown".into()),
        }
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
    if state.workflows.len() + state.codex_workflows.len() + state.opencode_workflows.len() > 16 {
        return Err("too many native workflows".into());
    }
    let valid_id = |id: &str| {
        id.strip_prefix(&format!("{}:", state.view.store))
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > 0 && n < state.next)
    };
    for run in &state.view.runs {
        if let Some(Launch::OpenCode {
            workflow,
            selection,
            activity,
            ..
        }) = state.launches.get(&run.id)
        {
            let w = state
                .opencode_workflows
                .get(workflow)
                .ok_or("missing OpenCode workflow")?;
            if run.kind != "agent"
                || run.source != w.source.id
                || (run.status == crate::model::Status::Complete && selection.is_none())
            {
                return Err("invalid OpenCode task record".into());
            }
            if activity.len() > 64 || activity.iter().any(|a| !a.valid()) {
                return Err("invalid OpenCode activity".into());
            }
            if let Some(selection) = selection {
                selection.validate()?;
                if let Binding::Bound { identity } = &w.binding {
                    if identity.selection != *selection {
                        return Err("OpenCode launch selection differs from bound identity".into());
                    }
                }
            }
        }
    }
    for (id, w) in &state.opencode_workflows {
        w.profile.validate()?;
        crate::model::check_page_fields(&w.source.page)?;
        if !valid_id(id) || !valid_id(&w.source.id) || state.workflows.contains_key(id) || state.codex_workflows.contains_key(id)
            || state.view.runs.iter().any(|r| &r.id == id) || w.revision == 0 || w.fingerprint != digest(&w.profile)?
            || !matches!(state.launches.get(&w.last_run), Some(Launch::OpenCode { workflow, .. }) if workflow == id)
            || state.view.runs.iter().rev().find(|run| matches!(state.launches.get(&run.id), Some(Launch::OpenCode { workflow, .. }) if workflow == id)).is_none_or(|run| run.id != w.last_run) {
            return Err("invalid OpenCode workflow identity".into());
        }
        match &w.binding {
            Binding::Bound { identity }
            | Binding::ReleaseUnknown {
                previous: Some(identity),
            } => identity.validate()?,
            _ => (),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Selection;
    use serde_json::json;

    #[test]
    fn selection_reads_one_current_model_and_mode() {
        let options = [
            json!({"id":"model","currentValue":"vendor/model"}),
            json!({"id":"mode","currentValue":"build"}),
            json!({"id":"temperature"}),
        ];
        assert_eq!(
            Selection::from_options(&options).unwrap(),
            Selection {
                model: "vendor/model".into(),
                mode: "build".into()
            }
        );
        let duplicate = [options[0].clone(), options[0].clone(), options[1].clone()];
        assert!(Selection::from_options(&duplicate).is_err());
    }
}
