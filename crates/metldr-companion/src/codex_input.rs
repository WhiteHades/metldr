use crate::{Result, flow::Owner};
use metldr_codex_session::{InputAnswer, InputId, InputPrompt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::mpsc::SyncSender};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TurnMode {
    Configured,
    Plan,
}
impl TurnMode {
    pub(super) fn native(self) -> metldr_codex_session::TurnMode {
        match self {
            Self::Configured => metldr_codex_session::TurnMode::Configured,
            Self::Plan => metldr_codex_session::TurnMode::Plan,
        }
    }
}

#[derive(Clone)]
pub(super) struct Pending {
    pub(super) key: InputId,
    pub(super) prompt: InputPrompt,
    pub(super) answer: SyncSender<InputAnswer>,
}

#[derive(Clone)]
pub(super) struct State {
    pub(super) run: String,
    pub(super) pending: BTreeMap<String, Pending>,
}

impl State {
    pub(super) fn public(&self) -> Result<Value> {
        let pending = self
            .pending
            .iter()
            .map(|(request, pending)| json!({"request":request,"prompt":prompt(&pending.prompt)}))
            .collect::<Vec<_>>();
        let value = json!({"run":self.run,"pending":pending});
        if serde_json::to_vec(&value)?.len() > 32 * 1024 {
            return Err("Codex input view exceeds bound".into());
        }
        Ok(value)
    }
}

