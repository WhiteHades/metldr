use crate::{
    Result,
    model::{Source, Store},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

pub(super) fn digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    pub executable: PathBuf,
    pub workspace: PathBuf,
    pub session_dir: PathBuf,
    pub provider: String,
    pub model: String,
    pub offline: bool,
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        for path in [&self.executable, &self.workspace, &self.session_dir] {
            if !path.is_absolute()
                || path.to_string_lossy().len() > 2048
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err("invalid Pi profile path".into());
            }
        }
        for value in [&self.provider, &self.model] {
            if value.trim().is_empty() || value.len() > 128 {
                return Err("invalid Pi model selection".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Binding {
    Pending,
    Bound { id: String, path: PathBuf },
    ReleaseUnknown { previous: Option<NativeSession> },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeSession {
    pub id: String,
    pub path: PathBuf,
}
impl Binding {
    pub fn block_recovery(&mut self) {
        let previous = match self {
            Self::Pending => None,
            Self::Bound { id, path } => Some(NativeSession {
                id: id.clone(),
                path: path.clone(),
            }),
            Self::ReleaseUnknown { .. } => return,
        };
        *self = Self::ReleaseUnknown { previous };
    }
    pub fn resume_path(&self) -> Result<Option<PathBuf>> {
        match self {
            Self::Pending => Ok(None),
            Self::ReleaseUnknown { .. } => {
                Err("Pi native session ownership is unknown after native host restart".into())
            }
            Self::Bound { path, .. } => {
                if !path.is_file() || path.canonicalize()? != *path {
                    return Err("saved Pi session file is missing or changed".into());
                }
                Ok(Some(path.clone()))
            }
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Activity {
    pub tool: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<crate::tool_report::Call>,
}
impl Activity {
    pub fn valid(&self) -> bool {
        !self.tool.is_empty()
            && self.tool.len() <= 128
            && matches!(self.phase.as_str(), "started" | "completed" | "failed")
            && match (self.phase.as_str(), self.outcome.as_deref()) {
                ("started", None)
                | ("completed", None | Some("succeeded" | "unspecified"))
                | ("failed", None | Some("failed" | "declined")) => true,
                _ => false,
            }
            && self.call.as_ref().is_none_or(|call| call.valid())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Launch {
    LegacyUnspecified,
    Messages {
        input_digest: String,
        model: String,
    },
    ConfiguredGoogle {
        input_digest: String,
        binding: crate::https_profile::Binding,
    },
    ConfiguredChat {
        input_digest: String,
        binding: crate::https_profile::Binding,
    },
    ConfiguredResponses {
        input_digest: String,
        binding: crate::https_profile::Binding,
    },
    ConfiguredMessages {
        input_digest: String,
        binding: crate::https_profile::Binding,
    },
    Fixture {
        input_digest: String,
    },
    Pi {
        input_digest: String,
        workflow: String,
        activity: Vec<Activity>,
    },
    Codex {
        input_digest: String,
        workflow: String,
        selection: Option<crate::codex_model::Selection>,
        #[serde(default)]
        activity: Vec<Activity>,
    },
    #[serde(rename = "opencode")]
    OpenCode {
        input_digest: String,
        workflow: String,
        selection: Option<crate::opencode_model::Selection>,
        activity: Vec<Activity>,
    },
    Text {
        input_digest: String,
        protocol: crate::provider_settings::Protocol,
        binding: crate::https_profile::Binding,
    },
}
impl Launch {
    pub fn input_digest(&self) -> Option<&str> {
        match self {
            Self::LegacyUnspecified => None,
            Self::Messages { input_digest, .. }
            | Self::ConfiguredResponses { input_digest, .. }
            | Self::ConfiguredGoogle { input_digest, .. }
            | Self::ConfiguredChat { input_digest, .. }
            | Self::ConfiguredMessages { input_digest, .. }
            | Self::Fixture { input_digest }
            | Self::Pi { input_digest, .. }
            | Self::Codex { input_digest, .. }
            | Self::OpenCode { input_digest, .. } => Some(input_digest),
            Self::Text { input_digest, .. } => Some(input_digest),
        }
    }
}

pub(super) fn validate(state: &Store) -> Result<()> {
    if state.schema != crate::STORE_VERSION
        || state.launches.len() != state.view.runs.len()
        || state.workflows.len() > 16
    {
        return Err("invalid versioned execution store".into());
    }
    let valid_id = |id: &str| {
        id.strip_prefix(&format!("{}:", state.view.store))
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|v| v > 0 && v < state.next)
    };
    for run in &state.view.runs {
        let launch = state.launches.get(&run.id).ok_or("missing launch record")?;
        if launch
            .input_digest()
            .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("invalid input digest".into());
        }
        if (run.kind == "browser")
            != matches!(
                launch,
                Launch::Messages { .. }
                    | Launch::ConfiguredMessages { .. }
                    | Launch::ConfiguredResponses { .. }
                    | Launch::ConfiguredGoogle { .. }
                    | Launch::ConfiguredChat { .. }
            )
            || matches!(launch, Launch::Messages { model, .. } if model != crate::https_profile::FIXTURE_MODEL)
        {
            return Err("invalid Messages route".into());
        }
        if let Launch::Text { binding, .. } = launch {
            binding.validate()?;
            validate_text_run(run)?;
        }
        if let Launch::ConfiguredMessages { binding, .. }
        | Launch::ConfiguredResponses { binding, .. }
        | Launch::ConfiguredGoogle { binding, .. }
        | Launch::ConfiguredChat { binding, .. } = launch
        {
            binding.validate()?;
        }
        if let Launch::Pi {
            workflow, activity, ..
        } = launch
        {
            let workflow = state.workflows.get(workflow).ok_or("missing workflow")?;
            if run.kind != "agent"
                || run.source != workflow.source.id
                || activity.len() > 16
                || activity.iter().any(|a| !a.valid())
            {
                return Err("invalid Pi task record".into());
            }
        } else if run.kind == "agent"
            && !matches!(launch, Launch::Codex { .. } | Launch::OpenCode { .. })
        {
            return Err("agent task has no native launch".into());
        }
    }
    for (id, w) in &state.workflows {
        w.profile.validate()?;
        crate::model::check_page_fields(&w.source.page)?;
        if !valid_id(id)
            || !valid_id(&w.source.id)
            || w.revision == 0
            || digest(&w.profile)? != w.fingerprint
            || state.view.runs.iter().any(|r| &r.id == id)
            || !matches!(state.launches.get(&w.last_run),Some(Launch::Pi{workflow,..}) if workflow==id)
        {
            return Err("invalid Pi workflow identity".into());
        }
        let identity = match &w.binding {
            Binding::Bound { id, path } => Some((id, path)),
            Binding::ReleaseUnknown {
                previous: Some(session),
            } => Some((&session.id, &session.path)),
            _ => None,
        };
        if let Some((id, path)) = identity {
            if id.is_empty()
                || id.len() > 128
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || path.parent() != Some(w.profile.session_dir.as_path())
                || path.extension().is_none_or(|s| s != "jsonl")
            {
                return Err("invalid native session binding".into());
            }
        }
    }
    Ok(())
}

fn validate_text_run(run: &crate::model::Run) -> Result<()> {
    let valid = (run.kind == "summary" && run.question.is_none())
        || (run.kind == "answer"
            && run
                .question
                .as_ref()
                .is_some_and(|question| !question.trim().is_empty() && question.len() <= 1024));
    if !valid {
        return Err("invalid text task record".into());
    }
    Ok(())
}

#[cfg(test)]
mod text_checks {
    use super::*;

    fn run(kind: &str, question: Option<&str>, native: bool) -> crate::model::Run {
        crate::model::Run {
            id: "store:1".into(),
            source: "store:1".into(),
            kind: kind.into(),
            question: question.map(str::to_owned),
            status: crate::model::Status::Complete,
            output: if native {
                metldr_conversation::Content::native()
            } else {
                Default::default()
            },
            error: None,
            corrections: Vec::new(),
        }
    }

    #[test]
    fn text_launch_accepts_flat_and_native_summary_or_answer() {
        for native in [false, true] {
            assert!(validate_text_run(&run("summary", None, native)).is_ok());
            assert!(validate_text_run(&run("answer", Some("Question?"), native)).is_ok());
            for invalid in [
                run("summary", Some("Question?"), native),
                run("answer", None, native),
                run("answer", Some("  "), native),
            ] {
                assert!(validate_text_run(&invalid).is_err());
            }
        }
    }
}

pub(super) fn legacy_launches(state: &Store) -> BTreeMap<String, Launch> {
    state
        .view
        .runs
        .iter()
        .map(|r| (r.id.clone(), Launch::LegacyUnspecified))
        .collect()
}
