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
        match route {
            Route::Messages(profile) => {
                let conversation =
                    profile
                        .messages_client(stop)?
                        .start(metldr_messages::Prompt {
                            instruction,
                            title: &source.title,
                            url: &source.url,
                            text: &source.text,
                        })?;
                Ok(Self::Messages { conversation })
            }
            Route::Google(profile) => {
                let conversation =
                    profile
                        .google_client(stop)?
                        .start(metldr_google_interactions::Prompt {
                            instruction,
                            title: &source.title,
                            url: &source.url,
                            text: &source.text,
                        })?;
                Ok(Self::Google { conversation })
            }
            Route::Responses(profile) => {
                let conversation =
                    profile
                        .responses_client(stop)?
                        .start(metldr_responses::Prompt {
                            instruction,
                            title: &source.title,
                            url: &source.url,
                            text: &source.text,
                        })?;
                Ok(Self::Responses { conversation })
            }
            Route::Chat(profile) => {
                let conversation =
                    profile
                        .chat_client(stop)?
                        .start(metldr_chat_completions::Prompt {
                            instruction,
                            title: &source.title,
                            url: &source.url,
                            text: &source.text,
                        })?;
                Ok(Self::Chat { conversation })
            }
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
