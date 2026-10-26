use std::collections::HashSet;

use metldr_conversation::{Completion, EntryRef, MAX_MESSAGES};
use serde::{Deserialize, Serialize};

use crate::model::{RunId, RunStatus, RunView};

const MAX_FRAME: usize = 65_536;
const MAX_TEXT: usize = 8_192;
const MAX_ID: usize = 512;
pub(crate) const MAX_HISTORY_MESSAGES: usize =
    16 * (MAX_MESSAGES + metldr_conversation::MAX_ARTIFACTS + metldr_conversation::MAX_TOOLS + 2);

mod message;
pub(crate) mod run_details;
pub(crate) mod tools;
pub(crate) use message::{Activity, AgUiMessage, RunOrigin, TableReference};
use run_details::RunDetailsReference;

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
    #[serde(rename = "ACTIVITY_SNAPSHOT")]
    ActivitySnapshot {
        message_id: String,
        #[serde(flatten)]
        activity: Activity,
        replace: bool,
    },
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum EntryState {
    Text(MessageState),
    Artifact {
        id: String,
        reference: TableReference,
    },
    Tools {
        id: String,
        reference: tools::ToolGroupReference,
    },
    RunDetails {
        id: String,
        reference: RunDetailsReference,
    },
}

impl EntryState {
    fn id(&self) -> &str {
        match self {
            Self::Text(message) => &message.id,
            Self::Artifact { id, .. } | Self::Tools { id, .. } | Self::RunDetails { id, .. } => id,
        }
    }

    fn text(&self) -> Option<&MessageState> {
        match self {
            Self::Text(message) => Some(message),
            Self::Artifact { .. } | Self::Tools { .. } | Self::RunDetails { .. } => None,
        }
    }

    fn snapshot(&self) -> AgUiMessage {
        match self {
            Self::Text(message) => AgUiMessage::Assistant {
                id: message.id.clone(),
                content: message.content.clone(),
            },
            Self::Artifact { id, reference } => AgUiMessage::Activity {
                id: id.clone(),
                activity: Activity::Table(reference.clone()),
            },
            Self::Tools { reference, .. } => AgUiMessage::native_tools(reference.clone()),
            Self::RunDetails { reference, .. } => AgUiMessage::run_details(reference.clone()),
        }
    }
}

pub(crate) struct RunStream {
    thread_id: String,
    run_id: String,
    prior_messages: Vec<AgUiMessage>,
    messages: Vec<EntryState>,
    started: bool,
    finished: bool,
}

impl RunStream {
    pub(crate) fn accept_question(&mut self, native: &RunId) -> Result<Vec<Event>, String> {
        let index = self
            .prior_messages
            .len()
            .checked_sub(1)
            .ok_or("the submitted question is missing")?;
        let id = format!("{native}:question");
        if self.started
            || !matches!(self.prior_messages[index], AgUiMessage::User { .. })
            || !valid_id(&id)
            || self.prior_messages[..index]
                .iter()
                .any(|message| message.id() == id)
        {
            return Err("the submitted question cannot bind to this native run".into());
        }
        let previous = std::mem::replace(self.prior_messages[index].id_mut(), id);
        let result = self.push_messages(Vec::new(), RunStatus::Running, None);
        if result.is_err() {
            *self.prior_messages[index].id_mut() = previous;
        }
        result
    }

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
            if !valid_id(message.id()) || !ids.insert(message.id()) {
                return Err("invalid or duplicate AG-UI message id".into());
            }
            let text = message.text().unwrap_or("");
            text_bytes += text.len();
            if text.len() > MAX_TEXT || text_bytes > MAX_FRAME {
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
        let tools = tools::groups(run, RunOrigin::Live);
        let details = RunDetailsReference::new(run, RunOrigin::Live);
        current.push(EntryState::RunDetails {
            id: details.message_id(),
            reference: details,
        });
        let mut has_entries = false;
        for entry in run.output.entries() {
            has_entries = true;
            match entry {
                EntryRef::Message {
                    key,
                    text,
                    completion,
                } => current.push(EntryState::Text(MessageState {
                    id: format!("{}:{}", run.id, key.dom_key()),
                    content: text.to_owned(),
                    completion: completion.clone(),
                })),
                EntryRef::Artifact { id } => {
                    let AgUiMessage::Activity {
                        id,
                        activity: Activity::Table(content),
                    } = AgUiMessage::table(&run.id, id)
                    else {
                        unreachable!()
                    };
                    current.push(EntryState::Artifact {
                        id,
                        reference: content,
                    });
                }
                EntryRef::Tool { id } => {
                    if let Some(reference) = tools.iter().find(|group| &group.group_id == id) {
                        current.push(EntryState::Tools {
                            id: reference.message_id(),
                            reference: reference.clone(),
                        });
                    }
                }
            }
        }
        if !has_entries {
            current.push(EntryState::Text(MessageState {
                id: format!("{}:flat", run.id),
                content: run.output.text().to_owned(),
                completion: Completion::Streaming,
            }));
        }
        self.push_messages(current, run.status, run.error.as_deref())
    }

