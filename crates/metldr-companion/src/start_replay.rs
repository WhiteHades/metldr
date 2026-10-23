use crate::{
    Result,
    agent_model::{Launch, digest},
    flow::Owner,
    history::Selection,
    model::ConversationAction as Action,
};
use serde_json::json;

impl Owner {
    pub(super) fn replay_start(
        &mut self,
        request: &str,
        scope: &Selection,
        action: &Action,
    ) -> Result<bool> {
        match action {
            Action::Summarize { .. }
            | Action::Ask { .. }
            | Action::PageText { .. }
            | Action::BrowserTask { .. }
            | Action::ResponsesTask { .. }
            | Action::GoogleTask { .. }
            | Action::ChatTask { .. }
            | Action::PiPrompt { .. }
            | Action::PiFollowUp { .. }
            | Action::CodexPrompt { .. }
            | Action::CodexFollowUp { .. }
            | Action::OpenCodePrompt { .. }
            | Action::OpenCodeFollowUp { .. } => (),
            Action::Capture { .. }
            | Action::NativeCorrection { .. }
            | Action::Watch { .. }
            | Action::Unwatch { .. }
            | Action::Stop { .. }
            | Action::BrowserDecision { .. }
            | Action::NativePermission { .. }
            | Action::PiUiReply { .. }
            | Action::CodexInputReply { .. } => return Ok(false),
        }
        let launch = self
            .record_history
            .as_mut()
            .ok_or("record history is unavailable")?
            .edit(|edit| edit.request_launch(scope, request))?;
        let Some(launch) = launch else {
            return Ok(false);
        };
        if launch.input_digest().is_none() {
            return Err("legacy start identity has no verifiable input or route; inspect its saved view instead".into());
        }
        let origin = self.fixture.as_ref().map(|fixture| fixture.origin.as_str());
        let identity = match (action, &launch) {
            (Action::Summarize { .. } | Action::Ask { .. }, Launch::Fixture { .. }) => {
                let origin = origin.ok_or("original fixture route is unavailable")?;
                json!({"command":action,"origin":origin,"model":"metldr-fixture"})
            }
            (
                Action::PageText {
                    source,
                    protocol,
                    fingerprint,
                    prompt,
                },
                Launch::Text {
                    protocol: original,
                    binding,
                    ..
                },
            ) if protocol == original && fingerprint == &binding.fingerprint => {
                json!({"operation":"page_text","source":source,"protocol":protocol,"binding":binding,"prompt":prompt})
            }
            (
                Action::BrowserTask {
                    source,
                    text,
                    fingerprint: None,
                },
                Launch::Messages { model, .. },
            ) => {
                json!({"source":source,"text":text,"origin":origin,"model":model})
            }
            (
                Action::BrowserTask {
                    source,
                    text,
                    fingerprint: Some(fingerprint),
                },
                Launch::ConfiguredMessages { binding, .. },
            ) if fingerprint == &binding.fingerprint => {
                json!({"source":source,"text":text,"origin":origin,"route":binding.public()})
            }
            (
                Action::ResponsesTask {
                    source,
                    text,
                    fingerprint,
                },
                Launch::ConfiguredResponses { binding, .. },
            ) if fingerprint == &binding.fingerprint => {
                json!({"source":source,"text":text,"origin":origin,"route":binding.public_responses()})
            }
            (
                Action::GoogleTask {
                    source,
                    text,
                    fingerprint,
                },
                Launch::ConfiguredGoogle { binding, .. },
            ) if fingerprint == &binding.fingerprint => {
                json!({"source":source,"text":text,"origin":origin,"route":binding.public_google()})
            }
            (
                Action::ChatTask {
                    source,
                    text,
                    fingerprint,
                },
                Launch::ConfiguredChat { binding, .. },
            ) if fingerprint == &binding.fingerprint => {
                json!({"source":source,"text":text,"origin":origin,"route":binding.public_chat_completions()})
            }
            (Action::PiPrompt { .. } | Action::PiFollowUp { .. }, Launch::Pi { .. })
            | (Action::CodexPrompt { .. } | Action::CodexFollowUp { .. }, Launch::Codex { .. })
            | (
                Action::OpenCodePrompt { .. } | Action::OpenCodeFollowUp { .. },
                Launch::OpenCode { .. },
            ) => {
                return self.replay_native_start(request, scope, action);
            }
            _ => {
                return Err(
                    "request identity already names a different input route or task kind".into(),
                );
            }
        };
        self.duplicate(scope, request, &digest(&identity)?)
    }
}
