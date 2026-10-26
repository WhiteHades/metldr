use super::*;
use crate::{
    ag_ui::{
        RunOrigin,
        run_details::{self, RunDetailsReference, RunReport},
    },
    model::RunStatus,
    run_display,
};

mod display;
pub use display::{UiRunReport, UiRunReportPage};

#[wasm_bindgen]
pub struct UiRunDetails {
    core: Rc<Core>,
    reference: RunDetailsReference,
    reports: Vec<RunReport>,
    route: String,
    status: RunStatus,
    run_id: String,
}

pub(super) async fn open(core: Rc<Core>, encoded: String) -> Result<UiRunDetails, FlowError> {
    core.ensure_current()?;
    if encoded.len() > crate::MAX_FRAME {
        return Err(FlowError::InvalidInput(
            "Run details reference exceeds its limit.".into(),
        ));
    }
    let reference: RunDetailsReference = serde_json::from_str(&encoded)
        .map_err(|_| FlowError::InvalidInput("Invalid run details reference.".into()))?;
    reference.validate().map_err(FlowError::InvalidInput)?;
    let run = core
        .referenced_run(&reference.run_id, &reference.origin)
        .await?;
    core.ensure_current()?;
    let reports = run_details::resolve(&run, &reference).map_err(|_| FlowError::HistoryStale)?;
    Ok(UiRunDetails {
        core,
        reference,
        reports,
        route: run_display::route(&run.route),
        status: run.status,
        run_id: run.id.as_str().into(),
    })
}

#[wasm_bindgen]
impl UiRunDetails {
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.run_id.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn route(&self) -> String {
        self.route.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn status(&self) -> String {
        self.status.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn report_count(&self) -> u32 {
        self.reports.len() as u32
    }

    pub fn page(&self, offset: u32) -> Result<UiRunReportPage, JsValue> {
        self.core.ensure_current().map_err(js_error)?;
        if matches!(self.reference.origin, RunOrigin::Live) {
            let view = self.core.page.view.borrow();
            let run = view
                .runs
                .iter()
                .find(|run| run.id == self.reference.run_id)
                .ok_or_else(|| js_error(FlowError::HistoryStale))?;
            run_details::resolve(run, &self.reference)
                .map_err(|_| js_error(FlowError::HistoryStale))?;
        }
        display::page(&self.reports, offset).map_err(|message| crate::err(&message))
    }
}
