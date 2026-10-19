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
    TextDelta { item_id: String, text: String },
    Message { item_id: String, text: String },
    ToolCall(ToolCall),
    Finished { turn: TurnId, status: TurnStatus },
}

impl CodexSession {
    pub(crate) async fn event(&mut self, value: Value) -> Result<Option<Event>> {
        let method = field(&value, "method")?;
        let params = &value["params"];
        let active = self
            .turn
            .as_ref()
            .ok_or_else(|| invalid("No active Codex turn"))?;
        if value.get("id").is_some() && method != "item/tool/call" {
            self.rpc.send(json!({"id":value["id"],"error":{"code":-32601,"message":"This client does not support this server request"}})).await?;
            return Err(invalid("Unsupported Codex server request"));
        }
        if !matches!(
            method.as_str(),
            "item/tool/call" | "item/agentMessage/delta" | "item/completed" | "turn/completed"
        ) {
            return Ok(None);
        }
        if params["threadId"] != self.info.id.0 {
            return Err(invalid("Codex event thread mismatch"));
        }
        let terminal = method == "turn/completed";
        let turn = if terminal {
            &params["turn"]["id"]
        } else {
            &params["turnId"]
        };
        if turn != active.as_str() {
            return Err(invalid("Codex event turn mismatch"));
        }
        Ok(match method.as_str() {
            "item/tool/call" => {
                let id = value
                    .get("id")
                    .filter(|id| id.is_string() || id.is_u64())
                    .cloned()
                    .ok_or_else(|| invalid("Codex tool request identity missing"))?;
                if self.pending_tools.len() >= 32 || self.pending_tools.contains(&id) {
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
            "turn/completed" => {
                let status = match params["turn"]["status"].as_str() {
                    Some("completed") => TurnStatus::Completed,
                    Some("interrupted") => TurnStatus::Interrupted,
                    Some("failed") => TurnStatus::Failed,
                    _ => return Err(invalid("Codex terminal status invalid")),
                };
                self.pending_tools.clear();
                Some(Event::Finished {
                    turn: self.turn.take().unwrap(),
                    status,
                })
            }
            _ => None,
        })
    }
}