fn prompt(prompt: &InputPrompt) -> Value {
    match prompt {
        InputPrompt::Questions {
            item,
            is_blocking,
            questions,
        } => {
            json!({"kind":"questions","item":item,"is_blocking":is_blocking,"questions":questions.iter().map(|q| json!({
            "id":q.id,"header":q.header,"text":q.text,"other":q.other,
            "options":q.options.iter().map(|o| json!({"label":o.label,"description":o.description})).collect::<Vec<_>>()
        })).collect::<Vec<_>>()})
        }
        InputPrompt::McpForm {
            server,
            message,
            fields,
        } => {
            json!({"kind":"mcp_form","server":server,"message":message,"fields":fields.iter().map(|f| json!({
            "name":f.name,"title":f.title,"description":f.description,"min_chars":f.min_chars,
            "max_chars":f.max_chars,"choices":f.choices,"default":f.default
        })).collect::<Vec<_>>()})
        }
        InputPrompt::McpToolApproval {
            server,
            message,
            review,
        } => json!({"kind":"mcp_tool_approval","server":server,"message":message,"review":review}),
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Answer {
    Questions {
        value: BTreeMap<String, Vec<String>>,
    },
    Accept {
        value: BTreeMap<String, String>,
    },
    Decline,
    Cancel,
}

impl Answer {
    pub(super) fn native(self) -> InputAnswer {
        match self {
            Self::Questions { value } => InputAnswer::Questions(value),
            Self::Accept { value } => InputAnswer::Accept(value),
            Self::Decline => InputAnswer::Decline,
            Self::Cancel => InputAnswer::Cancel,
        }
    }
}

impl Owner {
    pub(super) fn codex_input_offer(
        &mut self,
        run: &str,
        key: InputId,
        prompt: InputPrompt,
        answer: SyncSender<InputAnswer>,
    ) -> Result<()> {
        if !self.native_input_active(run, crate::agent_settings::Kind::Codex)? {
            let _ = answer.try_send(InputAnswer::Cancel);
            return Ok(());
        }
        let fallback_answer = answer.clone();
        let previous = self.codex_input.clone();
        let mut next = self.state.clone();
        let request = match next.id() {
            Ok(request) => request,
            Err(error) => {
                return self.fail_codex_input(run, fallback_answer, Some(&error.to_string()));
            }
        };
        if self
            .codex_input
            .as_ref()
            .is_some_and(|state| state.run == run && state.pending.len() >= 8)
        {
            return self.fail_codex_input(run, fallback_answer, None);
        }
        if self
            .codex_input
            .as_ref()
            .is_some_and(|state| state.run == run && state.pending.values().any(|p| p.key == key))
        {
            return self.fail_codex_input(run, fallback_answer, None);
        }
        if self
            .codex_input
            .as_ref()
            .is_none_or(|state| state.run != run)
        {
            self.codex_input = Some(State {
                run: run.into(),
                pending: BTreeMap::new(),
            });
        }
        let state = self
            .codex_input
            .as_mut()
            .ok_or("missing Codex input state")?;
        if state.pending.contains_key(&request) {
            return self.fail_codex_input(run, fallback_answer, None);
        }
        state.pending.insert(
            request.clone(),
            Pending {
                key,
                prompt,
                answer,
            },
        );
        let projected = match state.public() {
            Ok(value) => value,
            Err(error) => {
                return self.fail_codex_input(run, fallback_answer, Some(&error.to_string()));
            }
        };
        drop(projected);
        if let Err(error) = self.commit_native_input(run, crate::agent_settings::Kind::Codex, next)
        {
            if crate::native_input_records::fatal(error.as_ref()) {
                let _ = fallback_answer.try_send(InputAnswer::Cancel);
                return Err(error);
            }
            if self.record_history.is_some() {
                self.codex_input = previous;
                self.stop_native_record_inputs(
                    run,
                    "Codex input could not be presented",
                    Some(&error.to_string()),
                    true,
                )?;
                let _ = fallback_answer.try_send(InputAnswer::Cancel);
                return Ok(());
            }
            self.codex_input = previous;
            return self.fail_codex_input(run, fallback_answer, Some(&error.to_string()));
        }
        Ok(())
    }

    fn fail_codex_input(
        &mut self,
        run: &str,
        answer: SyncSender<InputAnswer>,
        error: Option<&str>,
    ) -> Result<()> {
        if self.record_history.is_some() {
            self.stop_native_record_inputs(run, "Codex input could not be presented", error, true)?;
            let _ = answer.try_send(InputAnswer::Cancel);
            return Ok(());
        }
        if let Some(operation) = &self.operation {
            if operation.run == run {
                operation
                    .stop
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let _ = answer.try_send(InputAnswer::Cancel);
        self.clear_codex_input(run);
        let mut next = self.state.clone();
        if let Some(record) = next.view.runs.iter_mut().find(|r| r.id == run) {
            record.status = crate::model::Status::StopRequested;
            record.error = Some("Codex input could not be presented".into());
        }
        self.commit(next)
    }

    pub(super) fn codex_input_reply(
        &mut self,
        id: &str,
        run: &str,
        request: &str,
        answer: Answer,
    ) -> Result<()> {
        if !self.native_input_active(run, crate::agent_settings::Kind::Codex)? {
            return Err("Codex input is no longer accepting replies".into());
        }
        let state = self
            .codex_input
            .as_ref()
            .filter(|state| state.run == run)
            .ok_or("Codex input request is stale")?;
        let pending = state
            .pending
            .get(request)
            .ok_or("Codex input request is stale")?;
        let answer = answer.native();
        if !pending.prompt.accepts(&answer) {
            return Err("Codex input answer is invalid".into());
        }
        let mut state = self
            .codex_input
            .clone()
            .ok_or("Codex input request is stale")?;
        let pending = state
            .pending
            .remove(request)
            .ok_or("Codex input request is stale")?;
        let previous = self.codex_input.replace(state);
        if let Err(error) =
            self.commit_native_input(run, crate::agent_settings::Kind::Codex, self.state.clone())
        {
            if !crate::native_input_records::fatal(error.as_ref()) {
                self.codex_input = previous;
            }
            return Err(error);
        }
        match pending.answer.try_send(answer) {
            Ok(()) => self.reply_view(Some(id), "view", true),
            Err(_) => Err("Codex input answer could not be delivered".into()),
        }
    }

    pub(super) fn codex_input_withdraw(&mut self, run: &str, key: &InputId) -> Result<()> {
        let previous = self.codex_input.clone();
        let Some(state) = self.codex_input.as_mut().filter(|state| state.run == run) else {
            return Ok(());
        };
        let before = state.pending.len();
        state.pending.retain(|_, pending| &pending.key != key);
        if state.pending.len() == before {
            return Ok(());
        }
        if state.pending.is_empty() {
            self.codex_input = None;
        }
        if self.record_history.is_some() {
            if let Err(error) = self.commit_native_input(
                run,
                crate::agent_settings::Kind::Codex,
                self.state.clone(),
            ) {
                if !crate::native_input_records::fatal(error.as_ref()) {
                    self.codex_input = previous;
                }
                return Err(error);
            }
            return Ok(());
        }
        self.publish()
    }

    pub(super) fn clear_codex_input(&mut self, run: &str) {
        if self
            .codex_input
            .as_ref()
            .is_some_and(|state| state.run == run)
        {
            self.codex_input = None;
        }
    }

    pub(super) fn disconnect_codex_input(&mut self) -> Result<()> {
        let Some(run) = self.codex_input.as_ref().map(|state| state.run.clone()) else {
            return Ok(());
        };
        let Some(operation) = self.operation.as_ref().filter(|op| op.run == run) else {
            self.clear_codex_input(&run);
            return self.publish();
        };
        if self.record_history.is_some() {
            return self.stop_native_record_inputs(
                &run,
                "Codex controller disconnected",
                None,
                false,
            );
        }
        operation
            .stop
            .store(true, std::sync::atomic::Ordering::Release);
        let mut next = self.state.clone();
        if let Some(record) = next.view.runs.iter_mut().find(|r| r.id == run)
            && !record.status.terminal()
        {
            record.status = crate::model::Status::StopRequested;
        }
        next.prevent_browser(&run, "Codex controller disconnected")?;
        self.cancel_browser_approval(&run);
        self.cancel_native_permissions(&run);
        self.clear_codex_input(&run);
        self.persist(next)
    }
}
