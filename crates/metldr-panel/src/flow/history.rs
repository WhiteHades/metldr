use super::{FlowClient, Reply, ReplyKind, Result};
use crate::{
    history::{
        Change, Changed, Context, ConversationPage, Cursor, Expected, GeneratedRead, Response,
        RunPage, RunReference, Scope, validate_reference,
    },
    model::{FlowError, RunId, RunView},
};
use metldr_conversation::{ArtifactCompletion, ArtifactId, Chart, GeneratedAnswer, Table};
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
            |bytes| match decode_artifact(bytes, run, id)?.content {
                ArtifactContent::Table { table } => Ok(table),
                _ => Err(FlowError::Protocol("Saved result is not a table.".into())),
            },
        )
        .await
    }

    pub(crate) async fn history_chart(
        &self,
        context: &Context,
        run: &RunId,
        id: &ArtifactId,
    ) -> Result<Chart> {
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
            |bytes| match decode_artifact(bytes, run, id)?.content {
                ArtifactContent::Chart { chart } => Ok(chart),
                _ => Err(FlowError::Protocol("Saved result is not a chart.".into())),
            },
        )
        .await
    }

    pub(crate) async fn history_generated(
        &self,
        context: &Context,
        run: &RunId,
        id: &ArtifactId,
        revision: Counter,
    ) -> Result<GeneratedRead> {
        context
            .scope
            .validate()
            .map_err(|_| FlowError::Protocol("Invalid history scope.".into()))?;
        validate_artifact_id(id.as_str(), &context.scope.store)?;
        validate_artifact_id(run.as_str(), &context.scope.store)?;
        if revision.0 == 0 {
            return Err(FlowError::Protocol("Invalid generated revision.".into()));
        }
        let pin = self
            .history_pin(context, DocumentKind::Artifact, id.as_str(), revision)
            .await?;
        let identity = match pin {
            Response::Pinned(identity) => identity,
            response @ Response::Stale(_) => {
                return Ok(GeneratedRead::Superseded(generated_superseded(
                    response, revision,
                )?));
            }
            _ => {
                return Err(FlowError::Protocol(
                    "Expected a saved result ticket.".into(),
                ));
            }
        };
        self.read_pinned(identity, |bytes| {
            match decode_artifact(bytes, run, id)?.content {
                ArtifactContent::Generated { source, completion } => {
                    if completion == ArtifactCompletion::Completed {
                        source.clone().finish().map_err(FlowError::Protocol)?;
                    }
                    Ok(GeneratedRead::Ready {
                        answer: source,
                        completion,
                    })
                }
                _ => Err(FlowError::Protocol(
                    "Saved result is not generated content.".into(),
                )),
            }
        })
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
        let pin = self.history_pin(context, kind, document, revision).await?;
        let identity = match pin {
            Response::Pinned(identity) => identity,
            Response::Stale(_) => return Err(FlowError::HistoryStale),
            _ => {
                return Err(FlowError::Protocol(
                    "Expected a saved result ticket.".into(),
                ));
            }
        };
        self.read_pinned(identity, decode).await
    }

    async fn history_pin(
        &self,
        context: &Context,
        kind: DocumentKind,
        document: &str,
        revision: Counter,
    ) -> Result<Response> {
        match self
            .request(
                json!({"op":"history_pin","scope":context.scope,"generation":context.generation,"kind":kind,"document":document,"revision":revision}),
                ReplyKind::History(Expected::Pin {
                    context: context.clone(),
                    kind,
                    document: document.to_owned(),
                    revision,
                }),
            )
            .await?
        {
            Reply::History(Response::Busy) => Err(FlowError::HistoryBusy),
            Reply::History(response @ (Response::Pinned(_) | Response::Stale(_))) => Ok(response),
            Reply::History(_) => Err(FlowError::Protocol(
                "Expected a saved result ticket.".into(),
            )),
            _ => Err(FlowError::Protocol("Expected a history reply.".into())),
        }
    }

    async fn read_pinned<T>(
        &self,
        identity: Identity,
        decode: impl FnOnce(&[u8]) -> Result<T>,
    ) -> Result<T> {
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
    content: ArtifactContent,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ArtifactContent {
    Table {
        table: Table,
    },
    Generated {
        source: GeneratedAnswer,
        completion: ArtifactCompletion,
    },
    Chart {
        chart: Chart,
    },
}

fn decode_artifact(bytes: &[u8], run: &RunId, id: &ArtifactId) -> Result<ArtifactRecord> {
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
    Ok(artifact)
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

fn generated_superseded(response: Response, requested: Counter) -> Result<Counter> {
    match response {
        Response::Stale(Some(observation)) if observation.revision.0 > requested.0 => {
            Ok(observation.revision)
        }
        _ => Err(FlowError::HistoryStale),
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use serde_json::Value;

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
            "content": {"kind":"table","table":{"title":"t","columns":["c"],"rows":[],"source":"s"}}
        });
        assert!(decode_artifact(&serde_json::to_vec(&value).unwrap(), &run, &id).is_ok());

        let mut wrong_owner = value.clone();
        wrong_owner["run"] = json!("0123456789abcdef0123456789abcdef:3");
        assert!(decode_artifact(&serde_json::to_vec(&wrong_owner).unwrap(), &run, &id).is_err());

        let mut extra = value;
        extra["extra"] = json!(true);
        assert!(decode_artifact(&serde_json::to_vec(&extra).unwrap(), &run, &id).is_err());
    }

    #[test]
    fn only_validated_newer_generated_pin_observations_are_superseded() {
        let context = crate::history::Context {
            scope: Scope {
                store: "0123456789abcdef0123456789abcdef".into(),
                conversation: "0123456789abcdef0123456789abcdef:1".into(),
                epoch: Counter(1),
            },
            generation: "generation-1".into(),
        };
        let expected = Expected::Pin {
            context: context.clone(),
            kind: DocumentKind::Artifact,
            document: "0123456789abcdef0123456789abcdef:2".into(),
            revision: Counter(2),
        };
        let stale_frame =
            |revision: Option<&str>, document: &str, scope: Value, generation: &str| {
                json!({
                    "v": metldr_history_pages::WIRE_VERSION,
                    "request": "pin",
                    "kind": "history_stale",
                    "observation": revision.map(|revision| json!({
                        "scope": scope,
                        "generation": generation,
                        "sequence": "4",
                        "kind": "artifact",
                        "document": document,
                        "revision": revision,
                        "status": null
                    }))
                })
            };
        let stale = |revision: &str, document: &str| {
            expected.decode(stale_frame(
                Some(revision),
                document,
                serde_json::to_value(&context.scope).unwrap(),
                &context.generation,
            ))
        };

        let newer = stale("3", "0123456789abcdef0123456789abcdef:2").unwrap();
        assert_eq!(generated_superseded(newer, Counter(2)).unwrap(), Counter(3));
        let equal = stale("2", "0123456789abcdef0123456789abcdef:2").unwrap();
        assert!(generated_superseded(equal, Counter(2)).is_err());
        let older = stale("1", "0123456789abcdef0123456789abcdef:2").unwrap();
        assert!(generated_superseded(older, Counter(2)).is_err());
        assert!(stale("3", "0123456789abcdef0123456789abcdef:3").is_err());
        let missing = expected
            .decode(stale_frame(
                None,
                "0123456789abcdef0123456789abcdef:2",
                serde_json::to_value(&context.scope).unwrap(),
                &context.generation,
            ))
            .unwrap();
        assert!(generated_superseded(missing, Counter(2)).is_err());

        let foreign_scope = json!({
            "store": context.scope.store,
            "conversation": "0123456789abcdef0123456789abcdef:3",
            "epoch": "1"
        });
        assert!(
            expected
                .decode(stale_frame(
                    Some("3"),
                    "0123456789abcdef0123456789abcdef:2",
                    foreign_scope,
                    &context.generation,
                ))
                .is_err()
        );
        assert!(
            expected
                .decode(stale_frame(
                    Some("3"),
                    "0123456789abcdef0123456789abcdef:2",
                    serde_json::to_value(&context.scope).unwrap(),
                    "generation-2",
                ))
                .is_err()
        );
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
