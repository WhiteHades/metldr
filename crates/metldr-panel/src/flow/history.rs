use super::{FlowClient, Reply, ReplyKind, Result};
use crate::{
    history::{
        Change, Changed, Context, ConversationPage, Cursor, Expected, Response, RunPage,
        RunReference, Scope, validate_reference,
    },
    model::{FlowError, RunView},
};
use metldr_history_pages::{Assembler, DocumentKind, Identity};
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
        let document = summary.id.as_str();
        let pin = self.history_request(json!({"op":"history_pin","scope":context.scope,"generation":context.generation,"kind":"run","document":document,"revision":summary.revision}), Expected::Pin {context:context.clone(),kind:DocumentKind::Run,document:document.to_owned(),revision:summary.revision}).await?;
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
            let run = RunView::decode_record(&reader.finish().map_err(codec_error)?, &summary.id)?;
            if run.source != summary.source
                || run.kind != summary.kind
                || run.status != summary.status
            {
                return Err(FlowError::Protocol(
                    "Saved result does not match its observation.".into(),
                ));
            }
            Ok(run)
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
