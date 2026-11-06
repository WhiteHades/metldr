use crate::Result;
use crate::model::{Command, ConversationAction, Request, Status, Store, valid_request_id};
use crate::store::Disk;
use metldr_conversation::Change;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, atomic::AtomicBool, mpsc::SyncSender},
    thread::JoinHandle,
};

mod view;

pub(super) fn fatal(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<io::Error>().is_some() || crate::history::is_failure(error)
}

pub(super) fn acknowledge<T>(
    ack: SyncSender<std::result::Result<T, String>>,
    result: Result<T>,
) -> Result<()> {
    match result {
        Ok(value) => {
            let _ = ack.send(Ok(value));
            Ok(())
        }
        Err(error) => {
            let _ = ack.send(Err(error.to_string()));
            if fatal(error.as_ref()) {
                Err(error)
            } else {
                Ok(())
            }
        }
    }
}

pub(super) enum Event {
    CorrectionReady {
        run: String,
        target: crate::native_correction::Target,
    },
    CorrectionDispatch {
        run: String,
        request: String,
        target: crate::native_correction::Target,
        ack: SyncSender<bool>,
    },
    CorrectionFinished {
        run: String,
        request: String,
        outcome: crate::native_correction::Phase,
    },
    CorrectionClosed {
        run: String,
    },
    Provider(crate::provider_settings::Completion),
    BrowserIo(crate::browser_io::Call),
    Browser(crate::browser_owner::Event),
    Input(crate::transport::Input),
    Text {
        run: String,
        change: Change,
    },
    Presentation {
        run: String,
        update: crate::presentation::Update,
        ack: SyncSender<std::result::Result<metldr_conversation::ArtifactId, String>>,
    },
    ToolProgress {
        run: String,
        call: crate::tool_report::Call,
    },
    Settled {
        run: String,
        result: std::result::Result<(), String>,
    },
    PiBound {
        run: String,
        workflow: String,
        binding: crate::agent_model::Binding,
        ack: std::sync::mpsc::SyncSender<std::result::Result<bool, String>>,
    },
    PiActivity {
        run: String,
        activity: crate::agent_model::Activity,
    },
    PiUi {
        run: String,
        change: crate::agent::ui::Change,
    },
    CodexInput {
        run: String,
        key: metldr_codex_session::InputId,
        prompt: metldr_codex_session::InputPrompt,
        answer: SyncSender<metldr_codex_session::InputAnswer>,
    },
    CodexInputWithdraw {
        run: String,
        key: metldr_codex_session::InputId,
    },
    PiSettled {
        run: String,
        result: crate::agent::Outcome,
        released: bool,
    },
    CodexBound {
        run: String,
        workflow: String,
        identity: crate::codex_model::Identity,
        selection: crate::codex_model::Selection,
        ack: std::sync::mpsc::SyncSender<std::result::Result<bool, String>>,
    },
    CodexActivity {
        run: String,
        activity: crate::agent_model::Activity,
    },
    CodexSettled {
        run: String,
        result: crate::agent::Outcome,
        released: bool,
    },
    OpenCodeBound {
        run: String,
        workflow: String,
        identity: crate::opencode_model::Identity,
        ack: std::sync::mpsc::SyncSender<std::result::Result<bool, String>>,
    },
    NativePermission {
        run: String,
        key: u64,
        details: serde_json::Value,
        ack: std::sync::mpsc::SyncSender<Option<String>>,
    },
    NativePermissionResolved {
        run: String,
        key: u64,
    },
    OpenCodeActivity {
        run: String,
        activity: crate::agent_model::Activity,
    },
    OpenCodeSettled {
        run: String,
        result: crate::agent::Outcome,
        released: bool,
    },
}
pub(super) struct PendingPermission {
    pub run: String,
    pub key: u64,
    pub details: serde_json::Value,
    pub ack: std::sync::mpsc::SyncSender<Option<String>>,
}
pub(super) struct Operation {
    pub(super) stop: Arc<AtomicBool>,
    pub(super) worker: JoinHandle<()>,
    pub(super) correction: Option<crate::native_correction::Slot>,
    pub(super) browser_io: Option<Box<crate::browser_io::Slot>>,
}
#[derive(Clone)]
pub(super) struct Subscription {
    pub(super) scope: crate::history::Selection,
    pub(super) run: String,
}

