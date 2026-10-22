use crate::{Result, TextEvent, Usage, reasoning};
use metldr_browser_tools::{Invocation, decode_call};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const CALL_MAX: usize = 32;
const ID_MAX: usize = 256;
const NAME_MAX: usize = 128;
const ARG_MAX: usize = 65_536;
const TEXT_MAX: usize = 8_192;

#[derive(Default)]
pub(super) struct State {
    response_id: Option<String>,
    role_seen: bool,
    assistant: Map<String, Value>,
    reasoning_ids: BTreeMap<String, Map<String, Value>>,
    text: String,
    content_string_seen: bool,
    calls: BTreeMap<u64, PartialCall>,
    finish: Option<String>,
    usage: Option<Usage>,
    usage_seen: bool,
    done: bool,
}

#[derive(Default)]
struct PartialCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

pub(super) struct Turn {
    pub assistant: Value,
    pub calls: Vec<Invocation>,
    pub text: String,
    pub usage: Option<Usage>,
}

impl State {
    pub fn frame(&mut self, event: &str, data: &str) -> Result<Option<TextEvent>> {
        if !matches!(event, "" | "message") || self.done {
            return Err("invalid Chat stream event".into());
        }
        if data == "[DONE]" {
            if self.done || self.finish.is_none() {
                return Err("Chat stream ended without a finish reason".into());
            }
            self.done = true;
            return Ok(None);
        }
        let root: Value = serde_json::from_str(data).map_err(|_| "invalid Chat stream JSON")?;
        if root.get("error").is_some_and(|error| !error.is_null()) {
            return Err("Chat stream returned an error".into());
        }
        let response_id = root
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= ID_MAX && !id.chars().any(char::is_control))
            .ok_or("invalid Chat response identity")?;
        if self
            .response_id
            .as_deref()
            .is_some_and(|old| old != response_id)
        {
            return Err("Chat response identity changed".into());
        }
        self.response_id
            .get_or_insert_with(|| response_id.to_owned());

