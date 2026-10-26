use super::{FlowClient, Reply, ReplyKind, Result};
use crate::{
    history::{
        Change, Changed, Context, ConversationPage, Cursor, Expected, Response, RunPage,
        RunReference, Scope, validate_reference,
    },
    model::{FlowError, RunId, RunView},
};
use metldr_conversation::{ArtifactId, Table};
use metldr_history_pages::{Assembler, Counter, DocumentKind, Identity};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone)]
pub(crate) struct ConversationRequest {
    id: String,
    scope: Scope,
    change: Change,
}

impl FlowClient {
    pub(crate) fn conversation_request(
        &self,
        scope: Scope,
        change: Change,
    ) -> Result<ConversationRequest> {
        Ok(ConversationRequest {
            id: self.inner.request_id()?,
            scope,
            change,
        })
    }

    pub(crate) async fn change_conversation(
        &self,
        request: &ConversationRequest,
    ) -> Result<Changed> {
        match self
            .request_with_id(
                request.id.clone(),
                json!({"op":"conversation_change","scope":request.scope,"change":request.change}),
                ReplyKind::History(Expected::Change {
                    scope: request.scope.clone(),
                    change: request.change.clone(),
                }),
            )
            .await?
        {
            Reply::History(Response::Changed(receipt)) => Ok(receipt),
            Reply::History(Response::Busy) => Err(FlowError::HistoryBusy),
            _ => Err(FlowError::Protocol(
                "Expected a conversation receipt.".into(),
            )),
        }
    }

    pub(crate) async fn history_conversations(
        &self,
        context: &Context,
        cursor: Option<Cursor>,
    ) -> Result<ConversationPage> {
        match self.history_request(json!({"op":"history_conversations","scope":context.scope,"generation":context.generation,"cursor":cursor}), Expected::Conversations {context:context.clone(),cursor}).await? {
            Response::Conversations(page) => Ok(page),
            _ => Err(FlowError::Protocol("Expected saved conversations.".into())),
        }
    }

    async fn history_request(
        &self,
        command: serde_json::Value,
        expected: Expected,
    ) -> Result<Response> {
        match self.request(command, ReplyKind::History(expected)).await? {
            Reply::History(Response::Busy) => Err(FlowError::HistoryBusy),
            Reply::History(Response::Stale(_)) => Err(FlowError::HistoryStale),
            Reply::History(response) => Ok(response),
            _ => Err(FlowError::Protocol("Expected a history reply.".into())),
        }
    }

    pub(crate) async fn history_current(&self) -> Result<Context> {
        match self
            .history_request(json!({"op":"history_current"}), Expected::Current)
            .await?
        {
            Response::Current(context) => Ok(context),
            _ => Err(FlowError::Protocol(
                "Expected the current history selection.".into(),
            )),
        }
    }

    pub(crate) async fn history_runs(
        &self,
        context: &Context,
        cursor: Option<Cursor>,
    ) -> Result<RunPage> {
        match self.history_request(json!({"op":"history_runs","scope":context.scope,"generation":context.generation,"cursor":cursor}), Expected::Runs {context:context.clone(),cursor}).await? {
            Response::Runs(page) => Ok(page),
            _ => Err(FlowError::Protocol("Expected saved work.".into())),
        }
    }

    pub(crate) async fn history_run(
        &self,
        context: &Context,
        summary: &RunReference,
    ) -> Result<RunView> {
        validate_reference(context, summary)?;
        self.history_document(
            context,
            DocumentKind::Run,
            summary.id.as_str(),
            summary.revision,
            |bytes| {
                let run = RunView::decode_record(bytes, &summary.id)?;
                if run.source != summary.source
                    || run.kind != summary.kind
                    || run.status != summary.status
                {
                    return Err(FlowError::Protocol(
                        "Saved result does not match its observation.".into(),
                    ));
                }
                Ok(run)
            },
        )
        .await
    }

    pub(crate) async fn history_artifact(
        &self,
        context: &Context,
        run: &RunId,
        id: &ArtifactId,
    ) -> Result<Table> {
        context
            .scope
            .validate()
            .map_err(|_| FlowError::Protocol("Invalid history scope.".into()))?;
        validate_artifact_id(id.as_str(), &context.scope.store)?;
        validate_artifact_id(run.as_str(), &context.scope.store)?;
        self.history_document(
            context,
            DocumentKind::Artifact,
            id.as_str(),
            Counter(1),
            |bytes| decode_artifact(bytes, run, id),
        )
        .await
    }

