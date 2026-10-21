use crate::{CodexSession, Result, TurnId, field, invalid};
use serde_json::{Value, json};

#[cfg(test)]
mod checks;
mod choices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalId {
    connection: u64,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalKind {
    Command,
    FileChange,
    Permissions,
}

#[derive(Clone, Debug)]
pub struct ApprovalChoice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Approval {
    id: ApprovalId,
    kind: ApprovalKind,
    review: Value,
    choices: Vec<ApprovalChoice>,
}

impl Approval {
    pub fn id(&self) -> &ApprovalId {
        &self.id
    }
    pub fn kind(&self) -> ApprovalKind {
        self.kind
    }
    pub fn review(&self) -> &Value {
        &self.review
    }
    pub fn choices(&self) -> &[ApprovalChoice] {
        &self.choices
    }
}

struct Pending {
    approval: Approval,
    request: Value,
    turn: TurnId,
    item: String,
    responses: Vec<Value>,
}

#[derive(Default)]
pub(crate) struct Approvals {
    generation: u64,
    pending: Vec<Pending>,
}

impl Approvals {
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    pub(crate) fn contains(&self, request: &Value) -> bool {
        self.pending.iter().any(|p| p.request == *request)
    }

    pub(crate) fn insert(
        &mut self,
        connection: u64,
        turn: &TurnId,
        request: Value,
        method: &str,
        params: &Value,
        changes: Option<Value>,
    ) -> Result<Approval> {
        if self.pending.len() >= 32 || self.contains(&request) {
            return Err(invalid("Codex approval limit or duplicate request"));
        }
        let item = field(params, "itemId")?;
        if !request.is_string() && !request.is_i64() && !request.is_u64() {
            return Err(invalid("Codex approval request identity invalid"));
        }
        let kind = match method {
            "item/commandExecution/requestApproval" => ApprovalKind::Command,
            "item/fileChange/requestApproval" => ApprovalKind::FileChange,
            "item/permissions/requestApproval" => ApprovalKind::Permissions,
            _ => return Err(invalid("Codex approval kind invalid")),
        };
        let mut review = params.clone();
        let object = review
            .as_object_mut()
            .ok_or_else(|| invalid("Codex approval missing"))?;
        for key in ["threadId", "turnId", "itemId", "startedAtMs", "approvalId"] {
            object.remove(key);
        }
        if kind == ApprovalKind::FileChange {
            object.insert(
                "changes".into(),
                changes.ok_or_else(|| invalid("Complete Codex file review unavailable"))?,
            );
        }
        if serde_json::to_vec(&review)?.len() > 32 * 1024 {
            return Err(invalid("Complete Codex approval review exceeds bound"));
        }
        let (names, responses) = choices::decode(kind, params)?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("Codex approval identity exhausted"))?;
        let approval = Approval {
            id: ApprovalId {
                connection,
                generation: self.generation,
            },
            kind,
            review,
            choices: names
                .into_iter()
                .enumerate()
                .map(|(i, name)| ApprovalChoice {
                    id: i.to_string(),
                    name,
                })
                .collect(),
        };
        self.pending.push(Pending {
            approval: approval.clone(),
            request,
            turn: turn.clone(),
            item,
            responses,
        });
        Ok(approval)
    }

    pub(crate) fn resolved(&mut self, request: &Value) -> Option<ApprovalId> {
        let index = self.pending.iter().position(|p| p.request == *request)?;
        Some(self.pending.swap_remove(index).approval.id)
    }

    pub(crate) fn file_pending(&self, item: &str) -> bool {
        self.pending
            .iter()
            .any(|p| p.item == item && p.approval.kind == ApprovalKind::FileChange)
    }

    pub(crate) fn refresh(
        &mut self,
        item: &str,
        changes: Value,
    ) -> Result<Vec<(ApprovalId, Approval)>> {
        let mut updated = Vec::new();
        for pending in self
            .pending
            .iter_mut()
            .filter(|p| p.item == item && p.approval.kind == ApprovalKind::FileChange)
        {
            if pending.approval.review["changes"] == changes {
                continue;
            }
            pending.approval.review["changes"] = changes.clone();
            if serde_json::to_vec(&pending.approval.review)?.len() > 32 * 1024 {
                return Err(invalid("Updated Codex approval review exceeds bound"));
            }
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("Codex approval identity exhausted"))?;
            let previous = pending.approval.id.clone();
            pending.approval.id.generation = self.generation;
            updated.push((previous, pending.approval.clone()));
        }
        Ok(updated)
    }

    fn response(
        &self,
        id: &ApprovalId,
        turn: &TurnId,
        option: Option<&str>,
    ) -> Result<(usize, Value, bool)> {
        let index = self
            .pending
            .iter()
            .position(|p| p.approval.id == *id && p.turn == *turn)
            .ok_or_else(|| invalid("Codex approval is no longer pending"))?;
        let pending = &self.pending[index];
        let response = if let Some(option) = option {
            let choice = pending
                .approval
                .choices
                .iter()
                .position(|c| c.id == option)
                .ok_or_else(|| invalid("Codex approval choice was not offered"))?;
            pending.responses[choice].clone()
        } else if pending.approval.kind == ApprovalKind::Permissions {
            json!({"permissions":{},"scope":"turn"})
        } else {
            json!({"decision":"cancel"})
        };
        let cancel = response["decision"] == "cancel";
        Ok((
            index,
            json!({"id":pending.request,"result":response}),
            cancel,
        ))
    }
}

impl CodexSession {
    /// Returns true only when the native reply requests turn cancellation.
    pub async fn reply_approval(
        &mut self,
        approval: &Approval,
        option: Option<&str>,
    ) -> Result<bool> {
        if self.failed || approval.id.connection != self.connection {
            return Err(invalid("Codex approval belongs to another connection"));
        }
        let turn = self
            .turn
            .as_ref()
            .ok_or_else(|| invalid("No active Codex turn"))?;
        let (index, response, cancel) = self.approvals.response(&approval.id, turn, option)?;
        if option.is_none() && approval.kind != ApprovalKind::Permissions {
            if self.interrupt().await? != crate::Interrupt::Acknowledged {
                return Err(invalid("Codex did not acknowledge approval cancellation"));
            }
            self.approvals.pending.swap_remove(index);
            return Ok(true);
        }
        self.rpc.send(response).await?;
        self.approvals.pending.swap_remove(index);
        Ok(cancel)
    }
}
