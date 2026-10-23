use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status, valid_request_id},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) const LIMIT: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub request: String,
    pub epoch: u64,
    pub text: String,
    pub state: Phase,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Phase {
    Pending,
    Dispatching,
    CodexAccepted { turn: String },
    PiQueued,
    PiHandled,
    Rejected { message: String },
    NotSent { message: String },
    Uncertain { message: String },
}

impl Phase {
    pub fn unresolved(&self) -> bool {
        matches!(self, Self::Pending | Self::Dispatching)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::CodexAccepted { turn } if turn.is_empty() || turn.len() > 4096 => {
                Err("invalid correction turn".into())
            }
            Self::Rejected { message }
            | Self::NotSent { message }
            | Self::Uncertain { message }
                if message.trim().is_empty() || message.len() > 256 =>
            {
                Err("invalid correction explanation".into())
            }
            _ => Ok(()),
        }
    }
}

impl Receipt {
    pub fn pending(request: &str, epoch: u64, text: &str) -> Result<Self> {
        let receipt = Self {
            request: request.into(),
            epoch,
            text: text.into(),
            state: Phase::Pending,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    fn validate(&self) -> Result<()> {
        if !valid_request_id(&self.request)
            || self.epoch == 0
            || self.text.trim().is_empty()
            || self.text.len() > 1024
        {
            return Err("invalid saved correction".into());
        }
        self.state.validate()
    }

    pub fn resolve(&mut self, next: Phase) -> Result<()> {
        next.validate()?;
        if self.state == next {
            return Ok(());
        }
        if !matches!(
            (&self.state, &next),
            (Phase::Pending, Phase::Dispatching | Phase::NotSent { .. })
                | (
                    Phase::Dispatching,
                    Phase::CodexAccepted { .. }
                        | Phase::PiQueued
                        | Phase::PiHandled
                        | Phase::Rejected { .. }
                        | Phase::NotSent { .. }
                        | Phase::Uncertain { .. }
                )
        ) {
            return Err("correction receipt cannot change outcome or move backwards".into());
        }
        self.state = next;
        Ok(())
    }

    pub fn public(&self, ordinal: usize, live: bool) -> Value {
        let state = match &self.state {
            Phase::CodexAccepted { .. } => json!({"kind":"codex_accepted"}),
            phase => serde_json::to_value(phase).expect("correction phase serializes"),
        };
        json!({"ordinal":ordinal,"request":self.request,
            "text":crate::tool_report::Preview::text(&self.text, if live {256} else {1024}),
            "state":state})
    }
}

pub(crate) fn validate(run: &Run) -> Result<()> {
    let mut ids = BTreeSet::new();
    if run.corrections.len() > LIMIT || (!run.corrections.is_empty() && run.kind != "agent") {
        return Err("invalid correction history".into());
    }
    for (index, receipt) in run.corrections.iter().enumerate() {
        receipt.validate()?;
        if !ids.insert(&receipt.request)
            || (receipt.state.unresolved()
                && (index + 1 != run.corrections.len() || run.status.terminal()))
        {
            return Err("invalid correction ordering or terminal state".into());
        }
    }
    Ok(())
}

pub(crate) fn validate_route(run: &Run, launch: &Launch) -> Result<()> {
    for receipt in &run.corrections {
        if !matches!(launch, Launch::Pi { .. } | Launch::Codex { .. })
            || matches!(
                (&receipt.state, launch),
                (Phase::CodexAccepted { .. }, Launch::Pi { .. })
                    | (Phase::PiQueued | Phase::PiHandled, Launch::Codex { .. })
            )
        {
            return Err("correction receipt has a different native route".into());
        }
    }
    Ok(())
}

pub(crate) fn validate_update(before: &Run, after: &Run) -> Result<()> {
    validate(after)?;
    if after.corrections.len() < before.corrections.len()
        || after.corrections.len() > before.corrections.len() + 1
    {
        return Err("correction history cannot be removed or replaced".into());
    }
    for (old, new) in before.corrections.iter().zip(&after.corrections) {
        if old.request != new.request || old.epoch != new.epoch || old.text != new.text {
            return Err("correction identity or text changed".into());
        }
        let mut expected = old.clone();
        expected.resolve(new.state.clone())?;
    }
    if after.corrections.len() > before.corrections.len()
        && (before.status != Status::Running
            || after.status != Status::Running
            || before
                .corrections
                .last()
                .is_some_and(|r| r.state.unresolved())
            || after
                .corrections
                .last()
                .is_none_or(|r| r.state != Phase::Pending))
    {
        return Err("correction admission requires a running available native task".into());
    }
    Ok(())
}

pub(crate) fn finish(run: &mut Run, unsent: &str, uncertain: &str) -> Result<()> {
    for receipt in &mut run.corrections {
        match receipt.state {
            Phase::Pending => receipt.resolve(Phase::NotSent {
                message: unsent.into(),
            })?,
            Phase::Dispatching => receipt.resolve(Phase::Uncertain {
                message: uncertain.into(),
            })?,
            _ => (),
        }
    }
    Ok(())
}

pub(crate) fn public(run: &Run, live: bool) -> Value {
    let start = if live {
        run.corrections.len().saturating_sub(4)
    } else {
        0
    };
    json!({"total":run.corrections.len(), "entries":run.corrections.iter().enumerate()
        .skip(start).map(|(index,receipt)| receipt.public(index+1,live)).collect::<Vec<_>>()})
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn saved_corrections_keep_identity_and_distinguish_unsent_from_uncertain() {
        let mut run = Run {
            id: "owned:1".into(),
            source: "owned:2".into(),
            kind: "agent".into(),
            question: Some("Read the page.".into()),
            status: Status::Running,
            output: metldr_conversation::Content::native(),
            error: None,
            corrections: vec![Receipt::pending("correction:1", 1, &"🙂".repeat(200)).unwrap()],
        };
        let saved = public(&run, false);
        let live = public(&run, true);
        assert_eq!(saved["entries"][0]["text"]["text"], "🙂".repeat(200));
        assert_eq!(saved["entries"][0]["text"]["truncated"], false);
        assert_eq!(live["entries"][0]["text"]["text"], "🙂".repeat(64));
        assert_eq!(live["entries"][0]["text"]["truncated"], true);
        let admitted = run.clone();
        run.corrections[0].text.push('x');
        assert!(validate_update(&admitted, &run).is_err());
        run = admitted.clone();
        finish(&mut run, "Never sent.", "Delivery unknown.").unwrap();
        assert!(matches!(run.corrections[0].state, Phase::NotSent { .. }));
        assert!(validate_update(&admitted, &run).is_ok());
        assert!(run.corrections[0].resolve(Phase::Dispatching).is_err());
        run = admitted;
        run.corrections[0].resolve(Phase::Dispatching).unwrap();
        let dispatched = run.clone();
        run.status = Status::Interrupted;
        assert!(validate(&run).is_err());
        finish(&mut run, "Never sent.", "Delivery unknown.").unwrap();
        assert!(matches!(run.corrections[0].state, Phase::Uncertain { .. }));
        assert!(validate_update(&dispatched, &run).is_ok());
        assert!(run.corrections[0].resolve(Phase::PiQueued).is_err());
    }
}