    fn push_messages(
        &mut self,
        current: Vec<EntryState>,
        status: RunStatus,
        error: Option<&str>,
    ) -> Result<Vec<Event>, String> {
        if self.finished {
            return Err("AG-UI run stream is already finished".into());
        }
        let mut ids = self
            .prior_messages
            .iter()
            .map(AgUiMessage::id)
            .collect::<HashSet<_>>();
        if current
            .iter()
            .any(|entry| !valid_id(entry.id()) || !ids.insert(entry.id()))
        {
            return Err("AG-UI run message id duplicates conversation history".into());
        }
        let texts = current
            .iter()
            .filter_map(EntryState::text)
            .collect::<Vec<_>>();
        if texts.len() > MAX_MESSAGES
            || current.len() - texts.len()
                > metldr_conversation::MAX_ARTIFACTS + metldr_conversation::MAX_TOOLS + 1
            || texts
                .iter()
                .map(|message| message.content.len())
                .sum::<usize>()
                > MAX_TEXT
        {
            return Err("AG-UI run exceeds conversation message or text bounds".into());
        }
        if let Some(last) = self
            .messages
            .iter()
            .rposition(|entry| matches!(entry, EntryState::Artifact { .. }))
        {
            if current.len() <= last || self.messages[..=last].iter().zip(&current).any(|(old, next)| old.id() != next.id()
                || matches!((old, next), (EntryState::Artifact { .. }, _) if old != next)) {
                return Err("saved table position or identity changed".into());
            }
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
            || current
                .iter()
                .zip(&self.messages)
                .any(|(next, old)| match (next, old) {
                    (EntryState::Text(next), EntryState::Text(old)) => {
                        next.id != old.id
                            || !next.content.starts_with(&old.content)
                            || (old.completion != Completion::Streaming
                                && (next.completion == Completion::Streaming
                                    || next.content != old.content))
                    }
                    (EntryState::Tools { id: next, .. }, EntryState::Tools { id: old, .. }) => {
                        next != old
                    }
                    (
                        EntryState::RunDetails { id: next, .. },
                        EntryState::RunDetails { id: old, .. },
                    ) => next != old,
                    _ => next != old,
                });
        if rewrite {
            events.extend(self.end_open_messages());
            events.push(Event::MessagesSnapshot {
                messages: self.snapshot_messages(&current),
            });
            if !terminal {
                events.extend(Self::open_messages(&current));
            }
        } else {
            for (index, entry) in current.iter().enumerate() {
                match entry {
                    EntryState::Artifact { id, reference }
                        if self.messages.get(index).is_none() =>
                    {
                        events.push(Event::ActivitySnapshot {
                            message_id: id.clone(),
                            activity: Activity::Table(reference.clone()),
                            replace: false,
                        })
                    }
                    EntryState::Artifact { .. } => (),
                    EntryState::Tools { id, reference } => {
                        let previous = self.messages.get(index);
                        if previous != Some(entry) {
                            events.push(Event::ActivitySnapshot {
                                message_id: id.clone(),
                                activity: Activity::NativeTools(reference.clone()),
                                replace: previous.is_some(),
                            });
                        }
                    }
                    EntryState::RunDetails { id, reference } => {
                        let previous = self.messages.get(index);
                        if previous != Some(entry) {
                            events.push(Event::ActivitySnapshot {
                                message_id: id.clone(),
                                activity: Activity::RunDetails(reference.clone()),
                                replace: previous.is_some(),
                            });
                        }
                    }
                    EntryState::Text(message) => {
                        let previous = self.messages.get(index).and_then(EntryState::text);
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
                        let was_ended =
                            previous.is_some_and(|old| old.completion != Completion::Streaming);
                        if !was_ended && (message.completion != Completion::Streaming || terminal) {
                            events.push(Event::TextMessageEnd {
                                message_id: message.id.clone(),
                            });
                        }
                    }
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
            events = self.end_open_messages();
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
                events.extend(Self::open_messages(&current));
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
        if serde_json::to_vec(&Event::MessagesSnapshot {
            messages: self.snapshot_messages(&current),
        })
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

    fn open_messages(entries: &[EntryState]) -> impl Iterator<Item = Event> + '_ {
        entries
            .iter()
            .filter_map(EntryState::text)
            .filter(|message| message.completion == Completion::Streaming)
            .map(|message| Event::TextMessageStart {
                message_id: message.id.clone(),
                role: MessageRole::Assistant,
            })
    }

    fn end_open_messages(&self) -> Vec<Event> {
        self.messages
            .iter()
            .filter_map(EntryState::text)
            .filter(|message| message.completion == Completion::Streaming)
            .map(|message| Event::TextMessageEnd {
                message_id: message.id.clone(),
            })
            .collect()
    }

    fn snapshot_messages(&self, current: &[EntryState]) -> Vec<AgUiMessage> {
        self.prior_messages
            .iter()
            .cloned()
            .chain(current.iter().map(EntryState::snapshot))
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
                        status: match status {
                            RunStatus::Complete => "complete",
                            RunStatus::Cancelled => "cancelled",
                            _ => "interrupted",
                        }
                        .into(),
                    },
                }
            }
            RunStatus::Running | RunStatus::StopRequested => {
                unreachable!("nonterminal status has no AG-UI terminal event")
            }
        }
    }