        let usage_present = root.get("usage").is_some_and(|usage| !usage.is_null());
        if let Some(usage) = root.get("usage").filter(|usage| !usage.is_null()) {
            let parsed = parse_usage(usage)?;
            if self.usage_seen && self.usage != Some(parsed) {
                return Err("Chat usage changed".into());
            }
            self.usage_seen = true;
            self.usage = Some(parsed);
        }
        let choices = root
            .get("choices")
            .and_then(Value::as_array)
            .ok_or("invalid Chat choices")?;
        if choices.is_empty() {
            if !usage_present {
                return Err("unexpected empty Chat choices".into());
            }
            return Ok(None);
        }
        if choices.len() != 1 || choices[0].get("index").and_then(Value::as_u64) != Some(0) {
            return Err("invalid Chat choice index".into());
        }
        let choice = &choices[0];
        let delta = choice
            .get("delta")
            .and_then(Value::as_object)
            .ok_or("invalid Chat delta")?;
        if self.finish.is_some() && delta_has_output(delta) {
            return Err("Chat output followed its finish reason".into());
        }
        if let Some(role) = delta.get("role") {
            if role.as_str() != Some("assistant") {
                return Err("invalid Chat assistant role".into());
            }
            self.assistant.insert("role".into(), json!("assistant"));
            self.role_seen = true;
        }
        if let Some(content) = delta.get("content") {
            if !content.is_null() && !content.is_string() {
                return Err("invalid Chat content delta".into());
            }
            if let Some(fragment) = content.as_str() {
                self.content_string_seen = true;
                let new_len = self
                    .text
                    .len()
                    .checked_add(fragment.len())
                    .ok_or("Chat text exceeds bound")?;
                if new_len > TEXT_MAX {
                    return Err("Chat text exceeds bound".into());
                }
                self.text.push_str(fragment);
            } else if !self.assistant.contains_key("content") {
                self.assistant.insert("content".into(), Value::Null);
            }
        }
        for field in ["reasoning", "reasoning_content", "reasoning_text"] {
            if let Some(piece) = delta.get(field) {
                if piece.is_null() {
                    self.assistant.entry(field).or_insert(Value::Null);
                } else {
                    let piece = piece.as_str().ok_or("invalid Chat reasoning delta")?;
                    let current_len = self
                        .assistant
                        .get(field)
                        .and_then(Value::as_str)
                        .map_or(0, str::len);
                    if current_len.saturating_add(piece.len()) > ARG_MAX {
                        return Err("Chat reasoning exceeds bound".into());
                    }
                    match self.assistant.get_mut(field) {
                        Some(Value::String(current)) => current.push_str(piece),
                        _ => {
                            self.assistant.insert(field.into(), json!(piece));
                        }
                    }
                }
            }
        }
        if let Some(details) = delta
            .get("reasoning_details")
            .filter(|details| !details.is_null())
        {
            let collected = self
                .assistant
                .entry("reasoning_details")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("invalid accumulated Chat reasoning details")?;
            reasoning::append(collected, &mut self.reasoning_ids, details)?;
        }
        if let Some(tool_calls) = delta.get("tool_calls").filter(|calls| !calls.is_null()) {
            let tool_calls = tool_calls.as_array().ok_or("invalid Chat tool calls")?;
            for item in tool_calls {
                self.add_call_delta(item)?;
            }
        }
        if let Some(reason) = choice
            .get("finish_reason")
            .filter(|reason| !reason.is_null())
        {
            let reason = reason.as_str().ok_or("invalid Chat finish reason")?;
            if let Some(old) = &self.finish {
                if old != reason || delta_has_output(delta) {
                    return Err("Chat output followed its finish reason".into());
                }
            } else {
                self.finish = Some(reason.to_owned());
            }
        } else if self.finish.is_some() && delta_has_output(delta) {
            return Err("Chat output followed its finish reason".into());
        }
        Ok(delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|text| TextEvent::Delta(text.to_owned())))
    }

    fn add_call_delta(&mut self, item: &Value) -> Result<()> {
        let item = item.as_object().ok_or("invalid Chat tool delta")?;
        let index = item
            .get("index")
            .and_then(Value::as_u64)
            .filter(|i| *i < CALL_MAX as u64)
            .ok_or("invalid Chat tool index")?;
        if !self.calls.contains_key(&index) && self.calls.len() >= CALL_MAX {
            return Err("too many Chat tool calls".into());
        }
        let call = self.calls.entry(index).or_default();
        if let Some(id) = item.get("id") {
            let id = id
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
                .ok_or("invalid Chat tool identity")?;
            if call.id.as_deref().is_some_and(|old| old != id) {
                return Err("Chat tool identity changed".into());
            }
            call.id.get_or_insert_with(|| id.to_owned());
        }
        if item
            .get("type")
            .is_some_and(|kind| kind.as_str() != Some("function"))
        {
            return Err("unsupported Chat tool type".into());
        }
        if let Some(function) = item.get("function") {
            let function = function.as_object().ok_or("invalid Chat function delta")?;
            if let Some(name) = function.get("name") {
                let name = name
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= NAME_MAX)
                    .ok_or("invalid Chat function name")?;
                if call.name.is_empty() {
                    call.name.push_str(name);
                } else if call.name != name {
                    return Err("Chat function name changed".into());
                }
            }
            if let Some(arguments) = function.get("arguments") {
                let arguments = arguments
                    .as_str()
                    .ok_or("invalid Chat function arguments")?;
                if call.arguments.len().saturating_add(arguments.len()) > ARG_MAX {
                    return Err("Chat function arguments exceed bound".into());
                }
                call.arguments.push_str(arguments);
            }
            if function
                .keys()
                .any(|key| !matches!(key.as_str(), "name" | "arguments"))
            {
                return Err("unsupported Chat function delta".into());
            }
        }
        if item
            .keys()
            .any(|key| !matches!(key.as_str(), "index" | "id" | "type" | "function"))
        {
            return Err("unsupported Chat tool delta".into());
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<Turn> {
        if !self.done {
            return Err("Chat stream is incomplete".into());
        }
        if !self.role_seen {
            return Err("Chat assistant role is missing".into());
        }
        let finish = self
            .finish
            .as_deref()
            .ok_or("Chat stream has no finish reason")?;
        let mut calls = Vec::with_capacity(self.calls.len());
        for expected in 0..self.calls.len() as u64 {
            let partial = self
                .calls
                .get(&expected)
                .ok_or("Chat tool indexes are not contiguous")?;
            let id = partial
                .id
                .as_ref()
                .ok_or("Chat tool identity is incomplete")?;
            if partial.name.is_empty() {
                return Err("Chat function name is incomplete".into());
            }
            let arguments: Value = serde_json::from_str(&partial.arguments)
                .map_err(|_| "invalid Chat function arguments")?;
            let call = decode_call(&partial.name, arguments)?;
            if calls.iter().any(|prior: &Invocation| prior.id == *id) {
                return Err("duplicate Chat tool identity".into());
            }
            calls.push(Invocation {
                id: id.clone(),
                call,
            });
        }
        match finish {
            "tool_calls" if calls.is_empty() => return Err("Chat finish has no tool calls".into()),
            "stop" if !calls.is_empty() && !has_encrypted_reasoning(&self.assistant) => {
                return Err("Chat stop with tool calls lacks encrypted reasoning".into());
            }
            "stop" if calls.is_empty() && self.text.is_empty() => {
                return Err("Chat stopped without an answer".into());
            }
            "stop" | "tool_calls" => (),
            _ => return Err("Chat stream did not finish successfully".into()),
        }
        self.assistant.insert(
            "content".into(),
            if self.content_string_seen {
                json!(self.text)
            } else {
                Value::Null
            },
        );
        if !calls.is_empty() {
            self.assistant.insert("tool_calls".into(), Value::Array(self.calls.values().map(|partial| {
                json!({"id":partial.id,"type":"function","function":{"name":partial.name,"arguments":partial.arguments}})
            }).collect()));
        }
        Ok(Turn {
            assistant: Value::Object(self.assistant),
            calls,
            text: self.text,
            usage: self.usage,
        })
    }
}

