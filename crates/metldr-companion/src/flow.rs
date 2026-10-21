use crate::Result;
use crate::model::{Command, Request, Run, Source, Status, Store, check_page_fields};
use crate::provider::provider;
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
    thread::{self, JoinHandle},
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
pub(super) struct Owner {
    pub(super) transport: crate::transport::Transport,
    pub(super) disk: Disk,
    pub(super) settings: crate::provider_settings::Runtime,
    pub(super) agent_runtime: crate::agent_settings::Runtime,
    pub(super) state: Store,
    pub(super) fixture: Option<crate::native::Fixture>,
    pub(super) subscribers: BTreeMap<String, String>,
    pub(super) operation: Option<Operation>,
    pub(super) tx: SyncSender<Event>,
    pub(super) pi_profile: Option<crate::agent_model::Profile>,
    pub(super) codex_profile: Option<crate::codex_model::Profile>,
    pub(super) opencode_profile: Option<crate::opencode_model::Profile>,
    pub(super) native_permissions: BTreeMap<String, PendingPermission>,
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
        let mut view = crate::public_view::render(
            state,
            self.pi_profile.as_ref(),
            self.codex_profile.as_ref(),
            self.opencode_profile.as_ref(),
            &self.native_permissions,
            self.browser_route.as_ref(),
            self.responses_route.as_ref(),
            self.google_route.as_ref(),
            self.chat_route.as_ref(),
            self.fixture.is_some(),
        )?;
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
    pub(super) fn publish(&mut self) -> Result<()> {
        self.reply_view(None, "changed", false)?;
        let subscriptions: Vec<_> = self
            .subscribers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (request, run) in subscriptions {
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
            if e.downcast_ref::<io::Error>().is_some() {
                return Err(e);
            }
            self.emit(
                json!({"v":crate::WIRE_VERSION,"request":id,"kind":"error","message":e.to_string()}),
            )?;
        }
        Ok(())
    }
    pub(super) fn command(&mut self, id: &str, command: Command) -> Result<()> {
        match command {
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
            Command::BrowserTask {
                source,
                text,
                fingerprint,
            } => self.browser_start(id, &source, &text, fingerprint.as_deref()),
            Command::GoogleTask {
                source,
                text,
                fingerprint,
            } => self.google_start(id, &source, &text, &fingerprint),
            Command::ResponsesTask {
                source,
                text,
                fingerprint,
            } => self.responses_start(id, &source, &text, &fingerprint),
            Command::ChatTask {
                source,
                text,
                fingerprint,
            } => self.chat_start(id, &source, &text, &fingerprint),
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
            Command::BrowserDecision {
                action,
                revision,
                approve,
            } => {
                self.browser_decide(&action, revision, approve)?;
                self.reply_view(Some(id), "view", true)
            }
            command @ (Command::PiPrompt { .. } | Command::PiFollowUp { .. }) => {
                self.pi_start(id, command)
            }
            command @ (Command::CodexPrompt { .. } | Command::CodexFollowUp { .. }) => {
                self.codex_start(id, command)
            }
            command @ (Command::OpenCodePrompt { .. } | Command::OpenCodeFollowUp { .. }) => {
                self.opencode_start(id, command)
            }
            Command::NativePermission {
                run,
                request,
                option,
            } => self.native_decide(id, &run, &request, option.as_deref()),
            Command::View => self.reply_view(Some(id), "view", true),
            Command::Capture {
                expected_store,
                expected_source,
                page,
            } => {
                if expected_store != self.state.view.store {
                    return Err("store is not current".into());
                }
                crate::model::check_page_fields(&page)?;
                if self.operation.is_some() {
                    return Err("another run is active".into());
                }
                if let Some(source) = &self.state.view.source
                    && source.page == page
                {
                    return self.reply_view(Some(id), "view", true);
                }
                if self
                    .state
                    .view
                    .source
                    .as_ref()
                    .map(|source| source.id.as_str())
                    != expected_source.as_deref()
                {
                    return Err("source is not current saved source".into());
                }
                let mut next = self.state.clone();
                let source = Source {
                    id: next.id()?,
                    page,
                };
                next.view.source = Some(source.clone());
                self.commit(next)?;
                self.reply_view(Some(id), "view", true)
            }
            Command::Watch { run } => {
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
                    self.subscribers.insert(id.to_owned(), run);
                }
                self.reply_view(Some(id), "view", status.terminal())
            }
            Command::Stop { run } => {
                let position = self
                    .state
                    .view
                    .runs
                    .iter()
                    .position(|r| r.id == run)
                    .ok_or("unknown run")?;
                if self.state.view.runs[position].status.terminal() {
                    return self.emit(
                        json!({"v":crate::WIRE_VERSION,"request":id,"kind":"already_terminal","run":run}),
                    );
                }
                let mut next = self.state.clone();
                next.view.runs[position].status = Status::StopRequested;
                next.prevent_browser(&run, "user stopped before dispatch")?;
                let permissions = self.take_native_permissions(&run);
                if let Err(error) = self.commit(next) {
                    self.native_permissions.extend(permissions);
                    return Err(error);
                }
                if let Some(operation) = &self.operation {
                    if operation.run == run {
                        operation.stop.store(true, Ordering::Release);
                    }
                }
                self.cancel_browser_approval(&run);
                for (_, permission) in permissions {
                    let _ = permission.ack.send(None);
                }
                self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"stopping","run":run}))
            }
            start @ (Command::Summarize { .. } | Command::Ask { .. }) => {
                let fixture = self.fixture.as_ref().ok_or("fixture is not configured")?;
                let origin = fixture.origin.clone();
                let address = fixture.address;
                let input_digest = crate::agent_model::digest(
                    &json!({"command":&start,"origin":origin,"model":"metldr-fixture"}),
                )?;
                if self.duplicate(id, &input_digest)? {
                    return Ok(());
                }
                if self.operation.is_some() {
                    return Err("another run is active".into());
                }
                if self.state.view.runs.len() >= 16 {
                    return Err("prototype run capacity reached".into());
                }
                let mut next = self.state.clone();
                let (kind, question) = match start {
                    Command::Summarize { page } => {
                        check_page_fields(&page)?;
                        if page.url != format!("{origin}/article")
                            && page.url != format!("{origin}/decoy")
                        {
                            return Err("page is outside the owned fixture grant".into());
                        }
                        let source_id = next.id()?;
                        next.view.source = Some(Source {
                            id: source_id,
                            page,
                        });
                        ("summary", None)
                    }
                    Command::Ask { source, text } => {
                        if text.trim().is_empty() || text.len() > 1024 {
                            return Err("invalid question".into());
                        }
                        if next.view.source.as_ref().is_none_or(|s| s.id != source) {
                            return Err("source is not current saved source".into());
                        }
                        ("answer", Some(text))
                    }
                    _ => unreachable!(),
                };
                let source = next
                    .view
                    .source
                    .as_ref()
                    .ok_or("no selected source")?
                    .clone();
                let history: Vec<_> = next
                    .view
                    .runs
                    .iter()
                    .filter(|r| {
                        r.source == source.id
                            && r.status == Status::Complete
                            && matches!(
                                next.launches.get(&r.id),
                                Some(crate::agent_model::Launch::Fixture { .. })
                            )
                    })
                    .map(|r| json!({"kind":r.kind,"question":r.question,"output":r.output.text()}))
                    .collect();
                let run = next.id()?;
                let body = json!({"model":"metldr-fixture","instruction":if kind == "summary" {"summarize"} else {"answer"},
                    "source":{"title":source.page.title,"url":source.page.url,"text":source.page.text},"question":question,"history":history});
                next.view.runs.push(Run {
                    id: run.clone(),
                    source: source.id,
                    kind: kind.into(),
                    question,
                    status: Status::Running,
                    output: Default::default(),
                    error: None,
                });
                next.starts.insert(id.into(), run.clone());
                next.launches.insert(
                    run.clone(),
                    crate::agent_model::Launch::Fixture { input_digest },
                );
                self.commit(next)?;
                let stop = Arc::new(AtomicBool::new(false));
                let worker_stop = stop.clone();
                let worker_run = run.clone();
                let tx = self.tx.clone();
                let worker = thread::spawn(move || {
                    let result = provider(address, &body, &worker_run, &worker_stop, &tx)
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Event::Settled {
                        run: worker_run,
                        result,
                    });
                });
                self.operation = Some(Operation {
                    run: run.clone(),
                    stop,
                    worker,
                });
                self.emit(json!({"v":crate::WIRE_VERSION,"request":id,"kind":"started","run":run}))
            }
        }
    }
    pub(super) fn text(&mut self, run: &str, change: Change) -> Result<()> {
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
