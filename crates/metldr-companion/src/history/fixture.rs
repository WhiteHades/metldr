use crate::{
    agent_model::{Activity, Launch},
    model::{Page, Run, Source, Status, Store, View},
};
use std::collections::BTreeMap;

const STORE: &str = "0123456789abcdef0123456789abcdef";

pub(super) fn legacy() -> Store {
    let page = |tab, document: &str, title: &str| Page {
        tab,
        document: document.into(),
        url: "http://127.0.0.1:4311/article".into(),
        title: title.into(),
        text: format!("Synthetic page for {title}."),
    };
    let source = |id: &str, page: Page| Source {
        id: format!("{STORE}:{id}"),
        page,
    };
    let current_source = source("01", page(1, "document-current", "Current source"));
    let pi_source = source("033", page(2, "document-pi", "Pi source"));
    let codex_source = source("034", page(3, "document-codex", "Codex source"));
    let opencode_source = source("035", page(4, "document-opencode", "OpenCode source"));

    let mut runs = Vec::new();
    let mut launches = BTreeMap::new();
    let mut add = |n: u8, source: &str, kind: &str, question: Option<&str>, status, launch| {
        let id = format!("{STORE}:{n:02}");
        runs.push(Run {
            id: id.clone(),
            source: format!("{STORE}:{source}"),
            kind: kind.into(),
            question: question.map(str::to_owned),
            status,
            output: Default::default(),
            error: None,
        });
        launches.insert(id, launch);
    };
    add(
        1,
        "33",
        "summary",
        None,
        Status::Complete,
        Launch::LegacyUnspecified,
    );
    add(
        2,
        "017",
        "summary",
        None,
        Status::Complete,
        Launch::Fixture {
            input_digest: "a".repeat(64),
        },
    );
    add(
        3,
        "018",
        "browser",
        None,
        Status::Complete,
        Launch::Messages {
            input_digest: "b".repeat(64),
            model: crate::https_profile::FIXTURE_MODEL.into(),
        },
    );
    add(
        4,
        "019",
        "browser",
        None,
        Status::Complete,
        Launch::ConfiguredGoogle {
            input_digest: "c".repeat(64),
            binding: route_binding("google"),
        },
    );
    add(
        5,
        "020",
        "browser",
        None,
        Status::Complete,
        Launch::ConfiguredChat {
            input_digest: "d".repeat(64),
            binding: route_binding("chat"),
        },
    );
    add(
        6,
        "021",
        "browser",
        None,
        Status::Complete,
        Launch::ConfiguredResponses {
            input_digest: "e".repeat(64),
            binding: route_binding("responses"),
        },
    );
    add(
        7,
        "022",
        "browser",
        None,
        Status::Complete,
        Launch::ConfiguredMessages {
            input_digest: "f".repeat(64),
            binding: route_binding("messages"),
        },
    );
    add(
        8,
        "023",
        "answer",
        Some("Synthetic question?"),
        Status::Complete,
        Launch::Text {
            input_digest: "1".repeat(64),
            protocol: crate::provider_settings::Protocol::Messages,
            binding: route_binding("text"),
        },
    );
    add(
        9,
        "033",
        "agent",
        None,
        Status::Running,
        Launch::Pi {
            input_digest: "2".repeat(64),
            workflow: format!("{STORE}:030"),
            activity: vec![Activity {
                tool: "browser.search".into(),
                phase: "started".into(),
                outcome: None,
                call: None,
            }],
        },
    );
    add(
        10,
        "034",
        "agent",
        None,
        Status::Complete,
        Launch::Codex {
            input_digest: "3".repeat(64),
            workflow: format!("{STORE}:031"),
            selection: Some(crate::codex_model::Selection {
                provider: "fixture-provider".into(),
                model: "fixture-model".into(),
            }),
            activity: Vec::new(),
        },
    );
    add(
        11,
        "035",
        "agent",
        None,
        Status::Complete,
        Launch::OpenCode {
            input_digest: "4".repeat(64),
            workflow: format!("{STORE}:032"),
            selection: Some(crate::opencode_model::Selection {
                model: "fixture-model".into(),
                mode: "build".into(),
            }),
            activity: Vec::new(),
        },
    );
    drop(add);
    {
        use metldr_conversation::{Change, Content, Ending, MessageKey, ToolId};
        let output = &mut runs[9].output;
        *output = Content::native();
        output
            .change(Change::Final {
                key: MessageKey::native("native-first".into()).unwrap(),
                text: "Repeated text".into(),
            })
            .unwrap();
        output
            .observe_tool(ToolId::new("native-tool".into()).unwrap())
            .unwrap();
        output
            .change(Change::Append {
                key: MessageKey::native("native-second".into()).unwrap(),
                text: "Repeated text".into(),
            })
            .unwrap();
        output.end(Ending::Complete).unwrap();
    }

    let pi_profile = crate::agent_model::Profile {
        executable: "/owned/bin/pi".into(),
        workspace: "/owned/workspace".into(),
        session_dir: "/owned/sessions".into(),
        provider: "fixture-provider".into(),
        model: "fixture-model".into(),
        offline: true,
    };
    let codex_profile = crate::codex_model::Profile {
        executable: "/owned/bin/codex".into(),
        workspace: "/owned/workspace".into(),
    };
    let opencode_profile = crate::opencode_model::Profile {
        executable: "/owned/bin/opencode".into(),
        workspace: "/owned/workspace".into(),
    };
    let mut workflows = BTreeMap::new();
    workflows.insert(
        format!("{STORE}:030"),
        crate::agent_model::Workflow {
            fingerprint: fixture_fingerprint(&pi_profile),
            profile: pi_profile,
            source: pi_source,
            revision: 3,
            last_run: format!("{STORE}:09"),
            binding: crate::agent_model::Binding::ReleaseUnknown {
                previous: Some(crate::agent_model::NativeSession {
                    id: "pi-session-1".into(),
                    path: "/owned/sessions/pi-session-1.jsonl".into(),
                }),
            },
        },
    );
    let mut codex_workflows = BTreeMap::new();
    codex_workflows.insert(
        format!("{STORE}:031"),
        crate::codex_model::Workflow {
            fingerprint: fixture_fingerprint(&codex_profile),
            profile: codex_profile,
            source: codex_source,
            revision: 2,
            last_run: format!("{STORE}:10"),
            binding: crate::codex_model::Binding::ReleaseUnknown {
                previous: Some(crate::codex_model::Identity {
                    thread: "thread-fixture-1".into(),
                    session: "session-fixture-1".into(),
                }),
            },
        },
    );
    let mut opencode_workflows = BTreeMap::new();
    opencode_workflows.insert(
        format!("{STORE}:032"),
        crate::opencode_model::Workflow {
            fingerprint: fixture_fingerprint(&opencode_profile),
            profile: opencode_profile,
            source: opencode_source,
            revision: 4,
            last_run: format!("{STORE}:11"),
            binding: crate::opencode_model::Binding::ReleaseUnknown {
                previous: Some(crate::opencode_model::Identity {
                    session: "session_fixture_1".into(),
                    selection: crate::opencode_model::Selection {
                        model: "fixture-model".into(),
                        mode: "build".into(),
                    },
                }),
            },
        },
    );

    let action = |n: &str, tool: &str, state| crate::browser_action::Action {
        id: format!("{STORE}:{n}"),
        revision: if matches!(&state, crate::browser_action::Phase::Settled { .. }) {
            4
        } else {
            3
        },
        proposal: crate::browser_action::Proposal {
            run: format!("{STORE}:09"),
            tool_use_id: tool.into(),
            binding: crate::browser_action::Binding {
                tab: 2,
                document: "document-pi".into(),
                target: "target-1".into(),
                frame: "frame-1".into(),
                session: "session-1".into(),
                unique_context: "context-1".into(),
                grant_revision: 1,
            },
            url: "http://127.0.0.1:4311/article".into(),
            intent: crate::browser_action::Intent::Field {
                field_id: "query".into(),
                field_label: "Search".into(),
                previous_value: "".into(),
                value: "fixture".into(),
            },
        },
        state,
    };
    let mut browser_actions = BTreeMap::new();
    browser_actions.insert(
        format!("{STORE}:050"),
        action(
            "050",
            "call-settled",
            crate::browser_action::Phase::Settled {
                outcome: crate::browser_action::Outcome::Verified {
                    observed: "fixture".into(),
                },
            },
        ),
    );
    browser_actions.insert(
        format!("{STORE}:051"),
        action(
            "051",
            "call-pending",
            crate::browser_action::Phase::Dispatching,
        ),
    );
    let mut alias = browser_actions[&format!("{STORE}:050")].clone();
    alias.id = format!("{STORE}:50");
    alias.proposal.tool_use_id = "call-alias".into();
    browser_actions.insert(alias.id.clone(), alias);

    let mut providers = crate::provider_settings::Settings {
        scope: Some("a".repeat(32)),
        ..Default::default()
    };
    providers.slots.insert(
        crate::provider_settings::Protocol::Messages,
        crate::provider_settings::Slot {
            revision: 1,
            selection: crate::provider_settings::Selection::Removed,
            last_request: Some("provider-remove".into()),
        },
    );
    providers.pending = Some(crate::provider_settings::Pending {
        request: "provider-save-next".into(),
        protocol: crate::provider_settings::Protocol::Messages,
        revision: 2,
        connection: crate::provider_settings::Connection {
            endpoint: "https://example.test/v1/messages".into(),
            model: "fixture-next".into(),
            credential: "b".repeat(32),
            trust_pem: None,
        },
    });
    providers.retired.push("c".repeat(32));
    let mut agent_settings = crate::agent_settings::Settings::default();
    agent_settings
        .slots
        .get_mut(&crate::agent_settings::Kind::Pi)
        .unwrap()
        .revision = 1;
    agent_settings
        .slots
        .get_mut(&crate::agent_settings::Kind::Pi)
        .unwrap()
        .selection = crate::agent_settings::Selection::Removed;
    agent_settings
        .slots
        .get_mut(&crate::agent_settings::Kind::Pi)
        .unwrap()
        .last_change = Some(crate::agent_settings::Receipt {
        request: "agent-remove".into(),
        expected_revision: 0,
        digest: "5".repeat(64),
    });

    Store {
        schema: crate::STORE_VERSION,
        view: View {
            sequence: 42,
            store: STORE.into(),
            source: Some(current_source),
            runs,
        },
        next: 60,
        starts: BTreeMap::new(),
        launches,
        workflows,
        codex_workflows,
        opencode_workflows,
        browser_actions,
        providers,
        agent_settings,
    }
}

fn fixture_fingerprint(value: &impl serde::Serialize) -> String {
    crate::agent_model::digest(value).expect("fixture profile serializes")
}

fn route_binding(route: &str) -> crate::https_profile::Binding {
    crate::https_profile::Binding {
        endpoint: format!("https://example.test/v1/{route}"),
        model: format!("fixture-{route}"),
        revision: 2,
        fingerprint: "6".repeat(64),
    }
}
