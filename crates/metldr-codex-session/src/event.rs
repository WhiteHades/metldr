use crate::{CodexSession, Result, TurnId, field, invalid};
use serde_json::{Value, json};

#[derive(Debug, PartialEq, Eq)]
pub enum TurnStatus {
    Completed,
    Interrupted,
    Failed,
}

pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub(crate) request: Value,
    pub(crate) connection: u64,
    pub(crate) turn: TurnId,
}

pub enum Event {
    SteerResult {
        request: crate::SteerId,
        outcome: crate::SteerOutcome,
    },
    TextDelta {
        item_id: String,
        text: String,
    },
    Message {
        item_id: String,
        text: String,
    },
    ToolCall(ToolCall),
    Input(crate::Input),
    InputResolved(crate::InputId),
    Approval(crate::Approval),
    ApprovalResolved(crate::ApprovalId),
    ApprovalsUpdated(Vec<(crate::ApprovalId, crate::Approval)>),
    ToolReport(crate::ToolReport),
    ToolProgress {
        turn: TurnId,
        item_id: String,
        output: crate::Preview,
    },
    Finished {
        turn: TurnId,
        status: TurnStatus,
    },
}

impl CodexSession {
    pub(crate) async fn event(&mut self, value: Value) -> Result<Option<Event>> {
        if value.get("method").is_none() {
            return self.steer_result(value).map(Some);
        }
        let method = field(&value, "method")?;
        let params = &value["params"];
        let approval = matches!(
            method.as_str(),
            "item/commandExecution/requestApproval"
                | "item/fileChange/requestApproval"
                | "item/permissions/requestApproval"
        );
        let input = matches!(
            method.as_str(),
            "item/tool/requestUserInput" | "mcpServer/elicitation/request"
        );
        if value.get("id").is_some() && method != "item/tool/call" && !approval && !input {
            self.rpc.send(json!({"id":value["id"],"error":{"code":-32601,"message":"This client does not support this server request"}})).await?;
            return Err(invalid("Unsupported Codex server request"));
        }
        if method == "serverRequest/resolved" {
            if params["threadId"] != self.info.id.as_str() {
                return Err(invalid("Codex resolution thread mismatch"));
            }
            let request = params
                .get("requestId")
                .filter(|id| id.is_string() || id.is_i64() || id.is_u64())
                .ok_or_else(|| invalid("Codex resolution request identity missing"))?;
            if let Some(id) = self.inputs.resolved(request) {
                return Ok(Some(Event::InputResolved(id)));
            }
            return Ok(self
                .approvals
                .resolved(request)
                .map(Event::ApprovalResolved));
        }
        if input {
            let turn = input_scope(
                params,
                self.info.id.as_str(),
                self.turn.as_ref(),
                method == "item/tool/requestUserInput",
            )?;
            let request = value
                .get("id")
                .cloned()
                .ok_or_else(|| invalid("Codex input request identity missing"))?;
            if self.pending_tools.contains(&request) || self.approvals.contains(&request) {
                return Err(invalid("Codex server request identity repeated"));
            }
            return Ok(Some(Event::Input(self.inputs.insert(
                self.connection,
                turn,
                request,
                &method,
                params,
            )?)));
        }
        if !approval
            && !matches!(
                method.as_str(),
                "item/tool/call"
                    | "item/agentMessage/delta"
                    | "item/started"
                    | "item/completed"
                    | "item/commandExecution/outputDelta"
                    | "item/fileChange/patchUpdated"
                    | "turn/completed"
            )
        {
            return Ok(None);
        }
        let active = self
            .turn
            .as_ref()
            .ok_or_else(|| invalid("No active Codex turn"))?;
        if approval {
            scope(params, self.info.id.as_str(), active.as_str(), false)?;
            let request = value
                .get("id")
                .cloned()
                .ok_or_else(|| invalid("Codex approval request identity missing"))?;
            if self.pending_tools.contains(&request) || self.inputs.contains(&request) {
                return Err(invalid("Codex server request identity repeated"));
            }
            let changes = if method == "item/fileChange/requestApproval" {
                Some(self.reports.changes(&field(params, "itemId")?)?)
            } else {
                None
            };
            return Ok(Some(Event::Approval(self.approvals.insert(
                self.connection,
                active,
                request,
                &method,
                params,
                changes,
            )?)));
        }
        let terminal = method == "turn/completed";
        scope(params, self.info.id.as_str(), active.as_str(), terminal)?;
        Ok(match method.as_str() {
            "item/tool/call" => {
                let id = value
                    .get("id")
                    .filter(|id| id.is_string() || id.is_u64())
                    .cloned()
                    .ok_or_else(|| invalid("Codex tool request identity missing"))?;
                if self.pending_tools.len() >= 32
                    || self.pending_tools.contains(&id)
                    || self.approvals.contains(&id)
                    || self.inputs.contains(&id)
                {
                    return Err(invalid("Codex tool request limit or duplicate identity"));
                }
                let call = ToolCall {
                    call_id: field(params, "callId")?,
                    name: field(params, "tool")?,
                    arguments: params
                        .get("arguments")
                        .cloned()
                        .ok_or_else(|| invalid("Codex tool arguments missing"))?,
                    request: id.clone(),
                    connection: self.connection,
                    turn: active.clone(),
                };
                self.pending_tools.push(id);
                Some(Event::ToolCall(call))
            }
            "item/agentMessage/delta" => Some(Event::TextDelta {
                item_id: field(params, "itemId")?,
                text: params["delta"]
                    .as_str()
                    .ok_or_else(|| invalid("Codex text delta missing"))?
                    .to_owned(),
            }),
            "item/completed" if params["item"]["type"] == "agentMessage" => Some(Event::Message {
                item_id: field(&params["item"], "id")?,
                text: params["item"]["text"]
                    .as_str()
                    .ok_or_else(|| invalid("Codex message text missing"))?
                    .to_owned(),
            }),
            "item/started" | "item/completed" => self
                .reports
                .decode(params, method == "item/started", active)?
                .map(Event::ToolReport),
            "item/commandExecution/outputDelta" => {
                self.reports
                    .progress(params, active)?
                    .map(|(item_id, output)| Event::ToolProgress {
                        turn: active.clone(),
                        item_id,
                        output,
                    })
            }
            "item/fileChange/patchUpdated" => {
                let item = self.reports.patch(params)?;
                if self.approvals.file_pending(&item) {
                    let updated = self
                        .approvals
                        .refresh(&item, self.reports.changes(&item)?)?;
                    (!updated.is_empty()).then_some(Event::ApprovalsUpdated(updated))
                } else {
                    None
                }
            }
            "turn/completed" => {
                let status = match params["turn"]["status"].as_str() {
                    Some("completed") => TurnStatus::Completed,
                    Some("interrupted") => TurnStatus::Interrupted,
                    Some("failed") => TurnStatus::Failed,
                    _ => return Err(invalid("Codex terminal status invalid")),
                };
                if status == TurnStatus::Completed && !self.approvals.is_empty() {
                    return Err(invalid("Codex completed with unanswered approvals"));
                }
                self.approvals.clear();
                self.pending_tools.clear();
                self.reports.clear();
                Some(Event::Finished {
                    turn: self.turn.take().unwrap(),
                    status,
                })
            }
            _ => None,
        })
    }
}

