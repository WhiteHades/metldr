use super::*;
use crate::ag_ui::{
    RunOrigin,
    tools::{self, ToolGroupReference, ToolRow},
};
use crate::model::RunStatus;

mod display;
pub use display::{UiNativeTool, UiNativeToolPage, UiToolPreview};

#[wasm_bindgen]
pub struct UiNativeTools {
    core: Rc<Core>,
    reference: ToolGroupReference,
    rows: Vec<ToolRow>,
    status: RunStatus,
}

pub(super) async fn open(core: Rc<Core>, encoded: String) -> Result<UiNativeTools, FlowError> {
    core.ensure_current()?;
    if encoded.len() > crate::MAX_FRAME {
        return Err(FlowError::InvalidInput(
            "Native tool reference exceeds its limit.".into(),
        ));
    }
    let reference: ToolGroupReference = serde_json::from_str(&encoded)
        .map_err(|_| FlowError::InvalidInput("Invalid native tool reference.".into()))?;
    reference.validate().map_err(FlowError::InvalidInput)?;
    let run = core
        .referenced_run(&reference.run_id, &reference.origin)
        .await?;
    let rows = tools::resolve(&run, &reference).map_err(|_| FlowError::HistoryStale)?;
    Ok(UiNativeTools {
        core,
        reference,
        rows,
        status: run.status,
    })
}

#[wasm_bindgen]
impl UiNativeTools {
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.reference.run_id.as_str().into()
    }

    #[wasm_bindgen(getter)]
    pub fn group_id(&self) -> String {
        self.reference.group_id.as_str().into()
    }

    #[wasm_bindgen(getter)]
    pub fn tool_count(&self) -> u32 {
        self.rows.len() as u32
    }

    #[wasm_bindgen(getter)]
    pub fn report_count(&self) -> u32 {
        self.reference.report_count
    }

    #[wasm_bindgen(getter)]
    pub fn run_status(&self) -> String {
        self.status.to_string()
    }

    pub fn page(&self, offset: u32) -> Result<UiNativeToolPage, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        if matches!(self.reference.origin, RunOrigin::Live) {
            let view = self.core.page.view.borrow();
            let run = view
                .runs
                .iter()
                .find(|run| run.id == self.reference.run_id)
                .ok_or_else(|| js_error(FlowError::HistoryStale))?;
            tools::resolve(run, &self.reference).map_err(|_| js_error(FlowError::HistoryStale))?;
        }
        display::page(&self.rows, offset).map_err(|message| crate::err(&message))
    }
}
