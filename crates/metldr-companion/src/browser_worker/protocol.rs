use super::{Invocation, Route};
use crate::{Result, model::Page};
use serde_json::Value;
use std::sync::atomic::AtomicBool;

pub(super) enum Protocol {
    Messages {
        conversation: metldr_messages::Conversation,
    },
    Google {
        conversation: metldr_google_interactions::Conversation,
    },
    Responses {
        conversation: metldr_responses::Conversation,
    },
    Chat {
        conversation: metldr_chat_completions::Conversation,
    },
}

impl Protocol {
    pub fn open(
        route: &Route,
        source: &Page,
        instruction: &str,
        stop: &AtomicBool,
    ) -> Result<Self> {
        let prompt = metldr_browser_tools::Prompt {
            instruction,
            title: &source.title,
            url: &source.url,
            text: &source.text,
        };
        match route {
            Route::Messages(profile) => Ok(Self::Messages {
                conversation: profile.messages_client(stop)?.start(prompt)?,
            }),
            Route::Google(profile) => Ok(Self::Google {
                conversation: profile.google_client(stop)?.start(prompt)?,
            }),
            Route::Responses(profile) => Ok(Self::Responses {
                conversation: profile.responses_client(stop)?.start(prompt)?,
            }),
            Route::Chat(profile) => Ok(Self::Chat {
                conversation: profile.chat_client(stop)?.start(prompt)?,
            }),
        }
    }

    pub async fn next(
        &mut self,
        stop: &AtomicBool,
        remaining_calls: usize,
        emit: impl FnMut(metldr_conversation::TextEvent) -> Result<()>,
    ) -> Result<Option<Invocation>> {
        let invocation = match self {
            Self::Messages { conversation } => conversation.next(stop, emit).await,
            Self::Responses { conversation } => conversation.next(stop, emit).await,
            Self::Google { conversation } => conversation.next(stop, emit).await,
            Self::Chat { conversation } => conversation.next(stop, remaining_calls, emit).await,
        }?;
        if invocation.is_some() && remaining_calls == 0 {
            return Err("browser task step limit reached".into());
        }
        Ok(invocation)
    }

    pub fn continuation(&mut self, result: &Value) -> Result<()> {
        match self {
            Self::Messages { conversation } => conversation.submit_tool_result(result),
            Self::Responses { conversation } => conversation.submit_tool_result(result),
            Self::Google { conversation } => conversation.submit_tool_result(result),
            Self::Chat { conversation } => conversation.submit_tool_result(result),
        }
    }
}