fn parse_usage(value: &Value) -> Result<Usage> {
    let object = value.as_object().ok_or("invalid Chat usage")?;
    let count = |key: &str| optional_count(object.get(key));
    let cached_nested = detail_count(object.get("prompt_tokens_details"), "cached_tokens")?;
    let cached_direct = count("prompt_cache_hit_tokens")?;
    if cached_nested.is_some() && cached_direct.is_some() && cached_nested != cached_direct {
        return Err("conflicting Chat cached usage".into());
    }
    let cached_input_tokens = cached_nested.or(cached_direct);
    let reasoning_tokens =
        detail_count(object.get("completion_tokens_details"), "reasoning_tokens")?;
    Ok(Usage {
        input_tokens: count("prompt_tokens")?,
        output_tokens: count("completion_tokens")?,
        total_tokens: count("total_tokens")?,
        cached_input_tokens,
        reasoning_tokens,
    })
}

fn optional_count(value: Option<&Value>) -> Result<Option<u64>> {
    value
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| "invalid Chat usage count".into())
        })
        .transpose()
}

fn detail_count(value: Option<&Value>, key: &str) -> Result<Option<u64>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = value.as_object().ok_or("invalid Chat usage details")?;
    optional_count(object.get(key))
}

fn has_encrypted_reasoning(assistant: &Map<String, Value>) -> bool {
    assistant
        .get("reasoning_details")
        .and_then(Value::as_array)
        .is_some_and(|details| {
            details.iter().any(|detail| {
                detail.get("type").and_then(Value::as_str) == Some("reasoning.encrypted")
                    && detail
                        .get("data")
                        .and_then(Value::as_str)
                        .is_some_and(|data| !data.is_empty())
            })
        })
}

fn delta_has_output(delta: &Map<String, Value>) -> bool {
    delta.iter().any(|(key, value)| match key.as_str() {
        "content" | "reasoning" | "reasoning_content" | "reasoning_text" => {
            value.as_str().is_some_and(|s| !s.is_empty())
        }
        "reasoning_details" | "tool_calls" => value.as_array().is_some_and(|a| !a.is_empty()),
        _ => false,
    })
}
