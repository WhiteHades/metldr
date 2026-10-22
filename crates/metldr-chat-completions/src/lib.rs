#![doc = include_str!("../README.md")]

#[cfg(test)]
mod checks;
mod client;
mod reasoning;
mod sse;
mod stream;
#[cfg(test)]
mod stream_checks;

pub use client::Client;
use metldr_browser_tools::Call;
pub use metldr_browser_tools::Invocation;
pub use metldr_conversation::TextEvent;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::AtomicBool,
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const REQUEST_MAX: usize = 131_072;

pub struct Prompt<'a> {
    pub instruction: &'a str,
    pub title: &'a str,
    pub url: &'a str,
    pub text: &'a str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

pub struct Conversation {
    client: Client,
    request: Value,
    phase: Phase,
    seen: BTreeMap<String, Call>,
    usage: Option<Usage>,
}

enum Phase {
    Ready,
    Queued(VecDeque<Invocation>),
    Awaiting {
        invocation: Invocation,
        rest: VecDeque<Invocation>,
    },
    Closed,
}

impl Client {
    pub fn start(self, prompt: Prompt<'_>) -> Result<Conversation> {
        let request = self.request(prompt);
        bound_request(&request)?;
        Ok(Conversation {
            client: self,
            request,
            phase: Phase::Ready,
            seen: BTreeMap::new(),
            usage: None,
        })
    }
}

impl Conversation {
    /// Releases calls serially only after the entire response passes validation.
    pub async fn next(
        &mut self,
        stop: &AtomicBool,
        remaining_calls: usize,
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<Option<Invocation>> {
        if let Err(error) = client::cancelled(stop) {
            self.phase = Phase::Closed;
            return Err(error);
        }
        let mut calls = match std::mem::replace(&mut self.phase, Phase::Closed) {
            Phase::Ready => {
                let turn = self.client.send(&self.request, stop, emit).await?;
                self.usage = turn.usage;
                if turn.calls.is_empty() {
                    return Ok(None);
                }
                if turn.calls.len() > remaining_calls {
                    return Err("Chat tool batch exceeds remaining call budget".into());
                }
                for invocation in &turn.calls {
                    if self
                        .seen
                        .get(&invocation.id)
                        .is_some_and(|call| call != &invocation.call)
                    {
                        return Err("Chat reused a tool identity with changed arguments".into());
                    }
                }
                for invocation in &turn.calls {
                    self.seen
                        .insert(invocation.id.clone(), invocation.call.clone());
                }
                self.request["messages"]
                    .as_array_mut()
                    .expect("private Chat history")
                    .push(turn.assistant);
                bound_request(&self.request)?;
                turn.calls.into()
            }
            Phase::Queued(calls) => calls,
            Phase::Awaiting { .. } => return Err("Chat tool result missing".into()),
            Phase::Closed => return Err("Chat conversation is closed".into()),
        };
        if calls.len() > remaining_calls {
            return Err("Chat tool batch exceeds remaining call budget".into());
        }
        client::cancelled(stop)?;
        let invocation = calls.pop_front().expect("validated queued Chat call");
        self.phase = Phase::Awaiting {
            invocation: invocation.clone(),
            rest: calls,
        };
        Ok(Some(invocation))
    }

    pub fn submit_tool_result(&mut self, result: &Value) -> Result<()> {
        let Phase::Awaiting { invocation, rest } =
            std::mem::replace(&mut self.phase, Phase::Closed)
        else {
            return Err("Chat conversation has no pending tool call".into());
        };
        let content = serde_json::to_string(result)?;
        if content.len() > REQUEST_MAX {
            return Err("Chat tool result exceeds bound".into());
        }
        self.request["messages"]
            .as_array_mut()
            .expect("private Chat history")
            .push(json!({
                "role":"tool", "tool_call_id":invocation.id, "content":content,
            }));
        bound_request(&self.request)?;
        if rest.is_empty() {
            self.phase = Phase::Ready;
        } else {
            self.phase = Phase::Queued(rest);
        }
        Ok(())
    }

    /// Usage from the most recent completed HTTP response, without estimated cost.
    pub fn usage(&self) -> Option<Usage> {
        self.usage
    }
}

fn bound_request(request: &Value) -> Result<()> {
    if serde_json::to_vec(request)?.len() > REQUEST_MAX {
        return Err("Chat request exceeds bound".into());
    }
    Ok(())
}
