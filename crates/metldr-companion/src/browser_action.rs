use crate::{
    Result,
    model::{Status, Store},
};
use serde::{Deserialize, Serialize};

mod intent;
mod selection;
mod target;
pub(super) use intent::{Intent, valid_destination};
pub(crate) use selection::{Choice, Selection};
pub(super) use target::Target;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub tab: i32,
    pub document: String,
    pub target: String,
    pub frame: String,
    pub session: String,
    pub unique_context: String,
    pub grant_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "intent::WireProposal")]
pub(super) struct Proposal {
    pub run: String,
    pub tool_use_id: String,
    pub binding: Binding,
    pub url: String,
    pub intent: Intent,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Outcome {
    Verified {
        observed: String,
    },
    Navigated {
        binding: Binding,
        url: String,
        loader: String,
    },
    ButtonClickObserved,
    SelectionObserved {
        selected: Choice,
    },
    Prevented {
        reason: String,
    },
    Uncertain {
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Phase {
    Pending,
    Approved,
    Dispatching,
    Settled { outcome: Outcome },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Action {
    pub id: String,
    pub revision: u64,
    pub proposal: Proposal,
    pub state: Phase,
}

impl Proposal {
    fn validate(&self) -> Result<()> {
        let b = &self.binding;
        if b.tab < 0
            || b.grant_revision == 0
            || !bounded(&self.url, 2048)
            || !bounded(&self.tool_use_id, 128)
            || ![
                &b.document,
                &b.target,
                &b.frame,
                &b.session,
                &b.unique_context,
            ]
            .into_iter()
            .all(|s| bounded(s, 128))
        {
            return Err("invalid browser proposal".into());
        }
        self.intent.validate(&self.url)
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

impl Store {
    pub(super) fn propose_browser(&mut self, proposal: Proposal) -> Result<Action> {
        proposal.validate()?;
        if let Some(action) = self.browser_actions.values().find(|a| {
            a.proposal.run == proposal.run && a.proposal.tool_use_id == proposal.tool_use_id
        }) {
            if action.proposal != proposal {
                return Err("browser tool identity was reused with a changed proposal".into());
            }
            return Ok(action.clone());
        }
        self.browser_run(&proposal.run)?;
        let source = match self.launches.get(&proposal.run) {
            Some(crate::agent_model::Launch::Pi { workflow, .. }) => {
                &self
                    .workflows
                    .get(workflow)
                    .ok_or("missing Pi browser source")?
                    .source
            }
            Some(crate::agent_model::Launch::Codex { workflow, .. }) => {
                &self
                    .codex_workflows
                    .get(workflow)
                    .ok_or("missing Codex browser source")?
                    .source
            }
            Some(crate::agent_model::Launch::OpenCode { workflow, .. }) => {
                &self
                    .opencode_workflows
                    .get(workflow)
                    .ok_or("missing OpenCode browser source")?
                    .source
            }
            _ => self.view.source.as_ref().ok_or("missing browser source")?,
        };
        let run = self
            .view
            .runs
            .iter()
            .find(|r| r.id == proposal.run)
            .ok_or("unknown browser run")?;
        let initial = if matches!(
            self.launches.get(&proposal.run),
            Some(
                crate::agent_model::Launch::Pi { .. }
                    | crate::agent_model::Launch::Codex { .. }
                    | crate::agent_model::Launch::OpenCode { .. }
            )
        ) {
            self.native_browser_target(&proposal.run)?
        } else {
            Target::from(&source.page)
        };
        if run.source != source.id || !self.browser_target_matches(&proposal, Some(&initial))? {
            return Err("browser proposal does not match the saved source".into());
        }
        if self.browser_actions.len() >= 16
            || self
                .browser_actions
                .values()
                .any(|a| !matches!(a.state, Phase::Settled { .. }))
        {
            return Err("another browser action is pending or capacity is reached".into());
        }
        let action = Action {
            id: self.id()?,
            revision: 1,
            proposal,
            state: Phase::Pending,
        };
        self.browser_actions
            .insert(action.id.clone(), action.clone());
        Ok(action)
    }

    pub(super) fn decide_browser(
        &mut self,
        id: &str,
        revision: u64,
        approve: bool,
    ) -> Result<Action> {
        let action = self
            .browser_actions
            .get(id)
            .ok_or("unknown browser action")?;
        self.browser_run(&action.proposal.run)?;
        if action.revision != revision || action.state != Phase::Pending {
            return Err("browser approval is stale".into());
        }
        self.change_browser(
            id,
            if approve {
                Phase::Approved
            } else {
                Phase::Settled {
                    outcome: Outcome::Prevented {
                        reason: "user declined".into(),
                    },
                }
            },
        )
    }

    pub(super) fn dispatch_browser(&mut self, id: &str, revision: u64) -> Result<Action> {
        let action = self
            .browser_actions
            .get(id)
            .ok_or("unknown browser action")?;
        self.browser_run(&action.proposal.run)?;
        if action.revision != revision || action.state != Phase::Approved {
            return Err("browser dispatch requires the current approval".into());
        }
        self.change_browser(id, Phase::Dispatching)
    }

    pub(super) fn settle_browser(
        &mut self,
        id: &str,
        revision: u64,
        outcome: Outcome,
    ) -> Result<Action> {
        let action = self
            .browser_actions
            .get(id)
            .ok_or("unknown browser action")?;
        if action.revision != revision || action.state != Phase::Dispatching {
            return Err("browser result is stale or was not dispatched".into());
        }
        validate_outcome(&outcome, &action.proposal)?;
        self.change_browser(id, Phase::Settled { outcome })
    }

    pub(super) fn prevent_browser(&mut self, run: &str, reason: &str) -> Result<()> {
        let ids: Vec<_> = self
            .browser_actions
            .values()
            .filter(|a| {
                a.proposal.run == run && matches!(a.state, Phase::Pending | Phase::Approved)
            })
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            self.change_browser(
                &id,
                Phase::Settled {
                    outcome: Outcome::Prevented {
                        reason: reason.into(),
                    },
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn interrupt_browser(&mut self) -> Result<bool> {
        let ids: Vec<_> = self
            .browser_actions
            .values()
            .filter(|a| !matches!(a.state, Phase::Settled { .. }))
            .map(|a| a.id.clone())
            .collect();
        for id in &ids {
            let outcome = if self.browser_actions[id].state == Phase::Dispatching {
                Outcome::Uncertain {
                    reason: "native host restarted after dispatch".into(),
                }
            } else {
                Outcome::Prevented {
                    reason: "native host restarted before dispatch".into(),
                }
            };
            self.change_browser(id, Phase::Settled { outcome })?;
        }
        Ok(!ids.is_empty())
    }

    pub(super) fn finish_browser(&mut self, run: &str) -> Result<()> {
        let ids: Vec<_> = self
            .browser_actions
            .values()
            .filter(|a| a.proposal.run == run && !matches!(a.state, Phase::Settled { .. }))
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            let outcome = if self.browser_actions[&id].state == Phase::Dispatching {
                Outcome::Uncertain {
                    reason: "worker ended without a verified browser result".into(),
                }
            } else {
                Outcome::Prevented {
                    reason: "worker ended before dispatch".into(),
                }
            };
            self.change_browser(&id, Phase::Settled { outcome })?;
        }
        Ok(())
    }

    fn browser_run(&self, run: &str) -> Result<()> {
        if !self
            .view
            .runs
            .iter()
            .any(|r| r.id == run && r.status == Status::Running)
        {
            return Err("browser run is not active".into());
        }
        Ok(())
    }

    fn change_browser(&mut self, id: &str, state: Phase) -> Result<Action> {
        let action = self
            .browser_actions
            .get_mut(id)
            .ok_or("unknown browser action")?;
        action.revision = action
            .revision
            .checked_add(1)
            .ok_or("browser revision exhausted")?;
        action.state = state;
        Ok(action.clone())
    }
}

pub(super) fn validate(state: &Store, valid_id: impl Fn(&str) -> bool) -> Result<()> {
    if state.browser_actions.len() > 16 {
        return Err("browser action capacity reached".into());
    }
    let mut tools = std::collections::BTreeSet::new();
    let mut pending = 0;
    for (id, action) in &state.browser_actions {
        action.proposal.validate()?;
        if id != &action.id
            || !valid_id(id)
            || action.revision == 0
            || !state.view.runs.iter().any(|r| r.id == action.proposal.run)
            || state.view.runs.iter().any(|r| &r.id == id)
            || state.view.source.as_ref().is_some_and(|s| &s.id == id)
            || !tools.insert((&action.proposal.run, &action.proposal.tool_use_id))
        {
            return Err("invalid saved browser action".into());
        }
        match &action.state {
            Phase::Pending if action.revision != 1 => return Err("invalid pending revision".into()),
            Phase::Approved if action.revision != 2 => {
                return Err("invalid approval revision".into());
            }
            Phase::Dispatching if action.revision != 3 => {
                return Err("invalid dispatch revision".into());
            }
            Phase::Settled { outcome } => {
                validate_outcome(outcome, &action.proposal)?;
                let valid_revision = match outcome {
                    Outcome::Verified { .. }
                    | Outcome::Navigated { .. }
                    | Outcome::Uncertain { .. } => action.revision == 4,
                    Outcome::SelectionObserved { .. } => {
                        action.revision == 4
                            && matches!(&action.proposal.intent, Intent::SelectOption { .. })
                    }
                    Outcome::ButtonClickObserved => {
                        action.revision == 4
                            && matches!(&action.proposal.intent, Intent::ActivateButton { .. })
                    }
                    Outcome::Prevented { .. } => (2..=4).contains(&action.revision),
                };
                if !valid_revision {
                    return Err("invalid settled revision".into());
                }
            }
            _ => (),
        }
        pending += usize::from(!matches!(action.state, Phase::Settled { .. }));
        if !matches!(action.state, Phase::Settled { .. })
            && !state
                .view
                .runs
                .iter()
                .any(|r| r.id == action.proposal.run && !r.status.terminal())
        {
            return Err("nonterminal browser action belongs to a terminal run".into());
        }
    }
    if pending > 1 {
        return Err("multiple pending browser actions".into());
    }
    target::validate(state)?;
    Ok(())
}

fn validate_outcome(outcome: &Outcome, proposal: &Proposal) -> Result<()> {
    match (outcome, &proposal.intent) {
        (Outcome::Verified { observed }, Intent::Field { value, .. }) if observed == value => {
            Ok(())
        }
        (Outcome::ButtonClickObserved, Intent::ActivateButton { .. }) => Ok(()),
        (Outcome::SelectionObserved { selected }, Intent::SelectOption { selection })
            if selected == &selection.selected =>
        {
            Ok(())
        }
        (
            Outcome::Navigated {
                binding,
                url,
                loader,
            },
            Intent::Navigate { destination },
        ) if url == destination
            && bounded(loader, 128)
            && binding.tab == proposal.binding.tab
            && binding.target == proposal.binding.target
            && binding.frame == proposal.binding.frame
            && binding.session == proposal.binding.session
            && bounded(&binding.document, 128)
            && bounded(&binding.unique_context, 128)
            && binding.document != proposal.binding.document
            && binding.unique_context != proposal.binding.unique_context
            && proposal.binding.grant_revision.checked_add(1) == Some(binding.grant_revision) =>
        {
            Ok(())
        }
        (Outcome::Prevented { reason } | Outcome::Uncertain { reason }, _)
            if bounded(reason, 200) =>
        {
            Ok(())
        }
        _ => Err("invalid browser outcome".into()),
    }
}

pub(super) fn reject_button_actions(
    actions: &std::collections::BTreeMap<String, Action>,
) -> Result<()> {
    if actions.values().any(|action| {
        matches!(&action.proposal.intent, Intent::ActivateButton { .. })
            || matches!(
                &action.state,
                Phase::Settled {
                    outcome: Outcome::ButtonClickObserved
                }
            )
    }) {
        return Err("legacy browser store contains button action data".into());
    }
    Ok(())
}

pub(super) fn reject_selection_actions(
    actions: &std::collections::BTreeMap<String, Action>,
) -> Result<()> {
    if actions.values().any(|action| {
        matches!(&action.proposal.intent, Intent::SelectOption { .. })
            || matches!(
                &action.state,
                Phase::Settled {
                    outcome: Outcome::SelectionObserved { .. }
                }
            )
    }) {
        return Err("legacy browser store contains selection action data".into());
    }
    Ok(())
}

#[cfg(test)]
mod selection_checks {
    use super::*;

    #[test]
    fn selection_receipt_must_match_the_approved_choice() {
        let selected = Choice {
            id: "option_2".into(),
            label: "North".into(),
            index: 1,
        };
        let proposal = Proposal {
            run: "store:1".into(),
            tool_use_id: "call-1".into(),
            binding: Binding {
                tab: 1,
                document: "document".into(),
                target: "target".into(),
                frame: "frame".into(),
                session: "session".into(),
                unique_context: "context".into(),
                grant_revision: 1,
            },
            url: "http://127.0.0.1:8000/article".into(),
            intent: Intent::SelectOption {
                selection: Selection {
                    control_id: "select_1".into(),
                    control_label: "Region".into(),
                    previous: None,
                    selected: selected.clone(),
                },
            },
        };
        assert!(
            validate_outcome(
                &Outcome::SelectionObserved {
                    selected: selected.clone(),
                },
                &proposal,
            )
            .is_ok()
        );
        assert!(
            validate_outcome(
                &Outcome::SelectionObserved {
                    selected: Choice {
                        id: "option_3".into(),
                        ..selected
                    },
                },
                &proposal,
            )
            .is_err()
        );
    }
}
