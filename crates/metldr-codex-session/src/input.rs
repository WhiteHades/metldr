use crate::{Result, TurnId, invalid};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod checks;
mod form;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputId {
    connection: u64,
    generation: u64,
}

pub struct Input {
    id: InputId,
    prompt: InputPrompt,
    turn: Option<TurnId>,
}
impl Input {
    pub fn id(&self) -> &InputId {
        &self.id
    }
    pub fn prompt(&self) -> &InputPrompt {
        &self.prompt
    }
    pub fn turn(&self) -> Option<&TurnId> {
        self.turn.as_ref()
    }
}

#[derive(Clone)]
pub enum InputPrompt {
    Questions {
        item: String,
        is_blocking: bool,
        questions: Vec<Question>,
    },
    McpForm {
        server: String,
        message: String,
        fields: Vec<FormField>,
    },
    McpToolApproval {
        server: String,
        message: String,
        review: String,
    },
}
#[derive(Clone)]
pub struct Question {
    pub id: String,
    pub header: String,
    pub text: String,
    pub options: Vec<QuestionOption>,
    pub other: bool,
}
#[derive(Clone)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}
#[derive(Clone)]
pub struct FormField {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub min_chars: usize,
    pub max_chars: usize,
    pub choices: Option<Vec<String>>,
    pub default: Option<String>,
}
pub enum InputAnswer {
    Questions(BTreeMap<String, Vec<String>>),
    Accept(BTreeMap<String, String>),
    Decline,
    Cancel,
}
impl InputPrompt {
    pub fn accepts(&self, answer: &InputAnswer) -> bool {
        form::encode(self, answer).is_ok()
    }
}

struct Pending {
    input: Input,
    request: Value,
    answered: bool,
}
#[derive(Default)]
pub(crate) struct Inputs {
    generation: u64,
    pending: Vec<Pending>,
}

impl Inputs {
    pub(crate) fn contains(&self, request: &Value) -> bool {
        self.pending.iter().any(|p| p.request == *request)
    }
    pub(crate) fn turn_pending(&self) -> bool {
        self.pending.iter().any(|p| p.input.turn.is_some())
    }
    pub(crate) fn insert(
        &mut self,
        connection: u64,
        turn: Option<TurnId>,
        request: Value,
        method: &str,
        params: &Value,
    ) -> Result<Input> {
        if self.pending.len() >= 8
            || self.contains(&request)
            || serde_json::to_vec(params)?.len() > 16 * 1024
            || !native_id(&request)
        {
            return Err(invalid(
                "Codex input request is invalid or exceeds its bound",
            ));
        }
        let prompt = if method == "item/tool/requestUserInput" {
            if turn.is_none() {
                return Err(invalid("Question input requires an active turn"));
            }
            form::questions(params)?
        } else if method == "mcpServer/elicitation/request" {
            form::elicitation(params)?
        } else {
            return Err(invalid("Unsupported Codex input method"));
        };
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("Codex input identity exhausted"))?;
        let input = Input {
            id: InputId {
                connection,
                generation: self.generation,
            },
            prompt,
            turn,
        };
        self.pending.push(Pending {
            input: Input {
                id: input.id.clone(),
                prompt: input.prompt.clone(),
                turn: input.turn.clone(),
            },
            request,
            answered: false,
        });
        Ok(input)
    }
    pub(crate) fn response(
        &self,
        input: &Input,
        answer: InputAnswer,
        current: Option<&TurnId>,
    ) -> Result<(usize, Value, Value)> {
        let index = self
            .pending
            .iter()
            .position(|p| {
                p.input.id == input.id
                    && !p.answered
                    && p.input
                        .turn
                        .as_ref()
                        .is_none_or(|turn| Some(turn) == current)
            })
            .ok_or_else(|| invalid("Codex input is no longer pending or turn is stale"))?;
        let result = form::encode(&self.pending[index].input.prompt, &answer)?;
        Ok((index, self.pending[index].request.clone(), result))
    }
    pub(crate) fn answered(&mut self, index: usize) {
        if let Some(p) = self.pending.get_mut(index) {
            p.answered = true;
        }
    }
    pub(crate) fn resolved(&mut self, request: &Value) -> Option<InputId> {
        let index = self.pending.iter().position(|p| p.request == *request)?;
        Some(self.pending.swap_remove(index).input.id)
    }
}

fn native_id(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 256)
        || v.as_i64().is_some()
        || v.as_u64().is_some()
}
fn bounded_string(v: &Value, max: usize, nonempty: bool) -> Result<String> {
    let s = v
        .as_str()
        .ok_or_else(|| invalid("Codex input field is unsupported"))?;
    if s.len() > max || (nonempty && s.is_empty()) {
        return Err(invalid("Codex input field is unsupported"));
    }
    Ok(s.to_owned())
}
fn controlled_id(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}

impl crate::CodexSession {
    pub async fn reply_input(&mut self, input: &Input, answer: InputAnswer) -> Result<()> {
        if self.failed {
            return Err(invalid("Codex session has failed"));
        }
        let (index, request, result) = self.inputs.response(input, answer, self.turn.as_ref())?;
        self.rpc
            .send(serde_json::json!({"id":request,"result":result}))
            .await?;
        self.inputs.answered(index);
        Ok(())
    }
}
