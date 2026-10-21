use super::*;
use metldr_codex_session::{Event as NativeEvent, Interrupt, TurnStatus};
use metldr_conversation::{Change, MessageKey};
use std::time::{Duration, Instant};

pub(super) async fn run(
    job: &Job,
    session: &mut CodexSession,
    tools: &mut Tools,
    stop: &AtomicBool,
    tx: &mpsc::SyncSender<Event>,
) -> Result<Outcome> {
    let info = session.info();
    let identity = Identity {
        thread: info.id.as_str().into(),
        session: info.session_id.clone(),
    };
    let selection = Selection {
        provider: info.provider.clone(),
        model: info.model.clone(),
    };
    let (ack, committed) = mpsc::sync_channel(1);
    tx.send(Event::CodexBound {
        run: job.run.clone(),
        workflow: job.workflow.clone(),
        identity,
        selection,
        ack,
    })?;
    if !committed.recv_timeout(Duration::from_secs(5))?? || stop.load(Ordering::Acquire) {
        return Ok(Outcome::Cancelled);
    }
    let prompt = agent::message(&job.text, &job.source)?;
    session
        .prompt(&format!("{prompt}\n\n{BROWSER_POLICY}"))
        .await?;
    let mut stop_wait = agent::StopWait::default();
    let mut interrupted = None;
    let mut review = super::approval::Review::default();
    let mut events = Events::default();
    loop {
        let stopping = stop.load(Ordering::Acquire) || review.cancelled;
        stop_wait.check(stopping, Instant::now())?;
        if stopping {
            review.poll(session, true).await?;
        }
        if interrupted.is_none() && stopping && !review.cancelled {
            interrupted = Some(session.interrupt().await?);
        }
        if events.queued == 256 {
            if review.has_answer()? {
                return Err("Codex event backlog prevented a safe approval reply".into());
            }
            events.queued = 1;
            tokio::task::yield_now().await;
        }
        let Some(event) = events.next(session.next_event()).await? else {
            // Drain ready revocations and patch updates before applying a human choice.
            review.poll(session, stop.load(Ordering::Acquire)).await?;
            continue;
        };
        match event {
            NativeEvent::Approval(request) => {
                if !review.cancelled {
                    review.request(request, session, job, tx, stopping).await?
                }
            }
            NativeEvent::ApprovalResolved(id) => review.resolved(&id, job, tx)?,
            NativeEvent::ApprovalsUpdated(updates) => {
                for (previous, request) in updates {
                    review.resolved(&previous, job, tx)?;
                    if !review.cancelled {
                        review.request(request, session, job, tx, stopping).await?;
                    }
                }
            }
            NativeEvent::TextDelta { item_id, text } => {
                text_change(
                    tx,
                    &job.run,
                    Change::Append {
                        key: MessageKey::native(item_id)?,
                        text,
                    },
                )?;
            }
            NativeEvent::Message { item_id, text } => {
                text_change(
                    tx,
                    &job.run,
                    Change::Final {
                        key: MessageKey::native(item_id)?,
                        text,
                    },
                )?;
            }
            NativeEvent::ToolProgress {
                item_id, output, ..
            } => {
                crate::tool_report::send_progress(
                    tx,
                    &job.run,
                    crate::tool_report::Call {
                        id: item_id,
                        title: None,
                        input: None,
                        output: Some(crate::tool_report::Preview {
                            text: output.text,
                            truncated: output.truncated,
                        }),
                    },
                )?;
            }
            NativeEvent::ToolReport(report) => {
                use metldr_codex_session::{ToolOutcome, ToolState};
                let (phase, outcome) = match report.state {
                    ToolState::Started => ("started", None),
                    ToolState::Finished(outcome) => (
                        if matches!(outcome, ToolOutcome::Failed | ToolOutcome::Declined) {
                            "failed"
                        } else {
                            "completed"
                        },
                        Some(
                            match outcome {
                                ToolOutcome::Succeeded => "succeeded",
                                ToolOutcome::Failed => "failed",
                                ToolOutcome::Declined => "declined",
                                ToolOutcome::Unspecified => "unspecified",
                            }
                            .into(),
                        ),
                    ),
                };
                let preview = |p: metldr_codex_session::Preview| crate::tool_report::Preview {
                    text: p.text,
                    truncated: p.truncated,
                };
                tx.send(Event::CodexActivity {
                    run: job.run.clone(),
                    activity: crate::agent_model::Activity {
                        tool: report.tool,
                        phase: phase.into(),
                        outcome,
                        call: Some(crate::tool_report::Call {
                            id: report.item_id,
                            title: Some(preview(report.title)),
                            input: Some(preview(report.input)),
                            output: report.output.map(preview),
                        }),
                    },
                })?;
            }
            NativeEvent::ToolCall(call) => {
                if review.cancelled {
                    continue;
                }
                let decoded = decode_call(&call.name, call.arguments.clone())?;
                let outcome = tools.dispatch(&call.call_id, decoded, stop, None).await;
                if stop.load(Ordering::Acquire) {
                    stop_wait.check(true, Instant::now())?;
                    if interrupted.is_none() {
                        interrupted = Some(session.interrupt().await?);
                    }
                    continue;
                }
                let outcome = outcome?;
                session
                    .reply_tool(call, &serde_json::to_string(&outcome)?, true)
                    .await?;
            }
            NativeEvent::Finished { status, .. } => {
                review.finish(job, tx, status == TurnStatus::Completed)?;
                return match status {
                    TurnStatus::Completed => Ok(Outcome::Complete),
                    TurnStatus::Interrupted
                        if interrupted == Some(Interrupt::Acknowledged) || review.cancelled =>
                    {
                        Ok(Outcome::Cancelled)
                    }
                    TurnStatus::Interrupted => {
                        Err("Codex interrupted without acknowledged Stop".into())
                    }
                    TurnStatus::Failed => Err("Codex reported a failed turn".into()),
                };
            }
        }
    }
}

