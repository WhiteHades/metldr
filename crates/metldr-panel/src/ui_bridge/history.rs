use metldr_conversation::EntryRef;

use crate::{
    ag_ui::{AgUiMessage, MessageRole},
    model::PageView,
};

pub(super) fn messages(view: &PageView) -> Vec<AgUiMessage> {
    let mut messages = Vec::new();
    for run in &view.runs {
        if let Some(question) = &run.question {
            messages.push(AgUiMessage {
                id: format!("{}:question", run.id),
                role: MessageRole::User,
                content: question.clone(),
            });
        }
        let mut has_entries = false;
        for entry in run.output.entries() {
            has_entries = true;
            if let EntryRef::Message { key, text, .. } = entry {
                messages.push(AgUiMessage {
                    id: format!("{}:{}", run.id, key.dom_key()),
                    role: MessageRole::Assistant,
                    content: text.to_owned(),
                });
            }
        }
        if !has_entries && !run.output.text().is_empty() {
            messages.push(AgUiMessage {
                id: format!("{}:flat", run.id),
                role: MessageRole::Assistant,
                content: run.output.text().to_owned(),
            });
        }
    }
    messages
}
