use super::{Binding, Intent, Outcome, Phase, Proposal};
use crate::{
    Result,
    model::{Page, Store},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Target {
    pub(crate) tab: i32,
    pub(crate) document: String,
    pub(crate) url: String,
}

impl Target {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.tab < 0 || self.document.is_empty() || self.document.len() > 128 {
            return Err("invalid native browser target".into());
        }
        crate::model::validate_page_url(&self.url)
    }
}

impl From<&Page> for Target {
    fn from(page: &Page) -> Self {
        Self {
            tab: page.tab,
            document: page.document.clone(),
            url: page.url.clone(),
        }
    }
}

impl Proposal {
    pub(crate) fn matches_position(
        &self,
        initial: Option<&Target>,
        navigation: Option<(&Binding, &str)>,
    ) -> bool {
        if let Some((binding, url)) = navigation {
            &self.binding == binding && self.url == url
        } else {
            self.binding.grant_revision == 1
                && initial.is_none_or(|target| {
                    self.binding.tab == target.tab
                        && self.binding.document == target.document
                        && self.url == target.url
                })
        }
    }
}

impl Store {
    pub(crate) fn native_browser_target_after(&self, run: &str) -> Result<Target> {
        let initial = self.native_browser_target(run)?;
        Ok(match self.navigation_destination(run, Some(&initial))? {
            Some((binding, url)) => Target {
                tab: binding.tab,
                document: binding.document.clone(),
                url: url.clone(),
            },
            None => initial,
        })
    }
    pub(crate) fn native_browser_target(&self, run: &str) -> Result<Target> {
        let (workflow, source) = match self.launches.get(run) {
            Some(crate::agent_model::Launch::Pi { workflow, .. }) => (
                workflow,
                &self
                    .workflows
                    .get(workflow)
                    .ok_or("missing Pi workflow")?
                    .source,
            ),
            Some(crate::agent_model::Launch::Codex { workflow, .. }) => (
                workflow,
                &self
                    .codex_workflows
                    .get(workflow)
                    .ok_or("missing Codex workflow")?
                    .source,
            ),
            Some(crate::agent_model::Launch::OpenCode { workflow, .. }) => (
                workflow,
                &self
                    .opencode_workflows
                    .get(workflow)
                    .ok_or("missing OpenCode workflow")?
                    .source,
            ),
            _ => return Err("browser target requires a native agent run".into()),
        };
        let mut target = Target::from(&source.page);
        for prior in &self.view.runs {
            if prior.id == run {
                return Ok(target);
            }
            if matches!(self.launches.get(&prior.id), Some(crate::agent_model::Launch::Pi { workflow: other, .. } | crate::agent_model::Launch::Codex { workflow: other, .. } | crate::agent_model::Launch::OpenCode { workflow: other, .. }) if other == workflow)
                && let Some((binding, url)) =
                    self.navigation_destination(&prior.id, Some(&target))?
            {
                target = Target {
                    tab: binding.tab,
                    document: binding.document.clone(),
                    url: url.clone(),
                };
            }
        }
        Err("unknown native browser run".into())
    }

    pub(super) fn browser_target_matches(
        &self,
        proposal: &Proposal,
        source: Option<&Target>,
    ) -> Result<bool> {
        let navigation = self
            .navigation_destination(&proposal.run, source)?
            .map(|(binding, url)| (binding, url.as_str()));
        Ok(proposal.matches_position(source, navigation))
    }

    fn navigation_destination(
        &self,
        run: &str,
        source: Option<&Target>,
    ) -> Result<Option<(&Binding, &String)>> {
        let mut transitions: Vec<_> = self
            .browser_actions
            .values()
            .filter_map(|action| {
                if action.proposal.run != run {
                    return None;
                }
                match &action.state {
                    Phase::Settled {
                        outcome: Outcome::Navigated { binding, url, .. },
                    } => Some((&action.proposal, binding, url)),
                    _ => None,
                }
            })
            .collect();
        transitions.sort_by_key(|(from, _, _)| from.binding.grant_revision);
        let mut previous = None;
        for (index, (from, binding, url)) in transitions.iter().enumerate() {
            if from.binding.grant_revision != index as u64 + 1 {
                return Err("browser navigation grant chain is discontinuous".into());
            }
            if let Some((old_binding, old_url)) = previous {
                if &from.binding != old_binding || &from.url != old_url {
                    return Err(
                        "browser navigation does not follow the verified destination".into(),
                    );
                }
            } else if source.is_some_and(|page| {
                from.binding.tab != page.tab
                    || from.binding.document != page.document
                    || from.url != page.url
            }) {
                return Err("browser navigation does not start from the saved source".into());
            }
            previous = Some((*binding, *url));
        }
        Ok(previous)
    }
}

pub(super) fn validate(state: &Store) -> Result<()> {
    for action in state.browser_actions.values() {
        let proposal = &action.proposal;
        let initial = if matches!(
            state.launches.get(&proposal.run),
            Some(
                crate::agent_model::Launch::Pi { .. }
                    | crate::agent_model::Launch::Codex { .. }
                    | crate::agent_model::Launch::OpenCode { .. }
            )
        ) {
            Some(state.native_browser_target(&proposal.run)?)
        } else {
            None
        };
        if proposal.binding.grant_revision == 1
            && initial.as_ref().is_some_and(|target| {
                proposal.binding.tab != target.tab
                    || proposal.binding.document != target.document
                    || proposal.url != target.url
            })
        {
            return Err("native browser action does not start at its verified target".into());
        }
        let matching_navigation=state.browser_actions.values().find(|prior| {
            prior.proposal.run==proposal.run && matches!(&prior.state,
                Phase::Settled { outcome:Outcome::Navigated { binding, url, .. } }
                if binding.grant_revision==proposal.binding.grant_revision && binding==&proposal.binding && url==&proposal.url)
        });
        if proposal.binding.grant_revision > 1 && matching_navigation.is_none() {
            return Err("browser action has no verified destination grant".into());
        }
        if !matches!(action.state, Phase::Settled { .. })
            && !state.browser_target_matches(proposal, initial.as_ref())?
        {
            return Err("pending browser action does not use the current destination".into());
        }
        if matches!(proposal.intent, Intent::Navigate { .. }) {
            state.navigation_destination(&proposal.run, initial.as_ref())?;
        }
    }
    Ok(())
}
