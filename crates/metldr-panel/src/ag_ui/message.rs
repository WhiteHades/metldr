use metldr_conversation::ArtifactId;
use serde::{Deserialize, Serialize};

use super::run_details::RunDetailsReference;
use super::tools::ToolGroupReference;
use crate::{history::RunReference, model::RunId};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum RunOrigin {
    Live,
    Saved { reference: RunReference },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TableReference {
    pub(crate) run_id: RunId,
    pub(crate) artifact_id: ArtifactId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "activityType", content = "content")]
pub(crate) enum Activity {
    #[serde(rename = "metldr.table")]
    Table(TableReference),
    #[serde(rename = "metldr.native-tools")]
    NativeTools(ToolGroupReference),
    #[serde(rename = "metldr.run-details")]
    RunDetails(RunDetailsReference),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "role",
    rename_all = "lowercase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum AgUiMessage {
    User {
        id: String,
        content: String,
    },
    Assistant {
        id: String,
        content: String,
    },
    Activity {
        id: String,
        #[serde(flatten)]
        activity: Activity,
    },
}

impl AgUiMessage {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::User { id, .. } | Self::Assistant { id, .. } | Self::Activity { id, .. } => id,
        }
    }

    pub(super) fn id_mut(&mut self) -> &mut String {
        match self {
            Self::User { id, .. } | Self::Assistant { id, .. } | Self::Activity { id, .. } => id,
        }
    }

    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::User { content, .. } | Self::Assistant { content, .. } => Some(content),
            Self::Activity { .. } => None,
        }
    }

    pub(crate) fn table(run: &RunId, artifact_id: &ArtifactId) -> Self {
        Self::Activity {
            id: format!("{run}:artifact:{}", artifact_id.as_str()),
            activity: Activity::Table(TableReference {
                run_id: run.clone(),
                artifact_id: artifact_id.clone(),
            }),
        }
    }

    pub(crate) fn native_tools(reference: ToolGroupReference) -> Self {
        Self::Activity {
            id: reference.message_id(),
            activity: Activity::NativeTools(reference),
        }
    }

    pub(crate) fn run_details(reference: RunDetailsReference) -> Self {
        Self::Activity {
            id: reference.message_id(),
            activity: Activity::RunDetails(reference),
        }
    }
}
