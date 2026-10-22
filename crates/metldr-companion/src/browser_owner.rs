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
    pub(super) fn saved_browser_action(&mut self, id: &str) -> Result<Action> {
        if let Some(history) = self.record_history.as_mut() {
            let scope = history.selection()?;
            return history.edit(|edit| edit.browser_action(&scope, id));
        }
        self.state
            .browser_actions
            .get(id)
            .cloned()
            .ok_or_else(|| "unknown browser action".into())
    }

    fn saved_browser_tool(&mut self, run: &str, tool: &str) -> Result<Option<Action>> {
        if let Some(history) = self.record_history.as_mut() {
            let scope = history.selection()?;
            return history.edit(|edit| edit.browser_by_tool(&scope, run, tool));
        }
        Ok(self
            .state
            .browser_actions
            .values()
            .find(|action| action.proposal.run == run && action.proposal.tool_use_id == tool)
            .cloned())
    }

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
                    if self.record_history.is_some() {
                        let run = proposal.run.clone();
                        return self.record_change(|edit, scope| {
                            let (action, created) = edit.propose_browser(scope, proposal)?;
                            let projected = if created {
                                Some(edit.project(scope, Some(&run))?)
                            } else {
                                None
                            };
                            Ok(((action, created), projected))
                        });
                    }
                    let mut next = self.state.clone();
                    let action = next.propose_browser(proposal)?;
                    self.commit(next)?;
                    Ok((action, true))
                })();
                match result {
                    Ok((action, false)) => {
                        let _ = ack.send(Ok(action));
                        Ok(())
                    }
                    Ok((action, true)) => {
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
                    let record = self.saved_browser_action(&action)?;
                    self.browser_operation(&record.proposal.run)?;
                    if self.record_history.is_some() {
                        let run = record.proposal.run.clone();
                        return self.record_change(|edit, scope| {
                            let record = edit.dispatch_browser(scope, &action, revision)?;
                            Ok((record, Some(edit.project(scope, Some(&run))?)))
                        });
                    }
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
                    let record = self.saved_browser_action(&action)?;
                    if self
                        .operation
                        .as_ref()
                        .is_none_or(|op| op.run != record.proposal.run)
                    {
                        return Err("browser result came from a stale operation".into());
                    }
                    if self.record_history.is_some() {
                        let run = record.proposal.run.clone();
                        return self.record_change(|edit, scope| {
                            let record = edit.settle_browser(scope, &action, revision, outcome)?;
                            Ok((record, Some(edit.project(scope, Some(&run))?)))
                        });
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
        let record = if self.record_history.is_some() {
            let run = pending.run.clone();
            self.record_change(|edit, scope| {
                let record = edit.decide_browser(scope, action, revision, approve)?;
                Ok((record, Some(edit.project(scope, Some(&run))?)))
            })?
        } else {
            let mut next = self.state.clone();
            let record = next.decide_browser(action, revision, approve)?;
            self.commit(next)?;
            record
        };
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
            if error.downcast_ref::<io::Error>().is_some()
                || crate::history::is_failure(error.as_ref())
            {
                Err(error)
            } else {
                Ok(())
            }
        }
    }
}