    async fn history_document<T>(
        &self,
        context: &Context,
        kind: DocumentKind,
        document: &str,
        revision: Counter,
        decode: impl FnOnce(&[u8]) -> Result<T>,
    ) -> Result<T> {
        let pin = self.history_request(json!({"op":"history_pin","scope":context.scope,"generation":context.generation,"kind":kind,"document":document,"revision":revision}), Expected::Pin {context:context.clone(),kind,document:document.to_owned(),revision}).await?;
        let Response::Pinned(identity) = pin else {
            return Err(FlowError::Protocol(
                "Expected a saved result ticket.".into(),
            ));
        };
        let mut guard = Pinned {
            flow: self.clone(),
            identity: Some(identity.clone()),
        };
        let result = async {
            let mut reader = Assembler::new(identity.clone()).map_err(codec_error)?;
            loop {
                let offset = reader.next_offset();
                match self
                    .history_request(
                        json!({"op":"history_page","identity":identity,"offset":offset}),
                        Expected::Page {
                            identity: identity.clone(),
                            offset,
                        },
                    )
                    .await?
                {
                    Response::Detail { request, bytes } => {
                        if reader.push(&bytes, &request).map_err(codec_error)? {
                            break;
                        }
                    }
                    _ => return Err(FlowError::Protocol("Expected a saved result page.".into())),
                }
            }
            decode(&reader.finish().map_err(codec_error)?)
        }
        .await;
        if let Some(identity) = guard.identity.clone() {
            let release = self
                .history_request(
                    json!({"op":"history_release","identity":identity}),
                    Expected::Release { identity },
                )
                .await;
            if release.is_ok() {
                guard.identity.take();
            }
            if result.is_ok() {
                release?;
            }
        }
        result
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRecord {
    id: ArtifactId,
    run: String,
    call: String,
    surface_id: String,
    table: Table,
}

fn decode_artifact(bytes: &[u8], run: &RunId, id: &ArtifactId) -> Result<Table> {
    let artifact: ArtifactRecord =
        serde_json::from_slice(bytes).map_err(|error| FlowError::Protocol(error.to_string()))?;
    if artifact.id != *id
        || artifact.run != run.as_str()
        || artifact.call.is_empty()
        || artifact.call.len() > 256
        || artifact.call.chars().any(char::is_control)
        || artifact.surface_id.is_empty()
        || artifact.surface_id.encode_utf16().count() > 256
        || artifact.surface_id.chars().any(char::is_control)
    {
        return Err(FlowError::Protocol("Invalid saved artifact.".into()));
    }
    Ok(artifact.table)
}

fn validate_artifact_id(value: &str, store: &str) -> Result<()> {
    let local = value
        .strip_prefix(&format!("{store}:"))
        .ok_or_else(|| FlowError::Protocol("Invalid saved artifact identity.".into()))?;
    let number = local
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| FlowError::Protocol("Invalid saved artifact identity.".into()))?;
    if local != number.to_string() {
        return Err(FlowError::Protocol(
            "Invalid saved artifact identity.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn artifact_decode_checks_owner_and_strict_record_shape() {
        let id = ArtifactId::new("0123456789abcdef0123456789abcdef:2".into()).unwrap();
        let run: RunId =
            serde_json::from_value(json!("0123456789abcdef0123456789abcdef:1")).unwrap();
        let value = json!({
            "id": id.as_str(),
            "run": run.as_str(),
            "call": "call-1",
            "surface_id": "surface-1",
            "table": {"title":"t","columns":["c"],"rows":[],"source":"s"}
        });
        assert!(decode_artifact(&serde_json::to_vec(&value).unwrap(), &run, &id).is_ok());

        let mut wrong_owner = value.clone();
        wrong_owner["run"] = json!("0123456789abcdef0123456789abcdef:3");
        assert!(decode_artifact(&serde_json::to_vec(&wrong_owner).unwrap(), &run, &id).is_err());

        let mut extra = value;
        extra["extra"] = json!(true);
        assert!(decode_artifact(&serde_json::to_vec(&extra).unwrap(), &run, &id).is_err());
    }
}

struct Pinned {
    flow: FlowClient,
    identity: Option<Identity>,
}

impl Drop for Pinned {
    fn drop(&mut self) {
        if let Some(identity) = &self.identity {
            if let Ok(request) = self.flow.inner.request_id() {
                let _ = self.flow.inner.post(
                    &request,
                    json!({"op":"history_release","identity":identity}),
                );
            }
        }
    }
}

fn codec_error(error: metldr_history_pages::Error) -> FlowError {
    FlowError::Protocol(error.to_string())
}
