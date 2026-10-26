use crate::model::RunRoute;

pub(crate) fn route(route: &RunRoute) -> String {
    match route {
        RunRoute::LegacyUnspecified => "legacy route unspecified".to_string(),
        RunRoute::Fixture => "fixture".to_string(),
        RunRoute::Messages { model } => format!("Owned model fixture, model {model}"),
        RunRoute::HttpsMessages { model, .. } => format!("Configured HTTPS model, model {model}"),
        RunRoute::HttpsChatCompletions { endpoint, model, .. } => format!("Chat Completions, model {model}, HTTPS endpoint {endpoint}"),
        RunRoute::HttpsResponses { endpoint, model, .. } => format!("OpenAI Responses, model {model}, HTTPS endpoint {endpoint}"),
        RunRoute::HttpsGoogleInteractions { endpoint, model, .. } => format!("Google Interactions, model {model}, HTTPS endpoint {endpoint}"),
        RunRoute::Text { protocol, endpoint, model, .. } => format!("{} page questions, model {model}, HTTPS endpoint {endpoint}. Sends the saved page and completed answers from this page and provider selection.", match protocol {
            crate::model::ProviderProtocol::Messages => "Messages",
            crate::model::ProviderProtocol::Responses => "OpenAI Responses",
            crate::model::ProviderProtocol::GoogleInteractions => "Google Interactions",
            crate::model::ProviderProtocol::ChatCompletions => "Chat Completions",
        }),
        RunRoute::Pi {
            provider, model, ..
        } => format!(
            "Pi configured provider {provider}, model {model}. Pi manages its endpoint, account, tools and permissions."
        ),
        RunRoute::Codex { provider, model, .. } => match (provider, model) {
            (Some(provider), Some(model)) => format!("Codex configured provider {provider}, model {model}. Codex manages its endpoint, account, tools and permissions."),
            _ => "Codex native session. Codex manages its provider, endpoint, account, tools and permissions.".to_string(),
        },
        RunRoute::OpenCode { model, mode, .. } => format!(
            "OpenCode native session{}{}.",
            model.as_ref().map(|value| format!(" using model {value}")).unwrap_or_default(),
            mode.as_ref().map(|value| format!(" in {value} mode")).unwrap_or_default(),
        ),
    }
}
