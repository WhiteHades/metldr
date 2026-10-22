use crate::Result;
use crate::model::{Command, ConversationAction, Request, Status, Store};
use crate::store::Disk;
use metldr_conversation::Change;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread::JoinHandle,
};

pub(super) enum Event {
    Provider(crate::provider_settings::Completion),
    BrowserIo(crate::browser_io::Call),
    Browser(crate::browser_owner::Event),
    Input(crate::transport::Input),
    Text {
        run: String,
        change: Change,
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
    pub(super) run: String,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) worker: JoinHandle<()>,
}
#[derive(Clone)]
pub(super) struct Subscription {
    pub(super) scope: crate::history::Selection,
    pub(super) run: String,
}

pub(super) struct Owner {
    pub(super) transport: crate::transport::Transport,
    pub(super) disk: Disk,
    pub(super) record_history: Option<crate::history::History>,
    pub(super) history_pins: crate::history_io::Pins,
    pub(super) settings: crate::provider_settings::Runtime,
    pub(super) agent_runtime: crate::agent_settings::Runtime,
    pub(super) state: Store,
    pub(super) browser_view: BTreeMap<String, crate::browser_action::Action>,
    pub(super) fixture: Option<crate::native::Fixture>,
    pub(super) subscribers: BTreeMap<String, Subscription>,
    pub(super) operation: Option<Operation>,
    pub(super) tx: SyncSender<Event>,
    pub(super) pi_profile: Option<crate::agent_model::Profile>,
    pub(super) codex_profile: Option<crate::codex_model::Profile>,
    pub(super) opencode_profile: Option<crate::opencode_model::Profile>,
    pub(super) native_permissions: BTreeMap<String, PendingPermission>,
    pub(super) pi_ui: Option<crate::pi_ui::State>,
    pub(super) codex_input: Option<crate::codex_input::State>,
    pub(super) browser_approval: Option<crate::browser_owner::PendingApproval>,
    pub(super) browser_io: Option<crate::browser_io::Slot>,
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
        let reply = self.view_reply(&self.state, request, kind, ended)?;
        self.emit(reply)
    }
    fn view_reply(
        &self,
        state: &Store,
        request: Option<&str>,
        kind: &str,
        ended: bool,
    ) -> Result<serde_json::Value> {
        let browser_actions = if self.record_history.is_some() {
            &self.browser_view
        } else {
            &state.browser_actions
        };
        let selection = self
            .record_history
            .as_ref()
            .map(|history| history.selection())
            .transpose()?;
        self.view_reply_with_browser_actions(
            state,
            request,
            kind,
            ended,
            Some(browser_actions),
            selection.as_ref(),
        )
    }
    fn view_reply_with_browser_actions(
        &self,
        state: &Store,
        request: Option<&str>,
        kind: &str,
        ended: bool,
        browser_actions: Option<&BTreeMap<String, crate::browser_action::Action>>,
        selection: Option<&crate::history::Selection>,
    ) -> Result<serde_json::Value> {
        let mut view = crate::public_view::render(
            state,
            self.pi_profile.as_ref(),
            self.codex_profile.as_ref(),
            self.opencode_profile.as_ref(),
            &self.native_permissions,
            self.pi_ui.as_ref(),
            self.codex_input.as_ref(),
            self.browser_route.as_ref(),
            self.responses_route.as_ref(),
            self.google_route.as_ref(),
            self.chat_route.as_ref(),
            self.fixture.is_some(),
        )?;
        if let Some(actions) = browser_actions {
            if actions.is_empty() {
                view.as_object_mut()
                    .ok_or("invalid public view")?
                    .remove("browser_actions");
            } else {
                view["browser_actions"] = crate::public_view::browser_actions(actions);
            }
        }
        view["selection"] = serde_json::to_value(selection.map(crate::history_io::Scope::from))?;
        view["provider_settings"] = self.settings.public(&state.providers);
        view["agent_settings"] = self.agent_runtime.public(
            &state.agent_settings,
            self.operation.is_some() || self.settings.job.is_some(),
        );
        let mut reply = json!({"v":crate::WIRE_VERSION,"request":request,"kind":kind,"view":view,"ended":ended});
        crate::tool_report::fit_wire(&mut reply)?;
        Ok(reply)
    }
    pub(super) fn commit(&mut self, next: Store) -> Result<()> {
        self.persist(next)?;
        self.publish()
    }
    pub(super) fn persist(&mut self, mut next: Store) -> Result<()> {
        if self.record_history.is_some() {
            return Err(
                io::Error::other("snapshot persistence cannot write record history").into(),
            );
        }
        next.view.sequence = self
            .state
            .view
            .sequence
            .checked_add(1)
            .ok_or("view sequence capacity reached")?;
        next.seal_outputs()?;
        crate::tool_report::fit_store(&mut next)?;
        next.validate()?;
        self.preflight(&next)?;
        self.disk.save(&next)?;
        self.state = next;
        Ok(())
    }
    pub(super) fn preflight(&self, state: &Store) -> Result<()> {
        // Required message metadata must fit even with a maximum length request ID.
        self.view_reply(state, Some(&"x".repeat(96)), "view", false)?;
        Ok(())
    }
    pub(super) fn preflight_records(
        &self,
        state: &Store,
        browser_actions: &BTreeMap<String, crate::browser_action::Action>,
        selection: &crate::history::Selection,
    ) -> Result<()> {
        self.view_reply_with_browser_actions(
            state,
            Some(&"x".repeat(96)),
            "view",
            false,
            Some(browser_actions),
            Some(selection),
        )?;
        Ok(())
    }
    pub(super) fn publish(&mut self) -> Result<()> {
        self.reply_view(None, "changed", false)?;
        let subscriptions: Vec<_> = self
            .subscribers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (request, subscription) in subscriptions {
            self.require_conversation(&subscription.scope)?;
            let run = subscription.run;
            let ended = self
                .state
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .is_some_and(|r| r.status.terminal());
            self.reply_view(Some(&request), "view", ended)?;
            if ended {
                self.subscribers.remove(&request);
            }
        }
        Ok(())
    }
    pub(super) fn request(&mut self, request: Request) -> Result<()> {
        let id = request.request;
        let result = self.command(&id, request.command);
        if let Err(e) = result {
            // Storage and transport failures are fatal; never continue after a failed commit.
            if e.downcast_ref::<io::Error>().is_some() || crate::history::is_failure(e.as_ref()) {
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
                if self.record_history.is_none() {
                    return Err("record history is unavailable".into());
                }
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
                self.require_conversation(scope)?;
                self.native_decide(id, &run, &request, option.as_deref())
            }
            ConversationAction::PiUiReply {
                run,
                request,
                answer,
            } => {
                self.require_conversation(scope)?;
                self.pi_ui_reply(id, &run, &request, answer)
            }
            ConversationAction::CodexInputReply {
                run,
                request,
                answer,
            } => {
                self.require_conversation(scope)?;
                self.codex_input_reply(id, &run, &request, answer)
            }
            ConversationAction::Watch { run } => {
                self.require_conversation(scope)?;
                if self.record_history.is_some() {
                    self.record_change(|edit, _| Ok(((), Some(edit.project(scope, Some(&run))?))))?;
                }
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
            ConversationAction::Stop { run } => {
                self.require_conversation(scope)?;
                self.stop_run(id, &run)
            }
            start @ (ConversationAction::Summarize { .. } | ConversationAction::Ask { .. }) => {
                self.fixture_start(id, scope, start)
            }
        }
    }

    pub(super) fn require_conversation(&self, expected: &crate::history::Selection) -> Result<()> {
        let current = self
            .record_history
            .as_ref()
            .ok_or("record history is unavailable")?
            .selection()?;
        if current != *expected {
            return Err("history selection changed".into());
        }
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
        let scope = self
            .record_history
            .as_ref()
            .map(|history| history.selection())
            .transpose()?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":kind,"scope":scope.as_ref().map(crate::history_io::Scope::from),"run":run}))
    }

    pub(super) fn stop_run(&mut self, id: &str, run: &str) -> Result<()> {
        if self.record_history.is_some() {
            return self.stop_record(id, run);
        }
        let position = self
            .state
            .view
            .runs
            .iter()
            .position(|r| r.id == run)
            .ok_or("unknown run")?;
        if self.state.view.runs[position].status.terminal() {
            return self.reply_stop(id, "already_terminal", run);
        }
        let mut next = self.state.clone();
        next.view.runs[position].status = Status::StopRequested;
        next.prevent_browser(run, "user stopped before dispatch")?;
        let permissions = self.take_native_permissions(run);
        let pi_ui = self.pi_ui.clone();
        let codex_input = self.codex_input.clone();
        self.clear_pi_ui(run);
        self.clear_codex_input(run);
        if let Err(error) = self.commit(next) {
            self.native_permissions.extend(permissions);
            self.pi_ui = pi_ui;
            self.codex_input = codex_input;
            return Err(error);
        }
        if let Some(operation) = &self.operation {
            if operation.run == run {
                operation.stop.store(true, Ordering::Release);
            }
        }
        self.cancel_browser_approval(run);
        drop(pi_ui);
        drop(codex_input);
        for (_, permission) in permissions {
            let _ = permission.ack.send(None);
        }
        self.reply_stop(id, "stopping", run)
    }

    pub(super) fn text(&mut self, run: &str, change: Change) -> Result<()> {
        if self.record_history.is_some() {
            self.record_change(|edit, selection| {
                let mut record = edit.run(selection, run)?;
                if record.status.terminal() || !record.output.change(change)? {
                    return Ok(((), None));
                }
                edit.put_run(selection, record)?;
                Ok(((), Some(edit.project(selection, Some(run))?)))
            })?;
            return Ok(());
        }
        let mut next = self.state.clone();
        let Some(record) = next.view.runs.iter_mut().find(|r| r.id == run) else {
            return Err("provider returned unknown run".into());
        };
        if record.status.terminal() {
            return Ok(());
        }
        if !record.output.change(change)? {
            return Ok(());
        }
        self.commit(next)
    }
    pub(super) fn settled(
        &mut self,
        run: &str,
        result: std::result::Result<(), String>,
        disconnected: bool,
    ) -> Result<()> {
        let operation = self
            .operation
            .take()
            .ok_or("settlement without operation")?;
        if operation.run != run {
            return Err("settlement run mismatch".into());
        }
        self.browser_io_finish(run);
        operation
            .worker
            .join()
            .map_err(|_| "provider worker panicked")?;
        if self.record_history.is_some() {
            self.record_change(|edit, selection| {
                edit.finish_browser(selection, run)?;
                let mut record = edit.run(selection, run)?;
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
                    record.error =
                        Some(result.err().unwrap_or_default().chars().take(200).collect());
                }
                edit.put_run(selection, record)?;
                Ok(((), Some(edit.project(selection, Some(run))?)))
            })?;
            self.cancel_browser_approval(run);
            return Ok(());
        }
        let mut next = self.state.clone();
        next.finish_browser(run)?;
        self.cancel_browser_approval(run);
        let record = next
            .view
            .runs
            .iter_mut()
            .find(|r| r.id == run)
            .ok_or("settlement without run")?;
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
        self.commit(next)
    }
}
