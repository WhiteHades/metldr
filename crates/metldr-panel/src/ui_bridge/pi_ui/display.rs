use super::*;
use crate::model::{PiUiDialog, PiUiNotice, PiUiPending, PiUiStatus, PiUiView, PiUiWidget};

#[wasm_bindgen]
pub struct UiPiSnapshot {
    pub(super) state: State,
}

#[wasm_bindgen]
impl UiPiSnapshot {
    #[wasm_bindgen(getter)]
    pub fn connected(&self) -> bool {
        self.state.connected
    }

    #[wasm_bindgen(getter)]
    pub fn stale(&self) -> bool {
        self.state.stale
    }

    #[wasm_bindgen(getter)]
    pub fn busy(&self) -> bool {
        !self.state.ready || !matches!(self.state.operation, Operation::Idle)
    }

    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.state.recovery_required || matches!(self.state.operation, Operation::Uncertain(_))
    }

    #[wasm_bindgen(getter)]
    pub fn notice(&self) -> Option<String> {
        if self.state.stale {
            return Some("The conversation changed. Reload to check Pi UI requests.".into());
        }
        if !self.state.connected {
            return Some("Connection lost. Reload to check Pi UI requests.".into());
        }
        if self.state.recovery_required {
            return Some("Reload to check saved work before replying.".into());
        }
        if !self.state.ready {
            return Some("Opening conversation.".into());
        }
        match self.state.operation {
            Operation::Idle => None,
            Operation::Sending(_) => Some("Sending reply to Pi.".into()),
            Operation::Waiting(_) => {
                Some("Reply received. Waiting for the request to close.".into())
            }
            Operation::Uncertain(_) => Some(
                "The reply result is unconfirmed. Reload to check the request before replying again."
                    .into(),
            ),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.state
            .ui
            .as_ref()
            .map(|ui| ui.run.as_str().to_owned())
            .unwrap_or_default()
    }

    #[wasm_bindgen(getter)]
    pub fn title(&self) -> Option<String> {
        self.state
            .ui
            .as_ref()
            .and_then(|ui| ui.title.as_deref())
            .map(crate::pi_ui::display_plain)
    }

    #[wasm_bindgen(getter)]
    pub fn editor_revision(&self) -> Option<String> {
        self.state
            .ui
            .as_ref()
            .and_then(|ui| ui.editor.as_ref())
            .map(|editor| editor.revision.to_string())
    }

    #[wasm_bindgen(getter)]
    pub fn editor_preview(&self) -> Option<String> {
        self.state
            .ui
            .as_ref()
            .and_then(|ui| ui.editor.as_ref())
            .map(|editor| crate::pi_ui::display_plain(&editor.text))
    }

    #[wasm_bindgen(getter)]
    pub fn editor_destination(&self) -> Option<String> {
        self.state.workflow.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn editor_enabled(&self) -> bool {
        self.state.connected
            && !self.state.stale
            && self.state.ready
            && !self.uncertain()
            && self.state.workflow.is_some()
            && self.state.ui.as_ref().is_some_and(|ui| ui.editor.is_some())
    }

    #[wasm_bindgen(getter)]
    pub fn notices_omitted(&self) -> String {
        self.state
            .ui
            .as_ref()
            .map(|ui| ui.notices_omitted.to_string())
            .unwrap_or_else(|| "0".into())
    }

    pub fn requests(&self) -> Vec<UiPiRequest> {
        self.state
            .ui
            .as_ref()
            .map(|ui| {
                ui.pending
                    .iter()
                    .map(|pending| UiPiRequest {
                        run: ui.run.as_str().to_owned(),
                        pending: pending.clone(),
                        enabled: self.state.enabled(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn statuses(&self) -> Vec<UiPiStatus> {
        self.state
            .ui
            .as_ref()
            .map(|ui| {
                ui.statuses
                    .iter()
                    .filter_map(|status| {
                        status.text.as_deref().map(|text| UiPiStatus {
                            key: status.key.clone(),
                            text: display_plain(text),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn widgets(&self) -> Vec<UiPiWidget> {
        self.state
            .ui
            .as_ref()
            .map(|ui| {
                ui.widgets
                    .iter()
                    .filter(|widget| widget.lines.is_some())
                    .cloned()
                    .map(Into::into)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn notices(&self) -> Vec<UiPiNotice> {
        self.state
            .ui
            .as_ref()
            .map(|ui| ui.notices.iter().cloned().map(Into::into).collect())
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
pub enum UiPiDialogKind {
    Select,
    Confirm,
    Input,
    Editor,
}

#[wasm_bindgen]
pub struct UiPiRequest {
    run: String,
    pending: PiUiPending,
    enabled: bool,
}

#[wasm_bindgen]
impl UiPiRequest {
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.run.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn request_id(&self) -> String {
        self.pending.request.as_str().to_owned()
    }

    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> UiPiDialogKind {
        match self.pending.dialog {
            PiUiDialog::Select { .. } => UiPiDialogKind::Select,
            PiUiDialog::Confirm { .. } => UiPiDialogKind::Confirm,
            PiUiDialog::Input { .. } => UiPiDialogKind::Input,
            PiUiDialog::Editor { .. } => UiPiDialogKind::Editor,
        }
    }

    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        let title = match &self.pending.dialog {
            PiUiDialog::Select { title, .. }
            | PiUiDialog::Confirm { title, .. }
            | PiUiDialog::Input { title, .. }
            | PiUiDialog::Editor { title, .. } => title,
        };
        crate::pi_ui::display_plain(title)
    }

    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        match &self.pending.dialog {
            PiUiDialog::Confirm { message, .. } => crate::pi_ui::display_plain(message),
            _ => String::new(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn placeholder(&self) -> String {
        match &self.pending.dialog {
            PiUiDialog::Input { placeholder, .. } => placeholder
                .as_deref()
                .map(crate::pi_ui::display_plain)
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn prefill(&self) -> String {
        match &self.pending.dialog {
            PiUiDialog::Editor { prefill, .. } => prefill.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn options(&self) -> Vec<UiPiOption> {
        match &self.pending.dialog {
            PiUiDialog::Select { options, .. } => options
                .iter()
                .map(|value| UiPiOption {
                    value: value.clone(),
                    label: crate::pi_ui::display_plain(value),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[wasm_bindgen]
pub struct UiPiOption {
    value: String,
    label: String,
}

#[wasm_bindgen]
impl UiPiOption {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> String {
        self.value.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.label.clone()
    }
}

#[wasm_bindgen]
pub struct UiPiStatus {
    key: String,
    text: String,
}

#[wasm_bindgen]
impl UiPiStatus {
    #[wasm_bindgen(getter)]
    pub fn key(&self) -> String {
        self.key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

#[wasm_bindgen]
pub struct UiPiWidget {
    key: String,
    lines: Vec<String>,
    above_editor: bool,
}

impl From<PiUiWidget> for UiPiWidget {
    fn from(widget: PiUiWidget) -> Self {
        Self {
            key: widget.key,
            lines: widget
                .lines
                .unwrap_or_default()
                .iter()
                .map(|line| crate::pi_ui::display_plain(line))
                .collect(),
            above_editor: widget.placement == "aboveEditor",
        }
    }
}

#[wasm_bindgen]
impl UiPiWidget {
    #[wasm_bindgen(getter)]
    pub fn key(&self) -> String {
        self.key.clone()
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn above_editor(&self) -> bool {
        self.above_editor
    }
}

#[wasm_bindgen]
pub struct UiPiNotice {
    level: String,
    message: String,
}

impl From<PiUiNotice> for UiPiNotice {
    fn from(notice: PiUiNotice) -> Self {
        Self {
            level: crate::pi_ui::display_plain(&notice.level),
            message: crate::pi_ui::display_plain(&notice.message),
        }
    }
}

#[wasm_bindgen]
impl UiPiNotice {
    #[wasm_bindgen(getter)]
    pub fn level(&self) -> String {
        self.level.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        self.message.clone()
    }
}
