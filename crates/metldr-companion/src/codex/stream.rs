use super::*;
use crate::native_correction::{Command, Phase, Receiver, Target};
use metldr_codex_session::{Event as NativeEvent, Interrupt, SteerId, SteerOutcome, TurnStatus};
use metldr_conversation::{Change, MessageKey};
use std::time::{Duration, Instant};

pub(super) async fn run(
    job: &Job,
    session: &mut CodexSession,
    tools: &mut Tools,
    stop: &AtomicBool,
    tx: &mpsc::SyncSender<Event>,
    corrections: &mut Receiver,
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
    let turn = session
        .prompt(&format!("{prompt}\n\n{BROWSER_POLICY}"), job.mode.native())
        .await?;
    let target = Target::Codex(turn.clone());
    corrections.ready(target.clone())?;
    let mut stop_wait = agent::StopWait::default();
    let mut interrupted = None;
    let mut review = super::approval::Review::default();
    let mut events = Events::default();
    let mut inputs = super::input::Inputs::default();
    let mut terminal = None;
    let mut drain_until = Instant::now();
    let mut held_correction: Option<Command> = None;
    let mut steering: Option<(Command, SteerId)> = None;
    loop {
        if terminal.is_some()
            && ((inputs.is_empty() && steering.is_none()) || Instant::now() >= drain_until)
        {
            return outcome(terminal.take().unwrap(), &interrupted, review.cancelled);
        }
        let stopping = stop.load(Ordering::Acquire) || review.cancelled;
        if stopping {
            if let Some(command) = held_correction.take() {
                corrections.finish(
                    &command.request,
                    Phase::NotSent {
                        message: "Codex correction was stopped before sending".into(),
                    },
                )?;
            }
        }
        if terminal.is_none() {
            stop_wait.check(stopping, Instant::now())?;
        }
        if terminal.is_none() && stopping {
            review.poll(session, true).await?;
            inputs.poll(session, job, tx, true, stop).await?;
        }
        if terminal.is_none() && interrupted.is_none() && stopping && !review.cancelled {
            interrupted = Some(session.interrupt().await?);
        }
        if events.queued == 256 {
            if review.has_answer()? || inputs.has_answer(stop)? {
                return Err("Codex event backlog prevented a safe approval reply".into());
            }
            events.queued = 1;
            tokio::task::yield_now().await;
        }
        let next = if terminal.is_some() {
            match tokio::time::timeout(
                drain_until.saturating_duration_since(Instant::now()),
                events.next(session.next_event()),
            )
            .await
            {
                Ok(next) => next,
                Err(_) => return outcome(terminal.take().unwrap(), &interrupted, review.cancelled),
            }
        } else {
            events.next(session.next_event()).await
        };
        let event = match next {
            Ok(event) => event,
            Err(_) if terminal.is_some() => {
                return outcome(terminal.take().unwrap(), &interrupted, review.cancelled);
            }
            Err(error) => return Err(error),
        };
        let Some(event) = event else {
            // Drain ready revocations and patch updates before applying a human choice.
            if terminal.is_none() {
                review.poll(session, stop.load(Ordering::Acquire)).await?;
                inputs
                    .poll(session, job, tx, review.cancelled, stop)
                    .await?;
                if let Some(command) = held_correction.take() {
                    if stop.load(Ordering::Acquire) || review.cancelled {
                        corrections.finish(
                            &command.request,
                            Phase::NotSent {
                                message: "Codex correction was stopped before sending".into(),
                            },
                        )?;
                    } else {
                        let id = session.steer(&turn, &command.text).await?;
                        steering = Some((command, id));
                    }
                } else if let Some(command) = corrections.take()? {
                    if corrections.authorize(&command, &target, stop)? {
                        held_correction = Some(command);
                        continue;
                    }
                    corrections.finish(
                        &command.request,
                        Phase::NotSent {
                            message: "Codex correction was not authorized for sending".into(),
                        },
                    )?;
                }
            }
            continue;
        };
        match event {
            NativeEvent::SteerResult { request, outcome } => {
                let Some((command, pending)) = steering.as_ref() else {
                    return Err("Codex returned an unexpected correction result".into());
                };
                if &request != pending {
                    return Err("Codex correction result identity mismatch".into());
                }
                let phase = match outcome {
                    SteerOutcome::Accepted { turn } => Phase::CodexAccepted {
                        turn: turn.as_str().into(),
                    },
                    SteerOutcome::Rejected { code } => Phase::Rejected {
                        message: format!("Codex rejected correction (code {code})"),
                    },
                };
                corrections.finish(&command.request, phase)?;
                steering = None;
            }
            NativeEvent::Input(input) => {
                inputs.request(input, job, tx, stopping || terminal.is_some())?
            }
            NativeEvent::InputResolved(id) => inputs.resolved(&id, job, tx)?,
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
                corrections.close()?;
                if let Some(command) = held_correction.take() {
                    corrections.finish(
                        &command.request,
                        Phase::NotSent {
                            message: "Codex turn ended before correction sending".into(),
                        },
                    )?;
                }
                review.finish(job, tx, status == TurnStatus::Completed)?;
                inputs.withdraw(job, tx)?;
                terminal = Some(status);
                drain_until = Instant::now() + Duration::from_secs(3);
            }
        }
    }
}

fn outcome(
    status: TurnStatus,
    interrupted: &Option<Interrupt>,
    cancelled: bool,
) -> Result<Outcome> {
    match status {
        TurnStatus::Completed => Ok(Outcome::Complete),
        TurnStatus::Interrupted if interrupted == &Some(Interrupt::Acknowledged) || cancelled => {
            Ok(Outcome::Cancelled)
        }
        TurnStatus::Interrupted => Err("Codex interrupted without acknowledged Stop".into()),
        TurnStatus::Failed => Err("Codex reported a failed turn".into()),
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
