use super::*;
use crate::model::{AgentProfile, valid_agent_profile};

#[wasm_bindgen]
pub struct UiAgentProfile {
    pub(super) profile: AgentProfile,
}

impl UiAgentProfile {
    pub(super) fn from_profile(profile: AgentProfile) -> Self {
        Self { profile }
    }

    fn checked(profile: AgentProfile) -> Result<Self, JsValue> {
        if !valid_agent_profile(&profile) {
            return Err(crate::err(
                "Agent paths or model settings are invalid or exceed their limits.",
            ));
        }
        Ok(Self::from_profile(profile))
    }
}

#[wasm_bindgen]
impl UiAgentProfile {
    pub fn codex(executable: String, workspace: String) -> Result<Self, JsValue> {
        Self::checked(AgentProfile::Codex {
            executable,
            workspace,
        })
    }
    pub fn opencode(executable: String, workspace: String) -> Result<Self, JsValue> {
        Self::checked(AgentProfile::Opencode {
            executable,
            workspace,
        })
    }
    pub fn pi(
        executable: String,
        workspace: String,
        session_dir: String,
        provider: String,
        model: String,
        offline: bool,
    ) -> Result<Self, JsValue> {
        Self::checked(AgentProfile::Pi {
            executable,
            workspace,
            session_dir,
            provider,
            model,
            offline,
        })
    }
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.profile.agent().name().into()
    }
    #[wasm_bindgen(getter)]
    pub fn executable(&self) -> String {
        match &self.profile {
            AgentProfile::Codex { executable, .. }
            | AgentProfile::Opencode { executable, .. }
            | AgentProfile::Pi { executable, .. } => executable.clone(),
        }
    }
    #[wasm_bindgen(getter)]
    pub fn workspace(&self) -> String {
        match &self.profile {
            AgentProfile::Codex { workspace, .. }
            | AgentProfile::Opencode { workspace, .. }
            | AgentProfile::Pi { workspace, .. } => workspace.clone(),
        }
    }
    #[wasm_bindgen(getter)]
    pub fn session_dir(&self) -> Option<String> {
        match &self.profile {
            AgentProfile::Pi { session_dir, .. } => Some(session_dir.clone()),
            _ => None,
        }
    }
    #[wasm_bindgen(getter)]
    pub fn provider(&self) -> Option<String> {
        match &self.profile {
            AgentProfile::Pi { provider, .. } => Some(provider.clone()),
            _ => None,
        }
    }
    #[wasm_bindgen(getter)]
    pub fn model(&self) -> Option<String> {
        match &self.profile {
            AgentProfile::Pi { model, .. } => Some(model.clone()),
            _ => None,
        }
    }
    #[wasm_bindgen(getter)]
    pub fn offline(&self) -> Option<bool> {
        match &self.profile {
            AgentProfile::Pi { offline, .. } => Some(*offline),
            _ => None,
        }
    }
}
