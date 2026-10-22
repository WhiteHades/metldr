use metldr_conversation::{Display, Projection};

use super::{
    ActivityOutcome, ActivityPhase, FlowError, MAX_ACTIVITY, MAX_PAGE_TEXT, RunId, RunKind,
    RunRoute, RunView, WireRun, parse_identity, valid_ascii_label, valid_fingerprint,
    valid_https_endpoint, valid_label,
};

const MAX_RUN_DOCUMENT: usize = 2 * 1024 * 1024;

impl WireRun {
    pub(super) fn into_view(self, store: &str) -> Result<RunView, FlowError> {
        let valid_id = |id: &str| parse_identity(id).is_ok_and(|(id_store, _)| id_store == store);
        if !valid_id(self.id.as_str())
            || !valid_id(self.source.as_str())
            || self.output.len() > MAX_PAGE_TEXT
            || self.question.as_ref().is_some_and(|text| {
                text.is_empty()
                    || text.len() > 1_024
                    || (!matches!(&self.route, RunRoute::LegacyUnspecified)
                        && text.trim().is_empty())
            })
            || self.error.as_ref().is_some_and(|text| text.len() > 256)
            || self.activity.len() > MAX_ACTIVITY
            || self.activity.iter().any(|item| {
                !valid_label(&item.tool, 128)
                    || !matches!(
                        (item.phase, item.outcome),
                        (ActivityPhase::Started, None)
                            | (
                                ActivityPhase::Completed,
                                None | Some(
                                    ActivityOutcome::Succeeded | ActivityOutcome::Unspecified
                                )
                            )
                            | (
                                ActivityPhase::Failed,
                                None | Some(ActivityOutcome::Failed | ActivityOutcome::Declined)
                            )
                    )
                    || item.call.as_ref().is_some_and(|call| {
                        call.id.is_empty()
                            || call.id.len() > 128
                            || call.id.chars().any(char::is_control)
                            || call
                                .title
                                .as_ref()
                                .is_some_and(|preview| preview.text.len() > 256)
                            || call
                                .input
                                .as_ref()
                                .is_some_and(|preview| preview.text.len() > 1_024)
                            || call
                                .output
                                .as_ref()
                                .is_some_and(|preview| preview.text.len() > 2_048)
                    })
            })
            || (!self.activity.is_empty()
                && !matches!(
                    &self.route,
                    RunRoute::Pi { .. } | RunRoute::Codex { .. } | RunRoute::OpenCode { .. }
                ))
            || matches!(&self.route, RunRoute::Text { .. } if !self.activity.is_empty()
            || !self.conversation.is_empty()
            || match self.kind {
                RunKind::Summary => self.question.is_some(),
                RunKind::Answer => self.question.as_ref().is_none_or(|question| question.trim().is_empty()),
                _ => true,
            })
            || (matches!(self.kind, RunKind::Browser)
                != matches!(
                    &self.route,
                    RunRoute::Messages { .. }
                        | RunRoute::HttpsMessages { .. }
                        | RunRoute::HttpsChatCompletions { .. }
                        | RunRoute::HttpsResponses { .. }
                        | RunRoute::HttpsGoogleInteractions { .. }
                ))
            || matches!(&self.route, RunRoute::Messages { model } if model != "fixture-browser-model")
            || matches!(&self.route, RunRoute::HttpsMessages { endpoint, model, revision, fingerprint }
                if !valid_https_endpoint(endpoint) || !valid_ascii_label(model, 128) || *revision == 0 || !valid_fingerprint(fingerprint))
            || matches!(&self.route, RunRoute::HttpsChatCompletions { endpoint, model, revision, fingerprint }
                if !valid_https_endpoint(endpoint) || !valid_ascii_label(model, 128) || *revision == 0 || !valid_fingerprint(fingerprint))
            || matches!(&self.route, RunRoute::HttpsResponses { endpoint, model, revision, fingerprint }
                if !valid_https_endpoint(endpoint) || !valid_ascii_label(model, 128) || *revision == 0 || !valid_fingerprint(fingerprint))
            || matches!(&self.route, RunRoute::HttpsGoogleInteractions { endpoint, model, revision, fingerprint }
                if !valid_https_endpoint(endpoint) || !valid_ascii_label(model, 128) || *revision == 0 || !valid_fingerprint(fingerprint))
            || matches!(&self.route, RunRoute::Text { endpoint, model, revision, fingerprint, .. }
                if !valid_https_endpoint(endpoint) || !valid_ascii_label(model, 128) || *revision == 0 || !valid_fingerprint(fingerprint))
            || (matches!(self.kind, RunKind::Agent)
                != matches!(
                    &self.route,
                    RunRoute::Pi { .. } | RunRoute::Codex { .. } | RunRoute::OpenCode { .. }
                ))
            || matches!(&self.route, RunRoute::Pi { workflow, provider, model }
                if !valid_id(workflow.as_str()) || !valid_label(provider, 128) || !valid_label(model, 128))
            || matches!(&self.route, RunRoute::Codex { workflow, provider, model }
                if !valid_id(workflow.as_str())
                    || provider.is_some() != model.is_some()
                    || provider.as_ref().is_some_and(|value| !valid_label(value, 128))
                    || model.as_ref().is_some_and(|value| !valid_label(value, 128)))
            || matches!(&self.route, RunRoute::OpenCode { workflow, model, mode }
                if !valid_id(workflow.as_str())
                    || model.is_some() != mode.is_some()
                    || model.as_ref().is_some_and(|value| !valid_label(value, 128))
                    || mode.as_ref().is_some_and(|value| !valid_label(value, 128)))
        {
            return Err(FlowError::Protocol("invalid saved run".into()));
        }
        let output = Display::new(Projection {
            output: self.output,
            conversation: self.conversation,
        })
        .map_err(|_| FlowError::Protocol("invalid saved run conversation".into()))?;
        Ok(RunView {
            id: self.id,
            source: self.source,
            kind: self.kind,
            question: self.question,
            status: self.status,
            output,
            error: self.error,
            route: self.route,
            activity: self.activity,
        })
    }
}

impl RunView {
    pub(crate) fn decode_record(bytes: &[u8], expected: &RunId) -> Result<Self, FlowError> {
        if bytes.len() > MAX_RUN_DOCUMENT {
            return Err(FlowError::Protocol(
                "saved run exceeds 2097152 bytes".into(),
            ));
        }
        let run: WireRun = serde_json::from_slice(bytes)
            .map_err(|error| FlowError::Protocol(error.to_string()))?;
        if run.id != *expected {
            return Err(FlowError::Protocol("saved run identity changed".into()));
        }
        let (store, _) = parse_identity(expected.as_str()).map_err(FlowError::Protocol)?;
        run.into_view(store)
    }
}
