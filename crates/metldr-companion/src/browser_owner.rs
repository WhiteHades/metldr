use crate::{
    Result,
    browser_action::{Action, Outcome, Proposal},
    flow::Owner,
};
use std::{
    io,
    sync::{atomic::Ordering, mpsc::SyncSender},
};

type Reply = SyncSender<std::result::Result<Action, String>>;

mod review;
pub(crate) use review::Budget;

pub(super) enum Event {
    Proposed {
        proposal: Proposal,
        ack: Reply,
        budget: Option<Budget>,
    },
    Dispatch {
        action: String,
        revision: u64,
        ack: Reply,
    },
    Observed {
        action: String,
        revision: u64,
        outcome: Outcome,
        ack: Reply,
    },
}

pub(super) struct PendingApproval {
    pub run: String,
    pub action: String,
    pub ack: Reply,
    review: Option<review::Review>,
}

impl PendingApproval {
    fn reply(self, result: std::result::Result<Action, String>) {
        drop(self.review);
        let _ = self.ack.send(result);
    }
}

impl Owner {
    fn browser_operation(&self, run: &str) -> Result<()> {
        if self
            .operation
            .as_ref()
            .is_none_or(|op| op.run != run || op.stop.load(Ordering::Acquire))
        {
            return Err("browser operation is stale or stopped".into());
        }
        Ok(())
    }

    pub(super) fn browser_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Proposed {
                proposal,
                ack,
                budget,
            } => {
                let existing = self.state.browser_actions.values().any(|a| {
                    a.proposal.run == proposal.run && a.proposal.tool_use_id == proposal.tool_use_id
                });
                let result = (|| -> Result<Action> {
                    if !existing {
                        self.browser_operation(&proposal.run)?;
                    }
                    if !existing && self.browser_approval.is_some() {
                        return Err("browser approval already pending".into());
                    }
                    if !existing && budget.as_ref().is_some_and(Budget::expired) {
                        return Err("browser tool deadline expired before proposal".into());
                    }
                    let mut next = self.state.clone();
                    let action = next.propose_browser(proposal)?;
                    if existing {
                        return Ok(action);
                    }
                    self.commit(next)?;
                    Ok(action)
                })();
                match result {
                    Ok(action) if existing => {
                        let _ = ack.send(Ok(action));
                        Ok(())
                    }
                    Ok(action) => {
                        let review = match budget.as_ref().map(Budget::pause).transpose() {
                            Ok(review) => review,
                            Err(error) => return reply(ack, Err(error)),
                        };
                        self.browser_approval = Some(PendingApproval {
                            run: action.proposal.run,
                            action: action.id,
                            ack,
                            review,
                        });
                        Ok(())
                    }
                    Err(error) => reply(ack, Err(error)),
                }
            }
            Event::Dispatch {
                action,
                revision,
                ack,
            } => {
                let result = (|| -> Result<Action> {
                    let record = self
                        .state
                        .browser_actions
                        .get(&action)
                        .ok_or("unknown browser action")?;
                    self.browser_operation(&record.proposal.run)?;
                    let mut next = self.state.clone();
                    let record = next.dispatch_browser(&action, revision)?;
                    // The worker receives permission only after the dispatch record is durable.
                    self.commit(next)?;
                    Ok(record)
                })();
                reply(ack, result)
            }
            Event::Observed {
                action,
                revision,
                outcome,
                ack,
            } => {
                let result = (|| -> Result<Action> {
                    let record = self
                        .state
                        .browser_actions
                        .get(&action)
                        .ok_or("unknown browser action")?;
                    if self
                        .operation
                        .as_ref()
                        .is_none_or(|op| op.run != record.proposal.run)
                    {
                        return Err("browser result came from a stale operation".into());
                    }
                    let mut next = self.state.clone();
                    let record = next.settle_browser(&action, revision, outcome)?;
                    self.commit(next)?;
                    Ok(record)
                })();
                reply(ack, result)
            }
        }
    }

    pub(super) fn browser_decide(
        &mut self,
        action: &str,
        revision: u64,
        approve: bool,
    ) -> Result<()> {
        let pending = self
            .browser_approval
            .as_ref()
            .filter(|p| p.action == action)
            .ok_or("browser action is not awaiting this approval")?;
        self.browser_operation(&pending.run)?;
        let mut next = self.state.clone();
        let record = next.decide_browser(action, revision, approve)?;
        self.commit(next)?;
        if let Some(pending) = self.browser_approval.take() {
            pending.reply(Ok(record));
        }
        Ok(())
    }

    pub(super) fn cancel_browser_approval(&mut self, run: &str) {
        if self.browser_approval.as_ref().is_some_and(|p| p.run == run)
            && let Some(pending) = self.browser_approval.take()
        {
            pending.reply(Err("browser action stopped before dispatch".into()));
        }
    }
}

fn reply(ack: Reply, result: Result<Action>) -> Result<()> {
    match result {
        Ok(record) => {
            let _ = ack.send(Ok(record));
            Ok(())
        }
        Err(error) => {
            let _ = ack.send(Err(error.to_string()));
            if error.downcast_ref::<io::Error>().is_some() {
                Err(error)
            } else {
                Ok(())
            }
        }
    }
}
