use crate::{
    Result,
    agent::{self, Outcome},
    browser_io::{Client, Notice},
    browser_worker::{BROWSER_POLICY, Tools, decode_call, definitions},
    codex_model::{Binding, Identity, Profile, Selection},
    flow::Event,
    model::Source,
};
use metldr_codex_session::{
    CodexSession, Event as NativeEvent, Interrupt, Thread, ToolSpec, TurnStatus,
};
use metldr_conversation::{Change, MessageKey};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub(super) struct Job {
    pub run: String,
    pub workflow: String,
    pub profile: Profile,
    pub binding: Binding,
    pub source: Source,
    pub target: crate::browser_action::Target,
    pub text: String,
}

pub(super) fn execute(
    job: Job,
    stop: Arc<AtomicBool>,
    tx: mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })
        .map(|runtime| runtime.block_on(execute_inner(&job, &stop, &tx, events)));
    let (result, released) = match result {
        Ok(result) => result,
        Err(error) => (Outcome::Failed(error.to_string()), true),
    };
    let _ = tx.send(Event::CodexSettled {
        run: job.run,
        result,
        released,
    });
}

async fn execute_inner(
    job: &Job,
    stop: &Arc<AtomicBool>,
    tx: &mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
) -> (Outcome, bool) {
    if stop.load(Ordering::Acquire) {
        return (Outcome::Cancelled, true);
    }
    let thread = match job.binding.resume() {
        Ok(Some(id)) => Thread::Resume(id),
        Ok(None) => Thread::New {
            tools: definitions()
                .into_iter()
                .map(|definition| ToolSpec {
                    name: definition.name.into(),
                    description: definition.description.into(),
                    input_schema: definition.input_schema,
                })
                .collect(),
        },
        Err(error) => return (Outcome::Failed(error.to_string()), false),
    };
    let mut session =
        match CodexSession::open(&job.profile.executable, &job.profile.workspace, thread).await {
            Ok(session) => session,
            Err(error) => return (Outcome::Failed(error.to_string()), false),
        };
    let mut tools = Tools::new(
        Client {
            run: job.run.clone(),
            tx: tx.clone(),
        },
        job.target.clone(),
        events,
        stop.clone(),
    );
    let result = run(job, &mut session, &mut tools, stop, tx).await;
    let browser_closed = tools.close().await;
    match (browser_closed, session.close().await) {
        (Ok(()), Ok(exit)) if exit.code == Some(0) && !exit.forced && exit.drained => (
            result.unwrap_or_else(|error| Outcome::Failed(error.to_string())),
            true,
        ),
        (Err(_), _) => (
            Outcome::Failed("Browser release was not confirmed after the Codex turn".into()),
            false,
        ),
        (Ok(()), Ok(exit)) => (
            Outcome::Failed(format!(
                "Codex shutdown was not confirmed: exit {:?}, forced {}, drained {}",
                exit.code, exit.forced, exit.drained
            )),
            false,
        ),
        (Ok(()), Err(_)) => (
            Outcome::Failed("Codex process shutdown failed".into()),
            false,
        ),
    }
}

async fn run(
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
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut interrupted = None;
    loop {
        if Instant::now() >= deadline {
            return Err("Codex task exceeded the owned prototype deadline".into());
        }
        if interrupted.is_none() && stop.load(Ordering::Acquire) {
            interrupted = Some(session.interrupt().await?);
        }
        let event =
            match tokio::time::timeout(Duration::from_millis(50), session.next_event()).await {
                Ok(event) => event?,
                Err(_) => continue,
            };
        match event {
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
                let decoded = decode_call(&call.name, call.arguments.clone())?;
                let outcome = tools.dispatch(&call.call_id, decoded, stop).await;
                if stop.load(Ordering::Acquire) {
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
                return match status {
                    TurnStatus::Completed => Ok(Outcome::Complete),
                    TurnStatus::Interrupted if interrupted == Some(Interrupt::Acknowledged) => {
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

fn text_change(tx: &mpsc::SyncSender<Event>, run: &str, change: Change) -> Result<()> {
    tx.send(Event::Text {
        run: run.into(),
        change,
    })?;
    Ok(())
}