pub(super) struct Owner {
    pub(super) transport: crate::transport::Transport,
    pub(super) disk: Disk,
    pub(super) record_history: crate::history::History,
    pub(super) history_pins: crate::history_io::Pins,
    pub(super) settings: crate::provider_settings::Runtime,
    pub(super) agent_runtime: crate::agent_settings::Runtime,
    pub(super) state: Store,
    pub(super) browser_view: BTreeMap<String, crate::browser_action::Action>,
    pub(super) fixture: Option<crate::native::Fixture>,
    pub(super) subscribers: BTreeMap<String, Subscription>,
    pub(super) operations: BTreeMap<String, Operation>,
    pub(super) tx: SyncSender<Event>,
    pub(super) pi_profile: Option<crate::agent_model::Profile>,
    pub(super) codex_profile: Option<crate::codex_model::Profile>,
    pub(super) opencode_profile: Option<crate::opencode_model::Profile>,
    pub(super) native_permissions: BTreeMap<String, PendingPermission>,
    pub(super) pi_ui: BTreeMap<String, crate::pi_ui::State>,
    pub(super) codex_input: BTreeMap<String, crate::codex_input::State>,
    pub(super) browser_approval: Option<crate::browser_owner::PendingApproval>,
    pub(super) browser_route: Option<crate::https_profile::Profile>,
    pub(super) responses_route: Option<crate::https_profile::Profile>,
    pub(super) google_route: Option<crate::https_profile::Profile>,
    pub(super) chat_route: Option<crate::https_profile::Profile>,
}

