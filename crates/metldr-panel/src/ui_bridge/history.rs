use metldr_conversation::EntryRef;
use serde::{Deserialize, Serialize};

use crate::{
    ag_ui::{
        AgUiMessage, MAX_HISTORY_MESSAGES, RunOrigin,
        run_details::RunDetailsReference,
        tools::{self, ToolGroupReference},
    },
    flow::FlowClient,
    history::{
        Context, Cursor, RunReference, Scope, before_run, validate_read_position,
        validate_reference,
    },
    model::{FlowError, RunId, RunView},
};

const MESSAGE_BUDGET: usize = crate::MAX_FRAME - 4_096;
const MAX_CURSOR: usize = 2_048;
const MAX_RUN_MESSAGES: usize = 1
    + metldr_conversation::MAX_MESSAGES
    + metldr_conversation::MAX_ARTIFACTS
    + metldr_conversation::MAX_TOOLS
    + 1;

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
        let messages = run_messages(&run, reference.clone());
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
    let messages = run_messages(&run, pending.run.clone());
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

fn run_messages(run: &RunView, reference: RunReference) -> Vec<AgUiMessage> {
    let details = RunDetailsReference::new(
        run,
        RunOrigin::Saved {
            reference: reference.clone(),
        },
    );
    let tools = tools::groups(run, RunOrigin::Saved { reference });
    run_messages_for(
        &run.id,
        run.question.as_deref(),
        &run.output,
        &tools,
        Some(details),
    )
}

fn run_messages_for(
    run_id: &RunId,
    question: Option<&str>,
    output: &metldr_conversation::Display,
    tools: &[ToolGroupReference],
    details: Option<RunDetailsReference>,
) -> Vec<AgUiMessage> {
    let mut messages = Vec::new();
    if let Some(question) = question {
        messages.push(AgUiMessage::User {
            id: format!("{run_id}:question"),
            content: question.to_owned(),
        });
    }
    if let Some(details) = details {
        messages.push(AgUiMessage::run_details(details));
    }
    let mut has_entries = false;
    for entry in output.entries() {
        has_entries = true;
        if let EntryRef::Message { key, text, .. } = entry {
            messages.push(AgUiMessage::Assistant {
                id: format!("{run_id}:{}", key.dom_key()),
                content: text.to_owned(),
            });
        } else if let EntryRef::Artifact { id, presentation } = entry {
            messages.push(match presentation {
                metldr_conversation::ArtifactPresentation::Table => AgUiMessage::table(run_id, id),
                metldr_conversation::ArtifactPresentation::Chart => AgUiMessage::chart(run_id, id),
                metldr_conversation::ArtifactPresentation::Generated {
                    revision,
                    completion,
                } => AgUiMessage::generated(run_id, id, *revision, *completion),
            });
        } else if let EntryRef::Tool { id } = entry
            && let Some(reference) = tools.iter().find(|group| &group.group_id == id)
        {
            messages.push(AgUiMessage::native_tools(reference.clone()));
        }
    }
    if !has_entries && !output.text().is_empty() {
        messages.push(AgUiMessage::Assistant {
            id: format!("{run_id}:flat"),
            content: output.text().to_owned(),
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
    use metldr_conversation::{ArtifactId, Display, Projection};

    #[test]
    fn archives_the_same_ordered_native_tools_with_an_exact_saved_reference() {
        let run = crate::ag_ui::tools::checks::run();
        let reference = RunReference {
            id: run.id.clone(),
            source: run.source.clone(),
            revision: metldr_history_pages::Counter(9),
            kind: run.kind,
            status: run.status,
        };
        let messages = run_messages(&run, reference.clone());
        let encoded = serde_json::to_value(&messages).unwrap();
        assert_eq!(
            messages.iter().map(AgUiMessage::id).collect::<Vec<_>>(),
            [
                "0123456789abcdef0123456789abcdef:1:question",
                "0123456789abcdef0123456789abcdef:1:details",
                "0123456789abcdef0123456789abcdef:1:observed:1",
                "0123456789abcdef0123456789abcdef:1:tools:call-1",
                "0123456789abcdef0123456789abcdef:1:observed:2",
                "0123456789abcdef0123456789abcdef:1:tools:call-3",
            ]
        );
        assert_eq!(encoded[1]["activityType"], "metldr.run-details");
        assert_eq!(encoded[3]["activityType"], "metldr.native-tools");
        assert_eq!(
            encoded[3]["content"]["toolIds"],
            serde_json::json!(["call-1", "call-2"])
        );
        assert_eq!(
            encoded[3]["content"]["origin"],
            serde_json::json!({"kind":"saved","reference":reference})
        );
        assert_eq!(encoded[5]["content"]["reportCount"], 0);
    }

    fn message(id: &str, content: &str) -> AgUiMessage {
        AgUiMessage::Assistant {
            id: id.into(),
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
        assert_eq!(packed[0].text(), messages[1].text());
        assert_eq!(encoded_bytes, serde_json::to_vec(&packed).unwrap().len());

        let mut messages = vec![AgUiMessage::User {
            id: "0123456789abcdef0123456789abcdef:9:question".into(),
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
    fn archives_text_and_table_entries_in_order_across_a_byte_boundary() {
        let run: RunId = serde_json::from_str("\"0123456789abcdef0123456789abcdef:2\"").unwrap();
        let artifact = ArtifactId::new("0123456789abcdef0123456789abcdef:2".into()).unwrap();
        let output = Display::new(Projection {
            output: "beforeafter".into(),
            conversation: vec![
                metldr_conversation::ProjectedEntry::Message {
                    key: metldr_conversation::MessageKey::observed(1).unwrap(),
                    start: 0,
                    end: 6,
                    completion: metldr_conversation::Completion::FinalSnapshot,
                },
                metldr_conversation::ProjectedEntry::Artifact {
                    id: artifact.clone(),
                    presentation: metldr_conversation::ArtifactPresentation::Table,
                },
                metldr_conversation::ProjectedEntry::Message {
                    key: metldr_conversation::MessageKey::observed(2).unwrap(),
                    start: 6,
                    end: 11,
                    completion: metldr_conversation::Completion::FinalSnapshot,
                },
            ],
        })
        .unwrap();
        let messages = run_messages_for(&run, Some("question"), &output, &[], None);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].text(), Some("question"));
        assert_eq!(messages[1].text(), Some("before"));
        assert_eq!(messages[2], AgUiMessage::table(&run, &artifact));
        assert_eq!(messages[3].text(), Some("after"));
        assert_eq!(
            serde_json::to_value(&messages[2]).unwrap(),
            serde_json::json!({
                "role": "activity",
                "id": format!("{run}:artifact:{}", artifact.as_str()),
                "activityType": "metldr.table",
                "content": {"runId": run, "artifactId": artifact}
            })
        );

        let suffix_bytes = serde_json::to_vec(&messages[2]).unwrap().len()
            + serde_json::to_vec(&messages[3]).unwrap().len()
            + 2;
        let mut newest = Vec::new();
        let mut bytes = 2;
        assert!(!append_reverse(&mut newest, &mut bytes, &messages, suffix_bytes).unwrap());
        let remaining = messages.len() - newest.len();
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
        older.reverse();
        newest.reverse();
        older.extend(newest);
        assert_eq!(older, messages);
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
