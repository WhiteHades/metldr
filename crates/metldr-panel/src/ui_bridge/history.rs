use metldr_conversation::EntryRef;

use crate::{
    ag_ui::{AgUiMessage, MAX_HISTORY_MESSAGES, MessageRole},
    flow::FlowClient,
    history::{Cursor, Scope, Summary},
    model::{FlowError, RunView},
};

pub(super) async fn messages(
    flow: &FlowClient,
    scope: &Scope,
) -> Result<Vec<AgUiMessage>, FlowError> {
    // shortcut: one-frame transcripts; use paged display history before the UI cutover.
    let context = flow.history_current().await?;
    if context.scope != *scope {
        return Err(FlowError::HistoryStale);
    }

    let mut summaries = Vec::<Summary>::new();
    let mut cursor: Option<Cursor> = None;
    loop {
        let page = flow.history_runs(&context, cursor).await?;
        if page.runs.len() > MAX_HISTORY_MESSAGES.saturating_sub(summaries.len()) {
            return Err(FlowError::Protocol(
                "Saved conversation exceeds the UI history run limit.".into(),
            ));
        }
        summaries.extend(page.runs);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }

    let mut messages = Vec::new();
    let mut bytes = 2usize;
    for summary in summaries.iter().rev() {
        let run = flow.history_run(&context, summary).await?;
        append_run(&mut messages, &mut bytes, &run)?;
    }
    Ok(messages)
}

fn append_run(
    messages: &mut Vec<AgUiMessage>,
    bytes: &mut usize,
    run: &RunView,
) -> Result<(), FlowError> {
    if let Some(question) = &run.question {
        push_message(
            messages,
            bytes,
            AgUiMessage {
                id: format!("{}:question", run.id),
                role: MessageRole::User,
                content: question.clone(),
            },
        )?;
    }

    let mut has_entries = false;
    for entry in run.output.entries() {
        has_entries = true;
        if let EntryRef::Message { key, text, .. } = entry {
            push_message(
                messages,
                bytes,
                AgUiMessage {
                    id: format!("{}:{}", run.id, key.dom_key()),
                    role: MessageRole::Assistant,
                    content: text.to_owned(),
                },
            )?;
        }
    }
    if !has_entries && !run.output.text().is_empty() {
        push_message(
            messages,
            bytes,
            AgUiMessage {
                id: format!("{}:flat", run.id),
                role: MessageRole::Assistant,
                content: run.output.text().to_owned(),
            },
        )?;
    }
    Ok(())
}

fn push_message(
    messages: &mut Vec<AgUiMessage>,
    bytes: &mut usize,
    message: AgUiMessage,
) -> Result<(), FlowError> {
    if messages.len() >= MAX_HISTORY_MESSAGES {
        return Err(FlowError::Protocol(
            "Saved conversation exceeds the UI history message limit.".into(),
        ));
    }
    let encoded =
        serde_json::to_vec(&message).map_err(|error| FlowError::Protocol(error.to_string()))?;
    *bytes += encoded.len() + usize::from(!messages.is_empty());
    if *bytes > crate::MAX_FRAME {
        return Err(FlowError::Protocol(
            "Saved conversation exceeds the UI frame limit.".into(),
        ));
    }
    messages.push(message);
    Ok(())
}
