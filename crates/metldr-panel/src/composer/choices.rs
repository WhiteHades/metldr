use super::*;
use crate::model::AgentKind;

pub(crate) struct Choice {
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) enabled: bool,
    pub(super) kind: Kind,
}

pub(super) enum Kind {
    Fixed(&'static str),
    Session { agent: AgentKind, revision: u64 },
}

pub(crate) fn list(view: Option<&PageView>) -> Vec<Choice> {
    let mut choices: Vec<_> = [
        ("model", "execution-model", "Test model"),
        ("browser", "execution-browser", "Browser task, Messages"),
        ("chat", "execution-chat", "Browser task, Chat Completions"),
        (
            "responses",
            "execution-responses",
            "Browser task, OpenAI Responses",
        ),
        (
            "google",
            "execution-google",
            "Browser task, Google Interactions",
        ),
        (
            "page:messages",
            "execution-page-messages",
            "Page questions, Messages",
        ),
        (
            "page:responses",
            "execution-page-responses",
            "Page questions, OpenAI Responses",
        ),
        (
            "page:google_interactions",
            "execution-page-google",
            "Page questions, Google Interactions",
        ),
        (
            "page:chat_completions",
            "execution-page-chat",
            "Page questions, Chat Completions",
        ),
        ("pi", "execution-pi", "Pi, new session"),
        ("codex", "execution-codex", "Codex, new session"),
        ("opencode", "execution-opencode", "OpenCode, new session"),
    ]
    .into_iter()
    .map(|(value, element, label)| Choice {
        value: value.into(),
        label: label.into(),
        enabled: view.is_some_and(|view| resolve(view, value).is_ok()),
        kind: Kind::Fixed(element),
    })
    .collect();
    let Some(view) = view else { return choices };
    for choice in &mut choices {
        let route = match choice.value.as_str() {
            "browser" => match &view.browser_route {
                Some(BrowserRouteView::Messages { model }) => {
                    choice.label = format!("Browser task, Messages, {model}");
                    None
                }
                Some(BrowserRouteView::HttpsMessages {
                    endpoint, model, ..
                }) => Some((
                    "Browser task",
                    "Messages",
                    endpoint.as_str(),
                    model.as_str(),
                )),
                None => None,
            },
            "chat" => view.chat_route.as_ref().map(
                |ChatRouteView::HttpsChatCompletions {
                     endpoint, model, ..
                 }| {
                    (
                        "Browser task",
                        "Chat Completions",
                        endpoint.as_str(),
                        model.as_str(),
                    )
                },
            ),
            "responses" => view.responses_route.as_ref().map(
                |ResponsesRouteView::HttpsResponses {
                     endpoint, model, ..
                 }| {
                    (
                        "Browser task",
                        "OpenAI Responses",
                        endpoint.as_str(),
                        model.as_str(),
                    )
                },
            ),
            "google" => view.google_route.as_ref().map(
                |GoogleRouteView::HttpsGoogleInteractions {
                     endpoint, model, ..
                 }| {
                    (
                        "Browser task",
                        "Google Interactions",
                        endpoint.as_str(),
                        model.as_str(),
                    )
                },
            ),
            value if value.starts_with("page:") => ProviderProtocol::ALL
                .into_iter()
                .find(|protocol| value.strip_prefix("page:") == Some(protocol.name()))
                .and_then(|protocol| {
                    page_route(view, protocol).map(|(endpoint, model, _)| {
                        ("Page questions", provider_name(protocol), endpoint, model)
                    })
                }),
            _ => None,
        };
        if let Some((task, protocol, endpoint, model)) = route {
            choice.label = format!("{task}, {protocol}, {model}, {endpoint}");
        }
    }
    for (agent, name, sessions) in [
        (AgentKind::Pi, "Pi", &view.agent_sessions),
        (AgentKind::Codex, "Codex", &view.codex_sessions),
        (AgentKind::Opencode, "OpenCode", &view.opencode_sessions),
    ] {
        for session in sessions {
            let id = session.workflow.as_str();
            let value = if agent == AgentKind::Pi {
                id.into()
            } else {
                format!("{}:{id}", agent.name())
            };
            let available = match agent {
                AgentKind::Pi => ready(view, session),
                AgentKind::Codex => codex_ready(view, session),
                AgentKind::Opencode => opencode_ready(view, session),
            };
            if available {
                choices.push(Choice {
                    value,
                    label: format!(
                        "Continue {name} session {}",
                        id.rsplit(':').next().unwrap_or(id)
                    ),
                    enabled: true,
                    kind: Kind::Session {
                        agent,
                        revision: session.revision,
                    },
                });
            }
        }
    }
    choices
}

#[cfg(test)]
mod checks {
    use super::*;
    use crate::model::{CodexProfileView, PiProfileView};

