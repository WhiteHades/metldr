use super::{Edit, Selection, record, storage};
use crate::{
    Result,
    agent_model::Launch,
    browser_action::{Action, Binding, Outcome, Phase, Target},
    history::schema::*,
};
use redb::{ReadableTable, ReadableTableMetadata};

impl Edit<'_> {
    pub(crate) fn browser_action(&self, scope: &Selection, id: &str) -> Result<Action> {
        self.checked((|| {
            let saved = self.browser_record(scope, id)?;
            let action = saved.value;
            let initial = self.browser_initial(scope, &action.proposal.run)?;
            let grant = if action.proposal.binding.grant_revision == 1 {
                None
            } else {
                Some(self.browser_grant(
                    scope,
                    &action.proposal.run,
                    action.proposal.binding.grant_revision,
                )?)
            };
            storage(|| {
                if !action.proposal.matches_position(
                    initial.as_ref(),
                    grant.as_ref().map(|(b, u)| (b, u.as_str())),
                ) {
                    return Err("saved browser proposal has no matching destination grant".into());
                }
                Ok(())
            })?;
            if !matches!(action.state, Phase::Settled { .. }) {
                let head = self.browser_navigation(scope, &action.proposal.run)?;
                storage(|| {
                    if !action.proposal.matches_position(
                        initial.as_ref(),
                        head.as_ref().map(|(b, u)| (b, u.as_str())),
                    ) {
                        return Err(
                            "open browser action does not use the current destination".into()
                        );
                    }
                    Ok(())
                })?;
            }
            Ok(action)
        })())
    }

    pub(crate) fn browser_by_tool(
        &self,
        scope: &Selection,
        run: &str,
        tool: &str,
    ) -> Result<Option<Action>> {
        self.checked((|| {
            self.require_selection(scope)?;
            self.run(scope, run)?;
            let id = storage(|| {
                Ok(self
                    .write
                    .open_table(ACTION_BY_TOOL)?
                    .get((run, tool))?
                    .map(|v| v.value().to_owned()))
            })?;
            id.map(|id| self.browser_action(scope, &id)).transpose()
        })())
    }

    pub(super) fn browser_record(&self, scope: &Selection, id: &str) -> Result<Record<Action>> {
        self.require_selection(scope)?;
        storage(|| {
            let saved: Record<Action> =
                record(self.write, ACTIONS, id)?.ok_or("browser action is absent")?;
            let a = &saved.value;
            a.validate()?;
            if saved.conversation != scope.conversation || a.id != id || !self.valid_id(id) {
                return Err("browser action ownership or identity differs".into());
            }
            let run = self
                .saved_run(&a.proposal.run)?
                .ok_or("browser action run is absent")?;
            let number = id
                .rsplit(':')
                .next()
                .ok_or("invalid browser action identity")?
                .parse::<u64>()?;
            if run.conversation != scope.conversation
                || self
                    .write
                    .open_table(ACTION_ORDER)?
                    .get((a.proposal.run.as_str(), number, id))?
                    .is_none_or(|v| v.value() != id)
                || self
                    .write
                    .open_table(ACTION_BY_TOOL)?
                    .get((a.proposal.run.as_str(), a.proposal.tool_use_id.as_str()))?
                    .is_none_or(|v| v.value() != id)
            {
                return Err("browser action index or run ownership differs".into());
            }
            let open = self
                .write
                .open_table(OPEN_ACTIONS)?
                .get(id)?
                .map(|v| v.value().to_owned());
            if matches!(a.state, Phase::Settled { .. }) {
                if open.is_some() {
                    return Err("settled browser action remains open".into());
                }
            } else if run.value.status.terminal()
                || open.as_deref() != Some(a.proposal.run.as_str())
            {
                return Err("open browser action index or task status differs".into());
            }
            match &a.state {
                Phase::Settled {
                    outcome: Outcome::Navigated { binding, .. },
                } if self
                    .write
                    .open_table(NAVIGATION_GRANTS)?
                    .get((a.proposal.run.as_str(), binding.grant_revision))?
                    .is_none_or(|v| v.value() != id) =>
                {
                    return Err("navigation grant index differs".into());
                }
                Phase::Settled {
                    outcome: Outcome::Uncertain { .. },
                } if self
                    .write
                    .open_table(UNCERTAIN_RUNS)?
                    .get(a.proposal.run.as_str())?
                    .is_none() =>
                {
                    return Err("uncertain browser result index is absent".into());
                }
                _ => (),
            }
            Ok(saved)
        })
    }

    pub(super) fn browser_initial(&self, scope: &Selection, run: &str) -> Result<Option<Target>> {
        let task = self.run(scope, run)?;
        let launch: Record<Launch> = storage(|| {
            record(self.write, LAUNCHES, run)?.ok_or("browser launch is absent".into())
        })?;
        if launch.conversation != scope.conversation {
            return storage(|| Err("browser launch ownership differs".into()));
        }
        if matches!(
            launch.value,
            Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
        ) {
            return self.native_initial_target(scope, run).map(Some);
        }
        self.source(&task.source)
            .map(|source| source.map(|s| Target::from(&s.page)))
    }

    fn browser_grant(
        &self,
        scope: &Selection,
        run: &str,
        revision: u64,
    ) -> Result<(Binding, String)> {
        storage(|| {
            let id = self
                .write
                .open_table(NAVIGATION_GRANTS)?
                .get((run, revision))?
                .ok_or("browser destination grant is absent")?
                .value()
                .to_owned();
            let action = self.browser_record(scope, &id)?.value;
            match action.state {
                Phase::Settled {
                    outcome: Outcome::Navigated { binding, url, .. },
                } if action.proposal.run == run && binding.grant_revision == revision => {
                    Ok((binding, url))
                }
                _ => Err("browser grant does not name its verified navigation".into()),
            }
        })
    }

    pub(super) fn browser_navigation(
        &self,
        scope: &Selection,
        run: &str,
    ) -> Result<Option<(Binding, String)>> {
        let id = storage(|| {
            Ok(self
                .write
                .open_table(NAVIGATION_HEADS)?
                .get(run)?
                .map(|v| v.value().to_owned()))
        })?;
        let Some(id) = id else { return Ok(None) };
        let action = self.browser_action(scope, &id)?;
        storage(|| match action.state {
            Phase::Settled {
                outcome: Outcome::Navigated { binding, url, .. },
            } if action.proposal.run == run => Ok(Some((binding, url))),
            _ => Err("browser navigation head is not a verified destination".into()),
        })
    }

    pub(super) fn browser_open(&self, scope: &Selection, run: &str) -> Result<Option<Action>> {
        let entry = storage(|| {
            let table = self.write.open_table(OPEN_ACTIONS)?;
            if table.len()? > 1 {
                return Err("multiple browser actions are open".into());
            }
            Ok(table
                .iter()?
                .next()
                .transpose()?
                .and_then(|(id, owner)| (owner.value() == run).then(|| id.value().to_owned())))
        })?;
        entry.map(|id| self.browser_action(scope, &id)).transpose()
    }

    pub(crate) fn uncertain_browser(&self, scope: &Selection, run: &str) -> Result<bool> {
        self.checked((|| {
            self.require_selection(scope)?;
            self.run(scope, run)?;
            let id = storage(|| {
                Ok(self
                    .write
                    .open_table(UNCERTAIN_RUNS)?
                    .get(run)?
                    .map(|v| v.value().to_owned()))
            })?;
            let Some(id) = id else { return Ok(false) };
            let action = self.browser_action(scope, &id)?;
            storage(|| {
                if action.proposal.run == run
                    && matches!(
                        action.state,
                        Phase::Settled {
                            outcome: Outcome::Uncertain { .. }
                        }
                    )
                {
                    Ok(true)
                } else {
                    Err("uncertain result index differs".into())
                }
            })
        })())
    }
}
