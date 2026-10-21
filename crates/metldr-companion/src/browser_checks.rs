use crate::{
    browser_action::{Action, Binding, Intent, Outcome, Phase, Proposal},
    browser_owner::Event,
    flow::{Operation, Owner},
    model::{Command, Page, Run, Source, Status, Store},
    store::Disk,
};
use std::{
    collections::BTreeMap,
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

fn with_owner(check: impl FnOnce(&mut Owner, Proposal)) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "metldr-browser-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (disk, mut state) = Disk::open(root.clone()).unwrap();
    let source = state.id().unwrap();
    let run = state.id().unwrap();
    state.view.source = Some(Source {
        id: source.clone(),
        page: Page {
            tab: 7,
            document: "document-1".into(),
            url: "http://127.0.0.1:1/article".into(),
            title: "Owned form".into(),
            text: "One field".into(),
        },
    });
    state.view.runs.push(Run {
        id: run.clone(),
        source,
        kind: "answer".into(),
        question: Some("Fill the owned field".into()),
        status: Status::Running,
        output: Default::default(),
        error: None,
    });
    state.launches.insert(
        run.clone(),
        crate::agent_model::Launch::Fixture {
            input_digest: "0".repeat(64),
        },
    );
    disk.save(&state).unwrap();
    let (tx, _) = mpsc::sync_channel(16);
    let mut owner = Owner {
        transport: crate::transport::Transport::stdio(),
        disk,
        settings: Default::default(),
        agent_runtime: Default::default(),
        state,
        fixture: Some(crate::native::Fixture {
            origin: "http://127.0.0.1:1".into(),
            address: "127.0.0.1:1".parse().unwrap(),
        }),
        subscribers: BTreeMap::new(),
        operation: Some(Operation {
            run: run.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            worker: std::thread::spawn(|| {}),
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
    };
    let proposal = Proposal {
        run,
        tool_use_id: "tool-1".into(),
        url: owner.fixture.as_ref().unwrap().origin.clone() + "/article",
        binding: Binding {
            tab: 7,
            document: "document-1".into(),
            target: "target-1".into(),
            frame: "frame-1".into(),
            session: "session-1".into(),
            unique_context: "unique-1".into(),
            grant_revision: 1,
        },
        intent: Intent::Field {
            field_id: "metldr-field".into(),
            field_label: "Email address".into(),
            previous_value: "Existing value".into(),
            value: "Approved value".into(),
        },
    };
    check(&mut owner, proposal);
    if let Some(operation) = owner.operation.take() {
        operation.worker.join().unwrap();
    }
    drop(owner);
    fs::remove_dir_all(root).unwrap();
}

fn saved(owner: &Owner) -> Store {
    let state: Store =
        serde_json::from_slice(&fs::read(owner.disk.directory.join("snapshot.json")).unwrap())
            .unwrap();
    state.validate().unwrap();
    state
}

fn bind_codex(owner: &mut Owner, run: &str) -> String {
    use crate::{
        agent_model::{Launch, digest},
        codex_model::{Binding as ThreadBinding, Profile, Workflow},
    };
    let source = owner.state.view.source.as_ref().unwrap().clone();
    let workflow = owner.state.id().unwrap();
    let profile = Profile {
        executable: "/tool/codex".into(),
        workspace: "/work".into(),
    };
    owner.state.codex_workflows.insert(
        workflow.clone(),
        Workflow {
            fingerprint: digest(&profile).unwrap(),
            profile,
            source,
            revision: 1,
            last_run: run.to_owned(),
            binding: ThreadBinding::Pending,
        },
    );
    owner.state.launches.insert(
        run.to_owned(),
        Launch::Codex {
            input_digest: "0".repeat(64),
            workflow: workflow.clone(),
            selection: None,
            activity: vec![],
        },
    );
    owner.state.view.runs[0].kind = "agent".into();
    workflow
}

#[test]
fn native_reviews_keep_distinct_durable_nonces_and_revoke_exact_requests() {
    with_owner(|owner, proposal| {
        bind_codex(owner, &proposal.run);
        let details = serde_json::json!({"agent":"codex","tool":"Command","title":"Run owned command","input":"owned command","options":[{"id":"0","name":"Allow once"}]});
        let (ack, first) = mpsc::sync_channel(1);
        owner
            .native_permission(&proposal.run, 1, details.clone(), ack)
            .unwrap();
        let first_nonce = owner.native_permissions.keys().next().unwrap().clone();
        assert_eq!(saved(owner).next, owner.state.next);
        let (ack, second) = mpsc::sync_channel(1);
        owner
            .native_permission(&proposal.run, 2, details, ack)
            .unwrap();
        assert_eq!(owner.native_permissions.len(), 2);
        let second_nonce = owner
            .native_permissions
            .keys()
            .find(|id| **id != first_nonce)
            .unwrap()
            .clone();
        assert!(
            owner
                .native_decide("invalid-choice", &proposal.run, &first_nonce, Some("other"))
                .is_err()
        );
        assert!(matches!(first.try_recv(), Err(mpsc::TryRecvError::Empty)));
        owner.native_permission_resolved(&proposal.run, 1).unwrap();
        assert!(matches!(
            first.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(
            owner
                .native_decide("stale-choice", &proposal.run, &first_nonce, Some("0"))
                .is_err()
        );
        assert!(owner.native_permissions.contains_key(&second_nonce));
        owner
            .native_decide("valid-choice", &proposal.run, &second_nonce, Some("0"))
            .unwrap();
        assert_eq!(second.try_recv().unwrap(), Some("0".into()));
        assert!(owner.native_permissions.is_empty());
    });
}

#[test]
fn codex_browser_follow_up_uses_the_verified_destination() {
    with_owner(|owner, mut field| {
        let workflow = bind_codex(owner, &field.run);
        let source = serde_json::to_value(&owner.state.view.source).unwrap();
        let mut navigation = field.clone();
        let destination = owner.fixture.as_ref().unwrap().origin.clone() + "/decoy";
        navigation.intent = Intent::Navigate {
            destination: destination.clone(),
        };
        let proposed = owner.state.propose_browser(navigation).unwrap();
        let approved = owner.state.decide_browser(&proposed.id, 1, true).unwrap();
        let dispatched = owner
            .state
            .dispatch_browser(&proposed.id, approved.revision)
            .unwrap();
        let mut binding = field.binding.clone();
        binding.document = "destination-document".into();
        binding.unique_context = "destination-context".into();
        binding.grant_revision = 2;
        owner
            .state
            .settle_browser(
                &proposed.id,
                dispatched.revision,
                Outcome::Navigated {
                    binding: binding.clone(),
                    url: destination.clone(),
                    loader: "destination-loader".into(),
                },
            )
            .unwrap();
        for turn in 0..2 {
            owner.state.view.runs.last_mut().unwrap().status = Status::Complete;
            if let Some(crate::agent_model::Launch::Codex { selection, .. }) =
                owner.state.launches.get_mut(&field.run)
            {
                *selection = Some(crate::codex_model::Selection {
                    provider: "owned".into(),
                    model: "owned".into(),
                });
            }
            let run = owner.state.id().unwrap();
            let mut record = owner.state.view.runs.last().unwrap().clone();
            record.id = run.clone();
            record.status = Status::Running;
            owner.state.view.runs.push(record);
            owner.state.launches.insert(
                run.clone(),
                crate::agent_model::Launch::Codex {
                    input_digest: "0".repeat(64),
                    workflow: workflow.clone(),
                    selection: None,
                    activity: vec![],
                },
            );
            owner
                .state
                .codex_workflows
                .get_mut(&workflow)
                .unwrap()
                .last_run = run.clone();
            field.run = run;
            field.tool_use_id = format!("follow-up-{turn}");
            let mut stale = field.clone();
            stale.binding.document = "document-1".into();
            stale.binding.grant_revision = 1;
            stale.url = owner.fixture.as_ref().unwrap().origin.clone() + "/article";
            assert!(owner.state.propose_browser(stale).is_err());
            field.binding = binding.clone();
            field.binding.grant_revision = 1;
            field.url = destination.clone();
            let action = owner
                .state
                .propose_browser(field.clone())
                .expect("follow up must use the verified destination with a fresh grant");
            owner.state.decide_browser(&action.id, 1, false).unwrap();
            owner.state.validate().unwrap();
        }
        assert_eq!(
            serde_json::to_value(&owner.state.view.source).unwrap(),
            source
        );
        let mut unrelated = owner.state.clone();
        unrelated.view.runs.last_mut().unwrap().status = Status::Complete;
        if let Some(crate::agent_model::Launch::Codex { selection, .. }) =
            unrelated.launches.get_mut(&field.run)
        {
            *selection = Some(crate::codex_model::Selection {
                provider: "owned".into(),
                model: "owned".into(),
            });
        }
        let other_workflow = unrelated.id().unwrap();
        let other_run = unrelated.id().unwrap();
        let mut other = unrelated.codex_workflows[&workflow].clone();
        other.last_run = other_run.clone();
        unrelated
            .codex_workflows
            .insert(other_workflow.clone(), other);
        let mut other_record = unrelated.view.runs.last().unwrap().clone();
        other_record.id = other_run.clone();
        other_record.status = Status::Running;
        unrelated.view.runs.push(other_record);
        unrelated.launches.insert(
            other_run.clone(),
            crate::agent_model::Launch::Codex {
                input_digest: "0".repeat(64),
                workflow: other_workflow,
                selection: None,
                activity: vec![],
            },
        );
        unrelated.validate().unwrap();
        field.run = other_run;
        field.tool_use_id = "unrelated-workflow".into();
        assert!(
            unrelated.propose_browser(field).is_err(),
            "another workflow must not inherit a destination"
        );
        let mut reordered = owner.state.clone();
        reordered.view.runs.swap(0, 1);
        assert!(reordered.validate().is_err());
    });
}

#[test]
fn native_browser_actions_keep_the_workflow_source_and_block_uncertain_recovery() {
    for pi in [false, true] {
        with_owner(|owner, proposal| {
            let workflow = bind_codex(owner, &proposal.run);
            if pi {
                use crate::agent_model::{Binding, Launch, Profile, Workflow, digest};
                let old = owner.state.codex_workflows.remove(&workflow).unwrap();
                let profile = Profile {
                    executable: "/tool/pi".into(),
                    workspace: "/work".into(),
                    session_dir: "/sessions".into(),
                    provider: "probe".into(),
                    model: "probe".into(),
                    offline: true,
                };
                owner.state.workflows.insert(
                    workflow.clone(),
                    Workflow {
                        fingerprint: digest(&profile).unwrap(),
                        profile,
                        source: old.source,
                        revision: 1,
                        last_run: proposal.run.clone(),
                        binding: Binding::Pending,
                    },
                );
                owner.state.launches.insert(
                    proposal.run.clone(),
                    Launch::Pi {
                        workflow,
                        input_digest: "0".repeat(64),
                        activity: vec![],
                    },
                );
            }
            let new_source = owner.state.id().unwrap();
            let selected = owner.state.view.source.as_mut().unwrap();
            selected.id = new_source;
            selected.page.tab = 8;
            selected.page.document = "another-document".into();
            selected.page.url = owner.fixture.as_ref().unwrap().origin.clone() + "/other";
            owner.state.validate().unwrap();
            let mut retargeted = proposal.clone();
            retargeted.tool_use_id = "another-tool".into();
            retargeted.binding.tab = 8;
            retargeted.binding.document = "another-document".into();
            retargeted.url = owner.fixture.as_ref().unwrap().origin.clone() + "/other";
            assert!(owner.state.propose_browser(retargeted).is_err());
            let action = owner.state.propose_browser(proposal).unwrap();
            assert_eq!(action.proposal.binding.tab, 7);
            assert_eq!(action.proposal.binding.document, "document-1");
            let approved = owner
                .state
                .decide_browser(&action.id, action.revision, true)
                .unwrap();
            let dispatched = owner
                .state
                .dispatch_browser(&action.id, approved.revision)
                .unwrap();
            owner
                .state
                .settle_browser(
                    &action.id,
                    dispatched.revision,
                    Outcome::Uncertain {
                        reason: "field observation was lost".into(),
                    },
                )
                .unwrap();
            owner
                .agent_settled(
                    &action.proposal.run,
                    crate::agent::Outcome::Failed("browser effect is uncertain".into()),
                    false,
                    true,
                )
                .unwrap();
            if pi {
                assert!(
                    owner
                        .state
                        .workflows
                        .values()
                        .next()
                        .unwrap()
                        .binding
                        .resume_path()
                        .is_err()
                );
            } else {
                assert!(
                    owner
                        .state
                        .codex_workflows
                        .values()
                        .next()
                        .unwrap()
                        .binding
                        .resume()
                        .is_err()
                );
            }
        });
    }
}

#[test]
fn saved_fixture_start_replays_but_cannot_retarget_https() {
    with_owner(|owner, proposal| {
        owner.operation.take().unwrap().worker.join().unwrap();
        let source = owner.state.view.source.as_ref().unwrap().id.clone();
        let text = "Fill the owned field";
        let digest = crate::agent_model::digest(&serde_json::json!({
            "source":source,"text":text,"origin":&owner.fixture.as_ref().unwrap().origin,"model":crate::https_profile::FIXTURE_MODEL
        }))
        .unwrap();
        let run = owner
            .state
            .view
            .runs
            .iter_mut()
            .find(|run| run.id == proposal.run)
            .unwrap();
        run.kind = "browser".into();
        run.status = Status::Complete;
        owner
            .state
            .starts
            .insert("saved-request".into(), proposal.run.clone());
        owner.state.launches.insert(
            proposal.run.clone(),
            crate::agent_model::Launch::Messages {
                input_digest: digest,
                model: crate::https_profile::FIXTURE_MODEL.into(),
            },
        );
        owner.disk.save(&owner.state).unwrap();
        owner.state = saved(owner);
        owner.browser_route = Some(
            crate::https_profile::Config::Fixture
                .bind(owner.fixture.as_ref().map(|fixture| fixture.address))
                .unwrap(),
        );
        owner
            .browser_start("saved-request", &source, text, None)
            .unwrap();
        assert!(owner.operation.is_none());
        let profile = crate::https_profile::Config::Https {
            endpoint: "https://example.com/v1/messages".into(),
            model: "resolved-model".into(),
            revision: 1,
            credential_env: "METLDR_UNRESOLVED_TEST_KEY".into(),
            trust_ca: None,
        }
        .bind(owner.fixture.as_ref().map(|fixture| fixture.address))
        .unwrap();
        let crate::https_profile::Profile::Https { binding, .. } = &profile else {
            unreachable!()
        };
        let binding = binding.clone();
        owner.browser_route = Some(profile);
        assert!(
            owner
                .browser_start("saved-request", &source, text, None)
                .is_err()
        );
        assert!(
            owner
                .browser_start("saved-request", &source, text, Some(&"0".repeat(64)))
                .is_err()
        );
        assert!(owner.operation.is_none());
        let digest = crate::agent_model::digest(&serde_json::json!({
            "source":source,"text":text,"origin":&owner.fixture.as_ref().unwrap().origin,"route":binding.public()
        }))
        .unwrap();
        owner.state.launches.insert(
            proposal.run.clone(),
            crate::agent_model::Launch::ConfiguredMessages {
                input_digest: digest,
                binding: binding.clone(),
            },
        );
        owner.disk.save(&owner.state).unwrap();
        owner.state = saved(owner);
        owner
            .browser_start("saved-request", &source, text, Some(&binding.fingerprint))
            .unwrap();
        assert!(owner.operation.is_none());
        assert_eq!(saved(owner).view.runs.len(), 1);
        let profile = crate::https_profile::Config::Https {
            endpoint: "https://example.com/v1/responses".into(),
            model: "configured-alias".into(),
            revision: 1,
            credential_env: "METLDR_UNRESOLVED_TEST_KEY".into(),
            trust_ca: None,
        }
        .bind_responses(owner.fixture.as_ref().map(|fixture| fixture.address))
        .unwrap();
        let crate::https_profile::Profile::Https { binding, .. } = &profile else {
            unreachable!()
        };
        let binding = binding.clone();
        owner.responses_route = Some(profile);
        assert!(
            owner
                .responses_start("other-request", &source, text, &"0".repeat(64))
                .is_err()
        );
        assert!(owner.operation.is_none());
        let digest = crate::agent_model::digest(&serde_json::json!({
            "source":source,"text":text,"origin":&owner.fixture.as_ref().unwrap().origin,"route":binding.public_responses()
        }))
        .unwrap();
        owner.state.launches.insert(
            proposal.run.clone(),
            crate::agent_model::Launch::ConfiguredResponses {
                input_digest: digest,
                binding: binding.clone(),
            },
        );
        owner.disk.save(&owner.state).unwrap();
        owner.state = saved(owner);
        owner
            .responses_start("saved-request", &source, text, &binding.fingerprint)
            .unwrap();
        assert!(owner.operation.is_none());
        owner.responses_route = None;
        let public = crate::public_view::render(
            &owner.state,
            None,
            None,
            None,
            &owner.native_permissions,
            owner.pi_ui.as_ref(),
            owner.codex_input.as_ref(),
            owner.browser_route.as_ref(),
            None,
            None,
            None,
            owner.fixture.is_some(),
        )
        .unwrap();
        assert!(public["responses_route"].is_null());
        assert_eq!(public["runs"][0]["route"], binding.public_responses());
        assert_eq!(saved(owner).view.runs.len(), 1);
        let profile = crate::https_profile::Config::Https {
            endpoint: "https://example.com/v1/interactions".into(),
            model: "configured-google".into(),
            revision: 1,
            credential_env: "METLDR_UNRESOLVED_GOOGLE_KEY".into(),
            trust_ca: None,
        }
        .bind_google(owner.fixture.as_ref().map(|fixture| fixture.address))
        .unwrap();
        let crate::https_profile::Profile::Https {
            binding: google, ..
        } = &profile
        else {
            unreachable!()
        };
        let google = google.clone();
        owner.google_route = Some(profile);
        assert!(
            owner
                .google_start("stale-google", &source, text, &"0".repeat(64))
                .is_err()
        );
        assert!(owner.operation.is_none());
        let digest = crate::agent_model::digest(&serde_json::json!({
            "source":source,"text":text,"origin":&owner.fixture.as_ref().unwrap().origin,"route":google.public_google()
        }))
        .unwrap();
        owner.state.launches.insert(
            proposal.run.clone(),
            crate::agent_model::Launch::ConfiguredGoogle {
                input_digest: digest,
                binding: google.clone(),
            },
        );
        owner.disk.save(&owner.state).unwrap();
        owner.state = saved(owner);
        owner
            .google_start("saved-request", &source, text, &google.fingerprint)
            .unwrap();
        assert!(owner.operation.is_none());
        owner.google_route = None;
        let public = crate::public_view::render(
            &owner.state,
            None,
            None,
            None,
            &owner.native_permissions,
            owner.pi_ui.as_ref(),
            owner.codex_input.as_ref(),
            owner.browser_route.as_ref(),
            None,
            None,
            None,
            owner.fixture.is_some(),
        )
        .unwrap();
        assert!(public["google_route"].is_null());
        assert_eq!(public["runs"][0]["route"], google.public_google());
        assert!(!public.to_string().contains("METLDR_UNRESOLVED_GOOGLE_KEY"));
        assert_eq!(saved(owner).view.runs.len(), 1);
        let profile = crate::https_profile::Config::Https {
            endpoint: "https://example.com/v1/chat/completions".into(),
            model: "configured-chat".into(),
            revision: 1,
            credential_env: "METLDR_UNRESOLVED_CHAT_KEY".into(),
            trust_ca: None,
        }
        .bind_chat_completions(owner.fixture.as_ref().map(|fixture| fixture.address))
        .unwrap();
        let crate::https_profile::Profile::Https { binding: chat, .. } = &profile else {
            unreachable!()
        };
        let chat = chat.clone();
        owner.chat_route = Some(profile);
        assert!(
            owner
                .chat_start("stale-chat", &source, text, &"0".repeat(64))
                .is_err()
        );
        assert!(owner.operation.is_none());
        let digest = crate::agent_model::digest(&serde_json::json!({
            "source":source,"text":text,"origin":&owner.fixture.as_ref().unwrap().origin,"route":chat.public_chat_completions()
        }))
        .unwrap();
        owner.state.launches.insert(
            proposal.run.clone(),
            crate::agent_model::Launch::ConfiguredChat {
                input_digest: digest,
                binding: chat.clone(),
            },
        );
        owner.disk.save(&owner.state).unwrap();
        owner.state = saved(owner);
        owner
            .chat_start("saved-request", &source, text, &chat.fingerprint)
            .unwrap();
        assert!(owner.operation.is_none());
        owner.chat_route = None;
        let public = crate::public_view::render(
            &owner.state,
            None,
            None,
            None,
            &owner.native_permissions,
            owner.pi_ui.as_ref(),
            owner.codex_input.as_ref(),
            owner.browser_route.as_ref(),
            None,
            None,
            None,
            owner.fixture.is_some(),
        )
        .unwrap();
        assert!(public["chat_route"].is_null());
        assert_eq!(public["runs"][0]["route"], chat.public_chat_completions());
        assert!(!public.to_string().contains("METLDR_UNRESOLVED_CHAT_KEY"));
        assert_eq!(saved(owner).view.runs.len(), 1);
    });
}

fn propose(
    owner: &mut Owner,
    p: Proposal,
) -> (Action, mpsc::Receiver<std::result::Result<Action, String>>) {
    let (ack, rx) = mpsc::sync_channel(1);
    owner
        .browser_event(Event::Proposed {
            proposal: p,
            ack,
            budget: None,
        })
        .unwrap();
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let action = owner.state.browser_actions.values().next().unwrap().clone();
    assert_eq!(saved(owner).browser_actions[&action.id], action);
    (action, rx)
}

fn dispatch(owner: &mut Owner, action: &Action) -> std::result::Result<Action, String> {
    let (ack, rx) = mpsc::sync_channel(1);
    owner
        .browser_event(Event::Dispatch {
            action: action.id.clone(),
            revision: action.revision,
            ack,
        })
        .unwrap();
    rx.recv().unwrap()
}

fn cached(owner: &mut Owner, proposal: Proposal, expected: &Action) {
    let before = fs::read(owner.disk.directory.join("snapshot.json")).unwrap();
    let (ack, rx) = mpsc::sync_channel(1);
    owner
        .browser_event(Event::Proposed {
            proposal,
            ack,
            budget: None,
        })
        .unwrap();
    assert_eq!(rx.recv().unwrap().unwrap(), *expected);
    assert_eq!(
        fs::read(owner.disk.directory.join("snapshot.json")).unwrap(),
        before
    );
}

#[test]
fn browser_review_budget_tracks_only_the_saved_pending_approval() {
    for decision in [Some(true), Some(false), None] {
        with_owner(|owner, proposal| {
            let budget = crate::browser_owner::Budget::new(std::time::Duration::from_secs(30));
            let (ack, rx) = mpsc::sync_channel(1);
            owner
                .browser_event(Event::Proposed {
                    proposal: proposal.clone(),
                    ack,
                    budget: Some(budget.clone()),
                })
                .unwrap();
            let action = saved(owner)
                .browser_actions
                .values()
                .next()
                .unwrap()
                .clone();
            assert!(matches!(action.state, Phase::Pending));
            assert!(budget.review_elapsed().is_some());
            assert!(rx.try_recv().is_err());

            let repeated = crate::browser_owner::Budget::new(std::time::Duration::from_secs(30));
            let (ack, cached) = mpsc::sync_channel(1);
            owner
                .browser_event(Event::Proposed {
                    proposal,
                    ack,
                    budget: Some(repeated.clone()),
                })
                .unwrap();
            assert_eq!(cached.recv().unwrap().unwrap(), action);
            assert!(repeated.review_elapsed().is_none());
            if let Some(approve) = decision {
                owner
                    .browser_decide(&action.id, action.revision, approve)
                    .unwrap();
                assert!(rx.recv().unwrap().is_ok());
            } else {
                owner
                    .command(
                        "stop",
                        Command::Stop {
                            run: action.proposal.run,
                        },
                    )
                    .unwrap();
                assert!(rx.recv().unwrap().is_err());
            }
            assert!(budget.review_elapsed().is_none());
        });
    }
    with_owner(|owner, proposal| {
        let budget = crate::browser_owner::Budget::new(std::time::Duration::ZERO);
        let (ack, rx) = mpsc::sync_channel(1);
        owner
            .browser_event(Event::Proposed {
                proposal,
                ack,
                budget: Some(budget),
            })
            .unwrap();
        assert!(rx.recv().unwrap().is_err());
        assert!(owner.browser_approval.is_none());
        assert!(saved(owner).browser_actions.is_empty());
    });
}

#[test]
fn browser_owner_saves_approval_dispatch_and_observation_before_acknowledging() {
    with_owner(|owner, proposal| {
        let (pending, rx) = propose(owner, proposal.clone());
        cached(owner, proposal.clone(), &pending);
        assert!(owner.browser_decide(&pending.id, 2, true).is_err());
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        owner
            .command(
                "approve-1",
                Command::BrowserDecision {
                    action: pending.id.clone(),
                    revision: 1,
                    approve: true,
                },
            )
            .unwrap();
        let approved = rx.recv().unwrap().unwrap();
        assert_eq!(approved.state, Phase::Approved);
        assert_eq!(saved(owner).browser_actions[&approved.id], approved);
        cached(owner, proposal.clone(), &approved);
        assert!(owner.browser_decide(&approved.id, 1, true).is_err());
        let dispatched = dispatch(owner, &approved).unwrap();
        assert_eq!(saved(owner).browser_actions[&approved.id], dispatched);
        cached(owner, proposal.clone(), &dispatched);
        assert!(dispatch(owner, &approved).is_err());
        let (ack, rx) = mpsc::sync_channel(1);
        owner
            .browser_event(Event::Observed {
                action: dispatched.id.clone(),
                revision: dispatched.revision,
                outcome: Outcome::Verified {
                    observed: "wrong field value".into(),
                },
                ack,
            })
            .unwrap();
        assert!(rx.recv().unwrap().is_err());
        assert_eq!(
            saved(owner).browser_actions[&approved.id].state,
            Phase::Dispatching
        );
        let (ack, rx) = mpsc::sync_channel(1);
        owner
            .browser_event(Event::Observed {
                action: dispatched.id.clone(),
                revision: dispatched.revision,
                outcome: Outcome::Verified {
                    observed: "Approved value".into(),
                },
                ack,
            })
            .unwrap();
        let verified = rx.recv().unwrap().unwrap();
        assert_eq!(saved(owner).browser_actions[&approved.id], verified);
        assert!(matches!(
            verified.state,
            Phase::Settled {
                outcome: Outcome::Verified { .. }
            }
        ));
        cached(owner, proposal.clone(), &verified);
        assert_eq!(
            owner.state.propose_browser(proposal.clone()).unwrap(),
            verified
        );
        let mut changed = proposal;
        let Intent::Field { value, .. } = &mut changed.intent else {
            unreachable!()
        };
        value.push('!');
        assert!(owner.state.propose_browser(changed).is_err());
        let mut changed = owner
            .state
            .browser_actions
            .values()
            .next()
            .unwrap()
            .proposal
            .clone();
        let Intent::Field { previous_value, .. } = &mut changed.intent else {
            unreachable!()
        };
        previous_value.push('!');
        assert!(owner.state.propose_browser(changed).is_err());
        assert_eq!(owner.state.browser_actions.len(), 1);
        let projected = crate::public_view::render(
            &owner.state,
            None,
            None,
            None,
            &owner.native_permissions,
            owner.pi_ui.as_ref(),
            owner.codex_input.as_ref(),
            None,
            None,
            None,
            None,
            owner.fixture.is_some(),
        )
        .unwrap();
        assert_eq!(projected["browser_actions"][0]["outcome"], "verified");
        assert_eq!(
            projected["browser_actions"][0]["field_label"],
            "Email address"
        );
        assert_eq!(
            projected["browser_actions"][0]["previous_value"],
            "Existing value"
        );
    });
    with_owner(|owner, proposal| {
        let (pending, rx) = propose(owner, proposal);
        owner.browser_decide(&pending.id, 1, true).unwrap();
        let approved = rx.recv().unwrap().unwrap();
        let obstruction = owner.disk.directory.join("snapshot.next");
        fs::create_dir(&obstruction).unwrap();
        let (ack, rx) = mpsc::sync_channel(1);
        assert!(
            owner
                .browser_event(Event::Dispatch {
                    action: approved.id.clone(),
                    revision: 2,
                    ack
                })
                .is_err()
        );
        assert!(rx.recv().unwrap().is_err());
        assert_eq!(
            saved(owner).browser_actions[&approved.id].state,
            Phase::Approved
        );
        fs::remove_dir(obstruction).unwrap();
    });
}

#[test]
fn browser_proposal_requires_bounded_previous_value() {
    with_owner(|owner, mut proposal| {
        let mut serialized = serde_json::to_value(&proposal).unwrap();
        let Intent::Field { previous_value, .. } = &mut proposal.intent else {
            unreachable!()
        };
        *previous_value = "x".repeat(1_025);
        assert!(owner.state.propose_browser(proposal).is_err());

        serialized["intent"]
            .as_object_mut()
            .unwrap()
            .remove("previous_value");
        assert!(serde_json::from_value::<Proposal>(serialized).is_err());
    });
}

#[test]
fn navigation_requires_durable_destination_before_a_new_field_proposal() {
    with_owner(|owner, mut field| {
        let original = serde_json::to_value(&owner.state.view.source).unwrap();
        let mut legacy = serde_json::to_value(&field).unwrap();
        let members = legacy.as_object_mut().unwrap().remove("intent").unwrap();
        for (name, value) in members.as_object().unwrap() {
            if name != "kind" {
                legacy[name] = value.clone();
            }
        }
        assert_eq!(serde_json::from_value::<Proposal>(legacy).unwrap(), field);
        let mut navigation = field.clone();
        let destination = owner.fixture.as_ref().unwrap().origin.clone() + "/decoy";
        navigation.intent = Intent::Navigate {
            destination: destination.clone(),
        };
        let (pending, rx) = propose(owner, navigation.clone());
        assert!(dispatch(owner, &pending).is_err());
        owner
            .browser_decide(&pending.id, pending.revision, true)
            .unwrap();
        let approved = rx.recv().unwrap().unwrap();
        let dispatched = dispatch(owner, &approved).unwrap();
        let mut target = field.binding.clone();
        target.document = "destination-document".into();
        target.unique_context = "destination-context".into();
        target.grant_revision = 2;
        let mut wrong = target.clone();
        wrong.grant_revision = 3;
        assert!(
            owner
                .state
                .settle_browser(
                    &dispatched.id,
                    dispatched.revision,
                    Outcome::Navigated {
                        binding: wrong,
                        url: destination.clone(),
                        loader: "loader".into()
                    }
                )
                .is_err()
        );
        let (ack, rx) = mpsc::sync_channel(1);
        owner
            .browser_event(Event::Observed {
                action: dispatched.id.clone(),
                revision: dispatched.revision,
                outcome: Outcome::Navigated {
                    binding: target.clone(),
                    url: destination.clone(),
                    loader: "loader".into(),
                },
                ack,
            })
            .unwrap();
        let settled = rx.recv().unwrap().unwrap();
        assert_eq!(saved(owner).browser_actions[&settled.id], settled);
        field.tool_use_id = "destination-field".into();
        assert!(owner.state.propose_browser(field.clone()).is_err());
        field.binding = target;
        field.url = destination;
        let pending_field = owner.state.propose_browser(field).unwrap();
        owner.state.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&owner.state.view.source).unwrap(),
            original
        );
        assert_eq!(owner.state.propose_browser(navigation).unwrap(), settled);
        let mut corrupted = owner.state.clone();
        corrupted
            .browser_actions
            .get_mut(&settled.id)
            .unwrap()
            .proposal
            .binding
            .grant_revision = 2;
        assert!(corrupted.validate().is_err());
        owner.state.browser_actions.remove(&pending_field.id);
    });
}

#[test]
fn browser_owner_stop_prevents_dispatch_and_preserves_uncertainty_after_dispatch() {
    with_owner(|owner, proposal| {
        let (pending, rx) = propose(owner, proposal);
        owner.browser_decide(&pending.id, 1, false).unwrap();
        let declined = rx.recv().unwrap().unwrap();
        assert!(matches!(
            declined.state,
            Phase::Settled {
                outcome: Outcome::Prevented { .. }
            }
        ));
        assert_eq!(saved(owner).browser_actions[&declined.id], declined);
        assert!(dispatch(owner, &declined).is_err());
    });
    for after_approval in [false, true] {
        with_owner(|owner, proposal| {
            let run = proposal.run.clone();
            let (pending, rx) = propose(owner, proposal);
            let candidate = if after_approval {
                owner.browser_decide(&pending.id, 1, true).unwrap();
                rx.recv().unwrap().unwrap()
            } else {
                pending.clone()
            };
            owner.command("stop", Command::Stop { run }).unwrap();
            if !after_approval {
                assert!(rx.recv().unwrap().is_err());
            }
            assert!(dispatch(owner, &candidate).is_err());
            assert!(matches!(
                saved(owner).browser_actions[&pending.id].state,
                Phase::Settled {
                    outcome: Outcome::Prevented { .. }
                }
            ));
        });
    }
    with_owner(|owner, proposal| {
        let run = proposal.run.clone();
        let (pending, rx) = propose(owner, proposal);
        owner.browser_decide(&pending.id, 1, true).unwrap();
        let approved = rx.recv().unwrap().unwrap();
        dispatch(owner, &approved).unwrap();
        owner
            .command("stop", Command::Stop { run: run.clone() })
            .unwrap();
        assert_eq!(
            saved(owner).browser_actions[&pending.id].state,
            Phase::Dispatching
        );
        owner.settled(&run, Err("stopped".into()), false).unwrap();
        assert!(matches!(
            saved(owner).browser_actions[&pending.id].state,
            Phase::Settled {
                outcome: Outcome::Uncertain { .. }
            }
        ));
        assert!(dispatch(owner, &approved).is_err());
    });
}

#[test]
fn browser_restart_never_replays_approved_or_dispatched_actions() {
    for phase in 0..3 {
        with_owner(|owner, proposal| {
            let (pending, rx) = propose(owner, proposal);
            if phase > 0 {
                owner.browser_decide(&pending.id, 1, true).unwrap();
                let approved = rx.recv().unwrap().unwrap();
                if phase == 2 {
                    dispatch(owner, &approved).unwrap();
                }
            }
            let mut restored = saved(owner);
            crate::recovery::interrupt(&mut restored).unwrap();
            let outcome = &restored.browser_actions[&pending.id].state;
            assert!(if phase == 2 {
                matches!(
                    outcome,
                    Phase::Settled {
                        outcome: Outcome::Uncertain { .. }
                    }
                )
            } else {
                matches!(
                    outcome,
                    Phase::Settled {
                        outcome: Outcome::Prevented { .. }
                    }
                )
            });
            assert!(restored.dispatch_browser(&pending.id, 2).is_err());
            let once = serde_json::to_vec(&restored).unwrap();
            crate::recovery::interrupt(&mut restored).unwrap();
            assert_eq!(serde_json::to_vec(&restored).unwrap(), once);
            restored.validate().unwrap();
        });
    }
    with_owner(|owner, _| {
        let old = serde_json::to_value(&owner.state).unwrap();
        assert!(old.get("browser_actions").is_none());
        let restored: Store = serde_json::from_value(old).unwrap();
        assert!(restored.browser_actions.is_empty());
    });
}

#[test]
fn browser_transport_correlates_replies_and_fails_closed_on_detach() {
    use crate::browser_io::{Call, Operation, Slot, WireReply};
    use serde_json::json;
    with_owner(|owner, proposal| {
        let run = proposal.run;
        let (slot, mut events) = Slot::new(run.clone(), 7);
        owner.browser_io = Some(slot);
        let (ack, mut reply) = tokio::sync::oneshot::channel();
        owner
            .browser_io_call(Call {
                run: run.clone(),
                operation: Operation::Command {
                    tab: 7,
                    session: None,
                    method: "Runtime.enable".into(),
                    params: json!({}),
                },
                ack,
            })
            .unwrap();
        owner.browser_io_reply(
            "stale-run",
            1,
            WireReply::Ok {
                value: json!({"wrong":true}),
            },
        );
        assert!(matches!(
            reply.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        owner.browser_io_reply(
            &run,
            2,
            WireReply::Ok {
                value: json!({"wrong":true}),
            },
        );
        assert!(matches!(
            reply.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        owner.browser_io_reply(
            &run,
            1,
            WireReply::Ok {
                value: json!({"right":true}),
            },
        );
        assert_eq!(
            reply.blocking_recv().unwrap().unwrap(),
            json!({"right":true})
        );
        owner.browser_io_event(
            &run,
            7,
            None,
            "Runtime.consoleAPICalled".into(),
            json!({"executionContextId":1}),
        );
        assert_eq!(
            events.blocking_recv().unwrap().method,
            "Runtime.consoleAPICalled"
        );
        let (ack, reply) = tokio::sync::oneshot::channel();
        owner
            .browser_io_call(Call {
                run: run.clone(),
                operation: Operation::Command {
                    tab: 7,
                    session: None,
                    method: "Runtime.enable".into(),
                    params: json!({}),
                },
                ack,
            })
            .unwrap();
        owner.browser_io_event(
            &run,
            7,
            None,
            "metldr.detached".into(),
            json!({"reason":"fixture"}),
        );
        assert!(reply.blocking_recv().unwrap().is_err());
        assert!(events.blocking_recv().is_none());
        assert!(
            owner.browser_io.is_some(),
            "retain the run's cleanup channel"
        );
        let (ack, reply) = tokio::sync::oneshot::channel();
        owner
            .browser_io_call(Call {
                run: run.clone(),
                operation: Operation::Command {
                    tab: 7,
                    session: None,
                    method: "Runtime.enable".into(),
                    params: json!({}),
                },
                ack,
            })
            .unwrap();
        assert!(reply.blocking_recv().unwrap().is_err());
        let (ack, reply) = tokio::sync::oneshot::channel();
        owner
            .browser_io_call(Call {
                run: run.clone(),
                operation: Operation::Detach { tab: 7 },
                ack,
            })
            .unwrap();
        owner.browser_io_event(
            &run,
            7,
            None,
            "Runtime.executionContextsCleared".into(),
            json!({}),
        );
        owner.browser_io_reply(
            &run,
            3,
            WireReply::Ok {
                value: json!({"detached":true}),
            },
        );
        assert_eq!(
            reply.blocking_recv().unwrap().unwrap(),
            json!({"detached":true})
        );
        owner.browser_io_finish(&run);
        assert!(owner.browser_io.is_none());
    });
}
