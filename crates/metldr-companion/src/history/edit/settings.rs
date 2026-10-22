use super::{Edit, Selection};
use crate::{Result, agent_settings, provider_settings};

impl Edit<'_> {
    pub(crate) fn settings(
        &self,
        expected: &Selection,
    ) -> Result<(provider_settings::Settings, agent_settings::Settings)> {
        let result = (|| {
            self.require_selection(expected)?;
            Ok((self.profile.providers.clone(), self.profile.agents.clone()))
        })();
        self.checked(result)
    }

    pub(crate) fn replace_settings(
        &mut self,
        expected: &Selection,
        providers: &provider_settings::Settings,
        agents: &agent_settings::Settings,
        next_providers: provider_settings::Settings,
        next_agents: agent_settings::Settings,
    ) -> Result<()> {
        let result = (|| {
            self.require_selection(expected)?;
            if serde_json::to_vec(providers)? != serde_json::to_vec(&self.profile.providers)?
                || serde_json::to_vec(agents)? != serde_json::to_vec(&self.profile.agents)?
            {
                return Err("saved settings changed; refresh the view before retrying".into());
            }
            next_providers.validate()?;
            next_agents.validate()?;
            self.profile.providers = next_providers;
            self.profile.agents = next_agents;
            self.changed = true;
            Ok(())
        })();
        self.checked(result)
    }
}
