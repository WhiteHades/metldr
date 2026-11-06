use crate::{
    Result,
    browser_action::{Action, Outcome, Proposal},
    flow::{Owner, acknowledge},
};
use std::sync::{atomic::Ordering, mpsc::SyncSender};

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
    pub(super) fn saved_browser_action(&mut self, id: &str) -> Result<Action> {
        self.record_history.edit(|edit| {
            let execution = edit.action_execution(id)?;
            edit.browser_action(&execution, id)
        })
    }

    fn saved_browser_tool(&mut self, run: &str, tool: &str) -> Result<Option<Action>> {
        self.record_history.edit(|edit| {
            let execution = edit.execution(run)?;
            edit.browser_by_tool(&execution, run, tool)
        })
    }

    fn browser_operation(&self, run: &str) -> Result<()> {
        if self
            .operations
            .get(run)
            .is_none_or(|op| op.stop.load(Ordering::Acquire))
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
                let result = (|| -> Result<(Action, bool)> {
                    if let Some(existing) =
                        self.saved_browser_tool(&proposal.run, &proposal.tool_use_id)?
                    {
                        if existing.proposal != proposal {
                            return Err(
                                "browser tool identity was reused with a changed proposal".into()
                            );
                        }
                        return Ok((existing, false));
                    }
                    self.browser_operation(&proposal.run)?;
                    if self.browser_approval.is_some() {
                        return Err("browser approval already pending".into());
                    }
                    if budget.as_ref().is_some_and(Budget::expired) {
                        return Err("browser tool deadline expired before proposal".into());
                    }
                    let run = proposal.run.clone();
                    self.record_execution(&run, |edit, execution| {
                        let (action, created) = edit.propose_browser(execution, proposal)?;
                        Ok((action, created))
                    })
                })();
                match result {
                    Ok((action, false)) => {
                        let _ = ack.send(Ok(action));
                        Ok(())
                    }
                    Ok((action, true)) => {
                        let review = match budget.as_ref().map(Budget::pause).transpose() {
                            Ok(review) => review,
                            Err(error) => return acknowledge(ack, Err(error)),
                        };
                        self.browser_approval = Some(PendingApproval {
                            run: action.proposal.run,
                            action: action.id,
                            ack,
                            review,
                        });
                        Ok(())
                    }
                    Err(error) => acknowledge(ack, Err(error)),
                }
            }
            Event::Dispatch {
                action,
                revision,
                ack,
            } => {
                let result = (|| -> Result<Action> {
                    let record = self.saved_browser_action(&action)?;
                    self.browser_operation(&record.proposal.run)?;
                    let run = record.proposal.run.clone();
                    self.record_execution(&run, |edit, execution| {
                        edit.dispatch_browser(execution, &action, revision)
                    })
                })();
                acknowledge(ack, result)
            }
            Event::Observed {
                action,
                revision,
                outcome,
                ack,
            } => {
                let result = (|| -> Result<Action> {
                    let record = self.saved_browser_action(&action)?;
                    if !self.operations.contains_key(&record.proposal.run) {
                        return Err("browser result came from a stale operation".into());
                    }
                    let run = record.proposal.run.clone();
                    self.record_execution(&run, |edit, execution| {
                        edit.settle_browser(execution, &action, revision, outcome)
                    })
                })();
                acknowledge(ack, result)
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
        let run = pending.run.clone();
        let record = self.record_execution(&run, |edit, execution| {
            edit.decide_browser(execution, action, revision, approve)
        })?;
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
