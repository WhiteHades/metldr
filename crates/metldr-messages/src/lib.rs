#![doc = include_str!("../README.md")]

#[cfg(test)]
mod checks;
mod client;
mod sse;
mod stream;

pub use client::Client;
pub use metldr_browser_tools::Invocation;
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
            return Err("Messages request exceeds bound".into());
        }
        Ok(Conversation {
            client: self,
            request,
            phase: Phase::Ready,
        })
    }
}

impl Conversation {
    /// Returns a call only after successful message and HTTP completion.
    /// A final answer returns `None`; text deltas and its final snapshot reach `emit`.
    pub async fn next(
        &mut self,
        stop: &AtomicBool,
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<Option<Invocation>> {
        match self.phase {
            Phase::Ready => {}
            Phase::Pending(_) => return Err("Messages tool result missing".into()),
            Phase::Closed => return Err("Messages conversation is closed".into()),
        }
        // An interrupted or failed request must never be replayed implicitly.
        self.phase = Phase::Closed;
        let turn = self.client.send(&self.request, stop, emit).await?;
        let invocation = turn.call.clone();
        if invocation.is_some() {
            self.phase = Phase::Pending(turn);
        }
        Ok(invocation)
    }

    pub fn submit_tool_result(&mut self, result: &Value) -> Result<()> {
        if !matches!(self.phase, Phase::Pending(_)) {
            return Err("Messages conversation has no pending tool call".into());
        }
        let output = serde_json::to_string(result)?;
        if output.len() > 131_072 {
            return Err("Messages tool result exceeds bound".into());
        }
        let Phase::Pending(turn) = std::mem::replace(&mut self.phase, Phase::Closed) else {
            unreachable!()
        };
        let invocation = turn.call.ok_or("Messages turn has no tool call")?;
        let messages = self.request["messages"]
            .as_array_mut()
            .ok_or("invalid Messages request history")?;
        messages.push(json!({"role":"assistant","content":turn.content}));
        messages.push(json!({"role":"user","content":[{
            "type":"tool_result","tool_use_id":invocation.id,"content":output
        }]}));
        self.phase = Phase::Ready;
        Ok(())
    }
}
