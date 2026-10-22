use crate::{
    Result,
    agent_model::Launch,
    model::{Source, Store},
};

#[derive(Clone, serde::Serialize)]
pub(crate) enum NativeWorkflow {
    Pi(crate::agent_model::Workflow),
    Codex(crate::codex_model::Workflow),
    OpenCode(crate::opencode_model::Workflow),
}

impl NativeWorkflow {
    pub(crate) fn from_store(state: &Store, launch: &Launch) -> Result<Self> {
        match launch {
            Launch::Pi { workflow, .. } => state.workflows.get(workflow).cloned().map(Self::Pi),
            Launch::Codex { workflow, .. } => state
                .codex_workflows
                .get(workflow)
                .cloned()
                .map(Self::Codex),
            Launch::OpenCode { workflow, .. } => state
                .opencode_workflows
                .get(workflow)
                .cloned()
                .map(Self::OpenCode),
            _ => return Err("callback has no native launch".into()),
        }
        .ok_or_else(|| "callback native workflow is absent".into())
    }
    pub(super) fn pending(&self) -> bool {
        matches!(
            self,
            Self::Pi(crate::agent_model::Workflow {
                binding: crate::agent_model::Binding::Pending,
                ..
            }) | Self::Codex(crate::codex_model::Workflow {
                binding: crate::codex_model::Binding::Pending,
                ..
            }) | Self::OpenCode(crate::opencode_model::Workflow {
                binding: crate::opencode_model::Binding::Pending,
                ..
            })
        )
    }

    pub(super) fn bound(&self) -> bool {
        matches!(
            self,
            Self::Pi(crate::agent_model::Workflow {
                binding: crate::agent_model::Binding::Bound { .. },
                ..
            }) | Self::Codex(crate::codex_model::Workflow {
                binding: crate::codex_model::Binding::Bound { .. },
                ..
            }) | Self::OpenCode(crate::opencode_model::Workflow {
                binding: crate::opencode_model::Binding::Bound { .. },
                ..
            })
        )
    }

    pub(super) fn validate_continuation(&self, after: &Self, run: &str) -> Result<()> {
        if !self.bound()
            || after.last_run() != run
            || self.revision().checked_add(1) != Some(after.revision())
        {
            return Err("native continuation is not ready or changed revision".into());
        }
        let mut restored = after.clone();
        macro_rules! restore {
            ($value:expr) => {{
                $value.last_run = self.last_run().into();
                $value.revision = self.revision();
            }};
        }
        match &mut restored {
            Self::Pi(value) => restore!(value),
            Self::Codex(value) => restore!(value),
            Self::OpenCode(value) => restore!(value),
        }
        if serde_json::to_vec(self)? != serde_json::to_vec(&restored)? {
            return Err("native continuation changed its profile source or binding".into());
        }
        Ok(())
    }

    pub(crate) fn source(&self) -> &Source {
        match self {
            Self::Pi(workflow) => &workflow.source,
            Self::Codex(workflow) => &workflow.source,
            Self::OpenCode(workflow) => &workflow.source,
        }
    }

    pub(super) fn revision(&self) -> u64 {
        match self {
            Self::Pi(workflow) => workflow.revision,
            Self::Codex(workflow) => workflow.revision,
            Self::OpenCode(workflow) => workflow.revision,
        }
    }

    pub(super) fn last_run(&self) -> &str {
        match self {
            Self::Pi(workflow) => &workflow.last_run,
            Self::Codex(workflow) => &workflow.last_run,
            Self::OpenCode(workflow) => &workflow.last_run,
        }
    }

    pub(super) fn matches(&self, launch: &Launch) -> bool {
        matches!(
            (self, launch),
            (Self::Pi(_), Launch::Pi { .. })
                | (Self::Codex(_), Launch::Codex { .. })
                | (Self::OpenCode(_), Launch::OpenCode { .. })
        )
    }

    pub(crate) fn install(&self, id: &str, state: &mut Store) {
        match self {
            Self::Pi(workflow) => {
                state.workflows.insert(id.to_owned(), workflow.clone());
            }
            Self::Codex(workflow) => {
                state
                    .codex_workflows
                    .insert(id.to_owned(), workflow.clone());
            }
            Self::OpenCode(workflow) => {
                state
                    .opencode_workflows
                    .insert(id.to_owned(), workflow.clone());
            }
        }
    }

