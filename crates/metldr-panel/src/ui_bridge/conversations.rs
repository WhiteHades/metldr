use serde::{Deserialize, Serialize};

use crate::{
    flow::FlowClient,
    history::{Context, ConversationSummary, Cursor, Scope, validate_read_position},
    model::FlowError,
};

const MAX_CURSOR: usize = 2_048;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Page {
    pub(super) conversations: Vec<ConversationSummary>,
    pub(super) older_cursor: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Continuation {
    context: Context,
    position: Cursor,
}

pub(super) async fn page(
    flow: &FlowClient,
    scope: &Scope,
    cursor: Option<&str>,
) -> Result<Page, FlowError> {
    let (context, position) = if let Some(cursor) = cursor {
        let continuation = decode_cursor(cursor)?;
        if &continuation.context.scope != scope {
            return Err(FlowError::HistoryStale);
        }
        (continuation.context, Some(continuation.position))
    } else {
        scope
            .validate()
            .map_err(|_| protocol("invalid conversation scope"))?;
        (flow.history_current().await?, None)
    };
    if &context.scope != scope {
        return Err(FlowError::HistoryStale);
    }

    let native = flow.history_conversations(&context, position).await?;
    let older_cursor = if let Some(position) = native.next {
        validate_read_position(&context, &position)?;
        let bytes = serde_json::to_vec(&Continuation { context, position })
            .map_err(|error| protocol(&error.to_string()))?;
        if bytes.len() > MAX_CURSOR {
            return Err(protocol("conversation cursor exceeds its byte limit"));
        }
        Some(String::from_utf8(bytes).map_err(|error| protocol(&error.to_string()))?)
    } else {
        None
    };
    let page = Page {
        conversations: native.conversations,
        older_cursor,
    };
    if serde_json::to_vec(&page)
        .map_err(|error| protocol(&error.to_string()))?
        .len()
        > crate::MAX_FRAME
    {
        return Err(protocol("conversation page exceeds its frame limit"));
    }
    Ok(page)
}

fn decode_cursor(cursor: &str) -> Result<Continuation, FlowError> {
    if cursor.len() > MAX_CURSOR {
        return Err(protocol("conversation cursor exceeds its byte limit"));
    }
    let continuation: Continuation =
        serde_json::from_str(cursor).map_err(|_| protocol("invalid conversation cursor"))?;
    validate_read_position(&continuation.context, &continuation.position)?;
    Ok(continuation)
}

fn protocol(message: &str) -> FlowError {
    FlowError::Protocol(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cursor_unknown_fields_and_oversized_input() {
        let store = "0123456789abcdef0123456789abcdef";
        let mut cursor = serde_json::json!({
            "context": {
                "scope": {"store": store, "conversation": format!("{store}:1"), "epoch": "1"},
                "generation": "g1"
            },
            "position": {"anchor": "4", "before": "3"}
        });
        assert!(decode_cursor(&cursor.to_string()).is_ok());
        cursor["extra"] = serde_json::json!(true);
        assert!(decode_cursor(&cursor.to_string()).is_err());
        assert!(decode_cursor(&" ".repeat(MAX_CURSOR + 1)).is_err());
    }
}