    #[test]
    fn choices_preserve_all_routes_and_only_ready_matching_sessions() {
        let expected = [
            "model",
            "browser",
            "chat",
            "responses",
            "google",
            "page:messages",
            "page:responses",
            "page:google_interactions",
            "page:chat_completions",
            "pi",
            "codex",
            "opencode",
        ];
        let unavailable = list(None);
        assert_eq!(
            unavailable
                .iter()
                .map(|choice| choice.value.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(unavailable.iter().all(|choice| !choice.enabled));
        let mut view = PageView::decode(br#"{"sequence":1,"store":"0123456789abcdef0123456789abcdef","selection":{"store":"0123456789abcdef0123456789abcdef","conversation":"0123456789abcdef0123456789abcdef:100","epoch":"1"},"source":{"id":"0123456789abcdef0123456789abcdef:1","tab":4,"document":"doc-1","url":"http://127.0.0.1:43123/article","title":"North report","text":"North has 17 units."},"runs":[],"native_correction":null,"pi_profile":null,"agent_sessions":[],"browser_route":null}"#).unwrap();
        view.fixture_available = true;
        let endpoint = "https://api.example.test/v1".to_owned();
        let model = "online-model".to_owned();
        let fingerprint = "a".repeat(64);
        view.browser_route = Some(BrowserRouteView::HttpsMessages {
            endpoint: endpoint.clone(),
            model: model.clone(),
            revision: 1,
            fingerprint: fingerprint.clone(),
        });
        view.responses_route = Some(ResponsesRouteView::HttpsResponses {
            endpoint: endpoint.clone(),
            model: model.clone(),
            revision: 1,
            fingerprint: fingerprint.clone(),
        });
        view.chat_route = Some(ChatRouteView::HttpsChatCompletions {
            endpoint: endpoint.clone(),
            model: model.clone(),
            revision: 1,
            fingerprint: fingerprint.clone(),
        });
        view.google_route = Some(GoogleRouteView::HttpsGoogleInteractions {
            endpoint: endpoint.clone(),
            model: model.clone(),
            revision: 1,
            fingerprint: fingerprint.clone(),
        });
        view.pi_profile = Some(PiProfileView {
            fingerprint: fingerprint.clone(),
            provider: "native-provider".into(),
            model,
        });
        view.codex_profile = Some(CodexProfileView {
            fingerprint: fingerprint.clone(),
        });
        view.opencode_profile = view.codex_profile.clone();
        let session = AgentSessionView {
            workflow: serde_json::from_str("\"0123456789abcdef0123456789abcdef:20\"").unwrap(),
            source: view.source().unwrap().clone(),
            profile: fingerprint.clone(),
            revision: 7,
            ready: true,
        };
        view.agent_sessions.push(session.clone());
        view.codex_sessions.push(session.clone());
        view.opencode_sessions.push(session);
        let choices = list(Some(&view));
        assert_eq!(choices.len(), 15);
        assert!(choices.iter().all(|choice| choice.enabled));
        assert_eq!(
            choices[6].label,
            "Page questions, OpenAI Responses, online-model, https://api.example.test/v1"
        );
        assert_eq!(choices[12].value, "0123456789abcdef0123456789abcdef:20");
        assert_eq!(
            choices[13].value,
            "codex:0123456789abcdef0123456789abcdef:20"
        );
        assert_eq!(
            choices[14].value,
            "opencode:0123456789abcdef0123456789abcdef:20"
        );
        assert!(
            choices
                .iter()
                .all(|choice| !choice.label.contains(&fingerprint))
        );
        for phase in 0..3 {
            for sessions in [
                &mut view.agent_sessions,
                &mut view.codex_sessions,
                &mut view.opencode_sessions,
            ] {
                sessions[0].ready = phase != 0;
                sessions[0].profile = if phase == 1 {
                    "b".repeat(64)
                } else {
                    fingerprint.clone()
                };
                if phase == 2 {
                    sessions[0].source =
                        serde_json::from_str("\"0123456789abcdef0123456789abcdef:2\"").unwrap();
                }
            }
            assert_eq!(
                list(Some(&view)).len(),
                12,
                "unavailable native session phase {phase}"
            );
        }
        view.responses_route = None;
        let choices = list(Some(&view));
        assert!(!choices[3].enabled && !choices[6].enabled);
        assert_eq!(choices[3].value, "responses");
        assert_eq!(choices[6].value, "page:responses");
    }
}