    pub(super) fn validate_update(&self, after: &Self) -> Result<()> {
        let invalid = || "invalid native workflow update";
        let (
            profile_same,
            fingerprint_same,
            source_same,
            last_run_same,
            before_revision,
            after_revision,
        ) = match (self, after) {
            (Self::Pi(before), Self::Pi(after)) => (
                before.profile == after.profile,
                before.fingerprint == after.fingerprint,
                before.source.id == after.source.id && before.source.page == after.source.page,
                before.last_run == after.last_run,
                before.revision,
                after.revision,
            ),
            (Self::Codex(before), Self::Codex(after)) => (
                before.profile == after.profile,
                before.fingerprint == after.fingerprint,
                before.source.id == after.source.id && before.source.page == after.source.page,
                before.last_run == after.last_run,
                before.revision,
                after.revision,
            ),
            (Self::OpenCode(before), Self::OpenCode(after)) => (
                before.profile == after.profile,
                before.fingerprint == after.fingerprint,
                before.source.id == after.source.id && before.source.page == after.source.page,
                before.last_run == after.last_run,
                before.revision,
                after.revision,
            ),
            _ => return Err(invalid().into()),
        };
        if !profile_same
            || !fingerprint_same
            || !source_same
            || !last_run_same
            || !(after_revision == before_revision
                || before_revision.checked_add(1) == Some(after_revision))
        {
            return Err(invalid().into());
        }

        let binding_valid = match (self, after) {
            (Self::Pi(before), Self::Pi(after)) => match (&before.binding, &after.binding) {
                (crate::agent_model::Binding::Pending, crate::agent_model::Binding::Pending)
                | (
                    crate::agent_model::Binding::Pending,
                    crate::agent_model::Binding::Bound { .. },
                )
                | (
                    crate::agent_model::Binding::Pending,
                    crate::agent_model::Binding::ReleaseUnknown { previous: None },
                ) => true,
                (
                    crate::agent_model::Binding::Bound {
                        id: before_id,
                        path: before_path,
                    },
                    crate::agent_model::Binding::Bound {
                        id: after_id,
                        path: after_path,
                    },
                ) => before_id == after_id && before_path == after_path,
                (
                    crate::agent_model::Binding::Bound { id, path },
                    crate::agent_model::Binding::ReleaseUnknown {
                        previous: Some(previous),
                    },
                ) => id == &previous.id && path == &previous.path,
                (
                    crate::agent_model::Binding::ReleaseUnknown { previous: before },
                    crate::agent_model::Binding::ReleaseUnknown { previous: after },
                ) => match (before, after) {
                    (None, None) => true,
                    (Some(before), Some(after)) => {
                        before.id == after.id && before.path == after.path
                    }
                    _ => false,
                },
                _ => false,
            },
            (Self::Codex(before), Self::Codex(after)) => match (&before.binding, &after.binding) {
                (crate::codex_model::Binding::Pending, crate::codex_model::Binding::Pending)
                | (
                    crate::codex_model::Binding::Pending,
                    crate::codex_model::Binding::Bound { .. },
                )
                | (
                    crate::codex_model::Binding::Pending,
                    crate::codex_model::Binding::ReleaseUnknown { previous: None },
                ) => true,
                (
                    crate::codex_model::Binding::Bound { identity: before },
                    crate::codex_model::Binding::Bound { identity: after },
                ) => before == after,
                (
                    crate::codex_model::Binding::Bound { identity },
                    crate::codex_model::Binding::ReleaseUnknown {
                        previous: Some(previous),
                    },
                ) => identity == previous,
                (
                    crate::codex_model::Binding::ReleaseUnknown { previous: before },
                    crate::codex_model::Binding::ReleaseUnknown { previous: after },
                ) => before == after,
                _ => false,
            },
            (Self::OpenCode(before), Self::OpenCode(after)) => {
                match (&before.binding, &after.binding) {
                    (
                        crate::opencode_model::Binding::Pending,
                        crate::opencode_model::Binding::Pending,
                    )
                    | (
                        crate::opencode_model::Binding::Pending,
                        crate::opencode_model::Binding::Bound { .. },
                    )
                    | (
                        crate::opencode_model::Binding::Pending,
                        crate::opencode_model::Binding::ReleaseUnknown { previous: None },
                    ) => true,
                    (
                        crate::opencode_model::Binding::Bound { identity: before },
                        crate::opencode_model::Binding::Bound { identity: after },
                    ) => before == after,
                    (
                        crate::opencode_model::Binding::Bound { identity },
                        crate::opencode_model::Binding::ReleaseUnknown {
                            previous: Some(previous),
                        },
                    ) => identity == previous,
                    (
                        crate::opencode_model::Binding::ReleaseUnknown { previous: before },
                        crate::opencode_model::Binding::ReleaseUnknown { previous: after },
                    ) => before == after,
                    _ => false,
                }
            }
            _ => false,
        };
        if binding_valid {
            Ok(())
        } else {
            Err(invalid().into())
        }
    }
}
