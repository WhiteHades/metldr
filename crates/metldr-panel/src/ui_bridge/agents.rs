use super::settings::{Family, Work};
use super::*;
use crate::model::{AgentKind, AgentSettingState, AgentSettingView};
use wasm_bindgen_futures::future_to_promise_typed;

mod profile;
pub use profile::UiAgentProfile;

#[wasm_bindgen]
pub struct UiAgentSettings {
    pub(super) page: Rc<PageSession>,
}

impl UiAgentSettings {
    pub(super) fn new(page: Rc<PageSession>) -> Self {
        Self { page }
    }
}

#[wasm_bindgen]
impl UiAgentSettings {
    pub fn configurations(&self) -> Vec<UiAgentConfiguration> {
        self.page
            .view
            .borrow()
            .agent_settings
            .agents
            .iter()
            .map(|row| UiAgentConfiguration::new(Rc::clone(&self.page), row.clone()))
            .collect()
    }

    pub fn configuration(
        &self,
        agent: String,
        revision: u64,
    ) -> Result<UiAgentConfiguration, JsValue> {
        self.page
            .view
            .borrow()
            .agent_settings
            .agents
            .iter()
            .find(|row| row.agent.name() == agent && row.revision == revision)
            .map(|row| UiAgentConfiguration::new(Rc::clone(&self.page), row.clone()))
            .ok_or_else(|| js_error(FlowError::HistoryStale))
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn refresh(&self) -> Result<Promise<js_sys::Undefined>, JsValue> {
        let work = Work::begin(Rc::clone(&self.page), Family::Agents, true).map_err(js_error)?;
        Ok(future_to_promise_typed(async move {
            work.refresh().await.map_err(js_error)
        }))
    }
}

#[wasm_bindgen]
pub struct UiAgentConfiguration {
    page: Rc<PageSession>,
    row: AgentSettingView,
}

impl UiAgentConfiguration {
    pub(super) fn new(page: Rc<PageSession>, row: AgentSettingView) -> Self {
        Self { page, row }
    }

    fn begin(&self) -> Result<Work, FlowError> {
        if !self
            .page
            .view
            .borrow()
            .agent_settings
            .agents
            .iter()
            .any(|row| row.agent == self.row.agent && row.revision == self.row.revision)
        {
            return Err(FlowError::HistoryStale);
        }
        Work::begin(Rc::clone(&self.page), Family::Agents, false)
    }
}

#[wasm_bindgen]
impl UiAgentConfiguration {
    #[wasm_bindgen(getter)]
    pub fn agent(&self) -> String {
        self.row.agent.name().into()
    }
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        match self.row.agent {
            AgentKind::Codex => "Codex",
            AgentKind::Opencode => "OpenCode",
            AgentKind::Pi => "Pi",
        }
        .into()
    }
    #[wasm_bindgen(getter)]
    pub fn revision(&self) -> u64 {
        self.row.revision
    }
    #[wasm_bindgen(getter)]
    pub fn state(&self) -> String {
        match self.row.state {
            AgentSettingState::Unmanaged => "unmanaged",
            AgentSettingState::Ready => "ready",
            AgentSettingState::Removed => "removed",
            AgentSettingState::Unavailable => "unavailable",
        }
        .into()
    }
    pub fn profile(&self) -> Option<UiAgentProfile> {
        self.row.profile.clone().map(UiAgentProfile::from_profile)
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn save(&self, profile: &UiAgentProfile) -> Result<Promise<js_sys::Undefined>, JsValue> {
        if profile.profile.agent() != self.row.agent {
            return Err(crate::err(
                "The profile belongs to another installed agent.",
            ));
        }
        let profile = profile.profile.clone();
        let work = self.begin().map_err(js_error)?;
        let revision = self.row.revision;
        Ok(future_to_promise_typed(async move {
            let result = async {
                work.current_flow()
                    .await?
                    .agent_save(revision, profile)
                    .await
            }
            .await;
            work.finish(result).map_err(js_error)
        }))
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn remove(&self) -> Result<Promise<js_sys::Undefined>, JsValue> {
        let work = self.begin().map_err(js_error)?;
        let agent = self.row.agent;
        let revision = self.row.revision;
        Ok(future_to_promise_typed(async move {
            let result = async {
                work.current_flow()
                    .await?
                    .agent_remove(agent, revision)
                    .await
            }
            .await;
            work.finish(result).map_err(js_error)
        }))
    }
}
