#![doc = include_str!("../README.md")]

#[cfg(test)]
mod checks;
mod client;
mod generated;
mod sse;
mod stream;

pub use client::Client;
pub use client::{
    Presentation, PresentationContent, PresentationConversation, PresentationInvocation,
    PresentationProgress,
};
pub use metldr_browser_tools::Invocation;
use metldr_browser_tools::decode_call;
pub use metldr_conversation::TextEvent;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const TEXT_MAX: usize = 8_192;

pub struct Prompt<'a> {
    pub instruction: &'a str,
    pub title: &'a str,
    pub url: &'a str,
    pub text: &'a str,
}

pub struct Conversation {
    client: Client,
    request: Value,
    phase: Phase,
}

enum Phase {
    Ready,
    Pending(stream::Turn),
    Closed,
}

impl Client {
    pub fn start(self, prompt: Prompt<'_>) -> Result<Conversation> {
        let request = self.request(prompt);
        if serde_json::to_vec(&request)?.len() > 131_072 {
            return Err("Responses request exceeds bound".into());
        }
        Ok(Conversation {
            client: self,
            request,
            phase: Phase::Ready,
        })
    }
}

impl Conversation {
    /// Returns a call only after successful response and HTTP completion.
    /// A final answer returns `None`; its ordered text events have reached `emit`.
    pub async fn next(
        &mut self,
        stop: &AtomicBool,
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<Option<Invocation>> {
        match self.phase {
            Phase::Ready => {}
            Phase::Pending(_) => return Err("Responses tool result missing".into()),
            Phase::Closed => return Err("Responses conversation is closed".into()),
        }
        // An interrupted or failed request must never be replayed implicitly.
        self.phase = Phase::Closed;
        let turn = self
            .client
            .send(&self.request, stop, emit, client::BROWSER_BOUNDS, |_| {
                Err("unexpected Responses presentation progress".into())
            })
            .await?;
        let invocation = if let Some(call) = turn.call.as_ref() {
            Some(Invocation {
                id: call.id.clone(),
                call: decode_call(&call.name, call.arguments.clone())?,
            })
        } else {
            None
        };
        if invocation.is_some() {
            self.phase = Phase::Pending(turn);
        }
        Ok(invocation)
    }

    pub fn submit_tool_result(&mut self, result: &Value) -> Result<()> {
        if !matches!(self.phase, Phase::Pending(_)) {
            return Err("Responses conversation has no pending tool call".into());
        }
        let output = serde_json::to_string(result)?;
        if output.len() > 131_072 {
            return Err("Responses tool result exceeds bound".into());
        }
        let Phase::Pending(turn) = std::mem::replace(&mut self.phase, Phase::Closed) else {
            unreachable!()
        };
        let invocation = turn.call.ok_or("Responses turn has no tool call")?;
        let input = self.request["input"]
            .as_array_mut()
            .ok_or("invalid Responses request history")?;
        input.extend(turn.output);
        input.push(
            json!({"type":"function_call_output","call_id":invocation.id,
            "output":output}),
        );
        self.phase = Phase::Ready;
        Ok(())
    }
}
