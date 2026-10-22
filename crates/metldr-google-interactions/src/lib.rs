#![doc = include_str!("../README.md")]

#[cfg(test)]
mod checks;
mod client;
mod sse;
mod stream;

pub use client::Client;
pub use metldr_browser_tools::Invocation;
use metldr_conversation::TextEvent;
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
            return Err("Google Interactions request exceeds bound".into());
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
    /// A final answer returns `None`; its text has already reached `emit`.
    pub async fn next(
        &mut self,
        stop: &AtomicBool,
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<Option<Invocation>> {
        match self.phase {
            Phase::Ready => {}
            Phase::Pending(_) => return Err("Google Interactions tool result missing".into()),
            Phase::Closed => return Err("Google Interactions conversation is closed".into()),
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
            return Err("Google Interactions conversation has no pending tool call".into());
        }
        let output = serde_json::to_string(result)?;
        if output.len() > 131_072 {
            return Err("Google Interactions tool result exceeds bound".into());
        }
        let Phase::Pending(turn) = std::mem::replace(&mut self.phase, Phase::Closed) else {
            unreachable!()
        };
        let invocation = turn
            .call
            .ok_or("Google Interactions turn has no tool call")?;
        let input = self.request["input"]
            .as_array_mut()
            .ok_or("invalid Google Interactions request history")?;
        let name = turn
            .steps
            .iter()
            .find(|step| step["type"] == "function_call")
            .and_then(|step| step["name"].as_str())
            .ok_or("Google function name absent")?
            .to_owned();
        input.extend(turn.steps);
        input.push(
            json!({"type":"function_result","name":name,"call_id":invocation.id,
            "result":[{"type":"text","text":output}]}),
        );
        self.phase = Phase::Ready;
        Ok(())
    }
}
