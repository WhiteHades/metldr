use crate::{Result, flow::Owner, model::Store};

fn viewed_run(state: &Store) -> Option<String> {
    // A native projection also includes its workflow head when viewing an older task.
    let latest = state
        .workflows
        .values()
        .map(|workflow| workflow.last_run.as_str())
        .chain(
            state
                .codex_workflows
                .values()
                .map(|workflow| workflow.last_run.as_str()),
        )
        .chain(
            state
                .opencode_workflows
                .values()
                .map(|workflow| workflow.last_run.as_str()),
        )
        .next();
    state
        .view
        .runs
        .iter()
        .find(|run| latest != Some(run.id.as_str()))
        .or_else(|| state.view.runs.first())
        .map(|run| run.id.clone())
}

impl Owner {
    pub(super) fn refresh_saved_settings(&mut self) -> Result<()> {
        if self.record_history.is_none() {
            return Ok(());
        }
        let run = viewed_run(&self.state);
        let previous_agents = serde_json::to_vec(&self.state.agent_settings)?;
        let previous_providers =
            serde_json::to_vec(&(&self.state.providers.scope, &self.state.providers.slots))?;
        let ((providers, agents), _) = self.persist_records(|edit, scope| {
            Ok((
                edit.settings(scope)?,
                Some(edit.project(scope, run.as_deref())?),
            ))
        })?;
        if previous_agents != serde_json::to_vec(&agents)? {
            for kind in [
                crate::agent_settings::Kind::Pi,
                crate::agent_settings::Kind::Codex,
                crate::agent_settings::Kind::Opencode,
            ] {
                self.refresh_agent_runtime(kind);
            }
        }
        if previous_providers != serde_json::to_vec(&(&providers.scope, &providers.slots))? {
            for protocol in crate::provider_settings::Protocol::ALL {
                if providers.managed(protocol) {
                    self.set_provider_route(protocol, None);
                    self.settings.states.remove(&protocol);
                }
            }
        }
        Ok(())
    }

    pub(super) fn persist_settings(&mut self, next: Store) -> Result<()> {
        if self.record_history.is_none() {
            return self.persist(next);
        }
        let providers = self.state.providers.clone();
        let agents = self.state.agent_settings.clone();
        let run = viewed_run(&self.state);
        self.persist_records(|edit, scope| {
            edit.replace_settings(
                scope,
                &providers,
                &agents,
                next.providers,
                next.agent_settings,
            )?;
            Ok(((), Some(edit.refresh_view(scope, run.as_deref())?)))
        })?;
        Ok(())
    }
}
