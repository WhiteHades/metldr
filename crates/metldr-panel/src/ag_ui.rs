use std::collections::HashSet;

use metldr_conversation::{Completion, EntryRef};
use serde::{Deserialize, Serialize};

use crate::model::{RunStatus, RunView};

const MAX_FRAME: usize = 65_536;
const MAX_MESSAGES: usize = 64;
const MAX_TEXT: usize = 8_192;
const MAX_ID: usize = 512;
const MAX_HISTORY_MESSAGES: usize = 16 * (MAX_MESSAGES + 1);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgUiMessage {
    pub(crate) id: String,
    pub(crate) role: MessageRole,
    pub(crate) content: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MessageRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub(crate) enum Event {
    #[serde(rename = "RUN_STARTED")]
    RunStarted { thread_id: String, run_id: String },
    #[serde(rename = "TEXT_MESSAGE_START")]
    TextMessageStart {
        message_id: String,
        role: MessageRole,
    },
    #[serde(rename = "TEXT_MESSAGE_CONTENT")]
    TextMessageContent { message_id: String, delta: String },
    #[serde(rename = "TEXT_MESSAGE_END")]
    TextMessageEnd { message_id: String },
    #[serde(rename = "MESSAGES_SNAPSHOT")]
    MessagesSnapshot { messages: Vec<AgUiMessage> },
    #[serde(rename = "RUN_FINISHED")]
    RunFinished {
        thread_id: String,
        run_id: String,
        result: RunResult,
    },
    #[serde(rename = "RUN_ERROR")]
    RunError { message: String, code: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunResult {
    status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MessageState {
    id: String,
    content: String,
    completion: Completion,
}

pub(crate) struct RunStream {
    thread_id: String,
    run_id: String,
    prior_messages: Vec<AgUiMessage>,
    messages: Vec<MessageState>,
    started: bool,
    finished: bool,
}

impl RunStream {
    pub(crate) fn new(
        thread_id: String,
        ui_run_id: String,
        prior_messages: Vec<AgUiMessage>,
    ) -> Result<Self, String> {
        if !valid_id(&thread_id)
            || !valid_id(&ui_run_id)
            || prior_messages.len() > MAX_HISTORY_MESSAGES
        {
            return Err("invalid AG-UI thread, run, or message count".into());
        }
        let mut ids = HashSet::with_capacity(prior_messages.len());
        let mut text_bytes = 0;
        for message in &prior_messages {
            if !valid_id(&message.id) || !ids.insert(&message.id) {
                return Err("invalid or duplicate AG-UI message id".into());
            }
            text_bytes += message.content.len();
            if message.content.len() > MAX_TEXT || text_bytes > MAX_FRAME {
                return Err("AG-UI history exceeds the conversation text bound".into());
            }
        }
        let stream = Self {
            thread_id,
            run_id: ui_run_id,
            prior_messages,
            messages: Vec::new(),
            started: false,
            finished: false,
        };
        stream.check_frame_bound()?;
        Ok(stream)
    }

    pub(crate) fn push(&mut self, run: &RunView) -> Result<Vec<Event>, String> {
        if self.finished {
            return Err("AG-UI run stream is already finished".into());
        }
        let mut current = Vec::new();
        let mut has_entries = false;
        for entry in run.output.entries() {
            has_entries = true;
            if let EntryRef::Message {
                key,
                text,
                completion,
            } = entry
            {
                current.push(MessageState {
                    id: format!("{}:{}", run.id, key.dom_key()),
                    content: text.to_owned(),
                    completion: completion.clone(),
                });
            }
        }
        if !has_entries {
            current.push(MessageState {
                id: format!("{}:flat", run.id),
                content: run.output.text().to_owned(),
                completion: Completion::Streaming,
            });
        }
        let mut ids = self
            .prior_messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<HashSet<_>>();
        if current.iter().any(|message| !ids.insert(&message.id)) {
            return Err("AG-UI run message id duplicates conversation history".into());
        }
        self.push_messages(current, run.status, run.error.as_deref())
    }

    fn push_messages(
        &mut self,
        current: Vec<MessageState>,
        status: RunStatus,
        error: Option<&str>,
    ) -> Result<Vec<Event>, String> {
        if current.len() > MAX_MESSAGES
            || current
                .iter()
                .map(|message| message.content.len())
                .sum::<usize>()
                > MAX_TEXT
        {
            return Err("AG-UI run exceeds conversation message or text bounds".into());
        }
        let was_started = self.started;
        let terminal = status.is_terminal();
        let mut events = Vec::new();
        if !self.started {
            events.push(Event::RunStarted {
                thread_id: self.thread_id.clone(),
                run_id: self.run_id.clone(),
            });
            if !self.prior_messages.is_empty() {
                events.push(Event::MessagesSnapshot {
                    messages: self.prior_messages.clone(),
                });
            }
        }

        let rewrite = current.len() < self.messages.len()
            || current.iter().zip(&self.messages).any(|(next, old)| {
                next.id != old.id
                    || !next.content.starts_with(&old.content)
                    || (old.completion != Completion::Streaming
                        && (next.completion == Completion::Streaming
                            || next.content != old.content))
            });
        if rewrite {
            events.extend(self.end_open_messages());
            events.push(Event::MessagesSnapshot {
                messages: self.snapshot_messages(&current),
            });
            if !terminal {
                events.extend(current.iter().filter_map(|message| {
                    (message.completion == Completion::Streaming).then(|| Event::TextMessageStart {
                        message_id: message.id.clone(),
                        role: MessageRole::Assistant,
                    })
                }));
            }
        } else {
            for (index, message) in current.iter().enumerate() {
                let previous = self.messages.get(index);
                if previous.is_none() {
                    events.push(Event::TextMessageStart {
                        message_id: message.id.clone(),
                        role: MessageRole::Assistant,
                    });
                }
                let previous_len = previous.map_or(0, |old| old.content.len());
                if message.content.len() > previous_len {
                    events.push(Event::TextMessageContent {
                        message_id: message.id.clone(),
                        delta: message.content[previous_len..].to_owned(),
                    });
                }
                let was_ended = previous.is_some_and(|old| old.completion != Completion::Streaming);
                if !was_ended && (message.completion != Completion::Streaming || terminal) {
                    events.push(Event::TextMessageEnd {
                        message_id: message.id.clone(),
                    });
                }
            }
        }

        if terminal {
            events.push(self.terminal_event(status, error));
        }
        if serde_json::to_vec(&events)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_FRAME
        {
            events = Vec::new();
            events.extend(self.end_open_messages());
            if !was_started {
                events.push(Event::RunStarted {
                    thread_id: self.thread_id.clone(),
                    run_id: self.run_id.clone(),
                });
            }
            events.push(Event::MessagesSnapshot {
                messages: self.snapshot_messages(&current),
            });
            if !terminal {
                events.extend(current.iter().filter_map(|message| {
                    (message.completion == Completion::Streaming).then(|| Event::TextMessageStart {
                        message_id: message.id.clone(),
                        role: MessageRole::Assistant,
                    })
                }));
            }
            if terminal {
                events.push(self.terminal_event(status, error));
            }
        }

        if serde_json::to_vec(&events)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_FRAME
        {
            return Err("AG-UI event batch exceeds the panel frame bound".into());
        }
        let snapshot = self.snapshot_messages(&current);
        if serde_json::to_vec(&Event::MessagesSnapshot { messages: snapshot })
            .map_err(|error| error.to_string())?
            .len()
            > MAX_FRAME
        {
            return Err("AG-UI snapshot exceeds the panel frame bound".into());
        }

        self.messages = current;
        self.started = true;
        self.finished = terminal;
        Ok(events)
    }

    fn end_open_messages(&self) -> Vec<Event> {
        self.messages
            .iter()
            .filter(|message| message.completion == Completion::Streaming)
            .map(|message| Event::TextMessageEnd {
                message_id: message.id.clone(),
            })
            .collect()
    }

    fn snapshot_messages(&self, current: &[MessageState]) -> Vec<AgUiMessage> {
        self.prior_messages
            .iter()
            .cloned()
            .chain(current.iter().map(|message| AgUiMessage {
                id: message.id.clone(),
                role: MessageRole::Assistant,
                content: message.content.clone(),
            }))
            .collect()
    }

    fn terminal_event(&self, status: RunStatus, error: Option<&str>) -> Event {
        match status {
            RunStatus::Failed => Event::RunError {
                message: error.unwrap_or("run failed").to_owned(),
                code: "run_failed".into(),
            },
            RunStatus::Complete | RunStatus::Cancelled | RunStatus::Interrupted => {
                Event::RunFinished {
                    thread_id: self.thread_id.clone(),
                    run_id: self.run_id.clone(),
                    result: RunResult {
                        status: status.to_string(),
                    },
                }
            }
            RunStatus::Running | RunStatus::StopRequested => {
                unreachable!("nonterminal status has no AG-UI terminal event")
            }
        }
    }

    fn check_frame_bound(&self) -> Result<(), String> {
        let messages = self.snapshot_messages(&self.messages);
        let bytes = serde_json::to_vec(&Event::MessagesSnapshot { messages })
            .map_err(|error| error.to_string())?
            .len();
        if bytes > MAX_FRAME {
            return Err("AG-UI snapshot exceeds the panel frame bound".into());
        }
        Ok(())
    }
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> RunStream {
        RunStream::new("thread".into(), "run".into(), vec![]).unwrap()
    }

    fn message(id: &str, content: &str, completion: Completion) -> MessageState {
        MessageState {
            id: id.into(),
            content: content.into(),
            completion,
        }
    }

    #[test]
    fn emits_unicode_appends_in_standard_event_order() {
        assert!(
            RunStream::new(
                "thread".into(),
                "run".into(),
                vec![
                    AgUiMessage {
                        id: "user".into(),
                        role: MessageRole::User,
                        content: "a".repeat(5000)
                    },
                    AgUiMessage {
                        id: "assistant".into(),
                        role: MessageRole::Assistant,
                        content: "b".repeat(5000)
                    },
                ]
            )
            .is_ok()
        );
        let mut stream = stream();
        let first = stream
            .push_messages(
                vec![message("run:native:answer", "hé", Completion::Streaming)],
                RunStatus::Running,
                None,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::json!([
                {"type":"RUN_STARTED","threadId":"thread","runId":"run"},
                {"type":"TEXT_MESSAGE_START","messageId":"run:native:answer","role":"assistant"},
                {"type":"TEXT_MESSAGE_CONTENT","messageId":"run:native:answer","delta":"hé"}
            ])
        );
        let next = stream
            .push_messages(
                vec![message(
                    "run:native:answer",
                    "héllo",
                    Completion::StreamEnded,
                )],
                RunStatus::Complete,
                None,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(next).unwrap(),
            serde_json::json!([
                {"type":"TEXT_MESSAGE_CONTENT","messageId":"run:native:answer","delta":"llo"},
                {"type":"TEXT_MESSAGE_END","messageId":"run:native:answer"},
                {"type":"RUN_FINISHED","threadId":"thread","runId":"run","result":{"status":"complete"}}
            ])
        );

        let mut bounded = RunStream::new(
            "thread".into(),
            "run".into(),
            (0..15)
                .map(|index| AgUiMessage {
                    id: format!("history:{index}"),
                    role: MessageRole::User,
                    content: "h".repeat(4_000),
                })
                .collect(),
        )
        .unwrap();
        assert!(
            bounded
                .push_messages(
                    vec![message(
                        "run:native:answer",
                        &"x".repeat(MAX_TEXT),
                        Completion::Streaming
                    )],
                    RunStatus::Complete,
                    None,
                )
                .is_err()
        );
        assert!(!bounded.started);
        assert!(!bounded.finished);
        assert!(bounded.messages.is_empty());
        assert!(matches!(
            bounded
                .push_messages(vec![], RunStatus::Running, None)
                .unwrap()
                .first(),
            Some(Event::RunStarted { .. })
        ));
    }

    #[test]
    fn rewrites_snapshot_and_interruption_does_not_claim_completion() {
        let mut stream = stream();
        stream
            .push_messages(
                vec![message(
                    "run:native:answer",
                    "partial",
                    Completion::Streaming,
                )],
                RunStatus::Running,
                None,
            )
            .unwrap();
        let events = stream
            .push_messages(
                vec![message(
                    "run:native:replacement",
                    "revised",
                    Completion::Streaming,
                )],
                RunStatus::Running,
                None,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(events).unwrap(),
            serde_json::json!([
                {"type":"TEXT_MESSAGE_END","messageId":"run:native:answer"},
                {"type":"MESSAGES_SNAPSHOT","messages":[{"id":"run:native:replacement","role":"assistant","content":"revised"}]},
                {"type":"TEXT_MESSAGE_START","messageId":"run:native:replacement","role":"assistant"}
            ])
        );
        let delta = stream
            .push_messages(
                vec![message(
                    "run:native:replacement",
                    "revised!",
                    Completion::Streaming,
                )],
                RunStatus::Running,
                None,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(delta).unwrap(),
            serde_json::json!([
                {"type":"TEXT_MESSAGE_CONTENT","messageId":"run:native:replacement","delta":"!"}
            ])
        );
        let terminal = stream
            .push_messages(
                vec![message(
                    "run:native:final",
                    "ending",
                    Completion::Interrupted,
                )],
                RunStatus::Interrupted,
                None,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(terminal).unwrap(),
            serde_json::json!([
                {"type":"TEXT_MESSAGE_END","messageId":"run:native:replacement"},
                {"type":"MESSAGES_SNAPSHOT","messages":[{"id":"run:native:final","role":"assistant","content":"ending"}]},
                {"type":"RUN_FINISHED","threadId":"thread","runId":"run","result":{"status":"interrupted"}}
            ])
        );
    }
}