#[derive(Default)]
struct Events {
    queued: usize,
}

impl Events {
    async fn next<T>(
        &mut self,
        event: impl std::future::Future<Output = Result<T>>,
    ) -> Result<Option<T>> {
        let wait = if self.queued == 0 {
            Duration::from_millis(50)
        } else {
            Duration::ZERO
        };
        match tokio::time::timeout(wait, event).await {
            Ok(event) => {
                self.queued += 1;
                event.map(Some)
            }
            Err(_) => {
                self.queued = 0;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[tokio::test]
    async fn ready_updates_drain_without_waiting_for_a_gap_in_streaming() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let (answer, decision) = mpsc::sync_channel(1);
        answer.send("allow").unwrap();
        tx.send("patch updated").await.unwrap();
        tx.send("request resolved").await.unwrap();
        let mut events = Events::default();
        for expected in ["patch updated", "request resolved"] {
            assert_eq!(
                events
                    .next(async { Ok(rx.recv().await.unwrap()) })
                    .await
                    .unwrap(),
                Some(expected)
            );
        }
        let output = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(5)).await;
                if tx.send("output").await.is_err() {
                    break;
                }
            }
        });
        // A continuous stream with gaps shorter than 50 ms must still admit a decision.
        tokio::time::timeout(Duration::from_millis(50), async {
            while events
                .next(async { Ok(rx.recv().await.unwrap()) })
                .await
                .unwrap()
                .is_some()
            {}
            assert_eq!(decision.try_recv().unwrap(), "allow");
        })
        .await
        .unwrap();
        output.abort();
    }
}

fn text_change(tx: &mpsc::SyncSender<Event>, run: &str, change: Change) -> Result<()> {
    tx.send(Event::Text {
        run: run.into(),
        change,
    })?;
    Ok(())
}
