use super::*;
use crate::{agent_model::Activity, tool_report::Call};
use metldr_conversation::{Change, MessageKey};
use metldr_opencode_session::{Event as NativeEvent, Permission};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

struct Pending {
    request: Permission,
    answer: mpsc::Receiver<Option<String>>,
    deadline: Instant,
}

struct ActiveTool {
    name: String,
    call: Call,
}

pub(super) async fn run(
    job: &Job,
    session: &mut OpenCodeSession,
    selection: &Selection,
    stop: &AtomicBool,
    failed: &AtomicBool,
    tx: &mpsc::SyncSender<Event>,
) -> Result<Outcome> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut pending: Vec<Pending> = Vec::new();
    let (mut tools, mut seen_tools): (BTreeMap<String, ActiveTool>, BTreeSet<String>) =
        (BTreeMap::new(), BTreeSet::new());
    let (mut cancelled, mut bytes, mut activities) = (false, 0usize, 0usize);
    loop {
        if Instant::now() >= deadline {
            return Err("OpenCode task exceeded the owned prototype deadline".into());
        }
        let stopping = stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire);
        if stopping && !cancelled {
            session.cancel().await?;
            cancelled = true;
        }
        let mut index = 0;
        while index < pending.len() {
            let answer = if stopping {
                Some(None)
            } else {
                match pending[index].answer.try_recv() {
                    Ok(option) => Some(option),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("OpenCode permission owner ended without a decision".into());
                    }
                }
            };
            if let Some(answer) = answer {
                let request = pending.swap_remove(index);
                session
                    .reply_permission(&request.request, answer.as_deref())
                    .await?;
            } else {
                if Instant::now() >= pending[index].deadline {
                    return Err("OpenCode native permission decision deadline expired".into());
                }
                index += 1;
            }
        }
        let event =
            match tokio::time::timeout(Duration::from_millis(30), session.next_event()).await {
                Ok(event) => event?,
                Err(_) => continue,
            };
        match event {
            NativeEvent::Permission(request) => {
                if stopping {
                    session.reply_permission(&request, None).await?;
                    continue;
                }
                if pending.len() >= 8 || serde_json::to_vec(request.details())?.len() > 8192 {
                    session.reply_permission(&request, None).await?;
                    return Err("OpenCode permission exceeds the application bound".into());
                }
                let (ack, answer) = mpsc::sync_channel(1);
                tx.send(Event::OpenCodePermission {
                    run: job.run.clone(),
                    details: request.details().clone(),
                    ack,
                })
                .map_err(|_| "OpenCode permission owner is unavailable")?;
                pending.push(Pending {
                    request,
                    answer,
                    deadline: Instant::now() + Duration::from_secs(30),
                });
            }
            NativeEvent::Update(update) => match update["sessionUpdate"].as_str() {
                Some("agent_message_chunk") => {
                    let message_id = update["messageId"]
                        .as_str()
                        .ok_or("OpenCode output message identity is missing")?;
                    let text = update["content"]["text"]
                        .as_str()
                        .ok_or("OpenCode output is not supported text")?;
                    bytes = bytes
                        .checked_add(text.len())
                        .filter(|n| *n <= crate::TEXT_MAX)
                        .ok_or("OpenCode output exceeds application bound")?;
                    tx.send(Event::Text {
                        run: job.run.clone(),
                        change: Change::Append {
                            key: MessageKey::native(message_id.into())?,
                            text: text.into(),
                        },
                    })
                    .map_err(|_| "OpenCode output owner is unavailable")?;
                }
                Some("agent_thought_chunk") => (),
                Some("tool_call" | "tool_call_update") => activity(
                    job,
                    &update,
                    &mut tools,
                    &mut seen_tools,
                    &mut activities,
                    tx,
                )?,
                Some("config_option_update") => {
                    let options = update["configOptions"]
                        .as_array()
                        .ok_or("OpenCode configuration update is malformed")?;
                    if Selection::from_options(options)? != *selection {
                        return Err("OpenCode native model or mode changed during the task".into());
                    }
                }
                _ => (),
            },
            NativeEvent::Finished { stop_reason } => {
                if !pending.is_empty() {
                    return Err("OpenCode settled with unanswered native permissions".into());
                }
                if failed.load(Ordering::Acquire) {
                    return Err("OpenCode browser service failed during the task".into());
                }
                return match stop_reason.as_str() {
                    "end_turn" if tools.is_empty() => Ok(Outcome::Complete),
                    "cancelled" if cancelled => Ok(Outcome::Cancelled),
                    _ => Err(
                        "OpenCode did not confirm successful completion or requested cancellation"
                            .into(),
                    ),
                };
            }
        }
    }
}

fn activity(
    job: &Job,
    update: &Value,
    tools: &mut BTreeMap<String, ActiveTool>,
    seen_tools: &mut BTreeSet<String>,
    count: &mut usize,
    tx: &mpsc::SyncSender<Event>,
) -> Result<()> {
    let id = update["toolCallId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or("OpenCode tool identity is malformed")?;
    let entry = if update["sessionUpdate"] == "tool_call" {
        let name = update["name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("OpenCode native tool name is missing")?;
        if tools.len() >= 16 || seen_tools.len() >= 16 || !seen_tools.insert(id.into()) {
            return Err("OpenCode native tool identity repeated or exceeds bound".into());
        }
        let mut call = Call::new(id)?;
        call.merge_opencode(update);
        let report_call = call.clone();
        tools.insert(
            id.to_owned(),
            ActiveTool {
                name: name.to_owned(),
                call,
            },
        );
        Some((name.to_owned(), "started", report_call))
    } else {
        if let Some(active) = tools.get_mut(id) {
            let before = active.call.clone();
            active.call.merge_opencode(update);
            if !matches!(update["status"].as_str(), Some("completed" | "failed"))
                && active.call != before
            {
                crate::tool_report::send_progress(tx, &job.run, active.call.clone())?;
            }
        }
        match update["status"].as_str() {
            Some("completed" | "failed") => {
                let active = tools
                    .remove(id)
                    .ok_or("OpenCode tool completion has no start")?;
                Some((
                    active.name,
                    if update["status"] == "completed" {
                        "completed"
                    } else {
                        "failed"
                    },
                    active.call,
                ))
            }
            _ => None,
        }
    };
    if let Some((tool, phase, call)) = entry {
        *count += 1;
        if *count > 32 {
            return Err("OpenCode native activity exceeds application bound".into());
        }
        tx.send(Event::OpenCodeActivity {
            run: job.run.clone(),
            activity: Activity {
                outcome: None,
                tool,
                phase: phase.into(),
                call: Some(call),
            },
        })
        .map_err(|_| "OpenCode activity owner is unavailable")?;
    }
    Ok(())
}
