use metldr_conversation::EntryRef;
use serde::{Deserialize, Serialize};

use crate::{
    ag_ui::{AgUiMessage, MAX_HISTORY_MESSAGES, MessageRole},
    flow::FlowClient,
    history::{
        Context, Cursor, RunReference, Scope, before_run, validate_read_position,
        validate_reference,
    },
    model::{FlowError, RunView},
};

const MESSAGE_BUDGET: usize = crate::MAX_FRAME - 4_096;
const MAX_CURSOR: usize = 2_048;
const MAX_RUN_MESSAGES: usize = 65;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Page {
    pub(super) messages: Vec<AgUiMessage>,
    pub(super) older_cursor: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Continuation {
    context: Context,
    older: Option<Cursor>,
    pending: Option<Pending>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    run: RunReference,
    remaining_messages: usize,
}

pub(super) async fn page(
    flow: &FlowClient,
    scope: &Scope,
    cursor: Option<&str>,
) -> Result<Page, FlowError> {
    let mut state = if let Some(cursor) = cursor {
        decode_cursor(cursor)?
    } else {
        scope
            .validate()
            .map_err(|_| protocol("invalid history scope"))?;
        let context = flow.history_current().await?;
        Continuation {
            context,
            older: None,
            pending: None,
        }
    };
    if &state.context.scope != scope {
        return Err(FlowError::HistoryStale);
    }
    let pending = state.pending.take();
    if let Some(pending) = pending {
        let anchor = state
            .older
            .as_ref()
            .ok_or_else(|| protocol("missing history cursor"))?
            .anchor;
        return pack_pending(flow, state, pending, anchor).await;
    }
    let native = flow
        .history_runs(&state.context, state.older.clone())
        .await?;
    state.older = native.next;
    pack(flow, state, native.runs, native.anchor).await
}

async fn pack(
    flow: &FlowClient,
    mut state: Continuation,
    summaries: Vec<crate::history::Summary>,
    anchor: metldr_history_pages::Counter,
) -> Result<Page, FlowError> {
    let mut reverse = Vec::new();
    let mut encoded_bytes = 2;
    for summary in summaries {
        let reference = summary.reference();
        let run = flow.history_run(&state.context, &reference).await?;
        let messages = run_messages(&run);
        let already = reverse.len();
        if !append_reverse(&mut reverse, &mut encoded_bytes, &messages, MESSAGE_BUDGET)? {
            state.pending = Some(Pending {
                run: reference.clone(),
                remaining_messages: messages.len() - (reverse.len() - already),
            });
            state.older = Some(before_run(&state.context, anchor, &reference)?);
            return finish(reverse, Some(state));
        }
        if reverse.len() >= MAX_HISTORY_MESSAGES {
            state.older = Some(before_run(&state.context, anchor, &reference)?);
            return finish(reverse, Some(state));
        }
    }
    finish(reverse, state.older.clone().map(|_| state))
}

async fn pack_pending(
    flow: &FlowClient,
    mut state: Continuation,
    pending: Pending,
    anchor: metldr_history_pages::Counter,
) -> Result<Page, FlowError> {
    let run = flow.history_run(&state.context, &pending.run).await?;
    let messages = run_messages(&run);
    if pending.remaining_messages > messages.len() {
        return Err(protocol("invalid pending history run"));
    }
    let prefix = &messages[..pending.remaining_messages];
    let mut reverse = Vec::new();
    let mut encoded_bytes = 2;
    let complete = append_reverse(&mut reverse, &mut encoded_bytes, prefix, MESSAGE_BUDGET)?;
    if !complete {
        let emitted = reverse.len();
        state.pending = Some(Pending {
            run: pending.run.clone(),
            remaining_messages: pending.remaining_messages - emitted,
        });
        state.older = Some(before_run(&state.context, anchor, &pending.run)?);
    }
    finish(reverse, Some(state))
}

fn append_reverse(
    reverse: &mut Vec<AgUiMessage>,
    encoded_bytes: &mut usize,
    messages: &[AgUiMessage],
    budget: usize,
) -> Result<bool, FlowError> {
    for message in messages.iter().rev() {
        let encoded = serde_json::to_vec(&message).map_err(|error| protocol(&error.to_string()))?;
        let addition = encoded.len() + usize::from(!reverse.is_empty());
        if *encoded_bytes + addition > budget {
            if reverse.is_empty() {
                return Err(protocol("saved message exceeds archive page limit"));
            }
            return Ok(false);
        }
        reverse.push(message.clone());
        if reverse.len() > MAX_HISTORY_MESSAGES {
            reverse.pop();
            return Ok(false);
        }
        *encoded_bytes += addition;
    }
    Ok(true)
}

fn finish(mut reverse: Vec<AgUiMessage>, state: Option<Continuation>) -> Result<Page, FlowError> {
    reverse.reverse();
    let older_cursor = if let Some(state) = state {
        let bytes = serde_json::to_vec(&state).map_err(|error| protocol(&error.to_string()))?;
        if bytes.len() > MAX_CURSOR {
            return Err(protocol("archive cursor exceeds its byte limit"));
        }
        Some(String::from_utf8(bytes).map_err(|error| protocol(&error.to_string()))?)
    } else {
        None
    };
    let page = Page {
        messages: reverse,
        older_cursor,
    };
    if serde_json::to_vec(&page)
        .map_err(|error| protocol(&error.to_string()))?
        .len()
        > crate::MAX_FRAME
    {
        return Err(protocol("archive page exceeds its frame limit"));
    }
    Ok(page)
}

fn decode_cursor(cursor: &str) -> Result<Continuation, FlowError> {
    if cursor.len() > MAX_CURSOR {
        return Err(protocol("archive cursor exceeds its byte limit"));
    }
    let state: Continuation =
        serde_json::from_str(cursor).map_err(|_| protocol("invalid archive cursor"))?;
    let position = state
        .older
        .as_ref()
        .ok_or_else(|| protocol("invalid archive cursor"))?;
    validate_read_position(&state.context, position)?;
    if let Some(pending) = &state.pending {
        let run_number = validate_reference(&state.context, &pending.run)?;
        if pending.remaining_messages == 0
            || pending.remaining_messages > MAX_RUN_MESSAGES
            || position.before.0 != run_number
        {
            return Err(protocol("invalid pending history run"));
        }
    }
    Ok(state)
}

fn run_messages(run: &RunView) -> Vec<AgUiMessage> {
    let mut messages = Vec::new();
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
    messages
}

fn protocol(message: &str) -> FlowError {
    FlowError::Protocol(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, content: &str) -> AgUiMessage {
        AgUiMessage {
            id: id.into(),
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }

    #[test]
    fn packs_newest_messages_first_without_truncating() {
        let newest = message(
            &format!("new:{}", "x".repeat(470)),
            &format!("quote: \\\"\n{}", "🙂".repeat(600)),
        );
        let older = message("old", "older text");
        let messages = vec![older, newest];
        let budget = serde_json::to_vec(&messages[1]).unwrap().len() + 2;
        let mut packed = Vec::new();
        let mut encoded_bytes = 2;
        assert!(!append_reverse(&mut packed, &mut encoded_bytes, &messages, budget).unwrap());
        assert_eq!(packed, vec![messages[1].clone()]);
        assert_eq!(packed[0].content, messages[1].content);
        assert_eq!(encoded_bytes, serde_json::to_vec(&packed).unwrap().len());

        let mut messages = vec![AgUiMessage {
            id: "0123456789abcdef0123456789abcdef:9:question".into(),
            role: MessageRole::User,
            content: "\0".repeat(1_024),
        }];
        for ordinal in 0..64 {
            messages.push(message(
                &format!(
                    "0123456789abcdef0123456789abcdef:9:native:{}",
                    format!("{ordinal:03}").repeat(42)
                ),
                &"\0".repeat(128),
            ));
        }
        assert!(serde_json::to_vec(&messages).unwrap().len() > crate::MAX_FRAME);
        let mut newest = Vec::new();
        let mut bytes = 2;
        assert!(!append_reverse(&mut newest, &mut bytes, &messages, MESSAGE_BUDGET).unwrap());
        let remaining = messages.len() - newest.len();
        let newest = finish(newest, None).unwrap();
        let mut older = Vec::new();
        let mut bytes = 2;
        assert!(
            append_reverse(
                &mut older,
                &mut bytes,
                &messages[..remaining],
                MESSAGE_BUDGET
            )
            .unwrap()
        );
        let older = finish(older, None).unwrap();
        assert_eq!(
            older
                .messages
                .into_iter()
                .chain(newest.messages)
                .collect::<Vec<_>>(),
            messages
        );
    }

    #[test]
    fn rejects_cursor_fields_outside_the_continuation_schema() {
        let store = "0123456789abcdef0123456789abcdef";
        let cursor = serde_json::json!({
            "context": {
                "scope": {"store": store, "conversation": format!("{store}:1"), "epoch": "1"},
                "generation": "g1"
            },
            "older": {"anchor": "2", "before": "1"},
            "pending": null,
            "extra": true
        })
        .to_string();
        assert!(decode_cursor(&cursor).is_err());
    }
}
