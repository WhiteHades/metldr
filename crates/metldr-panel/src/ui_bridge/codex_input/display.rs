use super::*;
use crate::model::{
    CodexFormField, CodexInputPending, CodexInputPrompt, CodexQuestion, CodexQuestionOption,
};

#[wasm_bindgen]
pub struct UiCodexSnapshot {
    pub(super) state: State,
}
#[wasm_bindgen]
impl UiCodexSnapshot {
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
            Some("The conversation changed. Reload to check Codex requests.".into())
        } else if !self.state.connected {
            Some("Connection lost. Reload to check Codex requests.".into())
        } else if self.state.recovery_required {
            Some("Reload to check saved work before replying.".into())
        } else if !self.state.ready {
            Some("Opening conversation.".into())
        } else {
            match self.state.operation {
                Operation::Idle => None,
                Operation::Sending(_) => Some("Sending reply to Codex.".into()),
                Operation::Waiting(_) => {
                    Some("Reply received. Waiting for the request to close.".into())
                }
                Operation::Uncertain(_) => Some(
                    "The reply result is unconfirmed. Reload to check the request before replying again."
                        .into(),
                ),
            }
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
    pub fn requests(&self) -> Vec<UiCodexRequest> {
        self.state
            .ui
            .as_ref()
            .map(|ui| {
                ui.pending
                    .iter()
                    .cloned()
                    .map(|pending| UiCodexRequest {
                        run: ui.run.as_str().into(),
                        pending,
                        enabled: self.state.enabled(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
pub enum UiCodexPromptKind {
    Questions,
    McpForm,
    McpToolApproval,
}
#[wasm_bindgen]
pub struct UiCodexRequest {
    run: String,
    pending: CodexInputPending,
    enabled: bool,
}
#[wasm_bindgen]
impl UiCodexRequest {
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.run.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn request_id(&self) -> String {
        self.pending.request.as_str().into()
    }
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> UiCodexPromptKind {
        match &self.pending.prompt {
            CodexInputPrompt::Questions { .. } => UiCodexPromptKind::Questions,
            CodexInputPrompt::McpForm { .. } => UiCodexPromptKind::McpForm,
            CodexInputPrompt::McpToolApproval { .. } => UiCodexPromptKind::McpToolApproval,
        }
    }
    #[wasm_bindgen(getter)]
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[wasm_bindgen(getter)]
    pub fn is_blocking(&self) -> bool {
        matches!(
            &self.pending.prompt,
            CodexInputPrompt::Questions {
                is_blocking: true,
                ..
            }
        )
    }
    #[wasm_bindgen(getter)]
    pub fn item(&self) -> String {
        match &self.pending.prompt {
            CodexInputPrompt::Questions { item, .. } => crate::pi_ui::display_plain(item),
            _ => String::new(),
        }
    }
    #[wasm_bindgen(getter)]
    pub fn server(&self) -> String {
        match &self.pending.prompt {
            CodexInputPrompt::McpForm { server, .. }
            | CodexInputPrompt::McpToolApproval { server, .. } => {
                crate::pi_ui::display_plain(server)
            }
            _ => String::new(),
        }
    }
    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        match &self.pending.prompt {
            CodexInputPrompt::McpForm { message, .. }
            | CodexInputPrompt::McpToolApproval { message, .. } => {
                crate::pi_ui::display_plain(message)
            }
            _ => String::new(),
        }
    }
    #[wasm_bindgen(getter)]
    pub fn review(&self) -> String {
        match &self.pending.prompt {
            CodexInputPrompt::McpToolApproval { review, .. } => crate::pi_ui::display_plain(review),
            _ => String::new(),
        }
    }
    pub fn questions(&self) -> Vec<UiCodexQuestion> {
        match &self.pending.prompt {
            CodexInputPrompt::Questions { questions, .. } => {
                questions.iter().cloned().map(Into::into).collect()
            }
            _ => Vec::new(),
        }
    }
    pub fn fields(&self) -> Vec<UiCodexField> {
        match &self.pending.prompt {
            CodexInputPrompt::McpForm { fields, .. } => {
                fields.iter().cloned().map(Into::into).collect()
            }
            _ => Vec::new(),
        }
    }
}
#[wasm_bindgen]
pub struct UiCodexQuestion {
    id: String,
    header: String,
    text: String,
    other: bool,
    options: Vec<CodexQuestionOption>,
}
impl From<CodexQuestion> for UiCodexQuestion {
    fn from(v: CodexQuestion) -> Self {
        Self {
            id: v.id,
            header: v.header,
            text: v.text,
            other: v.other,
            options: v.options,
        }
    }
}
#[wasm_bindgen]
impl UiCodexQuestion {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.id.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn header(&self) -> String {
        crate::pi_ui::display_plain(&self.header)
    }
    #[wasm_bindgen(getter)]
    pub fn text(&self) -> String {
        crate::pi_ui::display_plain(&self.text)
    }
    #[wasm_bindgen(getter)]
    pub fn other(&self) -> bool {
        self.other
    }
    pub fn options(&self) -> Vec<UiCodexOption> {
        self.options.iter().cloned().map(Into::into).collect()
    }
}
#[wasm_bindgen]
pub struct UiCodexOption {
    value: String,
    label: String,
    description: String,
}
impl From<CodexQuestionOption> for UiCodexOption {
    fn from(v: CodexQuestionOption) -> Self {
        Self {
            value: v.label.clone(),
            label: crate::pi_ui::display_plain(&v.label),
            description: crate::pi_ui::display_plain(&v.description),
        }
    }
}
impl UiCodexOption {
    fn choice(value: String) -> Self {
        Self {
            label: crate::pi_ui::display_plain(&value),
            value,
            description: String::new(),
        }
    }
}
#[wasm_bindgen]
impl UiCodexOption {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> String {
        self.value.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.label.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn description(&self) -> String {
        self.description.clone()
    }
}
#[wasm_bindgen]
pub struct UiCodexField {
    name: String,
    title: String,
    description: String,
    min_chars: u32,
    max_chars: u32,
    default_value: Option<String>,
    choices: Option<Vec<String>>,
}
impl From<CodexFormField> for UiCodexField {
    fn from(v: CodexFormField) -> Self {
        Self {
            name: v.name,
            title: v.title,
            description: v
                .description
                .map(|s| crate::pi_ui::display_plain(&s))
                .unwrap_or_default(),
            min_chars: v.min_chars,
            max_chars: v.max_chars,
            default_value: v.default,
            choices: v.choices,
        }
    }
}
#[wasm_bindgen]
impl UiCodexField {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.name.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        crate::pi_ui::display_plain(&self.title)
    }
    #[wasm_bindgen(getter)]
    pub fn description(&self) -> String {
        self.description.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn min_chars(&self) -> u32 {
        self.min_chars
    }
    #[wasm_bindgen(getter)]
    pub fn max_chars(&self) -> u32 {
        self.max_chars
    }
    #[wasm_bindgen(getter)]
    pub fn default_value(&self) -> Option<String> {
        self.default_value.clone()
    }
    pub fn choices(&self) -> Vec<UiCodexOption> {
        self.choices
            .as_ref()
            .map(|values| values.iter().cloned().map(UiCodexOption::choice).collect())
            .unwrap_or_default()
    }
}
