use super::*;
use crate::{ag_ui::tools::ToolReport, model::ToolPreviewView, pi_ui::display_plain};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    tools: Vec<ToolRow>,
    next_offset: Option<u32>,
}

#[wasm_bindgen]
pub struct UiNativeToolPage {
    data: Page,
}

pub(super) fn page(rows: &[ToolRow], offset: u32) -> Result<UiNativeToolPage, String> {
    let offset = offset as usize;
    if offset > rows.len() {
        return Err("Invalid native tool page offset.".into());
    }
    let mut data = Page {
        tools: vec![],
        next_offset: None,
    };
    for row in rows.iter().skip(offset).take(8) {
        data.tools.push(row.clone());
        data.next_offset =
            (offset + data.tools.len() < rows.len()).then_some((offset + data.tools.len()) as u32);
        if serde_json::to_vec(&data)
            .map_err(|error| error.to_string())?
            .len()
            > crate::MAX_FRAME - 1_024
        {
            data.tools.pop();
            if data.tools.is_empty() {
                return Err("Native tool preview exceeds its page limit.".into());
            }
            data.next_offset = Some((offset + data.tools.len()) as u32);
            break;
        }
    }
    Ok(UiNativeToolPage { data })
}

#[wasm_bindgen]
impl UiNativeToolPage {
    #[wasm_bindgen(getter)]
    pub fn next_offset(&self) -> Option<u32> {
        self.data.next_offset
    }

    pub fn tools(&self) -> Vec<UiNativeTool> {
        self.data
            .tools
            .iter()
            .cloned()
            .map(|row| UiNativeTool { row })
            .collect()
    }
}

#[wasm_bindgen]
pub struct UiNativeTool {
    row: ToolRow,
}

#[wasm_bindgen]
impl UiNativeTool {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.row.id.as_str().into()
    }

    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<String> {
        self.row
            .details
            .as_ref()
            .map(|details| display_plain(&details.name))
    }

    #[wasm_bindgen(getter)]
    pub fn phase(&self) -> Option<String> {
        self.row
            .details
            .as_ref()
            .map(|details| details.phase.to_string())
    }

    #[wasm_bindgen(getter)]
    pub fn outcome(&self) -> Option<String> {
        self.row
            .details
            .as_ref()
            .and_then(|details| details.outcome.map(|outcome| outcome.to_string()))
    }

    #[wasm_bindgen(getter)]
    pub fn reports(&self) -> u32 {
        self.row
            .details
            .as_ref()
            .map_or(0, |details| details.history.len() as u32)
    }

    pub fn history(&self) -> Vec<UiToolReport> {
        self.row
            .details
            .as_ref()
            .map(|details| {
                details
                    .history
                    .iter()
                    .copied()
                    .map(UiToolReport::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[wasm_bindgen(getter)]
    pub fn title(&self) -> Option<UiToolPreview> {
        self.row
            .details
            .as_ref()
            .and_then(|details| details.title.clone())
            .map(UiToolPreview::from)
    }

    #[wasm_bindgen(getter)]
    pub fn input(&self) -> Option<UiToolPreview> {
        self.row
            .details
            .as_ref()
            .and_then(|details| details.input.clone())
            .map(UiToolPreview::from)
    }

    #[wasm_bindgen(getter)]
    pub fn output(&self) -> Option<UiToolPreview> {
        self.row
            .details
            .as_ref()
            .and_then(|details| details.output.clone())
            .map(UiToolPreview::from)
    }
}

#[wasm_bindgen]
pub struct UiToolReport {
    report: ToolReport,
}

impl From<ToolReport> for UiToolReport {
    fn from(report: ToolReport) -> Self {
        Self { report }
    }
}

#[wasm_bindgen]
impl UiToolReport {
    #[wasm_bindgen(getter)]
    pub fn phase(&self) -> String {
        self.report.phase.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn outcome(&self) -> Option<String> {
        self.report.outcome.map(|outcome| outcome.to_string())
    }
}

#[wasm_bindgen]
pub struct UiToolPreview {
    preview: ToolPreviewView,
}

impl From<ToolPreviewView> for UiToolPreview {
    fn from(preview: ToolPreviewView) -> Self {
        Self { preview }
    }
}

#[wasm_bindgen]
impl UiToolPreview {
    #[wasm_bindgen(getter)]
    pub fn text(&self) -> String {
        display_plain(&self.preview.text)
    }

    #[wasm_bindgen(getter)]
    pub fn truncated(&self) -> bool {
        self.preview.truncated
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use crate::{
        ag_ui::tools::RetainedTool,
        model::{ActivityOutcome, ActivityPhase},
    };
    use metldr_conversation::ToolId;

    #[test]
    fn pages_every_retained_preview_under_the_encoded_byte_limit() {
        let rows = (0..metldr_conversation::MAX_TOOLS)
            .map(|index| ToolRow {
                id: ToolId::new(format!("call-{index}")).unwrap(),
                details: Some(RetainedTool {
                    name: "read".into(),
                    phase: ActivityPhase::Completed,
                    outcome: Some(ActivityOutcome::Succeeded),
                    history: (0..if index == 0 { 64 } else { 2 })
                        .map(|_| crate::ag_ui::tools::ToolReport {
                            phase: ActivityPhase::Completed,
                            outcome: Some(ActivityOutcome::Succeeded),
                        })
                        .collect(),
                    title: Some(ToolPreviewView {
                        text: "\0".repeat(256),
                        truncated: true,
                    }),
                    input: Some(ToolPreviewView {
                        text: "\0".repeat(1_024),
                        truncated: true,
                    }),
                    output: Some(ToolPreviewView {
                        text: "\0".repeat(2_048),
                        truncated: true,
                    }),
                }),
            })
            .collect::<Vec<_>>();
        let mut offset = 0;
        let mut ids = Vec::new();
        loop {
            let page = page(&rows, offset).unwrap();
            assert!(serde_json::to_vec(&page.data).unwrap().len() <= crate::MAX_FRAME - 1_024);
            assert!(!page.data.tools.is_empty() && page.data.tools.len() < 8);
            ids.extend(page.data.tools.iter().map(|row| row.id.as_str().to_owned()));
            match page.data.next_offset {
                Some(next) => {
                    assert!(next > offset);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(
            ids,
            (0..64)
                .map(|index| format!("call-{index}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(rows[0].details.as_ref().unwrap().history.len(), 64);
        assert_eq!(rows[1].details.as_ref().unwrap().history.len(), 2);
        assert!(page(&rows, 65).is_err());
    }
}
