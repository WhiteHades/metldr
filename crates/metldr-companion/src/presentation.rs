use crate::{
    Result,
    direct_output::DirectOutput,
    flow::{Event, Owner},
};
use metldr_conversation::ArtifactId;
use metldr_responses::{Client, PresentationContent, PresentationInvocation, PresentationProgress};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

const MAX_PRESENTATION_STEPS: usize = 4;

pub(super) enum Update {
    Preview(PresentationProgress),
    Complete(PresentationInvocation),
}

pub(super) async fn run(
    client: Client,
    system: &str,
    input: &str,
    run: &str,
    tx: &SyncSender<Event>,
    stop: &AtomicBool,
    output: &mut DirectOutput<'_>,
) -> Result<()> {
    let mut conversation = client.start_presentation(system, input)?;
    let mut calls = 0;
    loop {
        let Some(invocation) = conversation
            .next(
                stop,
                |event| output.accept(event),
                |progress| {
                    if calls >= MAX_PRESENTATION_STEPS {
                        return Err("presentation step limit reached".into());
                    }
                    save(run, tx, stop, Update::Preview(progress)).map(|_| ())
                },
            )
            .await?
        else {
            return Ok(());
        };
        if calls >= MAX_PRESENTATION_STEPS {
            return Err("presentation step limit reached".into());
        }
        calls += 1;
        let surface = invocation.presentation.surface_id.clone();
        let id = save(run, tx, stop, Update::Complete(invocation))?;
        conversation.submit_tool_result(
            &json!({"status":"saved","surfaceId":surface,"artifactId":id.as_str()}),
            |event| {
                if stop.load(Ordering::Acquire) {
                    return Err("presentation cancelled".into());
                }
                output.accept(event)
            },
        )?;
    }
}

fn save(
    run: &str,
    tx: &SyncSender<Event>,
    stop: &AtomicBool,
    update: Update,
) -> Result<ArtifactId> {
    if stop.load(Ordering::Acquire) {
        return Err("presentation cancelled".into());
    }
    let (ack, receipt) = mpsc::sync_channel(1);
    tx.try_send(Event::Presentation {
        run: run.into(),
        update,
        ack,
    })
    .map_err(|_| "presentation owner channel unavailable")?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("presentation cancelled".into());
        }
        match receipt.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(id)) => return Ok(id),
            Ok(Err(error)) => return Err(error.into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("presentation owner receipt unavailable".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => (),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("presentation owner receipt timed out".into());
            }
        }
    }
}

impl Owner {
    pub(super) fn presentation(
        &mut self,
        run: &str,
        update: Update,
        ack: SyncSender<std::result::Result<ArtifactId, String>>,
    ) -> Result<()> {
        let result = (|| {
            if self.record_history.is_none()
                || self
                    .operation
                    .as_ref()
                    .is_none_or(|active| active.run != run || active.stop.load(Ordering::Acquire))
            {
                return Err("presentation has no active history owner".into());
            }
            self.record_change(|edit, scope| {
                let id = match update {
                    Update::Preview(progress) => edit.preview_generated(
                        scope,
                        run,
                        &progress.call_id,
                        &progress.surface_id,
                        progress.content,
                    )?,
                    Update::Complete(invocation) => match invocation.presentation.content {
                        PresentationContent::Table(table) => edit.accept_table(
                            scope,
                            run,
                            &invocation.id,
                            &invocation.presentation.surface_id,
                            table,
                        )?,
                        PresentationContent::Chart(chart) => edit.accept_chart(
                            scope,
                            run,
                            &invocation.id,
                            &invocation.presentation.surface_id,
                            chart,
                        )?,
                        PresentationContent::Generated(source) => edit.complete_generated(
                            scope,
                            run,
                            &invocation.id,
                            &invocation.presentation.surface_id,
                            source,
                        )?,
                    },
                };
                Ok((id, Some(edit.project(scope, Some(run))?)))
            })
        })();
        match result {
            Ok(id) => {
                let _ = ack.try_send(Ok(id));
                Ok(())
            }
            Err(error) => {
                let _ = ack.try_send(Err(error.to_string()));
                if crate::history::is_failure(error.as_ref()) {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }
}
