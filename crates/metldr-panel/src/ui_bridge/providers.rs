use super::settings::{Family, Work};
use super::*;
use crate::model::{ProviderConnectionState, ProviderConnectionView};
use wasm_bindgen_futures::future_to_promise_typed;

#[wasm_bindgen]
pub struct UiProviderSettings {
    pub(super) page: Rc<PageSession>,
}

impl UiProviderSettings {
    pub(super) fn new(page: Rc<PageSession>) -> Self {
        Self { page }
    }
}

#[wasm_bindgen]
impl UiProviderSettings {
    #[wasm_bindgen(getter)]
    pub fn busy(&self) -> bool {
        !self.page.controls_ready() || self.page.settings_blocked()
    }

    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.page.settings.borrow().uncertain(Family::Providers)
    }

    #[wasm_bindgen(getter)]
    pub fn notice(&self) -> Option<String> {
        self.page.view.borrow().provider_settings.notice.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn cleanup_pending(&self) -> bool {
        self.page.view.borrow().provider_settings.cleanup_pending
    }

    pub fn connections(&self) -> Vec<UiProviderConnection> {
        self.page
            .view
            .borrow()
            .provider_settings
            .connections
            .iter()
            .map(|connection| UiProviderConnection {
                page: Rc::clone(&self.page),
                connection: connection.clone(),
            })
            .collect()
    }

    pub fn connection(
        &self,
        protocol: String,
        revision: u64,
    ) -> Result<UiProviderConnection, JsValue> {
        self.page
            .view
            .borrow()
            .provider_settings
            .connections
            .iter()
            .find(|row| row.protocol.name() == protocol && row.revision == revision)
            .map(|row| UiProviderConnection::new(Rc::clone(&self.page), row.clone()))
            .ok_or_else(|| js_error(FlowError::HistoryStale))
    }

    // The generator erases Promise's type argument in exported declarations.
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn refresh(&self) -> Result<Promise<js_sys::Undefined>, JsValue> {
        let work = Work::begin(Rc::clone(&self.page), Family::Providers, true).map_err(js_error)?;
        Ok(future_to_promise_typed(async move {
            work.refresh().await.map_err(js_error)
        }))
    }
}

#[wasm_bindgen]
pub struct UiProviderConnection {
    page: Rc<PageSession>,
    connection: ProviderConnectionView,
}

impl UiProviderConnection {
    pub(super) fn new(page: Rc<PageSession>, connection: ProviderConnectionView) -> Self {
        Self { page, connection }
    }
}

#[wasm_bindgen]
impl UiProviderConnection {
    #[wasm_bindgen(getter)]
    pub fn revision(&self) -> u64 {
        self.connection.revision
    }

    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        match self.connection.protocol {
            crate::model::ProviderProtocol::Messages => "Messages",
            crate::model::ProviderProtocol::Responses => "OpenAI Responses",
            crate::model::ProviderProtocol::GoogleInteractions => "Google Interactions",
            crate::model::ProviderProtocol::ChatCompletions => "Chat Completions",
        }
        .into()
    }

    #[wasm_bindgen(getter)]
    pub fn protocol(&self) -> String {
        self.connection.protocol.name().into()
    }

    #[wasm_bindgen(getter)]
    pub fn endpoint(&self) -> Option<String> {
        self.connection.endpoint.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn model(&self) -> Option<String> {
        self.connection.model.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn state(&self) -> String {
        match self.connection.state {
            ProviderConnectionState::Unmanaged => "unmanaged",
            ProviderConnectionState::Removed => "removed",
            ProviderConnectionState::Loading => "loading",
            ProviderConnectionState::Ready => "ready",
            ProviderConnectionState::Missing => "missing",
            ProviderConnectionState::Locked => "locked",
            ProviderConnectionState::Corrupt => "corrupt",
            ProviderConnectionState::Unavailable => "unavailable",
        }
        .into()
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn save(
        &self,
        endpoint: String,
        model: String,
        key: Option<String>,
    ) -> Result<Promise<js_sys::Undefined>, JsValue> {
        let work = self.begin().map_err(js_error)?;
        let protocol = self.connection.protocol;
        let revision = self.connection.revision;
        Ok(future_to_promise_typed(async move {
            let result = async {
                let flow = work.current_flow().await?;
                flow.provider_save(protocol, revision, endpoint, model, key)
                    .await
            }
            .await;
            work.finish(result).map_err(js_error)
        }))
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn remove(&self) -> Result<Promise<js_sys::Undefined>, JsValue> {
        let work = self.begin().map_err(js_error)?;
        let protocol = self.connection.protocol;
        let revision = self.connection.revision;
        Ok(future_to_promise_typed(async move {
            let result = async {
                let flow = work.current_flow().await?;
                flow.provider_remove(protocol, revision).await
            }
            .await;
            work.finish(result).map_err(js_error)
        }))
    }
}

impl UiProviderConnection {
    fn begin(&self) -> Result<Work, FlowError> {
        let current = self.page.view.borrow();
        if !current.provider_settings.connections.iter().any(|row| {
            row.protocol == self.connection.protocol && row.revision == self.connection.revision
        }) {
            return Err(FlowError::HistoryStale);
        }
        Work::begin(Rc::clone(&self.page), Family::Providers, false)
    }
}
