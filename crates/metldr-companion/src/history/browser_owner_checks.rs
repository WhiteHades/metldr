use super::History;
use crate::{
    agent::Outcome as AgentOutcome,
    agent_model::{Launch, digest},
    browser_action::{Binding, Intent, Outcome, Phase, Proposal},
    browser_owner::Event,
    codex_model::{self, Workflow},
    flow::{Operation, Owner},
    model::{Command, Page, Request, Run, Source, Status},
    store::Disk,
    transport::Transport,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> crate::Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "browser-owner-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_owner(directory: &Directory) -> crate::Result<(Owner, String)> {
    let (disk, mut state) = Disk::open(directory.0.clone())?;
    let source_id = state.id()?;
    let run = state.id()?;
    let workflow_id = state.id()?;
    let source = Source {
        id: source_id.clone(),
        page: Page {
            tab: 17,
            document: "document-1".into(),
            url: "http://127.0.0.1:4567/article".into(),
            title: "Owned page".into(),
            text: "Saved source text".into(),
        },
    };
    state.view.source = Some(source.clone());
    state.view.runs.push(Run {
        id: run.clone(),
        source: source_id,
        kind: "agent".into(),
        question: Some("browser lifecycle acceptance".into()),
        status: Status::Running,
        output: Default::default(),
        error: None,
        corrections: Vec::new(),
    });
    let profile = codex_model::Profile {
        executable: PathBuf::from("/usr/bin/true"),
        workspace: directory.0.join("workspace"),
    };
    let workflow = Workflow {
        fingerprint: digest(&profile)?,
        profile,
        source,
        revision: 1,
        last_run: run.clone(),
        binding: codex_model::Binding::Bound {
            identity: codex_model::Identity {
                thread: "thread-fixture-1".into(),
                session: "codex-session-fixture".into(),
            },
        },
    };
    state.codex_workflows.insert(workflow_id.clone(), workflow);
    state.launches.insert(
        run.clone(),
        Launch::Codex {
            input_digest: "a".repeat(64),
            workflow: workflow_id,
            selection: None,
            activity: vec![],
        },
    );
    disk.save(&state)?;
    let original = fs::read(directory.0.join("snapshot.json"))?;
    let mut history =
        History::import_new(&directory.0.join("history.redb"), &original, disk.lease())?;
    let selection = history.selection()?;
    let state = history.edit(|edit| edit.project(&selection, Some(&run)))?;
    let (transport, _) = Transport::captured();
    let (tx, _) = mpsc::sync_channel(16);
    Ok((
        Owner {
            transport,
            disk,
            state,
            browser_view: Default::default(),
            record_history: Some(history),
            history_pins: Default::default(),
            settings: Default::default(),
            agent_runtime: Default::default(),
            fixture: None,
            subscribers: BTreeMap::new(),
            operation: Some(Operation {
                run: run.clone(),
                stop: Arc::new(AtomicBool::new(false)),
                worker: std::thread::spawn(|| {}),
                correction: None,
            }),
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: BTreeMap::new(),
            pi_ui: None,
            codex_input: None,
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
        },
        run,
    ))
}

fn proposal(run: &str, tool: &str, binding: &Binding, url: &str, intent: Intent) -> Proposal {
    Proposal {
        run: run.into(),
        tool_use_id: tool.into(),
        binding: binding.clone(),
        url: url.into(),
        intent,
    }
}

fn field() -> Intent {
    Intent::Field {
        field_id: "owned-field".into(),
        field_label: "Email".into(),
        previous_value: "old@example.invalid".into(),
        value: "new@example.invalid".into(),
    }
}