impl Owner {
    pub(super) fn emit(&mut self, value: serde_json::Value) -> Result<()> {
        self.transport.emit(value)
    }
    pub(super) fn reply_view(
        &mut self,
        request: Option<&str>,
        kind: &str,
        ended: bool,
    ) -> Result<()> {
        let (history, view) = self.view_parts();
        let selection = history.selection()?;
        let choices = history.native_choices(
            &selection,
            view.state.view.source.as_ref(),
            &view.native_fingerprints()?,
        )?;
        let reply = view.reply(
            view.state,
            request,
            kind,
            ended,
            view.browser_actions,
            &selection,
            &choices,
        )?;
        self.emit(reply)
    }
    fn reply_watch(&mut self, request: &str, subscription: &Subscription) -> Result<bool> {
        self.require_conversation(&subscription.scope)?;
        if let Some(run) = self
            .state
            .view
            .runs
            .iter()
            .find(|run| run.id == subscription.run)
        {
            let ended = run.status.terminal();
            self.reply_view(Some(request), "view", ended)?;
            return Ok(ended);
        }
        let (history, view) = self.view_parts();
        let profiles = view.native_fingerprints()?;
        let (reply, ended) = history.edit(|edit| {
            let state = edit.project(&subscription.scope, Some(&subscription.run))?;
            let ended = state
                .view
                .runs
                .iter()
                .find(|run| run.id == subscription.run)
                .ok_or("watched run is absent")?
                .status
                .terminal();
            let runs: Vec<_> = state.view.runs.iter().map(|run| run.id.as_str()).collect();
            let actions = edit.browser_view(&subscription.scope, &runs)?;
            let choices =
                edit.native_choices(&subscription.scope, state.view.source.as_ref(), &profiles)?;
            Ok((
                view.reply(
                    &state,
                    Some(request),
                    "view",
                    ended,
                    &actions,
                    &subscription.scope,
                    &choices,
                )?,
                ended,
            ))
        })?;
        self.emit(reply)?;
        Ok(ended)
    }
    pub(super) fn publish(&mut self) -> Result<()> {
        self.reply_view(None, "changed", false)?;
        let subscriptions: Vec<_> = self
            .subscribers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (request, subscription) in subscriptions {
            match self.reply_watch(&request, &subscription) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(error) if fatal(error.as_ref()) => return Err(error),
                Err(_) => {
                    self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"error","message":"watched run is unavailable"}))?;
                }
            }
            self.subscribers.remove(&request);
        }
        self.prune_pi_ui();
        Ok(())
    }
    pub(super) fn request(&mut self, request: Request) -> Result<()> {
        let id = request.request;
        let result = self.command(&id, request.command);
        if let Err(e) = result {
            if fatal(e.as_ref()) {
                return Err(e);
            }
            if let Some(error) = e.downcast_ref::<crate::history_io::ReadError>() {
                return self.emit(match error {
                    crate::history_io::ReadError::Busy => json!({"v":crate::WIRE_VERSION,"request":id,"kind":"history_busy"}),
                    crate::history_io::ReadError::Stale => json!({"v":crate::WIRE_VERSION,"request":id,"kind":"history_stale","observation":null}),
                });
            }
            if e.downcast_ref::<crate::history::ConversationBusy>()
                .is_some()
            {
                return self.emit(
                    json!({"v":crate::WIRE_VERSION,"request":id,"kind":"conversation_busy"}),
                );
            }
            self.emit(
                json!({"v":crate::WIRE_VERSION,"request":id,"kind":"error","message":e.to_string()}),
            )?;
        }
        Ok(())
    }
    pub(super) fn command(&mut self, id: &str, command: Command) -> Result<()> {
        match command {
            Command::HistoryCurrent => self.history_current(id),
            Command::ConversationChange { scope, change } => {
                self.conversation_change(id, scope, change)
            }
            Command::HistoryConversations {
                scope,
                generation,
                cursor,
            } => self.history_conversations(id, scope, &generation, cursor),
            Command::HistoryRuns {
                scope,
                generation,
                cursor,
            } => self.history_runs(id, scope, &generation, cursor),
            Command::HistoryPin {
                scope,
                generation,
                kind,
                document,
                revision,
            } => self.history_pin(id, scope, &generation, kind, &document, revision),
            Command::HistoryPage { identity, offset } => self.history_page(id, identity, offset),
            Command::HistoryRelease { identity } => self.history_release(id, identity),
            Command::ProviderSave {
                protocol,
                expected_revision,
                endpoint,
                model,
                key,
            } => self.provider_save(id, protocol, expected_revision, endpoint, model, key),
            Command::ProviderRemove {
                protocol,
                expected_revision,
            } => self.provider_remove(id, protocol, expected_revision),
            Command::ProviderRefresh => self.provider_load(Some(id.into())),
            Command::AgentSave {
                expected_revision,
                profile,
            } => self.agent_save(id, expected_revision, profile),
            Command::AgentRemove {
                agent,
                expected_revision,
            } => self.agent_remove(id, agent, expected_revision),
            Command::AgentRefresh => self.agent_refresh(id),
            Command::BrowserReply { call, run, result } => {
                self.browser_io_reply(&run, call, result);
                Ok(())
            }
            Command::BrowserEvent {
                run,
                tab,
                session,
                method,
                params,
            } => {
                self.browser_io_event(&run, tab, session, method, params);
                Ok(())
            }
            Command::View => self.reply_view(Some(id), "view", true),
            Command::Conversation { scope, action } => {
                scope.validate()?;
                let expected = crate::history::Selection {
                    store: scope.store,
                    conversation: scope.conversation,
                    epoch: scope.epoch.0,
                };
                self.conversation_action(id, &expected, action)
            }
        }
    }

    fn conversation_action(
        &mut self,
        id: &str,
        scope: &crate::history::Selection,
        action: ConversationAction,
    ) -> Result<()> {
        if self.replay_start(id, scope, &action)? {
            return Ok(());
        }
        match action {
            ConversationAction::NativeCorrection { run, text } => {
                self.require_conversation(scope)?;
                self.correct_native(id, scope, &run, &text)
            }
            ConversationAction::Capture {
                expected_source,
                page,
            } => {
                if !self.conversation_idle() {
                    return Err(crate::history::ConversationBusy.into());
                }
                let run = self.state.view.runs.last().map(|run| run.id.clone());
                self.record_change(|edit, _| {
                    edit.capture(scope, expected_source.as_deref(), page)?;
                    Ok(((), Some(edit.project(scope, run.as_deref())?)))
                })?;
                self.reply_view(Some(id), "view", true)
            }
            ConversationAction::PageText {
                source,
                protocol,
                fingerprint,
                prompt,
            } => self.page_text_start(id, scope, &source, protocol, &fingerprint, prompt),
            ConversationAction::BrowserTask {
                source,
                text,
                fingerprint,
            } => self.browser_start(id, scope, &source, &text, fingerprint.as_deref()),
            ConversationAction::GoogleTask {
                source,
                text,
                fingerprint,
            } => self.google_start(id, scope, &source, &text, &fingerprint),
            ConversationAction::ResponsesTask {
                source,
                text,
                fingerprint,
            } => self.responses_start(id, scope, &source, &text, &fingerprint),
            ConversationAction::ChatTask {
                source,
                text,
                fingerprint,
            } => self.chat_start(id, scope, &source, &text, &fingerprint),
            ConversationAction::BrowserDecision {
                action,
                revision,
                approve,
            } => {
                self.require_conversation(scope)?;
                self.record_history
                    .edit(|edit| edit.current_action_execution(scope, &action).map(|_| ()))?;
                self.browser_decide(&action, revision, approve)?;
                self.reply_view(Some(id), "view", true)
            }
            command @ (ConversationAction::PiPrompt { .. }
            | ConversationAction::PiFollowUp { .. }) => self.pi_start(id, scope, command),
            command @ (ConversationAction::CodexPrompt { .. }
            | ConversationAction::CodexFollowUp { .. }) => self.codex_start(id, scope, command),
            command @ (ConversationAction::OpenCodePrompt { .. }
            | ConversationAction::OpenCodeFollowUp { .. }) => {
                self.opencode_start(id, scope, command)
            }
            ConversationAction::NativePermission {
                run,
                request,
                option,
            } => {
                self.require_current_run(scope, &run)?;
                self.native_decide(id, &run, &request, option.as_deref())
            }
            ConversationAction::PiUiReply {
                run,
                request,
                answer,
            } => {
                self.require_current_run(scope, &run)?;
                self.pi_ui_reply(id, &run, &request, answer)
            }
            ConversationAction::CodexInputReply {
                run,
                request,
                answer,
            } => {
                self.require_current_run(scope, &run)?;
                self.codex_input_reply(id, &run, &request, answer)
            }
            ConversationAction::Watch { run } => {
                self.require_conversation(scope)?;
                self.record_change(|edit, _| Ok(((), Some(edit.project(scope, Some(&run))?))))?;
                let status = self
                    .state
                    .view
                    .runs
                    .iter()
                    .find(|r| r.id == run)
                    .ok_or("unknown run")?
                    .status;
                if !status.terminal() {
                    if self.subscribers.len() >= 16 {
                        return Err("watch capacity reached".into());
                    }
                    self.subscribers.insert(
                        id.to_owned(),
                        Subscription {
                            scope: scope.clone(),
                            run,
                        },
                    );
                }
                self.reply_view(Some(id), "view", status.terminal())
            }
            ConversationAction::Unwatch { run, watch } => {
                self.require_conversation(scope)?;
                if !valid_request_id(&watch) {
                    return Err("invalid watch identity".into());
                }
                if self.subscribers.get(&watch).is_some_and(|subscription| {
                    subscription.scope == *scope && subscription.run == run
                }) {
                    self.subscribers.remove(&watch);
                }
                self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"unwatched"}))
            }
            ConversationAction::Stop { run } => {
                self.require_current_run(scope, &run)?;
                self.stop_run(id, &run)
            }
            start @ (ConversationAction::Summarize { .. } | ConversationAction::Ask { .. }) => {
                self.fixture_start(id, scope, start)
            }
        }
    }

    pub(super) fn require_conversation(&self, expected: &crate::history::Selection) -> Result<()> {
        let current = self.record_history.selection()?;
        if current != *expected {
            return Err("history selection changed".into());
        }
        Ok(())
    }

    fn require_current_run(
        &mut self,
        expected: &crate::history::Selection,
        run: &str,
    ) -> Result<()> {
        self.require_conversation(expected)?;
        self.record_history
            .edit(|edit| edit.current_execution(expected, run).map(|_| ()))?;
        Ok(())
    }

    pub(super) fn started_reply(
        &self,
        id: &str,
        scope: &crate::history::Selection,
        run: &str,
    ) -> Result<serde_json::Value> {
        let scope = crate::history_io::Scope::from(scope);
        scope.validate()?;
        Ok(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"started","scope":scope,"run":run}))
    }

    pub(super) fn reply_stop(&mut self, id: &str, kind: &str, run: &str) -> Result<()> {
        let scope = self.record_history.selection()?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":kind,"scope":crate::history_io::Scope::from(&scope),"run":run}))
    }

    pub(super) fn text(&mut self, run: &str, change: Change) -> Result<()> {
        if matches!(change, Change::Artifact(_)) {
            return Err("artifact references require a committed table record".into());
        }
        self.record_execution(run, |edit, execution| {
            let mut record = edit.run(execution, run)?;
            if record.status.terminal() || !record.output.change(change)? {
                return Ok(());
            }
            edit.put_run(execution, record)?;
            Ok(())
        })
    }
    pub(super) fn settled(
        &mut self,
        run: &str,
        result: std::result::Result<(), String>,
        disconnected: bool,
    ) -> Result<()> {
        let mut operation = self.operations.remove(run).ok_or_else(|| {
            if self.operations.is_empty() {
                "settlement without operation"
            } else {
                "settlement run mismatch"
            }
        })?;
        drop(operation.browser_io.take());
        operation
            .worker
            .join()
            .map_err(|_| "provider worker panicked")?;
        self.record_execution(run, |edit, execution| {
            edit.finish_browser(execution, run)?;
            let mut record = edit.run(execution, run)?;
            record.status = if result.is_ok() {
                Status::Complete
            } else if disconnected {
                Status::Interrupted
            } else if record.status == Status::StopRequested {
                Status::Cancelled
            } else {
                Status::Failed
            };
            if record.status == Status::Failed {
                record.error = Some(result.err().unwrap_or_default().chars().take(200).collect());
            }
            edit.put_run(execution, record)?;
            Ok(())
        })?;
        self.cancel_browser_approval(run);
        Ok(())
    }
}
