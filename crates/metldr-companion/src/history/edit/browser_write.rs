use super::{Edit, Selection, record, storage};
use crate::{
    Result,
    agent_model::Launch,
    browser_action::{Action, Outcome, Phase, Proposal, Target},
    history::schema::*,
    model::{Run, Status},
};
use redb::ReadableTableMetadata;

impl Edit<'_> {
    pub(crate) fn propose_browser(
        &mut self,
        scope: &Selection,
        proposal: Proposal,
    ) -> Result<(Action, bool)> {
        let result = (|| {
            self.require_selection(scope)?;
            proposal.validate()?;
            if let Some(action) =
                self.browser_by_tool(scope, &proposal.run, &proposal.tool_use_id)?
            {
                if action.proposal != proposal {
                    return Err("browser tool identity was reused with a changed proposal".into());
                }
                return Ok((action, false));
            }
            let initial = self.browser_running(scope, &proposal.run)?;
            let navigation = self.browser_navigation(scope, &proposal.run)?;
            if !proposal.matches_position(
                Some(&initial),
                navigation.as_ref().map(|(b, u)| (b, u.as_str())),
            ) {
                return Err("browser proposal does not match the saved destination".into());
            }
            if storage(|| Ok(self.write.open_table(OPEN_ACTIONS)?.len()? != 0))? {
                return Err("another browser action is pending".into());
            }
            let id = self.id()?;
            let number = self.new_record_id(&id)?;
            let action = Action::pending(id, proposal)?;
            storage(|| {
                self.write.open_table(ACTIONS)?.insert(
                    action.id.as_str(),
                    encode(&Record {
                        conversation: scope.conversation.clone(),
                        revision: 1,
                        value: &action,
                    })?
                    .as_slice(),
                )?;
                if self
                    .write
                    .open_table(ACTION_ORDER)?
                    .insert(
                        (action.proposal.run.as_str(), number, action.id.as_str()),
                        action.id.as_str(),
                    )?
                    .is_some()
                    || self
                        .write
                        .open_table(ACTION_BY_TOOL)?
                        .insert(
                            (
                                action.proposal.run.as_str(),
                                action.proposal.tool_use_id.as_str(),
                            ),
                            action.id.as_str(),
                        )?
                        .is_some()
                    || self
                        .write
                        .open_table(OPEN_ACTIONS)?
                        .insert(action.id.as_str(), action.proposal.run.as_str())?
                        .is_some()
                {
                    return Err("browser admission index collides".into());
                }
                Ok(())
            })?;
            self.changed = true;
            self.browser_action(scope, &action.id)?;
            Ok((action, true))
        })();
        self.checked(result)
    }

    pub(crate) fn decide_browser(
        &mut self,
        scope: &Selection,
        id: &str,
        revision: u64,
        approve: bool,
    ) -> Result<Action> {
        let result = (|| {
            let action = self.browser_action(scope, id)?;
            self.browser_running(scope, &action.proposal.run)?;
            let next = action.decide(revision, approve)?;
            self.put_browser(scope, next)
        })();
        self.checked(result)
    }

    pub(crate) fn dispatch_browser(
        &mut self,
        scope: &Selection,
        id: &str,
        revision: u64,
    ) -> Result<Action> {
        let result = (|| {
            let action = self.browser_action(scope, id)?;
            self.browser_running(scope, &action.proposal.run)?;
            let next = action.dispatch(revision)?;
            self.put_browser(scope, next)
        })();
        self.checked(result)
    }

    pub(crate) fn settle_browser(
        &mut self,
        scope: &Selection,
        id: &str,
        revision: u64,
        outcome: Outcome,
    ) -> Result<Action> {
        let result = (|| {
            let action = self.browser_action(scope, id)?;
            let next = action.settle(revision, outcome)?;
            self.put_browser(scope, next)
        })();
        self.checked(result)
    }

    pub(crate) fn prevent_browser(
        &mut self,
        scope: &Selection,
        run: &str,
        reason: &str,
    ) -> Result<()> {
        let result = (|| {
            self.run(scope, run)?;
            if let Some(action) = self.browser_open(scope, run)?
                && let Some(next) = action.prevent(reason)?
            {
                self.put_browser(scope, next)?;
            }
            Ok(())
        })();
        self.checked(result)
    }

    pub(crate) fn finish_browser(&mut self, scope: &Selection, run: &str) -> Result<bool> {
        let result = (|| {
            self.run(scope, run)?;
            if let Some(action) = self.browser_open(scope, run)?
                && let Some(next) = action.finish(
                    "worker ended before dispatch",
                    "worker ended without a verified browser result",
                )?
            {
                self.put_browser(scope, next)?;
            }
            self.uncertain_browser(scope, run)
        })();
        self.checked(result)
    }

    pub(super) fn require_browser_finalized(&self, scope: &Selection, run: &Run) -> Result<()> {
        if run.status.terminal() && self.browser_open(scope, &run.id)?.is_some() {
            return Err("browser actions must finish before task completion".into());
        }
        Ok(())
    }

    fn browser_running(&self, scope: &Selection, id: &str) -> Result<Target> {
        let run = self.run(scope, id)?;
        if run.status != Status::Running {
            return Err("browser run is not active".into());
        }
        let launch: Record<Launch> =
            storage(|| record(self.write, LAUNCHES, id)?.ok_or("browser launch is absent".into()))?;
        if !matches!(
            launch.value,
            Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
        ) && self
            .selected_source(scope)?
            .is_none_or(|source| source.id != run.source)
        {
            return Err("browser source is not current".into());
        }
        self.browser_initial(scope, id)?
            .ok_or("browser source content is unavailable".into())
    }

    pub(super) fn put_browser(&mut self, scope: &Selection, next: Action) -> Result<Action> {
        let before = self.browser_record(scope, &next.id)?;
        next.validate()?;
        if next.proposal != before.value.proposal
            || before.value.revision.checked_add(1) != Some(next.revision)
        {
            return Err("browser update changed identity or skipped a revision".into());
        }
        if matches!(
            next.state,
            Phase::Settled {
                outcome: Outcome::Navigated { .. }
            }
        ) {
            let initial = self.browser_initial(scope, &next.proposal.run)?;
            let previous = self.browser_navigation(scope, &next.proposal.run)?;
            if !next.proposal.matches_position(
                initial.as_ref(),
                previous.as_ref().map(|(b, u)| (b, u.as_str())),
            ) {
                return Err("navigation does not follow the current destination".into());
            }
        }
        storage(|| {
            self.write.open_table(ACTIONS)?.insert(
                next.id.as_str(),
                encode(&Record {
                    conversation: scope.conversation.clone(),
                    revision: before
                        .revision
                        .checked_add(1)
                        .ok_or("browser record revision exhausted")?,
                    value: &next,
                })?
                .as_slice(),
            )?;
            if matches!(next.state, Phase::Settled { .. }) {
                self.write
                    .open_table(OPEN_ACTIONS)?
                    .remove(next.id.as_str())?;
            }
            match &next.state {
                Phase::Settled {
                    outcome: Outcome::Navigated { binding, url, .. },
                } => {
                    if self
                        .write
                        .open_table(NAVIGATION_GRANTS)?
                        .insert(
                            (next.proposal.run.as_str(), binding.grant_revision),
                            next.id.as_str(),
                        )?
                        .is_some()
                    {
                        return Err("navigation grant already exists".into());
                    }
                    self.write
                        .open_table(NAVIGATION_HEADS)?
                        .insert(next.proposal.run.as_str(), next.id.as_str())?;
                    let launch: Record<Launch> = record(self.write, LAUNCHES, &next.proposal.run)?
                        .ok_or("navigation launch is absent")?;
                    if matches!(
                        launch.value,
                        Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
                    ) {
                        let saved: Record<NativeTarget> =
                            record(self.write, NATIVE_TARGETS, &next.proposal.run)?
                                .ok_or("native destination is absent")?;
                        let mut target = saved.value;
                        target.current = Target {
                            tab: binding.tab,
                            document: binding.document.clone(),
                            url: url.clone(),
                        };
                        target.navigation = Some(next.id.clone());
                        self.write.open_table(NATIVE_TARGETS)?.insert(
                            next.proposal.run.as_str(),
                            encode(&Record {
                                conversation: scope.conversation.clone(),
                                revision: saved
                                    .revision
                                    .checked_add(1)
                                    .ok_or("native destination revision exhausted")?,
                                value: target,
                            })?
                            .as_slice(),
                        )?;
                    }
                }
                Phase::Settled {
                    outcome: Outcome::Uncertain { .. },
                } => {
                    self.write
                        .open_table(UNCERTAIN_RUNS)?
                        .insert(next.proposal.run.as_str(), next.id.as_str())?;
                }
                _ => (),
            }
            Ok(())
        })?;
        self.changed = true;
        self.browser_action(scope, &next.id)?;
        Ok(next)
    }
}
