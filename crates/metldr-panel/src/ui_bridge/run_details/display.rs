use super::*;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    reports: Vec<RunReport>,
    next_offset: Option<u32>,
}

#[wasm_bindgen]
pub struct UiRunReportPage {
    data: Page,
}

pub(super) fn page(reports: &[RunReport], offset: u32) -> Result<UiRunReportPage, String> {
    let offset = offset as usize;
    if offset > reports.len() {
        return Err("Invalid run report page offset.".into());
    }
    let mut data = Page {
        reports: Vec::new(),
        next_offset: None,
    };
    for report in reports.iter().skip(offset).take(16) {
        data.reports.push(report.clone());
        data.next_offset = (offset + data.reports.len() < reports.len())
            .then_some((offset + data.reports.len()) as u32);
        if serde_json::to_vec(&data)
            .map_err(|error| error.to_string())?
            .len()
            >= crate::MAX_FRAME - 1_024
        {
            data.reports.pop();
            if data.reports.is_empty() {
                return Err("Run report exceeds its page limit.".into());
            }
            data.next_offset = Some((offset + data.reports.len()) as u32);
            break;
        }
    }
    Ok(UiRunReportPage { data })
}

#[wasm_bindgen]
impl UiRunReportPage {
    #[wasm_bindgen(getter)]
    pub fn next_offset(&self) -> Option<u32> {
        self.data.next_offset
    }

    pub fn reports(&self) -> Vec<UiRunReport> {
        self.data
            .reports
            .iter()
            .cloned()
            .map(|report| UiRunReport { report })
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiRunReport {
    report: RunReport,
}

#[wasm_bindgen]
impl UiRunReport {
    #[wasm_bindgen(getter)]
    pub fn tool(&self) -> String {
        self.report.tool.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn phase(&self) -> String {
        self.report.phase.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn outcome(&self) -> Option<String> {
        self.report.outcome.map(|outcome| outcome.to_string())
    }

    #[wasm_bindgen(getter)]
    pub fn call_id(&self) -> Option<String> {
        self.report.call_id.clone()
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use crate::model::{ActivityOutcome, ActivityPhase};

    #[test]
    fn pages_reports_in_order_within_the_size_limit() {
        let reports = (0..17)
            .map(|index| RunReport {
                tool: format!("tool-{index}"),
                phase: ActivityPhase::Completed,
                outcome: Some(ActivityOutcome::Succeeded),
                call_id: None,
            })
            .collect::<Vec<_>>();
        let first = page(&reports, 0).unwrap();
        assert_eq!(first.data.reports.len(), 16);
        assert_eq!(first.data.next_offset, Some(16));
        assert!(serde_json::to_vec(&first.data).unwrap().len() < crate::MAX_FRAME - 1_024);
        let last = page(&reports, 16).unwrap();
        assert_eq!(last.data.reports.len(), 1);
        assert_eq!(last.data.reports[0].tool, "tool-16");
        assert_eq!(last.data.next_offset, None);
        assert!(page(&reports, 18).is_err());
    }
}