    fn check_frame_bound(&self) -> Result<(), String> {
        if serde_json::to_vec(&Event::MessagesSnapshot {
            messages: self.snapshot_messages(&self.messages),
        })
        .map_err(|error| error.to_string())?
        .len()
            > MAX_FRAME
        {
            return Err("AG-UI snapshot exceeds the panel frame bound".into());
        }
        Ok(())
    }
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod artifact_checks;

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> RunStream {
        RunStream::new("thread".into(), "run".into(), vec![]).unwrap()
    }

    #[test]
    fn run_details_revisions_replace_one_stable_activity_message() {
        let mut run = tools::checks::run();
        let mut stream = stream();
        let initial = serde_json::to_value(stream.push(&run).unwrap()).unwrap();
        let detail = initial
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["activityType"] == "metldr.run-details")
            .unwrap();
        let id = detail["messageId"].clone();
        run.route = crate::model::RunRoute::Fixture;
        let changed = serde_json::to_value(stream.push(&run).unwrap()).unwrap();
        assert_eq!(changed.as_array().unwrap().len(), 1);
        assert_eq!(changed[0]["type"], "ACTIVITY_SNAPSHOT");
        assert_eq!(changed[0]["activityType"], "metldr.run-details");
        assert_eq!(changed[0]["messageId"], id);
        assert_eq!(changed[0]["replace"], true);
    }

    fn message(id: &str, content: &str, completion: Completion) -> EntryState {
        EntryState::Text(MessageState {
            id: id.into(),
            content: content.into(),
            completion,
        })
    }

    #[test]
    fn binds_the_submitted_question_to_its_saved_identity_before_streaming() {
        let native: RunId =
            serde_json::from_value(serde_json::json!("0123456789abcdef0123456789abcdef:7"))
                .unwrap();
        let mut stream = RunStream::new(
            "thread".into(),
            "ui-run".into(),
            vec![
                AgUiMessage::Assistant {
                    id: "earlier".into(),
                    content: "Saved answer".into(),
                },
                AgUiMessage::User {
                    id: "submitted".into(),
                    content: "Next question".into(),
                },
            ],
        )
        .unwrap();
        let events = stream.accept_question(&native).unwrap();
        assert!(matches!(&events[0], Event::RunStarted { thread_id, run_id }
            if thread_id == "thread" && run_id == "ui-run"));
        assert_eq!(events.len(), 2);
        let Event::MessagesSnapshot { messages } = &events[1] else {
            panic!("the first update must include saved history");
        };
        assert_eq!(messages[0].id(), "earlier");
        assert_eq!(messages[1].id(), format!("{native}:question"));
        assert_eq!(messages[1].text(), Some("Next question"));
        assert!(stream.accept_question(&native).is_err());
        assert!(
            stream
                .push_messages(vec![], RunStatus::Running, None)
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            stream
                .push_messages(vec![], RunStatus::Cancelled, None)
                .unwrap()
                .as_slice(),
            [Event::RunFinished { .. }]
        ));
    }

    #[test]
    fn emits_unicode_appends_in_standard_event_order() {
        assert!(
            RunStream::new(
                "thread".into(),
                "run".into(),
                vec![
                    AgUiMessage::User {
                        id: "user".into(),
                        content: "a".repeat(5000),
                    },
                    AgUiMessage::Assistant {
                        id: "assistant".into(),
                        content: "b".repeat(5000),
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
        let repeated = stream
            .push_messages(
                vec![message("run:native:answer", "hé", Completion::Streaming)],
                RunStatus::Running,
                None,
            )
            .unwrap();
        assert!(repeated.is_empty());
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
                .map(|index| AgUiMessage::User {
                    id: format!("history:{index}"),
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