fn input_scope(
    params: &Value,
    thread: &str,
    active: Option<&TurnId>,
    question: bool,
) -> Result<Option<TurnId>> {
    if params["threadId"] != thread {
        return Err(invalid("Codex input thread mismatch"));
    }
    match params.get("turnId") {
        None | Some(Value::Null) if !question => Ok(None),
        Some(Value::String(turn)) if active.is_some_and(|id| id.as_str() == turn) => {
            Ok(active.cloned())
        }
        _ => Err(invalid("Codex input turn mismatch")),
    }
}

fn scope(params: &Value, thread: &str, turn: &str, terminal: bool) -> Result<()> {
    if params["threadId"] != thread {
        return Err(invalid("Codex event thread mismatch"));
    }
    let observed = if terminal {
        &params["turn"]["id"]
    } else {
        &params["turnId"]
    };
    if observed != turn {
        return Err(invalid("Codex event turn mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod checks {
    use super::*;
    #[test]
    fn native_notifications_keep_thread_and_turn_scope() {
        let mut params = json!({"threadId":"thread-1","turnId":"turn-2"});
        assert!(scope(&params, "thread-1", "turn-2", false).is_ok());
        assert!(scope(&params, "thread-other", "turn-2", false).is_err());
        assert!(scope(&params, "thread-1", "turn-other", false).is_err());
        params["turn"] = json!({"id":"turn-2"});
        assert!(scope(&params, "thread-1", "turn-2", true).is_ok());
        let active = TurnId("turn-2".into());
        params["turnId"] = Value::Null;
        assert!(
            input_scope(&params, "thread-1", Some(&active), false)
                .unwrap()
                .is_none()
        );
        assert!(input_scope(&params, "thread-1", None, false).is_ok());
        assert!(input_scope(&params, "thread-1", Some(&active), true).is_err());
        params["turnId"] = json!("turn-2");
        assert!(input_scope(&params, "thread-1", Some(&active), true).is_ok());
        assert!(input_scope(&params, "thread-1", None, false).is_err());
    }
}