fn approve_dispatch_settle(
    owner: &mut Owner,
    proposal: Proposal,
    outcome: Outcome,
) -> crate::Result<crate::browser_action::Action> {
    let (ack, reply) = mpsc::sync_channel(1);
    owner.browser_event(Event::Proposed {
        proposal,
        ack,
        budget: None,
    })?;
    let action = owner.browser_approval.as_ref().unwrap().action.clone();
    owner.browser_decide(&action, 1, true)?;
    let approved = reply.recv()?.map_err(std::io::Error::other)?;
    let (ack, reply) = mpsc::sync_channel(1);
    owner.browser_event(Event::Dispatch {
        action: action.clone(),
        revision: approved.revision,
        ack,
    })?;
    let dispatched = reply.recv()?.map_err(std::io::Error::other)?;
    if let Intent::Navigate { destination } = &dispatched.proposal.intent {
        let (slot, _events) = crate::browser_io::Slot::new(
            dispatched.proposal.run.clone(),
            dispatched.proposal.binding.tab,
        );
        owner.browser_io = Some(slot);
        owner.browser_view.clear();
        let (ack, mut rpc_reply) = tokio::sync::oneshot::channel();
        owner.browser_io_call(crate::browser_io::Call {
            run: dispatched.proposal.run.clone(),
            operation: crate::browser_io::Operation::Navigate {
                action: dispatched.id.clone(),
                revision: dispatched.revision,
                binding: dispatched.proposal.binding.clone(),
                url: dispatched.proposal.url.clone(),
                destination: destination.clone(),
                params: serde_json::json!({"url":destination}),
            },
            ack,
        })?;
        assert!(matches!(
            rpc_reply.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        owner.browser_io_reply(
            &dispatched.proposal.run,
            1,
            crate::browser_io::WireReply::Ok {
                value: serde_json::json!({"loaderId":"loader-2"}),
            },
        );
        assert!(rpc_reply.try_recv()?.is_ok());
    }
    let (ack, reply) = mpsc::sync_channel(1);
    owner.browser_event(Event::Observed {
        action,
        revision: dispatched.revision,
        outcome,
        ack,
    })?;
    let settled = reply.recv()?.map_err(std::io::Error::other)?;
    Ok(settled)
}

fn saved_action(owner: &mut Owner, id: &str) -> crate::Result<crate::browser_action::Action> {
    let history = owner.record_history.as_mut().unwrap();
    let selection = history.selection()?;
    history.edit(|edit| edit.browser_action(&selection, id))
}

#[test]
fn actual_owner_browser_lifecycle_is_record_durable_and_fails_closed() -> crate::Result<()> {
    let directory = Directory::new()?;
    let (mut owner, run) = make_owner(&directory)?;
    let mut binding = Binding {
        tab: 17,
        document: "document-1".into(),
        target: "target-1".into(),
        frame: "frame-1".into(),
        session: "session-1".into(),
        unique_context: "context-1".into(),
        grant_revision: 1,
    };
    let initial_binding = binding.clone();
    let source_url = "http://127.0.0.1:4567/article";
    let destination = "http://127.0.0.1:4567/decoy";
    let mut navigation = proposal(
        &run,
        "tool-0",
        &binding,
        source_url,
        Intent::Navigate {
            destination: destination.into(),
        },
    );
    let mut next_binding = binding.clone();
    next_binding.document = "document-2".into();
    next_binding.unique_context = "context-2".into();
    next_binding.grant_revision = 2;
    let nav = approve_dispatch_settle(
        &mut owner,
        navigation.clone(),
        Outcome::Navigated {
            binding: next_binding.clone(),
            url: destination.into(),
            loader: "loader-2".into(),
        },
    )?;
    assert!(matches!(
        &nav.state,
        Phase::Settled {
            outcome: Outcome::Navigated { .. }
        }
    ));
    binding = next_binding;
    for index in 1..17 {
        let (intent, outcome) = if index == 1 {
            use crate::browser_action::{Direction, Point, Scroll, ScrollKind};
            let before: Point = serde_json::from_value(serde_json::json!({"x":-40.5,"y":120.25}))?;
            let after: Point = serde_json::from_value(serde_json::json!({"x":-40.5,"y":620.25}))?;
            (
                Intent::Scroll {
                    scroll: Scroll {
                        handle: "r1-1-0".into(),
                        kind: ScrollKind::Region,
                        label: "Results".into(),
                        direction: Direction::Down,
                        pixels: 500,
                        before,
                    },
                },
                Outcome::ScrollObserved { after },
            )
        } else {
            (
                field(),
                Outcome::Verified {
                    observed: "new@example.invalid".into(),
                },
            )
        };
        approve_dispatch_settle(
            &mut owner,
            proposal(
                &run,
                &format!("tool-{index}"),
                &binding,
                destination,
                intent,
            ),
            outcome,
        )?;
    }
    assert_eq!(owner.browser_view.len(), 16);
    assert!(!owner.browser_view.contains_key(&nav.id));
    let oldest = saved_action(&mut owner, &nav.id)?;
    assert_eq!(oldest, nav);
    let history_path = owner.disk.directory.join("history.redb");
    drop(owner.record_history.take());
    let mut history = History::open(&history_path, owner.disk.lease())?;
    let scope = history.selection()?;
    let (latest, target) = history.edit(|edit| {
        let task = edit.native_task(&scope, &run)?;
        let Launch::Codex { workflow, .. } = task.launch else {
            return Err("fixture launch changed".into());
        };
        edit.native_latest(&scope, &workflow)
    })?;
    assert_eq!(latest, run);
    assert_eq!(target.document, binding.document);
    assert_eq!(target.url, destination);
    owner.record_history = Some(history);

    navigation.tool_use_id = "stale-grant".into();
    navigation.intent = field();
    let (ack, reply) = mpsc::sync_channel(1);
    owner.browser_event(Event::Proposed {
        proposal: navigation.clone(),
        ack,
        budget: None,
    })?;
    assert!(reply.recv()?.is_err());

    let pending = proposal(&run, "pending-stop", &binding, destination, field());
    let (ack, reply) = mpsc::sync_channel(1);
    owner.browser_event(Event::Proposed {
        proposal: pending,
        ack,
        budget: None,
    })?;
    let pending_id = owner.browser_approval.as_ref().unwrap().action.clone();
    owner.request(Request {
        v: crate::WIRE_VERSION,
        request: "stop-pending".into(),
        command: Command::Conversation {
            scope: crate::history_io::Scope::from(&crate::tests::owner_scope(&owner)),
            action: crate::model::ConversationAction::Stop { run: run.clone() },
        },
    })?;
    assert!(reply.recv()?.is_err());
    let prevented = saved_action(&mut owner, &pending_id)?;
    assert!(matches!(
        prevented.state,
        Phase::Settled {
            outcome: Outcome::Prevented { .. }
        }
    ));
    assert!(
        owner
            .operation
            .as_ref()
            .unwrap()
            .stop
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let (ack, reply) = mpsc::sync_channel(1);
    owner.browser_event(Event::Proposed {
        proposal: {
            let mut retry = navigation.clone();
            retry.tool_use_id = "tool-0".into();
            retry.intent = Intent::Navigate {
                destination: destination.into(),
            };
            retry
        },
        ack,
        budget: None,
    })?;
    assert_eq!(reply.recv()?.unwrap(), nav);
    assert!(owner.browser_approval.is_none());
    owner.agent_settled(&run, AgentOutcome::Cancelled, false, true)?;
    assert!(matches!(
        &owner.state.codex_workflows.values().next().unwrap().binding,
        codex_model::Binding::Bound { .. }
    ));

    let second_dir = Directory::new()?;
    let (mut stopped, stopped_run) = make_owner(&second_dir)?;
    let pending = proposal(
        &stopped_run,
        "dispatched-stop",
        &initial_binding,
        source_url,
        field(),
    );
    let (ack, reply) = mpsc::sync_channel(1);
    stopped.browser_event(Event::Proposed {
        proposal: pending,
        ack,
        budget: None,
    })?;
    let action = stopped.browser_approval.as_ref().unwrap().action.clone();
    stopped.browser_decide(&action, 1, true)?;
    let approved = reply.recv()?.unwrap();
    let (ack, reply) = mpsc::sync_channel(1);
    stopped.browser_event(Event::Dispatch {
        action: action.clone(),
        revision: approved.revision,
        ack,
    })?;
    let _dispatched = reply.recv()?.unwrap();
    stopped.request(Request {
        v: crate::WIRE_VERSION,
        request: "stop-dispatched".into(),
        command: Command::Conversation {
            scope: crate::history_io::Scope::from(&crate::tests::owner_scope(&stopped)),
            action: crate::model::ConversationAction::Stop {
                run: stopped_run.clone(),
            },
        },
    })?;
    stopped.agent_settled(&stopped_run, AgentOutcome::Cancelled, false, true)?;
    let uncertain = saved_action(&mut stopped, &action)?;
    assert!(matches!(
        uncertain.state,
        Phase::Settled {
            outcome: Outcome::Uncertain { .. }
        }
    ));
    assert!(matches!(
        &stopped
            .state
            .codex_workflows
            .values()
            .next()
            .unwrap()
            .binding,
        codex_model::Binding::ReleaseUnknown { .. }
    ));

    let failure_dir = Directory::new()?;
    let (mut failed_owner, failed_run) = make_owner(&failure_dir)?;
    let pending = proposal(
        &failed_run,
        "publication-fails",
        &initial_binding,
        source_url,
        field(),
    );
    let (ack, reply) = mpsc::sync_channel(1);
    failed_owner.transport.fail_next_emit();
    assert!(
        failed_owner
            .browser_event(Event::Proposed {
                proposal: pending,
                ack,
                budget: None,
            })
            .is_err()
    );
    assert!(reply.recv()?.is_err());
    assert!(failed_owner.browser_approval.is_none());
    failed_owner
        .operation
        .take()
        .unwrap()
        .worker
        .join()
        .map_err(|_| "fixture worker panicked")?;
    let history_path = failed_owner.disk.directory.join("history.redb");
    drop(failed_owner.record_history.take());
    let mut reopened = History::open(&history_path, failed_owner.disk.lease())?;
    let selection = reopened.selection()?;
    let durable = reopened
        .edit(|edit| edit.browser_by_tool(&selection, &failed_run, "publication-fails"))?
        .ok_or("published proposal was not durable")?;
    assert!(matches!(durable.state, Phase::Pending));

    let corrupt_dir = Directory::new()?;
    let (mut corrupt, corrupt_run) = make_owner(&corrupt_dir)?;
    let (ack, reply) = mpsc::sync_channel(1);
    corrupt.browser_event(Event::Proposed {
        proposal: proposal(
            &corrupt_run,
            "corrupt-index",
            &initial_binding,
            source_url,
            field(),
        ),
        ack,
        budget: None,
    })?;
    let action = corrupt.browser_approval.as_ref().unwrap().action.clone();
    corrupt.browser_decide(&action, 1, true)?;
    let approved = reply.recv()?.unwrap();
    let number = action.rsplit(':').next().unwrap().parse::<u64>()?;
    {
        let history = corrupt.record_history.as_ref().unwrap();
        let write = history.db.begin_write()?;
        assert!(
            write
                .open_table(super::schema::ACTION_ORDER)?
                .remove((corrupt_run.as_str(), number, action.as_str()))?
                .is_some()
        );
        write.commit()?;
    }
    let (transport, frames) = Transport::captured();
    corrupt.transport = transport;
    let (ack, reply) = mpsc::sync_channel(1);
    let error = corrupt
        .browser_event(Event::Dispatch {
            action,
            revision: approved.revision,
            ack,
        })
        .unwrap_err();
    assert!(super::is_failure(error.as_ref()));
    assert!(reply.recv()?.is_err());
    assert!(frames.lock().unwrap().is_empty());
    corrupt
        .operation
        .take()
        .unwrap()
        .worker
        .join()
        .map_err(|_| "fixture worker panicked")?;
    Ok(())
}
