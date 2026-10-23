use crate::model::{FlowError, RunId, RunKind, RunStatus, SourceId};
pub(crate) use metldr_history_pages::Scope;
use metldr_history_pages::{Counter, DocumentKind, Identity};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const VERSION: u64 = metldr_history_pages::WIRE_VERSION as u64;
const MAX_PAGE: usize = 32_768;
const MAX_RUNS: usize = 16;
const MAX_CONVERSATIONS: usize = 16;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    pub(crate) scope: Scope,
    pub(crate) generation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub(crate) anchor: Counter,
    pub(crate) before: Counter,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preview {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    pub(crate) id: RunId,
    pub(crate) source: SourceId,
    pub(crate) source_available: bool,
    pub(crate) revision: Counter,
    pub(crate) kind: RunKind,
    pub(crate) status: RunStatus,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) question: Option<Preview>,
    pub(crate) output: Preview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunPage {
    pub(crate) store: String,
    pub(crate) conversation: String,
    pub(crate) epoch: Counter,
    pub(crate) sequence: Counter,
    pub(crate) anchor: Counter,
    pub(crate) runs: Vec<Summary>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) next: Option<Cursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Change {
    Create,
    Select { conversation: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationSummary {
    pub(crate) id: String,
    pub(crate) title: Preview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationPage {
    pub(crate) store: String,
    pub(crate) conversation: String,
    pub(crate) epoch: Counter,
    pub(crate) sequence: Counter,
    pub(crate) anchor: Counter,
    pub(crate) conversations: Vec<ConversationSummary>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) next: Option<Cursor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Changed {
    pub(crate) original: Scope,
    pub(crate) current: Context,
    pub(crate) sequence: Counter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Expected {
    Current,
    Runs {
        context: Context,
        cursor: Option<Cursor>,
    },
    Conversations {
        context: Context,
        cursor: Option<Cursor>,
    },
    Change {
        scope: Scope,
        change: Change,
    },
    Pin {
        context: Context,
        kind: DocumentKind,
        document: String,
        revision: Counter,
    },
    Page {
        identity: Identity,
        offset: u32,
    },
    Release {
        identity: Identity,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Response {
    Current(Context),
    Runs(RunPage),
    Conversations(ConversationPage),
    Changed(Changed),
    Pinned(Identity),
    Detail { request: String, bytes: Vec<u8> },
    Released(bool),
    Busy,
    Stale(Option<Observation>),
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub(crate) scope: Scope,
    pub(crate) generation: String,
    pub(crate) sequence: Counter,
    pub(crate) kind: DocumentKind,
    pub(crate) document: String,
    pub(crate) revision: Counter,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) status: Option<RunStatus>,
}

#[derive(Deserialize)]
struct Header {
    v: u64,
    request: String,
    kind: String,
}

impl Expected {
    pub(crate) fn decode(&self, value: Value) -> Result<Response, FlowError> {
        let header: Header = serde_json::from_value(value.clone())
            .map_err(|_| protocol("invalid history response"))?;
        if header.v != VERSION || !valid_token(&header.request, 96) {
            return Err(protocol("invalid history response header"));
        }

        match (self, header.kind.as_str()) {
            (Self::Current, "history_current") => {
                let frame: CurrentFrame = decode_frame(value)?;
                validate_scope(&frame.scope)?;
                validate_generation(&frame.generation)?;
                Ok(Response::Current(Context {
                    scope: frame.scope,
                    generation: frame.generation,
                }))
            }
            (Self::Runs { context, cursor }, "history_runs") => {
                let page_bytes = value
                    .get("page")
                    .and_then(|page| serde_json::to_vec(page).ok())
                    .ok_or_else(|| protocol("invalid history page"))?;
                if page_bytes.len() > MAX_PAGE {
                    return Err(protocol("history page exceeds its byte limit"));
                }
                let frame: RunsFrame = decode_frame(value)?;
                validate_context(context)?;
                if frame.generation != context.generation {
                    return Err(protocol("history generation changed"));
                }
                validate_page(&frame.page, context, cursor.as_ref())?;
                Ok(Response::Runs(frame.page))
            }
            (Self::Conversations { context, cursor }, "history_conversations") => {
                let page_bytes = value
                    .get("page")
                    .and_then(|page| serde_json::to_vec(page).ok())
                    .ok_or_else(|| protocol("invalid history page"))?;
                if page_bytes.len() > MAX_PAGE {
                    return Err(protocol("history page exceeds its byte limit"));
                }
                let frame: ConversationsFrame = decode_frame(value)?;
                validate_context(context)?;
                if frame.generation != context.generation {
                    return Err(protocol("history generation changed"));
                }
                validate_conversation_page(&frame.page, context, cursor.as_ref())?;
                Ok(Response::Conversations(frame.page))
            }
            (Self::Change { scope, change }, "conversation_changed") => {
                let frame: ChangedFrame = decode_frame(value)?;
                validate_scope(scope)?;
                validate_change(change, scope)?;
                let changed = Changed {
                    original: frame.original,
                    current: Context {
                        scope: frame.current,
                        generation: frame.generation,
                    },
                    sequence: frame.sequence,
                };
                validate_changed(&changed, scope, change)?;
                Ok(Response::Changed(changed))
            }
            (
                Self::Pin {
                    context,
                    kind,
                    document,
                    revision,
                },
                "history_pinned",
            ) => {
                let frame: PinnedFrame = decode_frame(value)?;
                validate_context(context)?;
                if !identity_matches(&frame.identity, context, *kind, document, *revision) {
                    return Err(protocol("history pin identity differs"));
                }
                Ok(Response::Pinned(frame.identity))
            }
            (Self::Pin { .. }, "history_busy") => {
                decode_frame::<BusyFrame>(value)?;
                Ok(Response::Busy)
            }
            (Self::Change { .. }, "conversation_busy") => {
                decode_frame::<BusyFrame>(value)?;
                Ok(Response::Busy)
            }
            (
                Self::Pin {
                    kind: DocumentKind::Source,
                    context,
                    document,
                    revision,
                },
                "history_unavailable",
            ) => {
                let frame: UnavailableFrame = decode_frame(value)?;
                validate_context(context)?;
                if frame.scope != context.scope
                    || frame.generation != context.generation
                    || frame.document != *document
                    || frame.revision != *revision
                {
                    return Err(protocol("unavailable source identity differs"));
                }
                Ok(Response::Unavailable)
            }
            (Self::Page { identity, offset }, "detail") => {
                validate_identity(identity)?;
                if header.request.is_empty() {
                    return Err(protocol("invalid detail request"));
                }
                let response_identity: Identity = serde_json::from_value(
                    value
                        .get("identity")
                        .cloned()
                        .ok_or_else(|| protocol("missing detail identity"))?,
                )
                .map_err(|_| protocol("invalid detail identity"))?;
                let response_offset = value.get("offset").and_then(Value::as_u64);
                if response_identity != *identity || response_offset != Some(u64::from(*offset)) {
                    return Err(protocol("detail page identity differs"));
                }
                let bytes =
                    serde_json::to_vec(&value).map_err(|_| protocol("invalid detail page"))?;
                Ok(Response::Detail {
                    request: header.request,
                    bytes,
                })
            }
            (Self::Release { identity }, "history_released") => {
                let frame: ReleasedFrame = decode_frame(value)?;
                validate_identity(identity)?;
                if frame.identity != *identity {
                    return Err(protocol("released history identity differs"));
                }
                Ok(Response::Released(frame.released))
            }
            (_, "history_stale") => {
                let frame: StaleFrame = decode_frame(value)?;
                if matches!(self, Self::Current) {
                    return Err(protocol("unexpected stale current response"));
                }
                if let Some(observation) = frame.observation.as_ref() {
                    validate_observation(observation)?;
                    if matches!(self, Self::Conversations { .. } | Self::Change { .. })
                        || !stale_matches(self, observation)
                    {
                        return Err(protocol("stale history observation differs"));
                    }
                }
                Ok(Response::Stale(frame.observation))
            }
            _ => Err(protocol("unexpected history response")),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentFrame {
    v: u64,
    request: String,
    kind: String,
    scope: Scope,
    generation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsFrame {
    v: u64,
    request: String,
    kind: String,
    generation: String,
    page: RunPage,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationsFrame {
    v: u64,
    request: String,
    kind: String,
    generation: String,
    page: ConversationPage,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangedFrame {
    v: u64,
    request: String,
    kind: String,
    original: Scope,
    current: Scope,
    generation: String,
    sequence: Counter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedFrame {
    v: u64,
    request: String,
    kind: String,
    identity: Identity,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusyFrame {
    v: u64,
    request: String,
    kind: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnavailableFrame {
    v: u64,
    request: String,
    kind: String,
    scope: Scope,
    generation: String,
    document: String,
    revision: Counter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleasedFrame {
    v: u64,
    request: String,
    kind: String,
    identity: Identity,
    released: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaleFrame {
    v: u64,
    request: String,
    kind: String,
    #[serde(deserialize_with = "Option::deserialize")]
    observation: Option<Observation>,
}

fn decode_frame<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, FlowError> {
    let frame: T =
        serde_json::from_value(value).map_err(|_| protocol("invalid history response"))?;
    Ok(frame)
}

fn validate_page(
    page: &RunPage,
    context: &Context,
    cursor: Option<&Cursor>,
) -> Result<(), FlowError> {
    if page.runs.len() > MAX_RUNS
        || page.store != context.scope.store
        || page.conversation != context.scope.conversation
        || page.epoch != context.scope.epoch
        || cursor.is_some_and(|cursor| cursor.anchor != page.anchor)
    {
        return Err(protocol("invalid history page"));
    }
    let mut last = None;
    let mut ids = std::collections::HashSet::new();
    for run in &page.runs {
        let number = validate_document_id(run.id.as_str(), &page.store)?;
        validate_document_id(run.source.as_str(), &page.store)?;
        if run.revision.0 == 0
            || !ids.insert(run.id.as_str())
            || last.is_some_and(|previous| number >= previous)
            || number > page.anchor.0
            || cursor.is_some_and(|cursor| number >= cursor.before.0)
            || run
                .question
                .as_ref()
                .is_some_and(|preview| preview.text.len() > 256)
            || run.output.text.len() > 512
        {
            return Err(protocol("invalid history summary"));
        }
        last = Some(number);
    }
    if cursor.is_some_and(|cursor| {
        cursor.anchor.0 == 0 || cursor.before.0 == 0 || cursor.before.0 > cursor.anchor.0
    }) || page.next.as_ref().is_some_and(|next| {
        last != Some(next.before.0)
            || next.anchor != page.anchor
            || next.before.0 == 0
            || next.before.0 > next.anchor.0
    }) || (page.next.is_some() && page.runs.is_empty())
    {
        return Err(protocol("invalid history cursor"));
    }
    Ok(())
}

fn validate_conversation_page(
    page: &ConversationPage,
    context: &Context,
    cursor: Option<&Cursor>,
) -> Result<(), FlowError> {
    if page.conversations.len() > MAX_CONVERSATIONS
        || page.store != context.scope.store
        || page.conversation != context.scope.conversation
        || page.epoch != context.scope.epoch
        || cursor.is_some_and(|cursor| cursor.anchor != page.anchor)
    {
        return Err(protocol("invalid conversation page"));
    }
    let mut last = None;
    let mut ids = std::collections::HashSet::new();
    for conversation in &page.conversations {
        let number = validate_conversation_id(&conversation.id, &page.store)?;
        if !ids.insert(conversation.id.as_str())
            || last.is_some_and(|previous| number >= previous)
            || number > page.anchor.0
            || cursor.is_some_and(|cursor| number >= cursor.before.0)
            || conversation.title.text.len() > 256
        {
            return Err(protocol("invalid conversation summary"));
        }
        last = Some(number);
    }
    if cursor.is_some_and(|cursor| {
        cursor.anchor.0 == 0 || cursor.before.0 == 0 || cursor.before.0 > cursor.anchor.0
    }) || page.next.as_ref().is_some_and(|next| {
        last != Some(next.before.0)
            || next.anchor != page.anchor
            || next.before.0 == 0
            || next.before.0 > next.anchor.0
    }) || (page.next.is_some() && page.conversations.is_empty())
    {
        return Err(protocol("invalid conversation cursor"));
    }
    Ok(())
}

fn validate_change(change: &Change, scope: &Scope) -> Result<(), FlowError> {
    if let Change::Select { conversation } = change {
        validate_conversation_id(conversation, &scope.store)?;
    }
    Ok(())
}

fn validate_changed(changed: &Changed, expected: &Scope, change: &Change) -> Result<(), FlowError> {
    validate_scope(&changed.original)?;
    validate_context(&changed.current)?;
    if changed.original.store != expected.store
        || expected.epoch.0.checked_add(1) != Some(changed.original.epoch.0)
        || changed.current.scope.store != changed.original.store
        || changed.current.scope.epoch.0 < changed.original.epoch.0
        || (changed.current.scope.epoch == changed.original.epoch
            && changed.current.scope != changed.original)
        || match change {
            Change::Create => changed.original.conversation == expected.conversation,
            Change::Select { conversation } => changed.original.conversation != *conversation,
        }
    {
        return Err(protocol("invalid conversation change receipt"));
    }
    Ok(())
}

fn validate_context(context: &Context) -> Result<(), FlowError> {
    validate_scope(&context.scope)?;
    validate_generation(&context.generation)
}

fn validate_scope(scope: &Scope) -> Result<(), FlowError> {
    scope
        .validate()
        .map_err(|_| protocol("invalid history scope"))
}

fn validate_conversation_id(value: &str, store: &str) -> Result<u64, FlowError> {
    let number = validate_document_id(value, store)?;
    let local = value
        .strip_prefix(&format!("{store}:"))
        .ok_or_else(|| protocol("invalid history conversation identity"))?;
    if local.starts_with('0') || number.to_string() != local {
        return Err(protocol("invalid history conversation identity"));
    }
    Ok(number)
}

fn validate_generation(value: &str) -> Result<(), FlowError> {
    if !valid_token(value, 64) {
        Err(protocol("invalid history generation"))
    } else {
        Ok(())
    }
}

fn validate_identity(identity: &Identity) -> Result<(), FlowError> {
    let scope = Scope {
        store: identity.store.clone(),
        conversation: identity.conversation.clone(),
        epoch: identity.epoch,
    };
    validate_scope(&scope)?;
    validate_generation(&identity.generation)?;
    validate_document_id(&identity.document, &identity.store)?;
    if identity.revision.0 == 0 || !valid_token(&identity.ticket, 96) {
        return Err(protocol("invalid history pin identity"));
    }
    Ok(())
}

fn identity_matches(
    identity: &Identity,
    context: &Context,
    kind: DocumentKind,
    document: &str,
    revision: Counter,
) -> bool {
    validate_identity(identity).is_ok()
        && identity.store == context.scope.store
        && identity.conversation == context.scope.conversation
        && identity.epoch == context.scope.epoch
        && identity.generation == context.generation
        && identity.kind == kind
        && identity.document == document
        && identity.revision == revision
}

fn validate_observation(observation: &Observation) -> Result<(), FlowError> {
    validate_scope(&observation.scope)?;
    validate_generation(&observation.generation)?;
    validate_document_id(&observation.document, &observation.scope.store)?;
    if observation.revision.0 == 0
        || (observation.kind == DocumentKind::Run) != observation.status.is_some()
    {
        return Err(protocol("invalid history observation"));
    }
    Ok(())
}

fn stale_matches(expected: &Expected, observation: &Observation) -> bool {
    match expected {
        Expected::Runs { context, .. } => {
            observation.scope == context.scope && observation.generation == context.generation
        }
        Expected::Pin {
            context,
            kind,
            document,
            ..
        } => {
            observation.scope == context.scope
                && observation.generation == context.generation
                && observation.kind == *kind
                && observation.document == *document
        }
        Expected::Page { identity, .. } | Expected::Release { identity } => {
            observation.scope.store == identity.store
                && observation.scope.conversation == identity.conversation
                && observation.scope.epoch == identity.epoch
                && observation.generation == identity.generation
                && observation.kind == identity.kind
                && observation.document == identity.document
        }
        Expected::Conversations { .. } | Expected::Change { .. } => false,
        Expected::Current => false,
    }
}

fn validate_document_id(value: &str, store: &str) -> Result<u64, FlowError> {
    if value.len() > 49_152 {
        return Err(protocol("invalid history document identity"));
    }
    let local = value
        .strip_prefix(&format!("{store}:"))
        .ok_or_else(|| protocol("invalid history document identity"))?;
    let number = local
        .parse::<u64>()
        .map_err(|_| protocol("invalid history document identity"))?;
    if local.is_empty() || number == 0 {
        return Err(protocol("invalid history document identity"));
    }
    Ok(number)
}

fn valid_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.:".contains(&byte))
}

fn protocol(message: &str) -> FlowError {
    FlowError::Protocol(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_conversation_change_receipts_and_rejects_mismatches() {
        let store = "0123456789abcdef0123456789abcdef";
        let expected = Scope {
            store: store.into(),
            conversation: format!("{store}:1"),
            epoch: Counter(1),
        };
        let frame = json!({
            "v": VERSION,
            "request": "r1",
            "kind": "conversation_changed",
            "original": {"store": store, "conversation": format!("{store}:2"), "epoch": "2"},
            "current": {"store": store, "conversation": format!("{store}:1"), "epoch": "3"},
            "generation": "g1",
            "sequence": "7"
        });
        let create = Expected::Change {
            scope: expected.clone(),
            change: Change::Create,
        };
        assert!(matches!(
            create.decode(frame.clone()),
            Ok(Response::Changed(Changed {
                original: Scope {
                    epoch: Counter(2),
                    ..
                },
                current: Context {
                    scope: Scope {
                        epoch: Counter(3),
                        ..
                    },
                    ..
                },
                sequence: Counter(7),
            }))
        ));

        let mut wrong_epoch = frame.clone();
        wrong_epoch["original"]["epoch"] = json!("3");
        assert!(create.decode(wrong_epoch).is_err());

        let select = Expected::Change {
            scope: expected.clone(),
            change: Change::Select {
                conversation: format!("{store}:2"),
            },
        };
        assert!(matches!(
            select.decode(frame.clone()),
            Ok(Response::Changed(_))
        ));

        let select_wrong_target = Expected::Change {
            scope: expected,
            change: Change::Select {
                conversation: format!("{store}:3"),
            },
        };
        assert!(select_wrong_target.decode(frame.clone()).is_err());

        let mut extra_field = frame;
        extra_field["unexpected"] = json!(true);
        assert!(create.decode(extra_field).is_err());
    }
}
