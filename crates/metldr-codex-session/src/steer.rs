use crate::{CodexSession, Event, Result, TurnId, field, invalid, rpc};
use serde_json::{Value, json};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteerId {
    connection: u64,
    request: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SteerOutcome {
    Accepted { turn: TurnId },
    Rejected { code: i64 },
}

pub(crate) struct Pending {
    id: SteerId,
    turn: TurnId,
}

impl CodexSession {
    /// Sends input to the expected active turn. Acceptance arrives as `Event::SteerResult`.
    pub async fn steer(&mut self, expected: &TurnId, text: &str) -> Result<SteerId> {
        if self.failed
            || self.steering.is_some()
            || self.turn.as_ref() != Some(expected)
            || text.trim().is_empty()
            || text.len() > 1024
        {
            return Err(invalid("Codex correction is invalid, pending or stale"));
        }
        let request = self
            .rpc
            .begin_steer(json!({
                "threadId": self.info.id.as_str(),
                "expectedTurnId": expected.as_str(),
                "input": [{"type":"text", "text":text, "text_elements":[]}],
            }))
            .await?;
        let id = SteerId {
            connection: self.connection,
            request,
        };
        self.steering = Some(Pending {
            id: id.clone(),
            turn: expected.clone(),
        });
        Ok(id)
    }

    pub(crate) fn steer_result(&mut self, value: Value) -> Result<Event> {
        let pending = self
            .steering
            .as_ref()
            .filter(|p| value["id"].as_u64() == Some(p.id.request))
            .ok_or_else(|| invalid("Codex steering response identity mismatch"))?;
        let outcome = match rpc::response(&value) {
            Ok(result) => {
                let turn = TurnId(field(&result, "turnId")?);
                if turn != pending.turn {
                    return Err(invalid("Codex steering response turn mismatch"));
                }
                SteerOutcome::Accepted { turn }
            }
            Err(error) => match error.downcast_ref::<rpc::Rejected>() {
                Some(rejected) => SteerOutcome::Rejected {
                    code: rejected.code,
                },
                None => return Err(error),
            },
        };
        self.rpc.finish_steer(pending.id.request)?;
        let pending = self.steering.take().unwrap();
        Ok(Event::SteerResult {
            request: pending.id,
            outcome,
        })
    }
}
