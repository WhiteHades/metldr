use crate::{
    Result,
    direct_output::DirectOutput,
    flow::{Event, Owner},
};
use metldr_conversation::ArtifactId;
use metldr_responses::{Client, PresentationInvocation};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

const MAX_PRESENTATION_STEPS: usize = 4;

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
            .next(stop, |event| output.accept(event))
            .await?
        else {
            return Ok(());
        };
        if calls >= MAX_PRESENTATION_STEPS {
            return Err("table response step limit reached".into());
        }
        calls += 1;
        let surface = invocation.presentation.surface_id.clone();
        let (ack, receipt) = mpsc::sync_channel(1);
        tx.try_send(Event::Table {
            run: run.into(),
            invocation,
            ack,
        })
        .map_err(|_| "table owner channel unavailable")?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let id = loop {
            if stop.load(Ordering::Acquire) {
                return Err("table response cancelled".into());
            }
            match receipt.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(id)) => break id,
                Ok(Err(error)) => return Err(error.into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("table owner receipt unavailable".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => (),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err("table owner receipt timed out".into());
                }
            }
        };
        conversation.submit_tool_result(
            &json!({"status":"saved","surfaceId":surface,"artifactId":id.as_str()}),
            |event| {
                if stop.load(Ordering::Acquire) {
                    return Err("table response cancelled".into());
                }
                output.accept(event)
            },
        )?;
    }
}

impl Owner {
    pub(super) fn table(
        &mut self,
        run: &str,
        invocation: PresentationInvocation,
        ack: SyncSender<std::result::Result<ArtifactId, String>>,
    ) -> Result<()> {
        let result = (|| {
            if self.record_history.is_none()
                || self
                    .operation
                    .as_ref()
                    .is_none_or(|active| active.run != run)
            {
                return Err("table response has no active history owner".into());
            }
            self.record_change(|edit, scope| {
                let id = edit.accept_table(
                    scope,
                    run,
                    &invocation.id,
                    &invocation.presentation.surface_id,
                    invocation.presentation.table,
                )?;
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
