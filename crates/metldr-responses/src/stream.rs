use crate::Result;
use metldr_conversation::TextEvent;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) struct Turn {
    pub output: Vec<Value>,
    pub call: Option<FunctionCall>,
    pub text: String,
    pub deferred: Vec<TextEvent>,
}

pub(crate) struct FunctionCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Default)]
struct Text {
    value: String,
    done: bool,
    emitted: usize,
}

struct Item {
    initial: Value,
    done: Option<Value>,
    arguments: String,
    arguments_done: bool,
    text: BTreeMap<u64, Text>,
}

#[derive(Default)]
pub(super) struct State {
    identity: Option<(String, String)>,
    sequence: Option<u64>,
    items: BTreeMap<u64, Item>,
    terminal: Option<Vec<Value>>,
    events: usize,
    text_bytes: usize,
    emit_item: u64,
    emit_part: u64,
    limits: Limits,
}

struct Limits {
    arguments: usize,
    events: usize,
    pause_at_call: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            arguments: 16_384,
            events: 4_096,
            pause_at_call: false,
        }
    }
}

impl State {
    pub fn with_limits(arguments: usize, events: usize, pause_at_call: bool) -> Self {
        Self {
            limits: Limits {
                arguments,
                events,
                pause_at_call,
            },
            ..Self::default()
        }
    }
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| "invalid Responses string field".into())
}

fn identity(value: &Value, key: &str) -> Result<String> {
    let text = string(value, key)?;
    if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
        return Err("invalid Responses identity".into());
    }
    Ok(text.into())
}

fn index(value: &Value, key: &str, bound: u64) -> Result<u64> {
    value[key]
        .as_u64()
        .filter(|v| *v < bound)
        .ok_or_else(|| "invalid Responses output index".into())
}

impl State {
    fn item(&mut self, value: &Value, kind: &str) -> Result<&mut Item> {
        let index = index(value, "output_index", 64)?;
        let item = self
            .items
            .get_mut(&index)
            .ok_or("unknown Responses output item")?;
        if item.done.is_some()
            || item.initial["type"] != kind
            || item.initial["id"].as_str() != Some(string(value, "item_id")?)
        {
            return Err("Responses output item identity changed".into());
        }
        Ok(item)
    }

    pub fn frame(&mut self, event: &str, data: &str) -> Result<Vec<TextEvent>> {
        if self.terminal.is_some() {
            return Err("Responses event after completion".into());
        }
        self.events += 1;
        if self.events > self.limits.events {
            return Err("Responses event count exceeds bound".into());
        }
        let value: Value =
            serde_json::from_str(data).map_err(|_| "invalid Responses event JSON")?;
        let kind = string(&value, "type")?;
        if !event.is_empty() && event != kind {
            return Err("Responses event envelope mismatch".into());
        }
        let sequence = value["sequence_number"]
            .as_u64()
            .ok_or("missing Responses sequence")?;
        if self.sequence.is_some_and(|previous| sequence <= previous) {
            return Err("Responses event sequence repeated or reversed".into());
        }
        self.sequence = Some(sequence);
        if kind == "response.created" {
            if self.identity.is_some() {
                return Err("duplicate Responses creation".into());
            }
            let response = &value["response"];
            if response["object"] != "response" || response["store"] != false {
                return Err("invalid Responses creation envelope".into());
            }
            self.identity = Some((identity(response, "id")?, identity(response, "model")?));
            return Ok(Vec::new());
        }
        if self.identity.is_none() {
            return Err("Responses event before creation".into());
        }
        match kind {
            "response.output_item.added" => {
                let at = index(&value, "output_index", 64)?;
                let initial = value["item"].clone();
                let id = identity(&initial, "id")?;
                if self.items.contains_key(&at)
                    || self.items.values().any(|i| i.initial["id"] == id)
                    || !matches!(
                        initial["type"].as_str(),
                        Some("message" | "reasoning" | "function_call")
                    )
                {
                    return Err("duplicate or unsupported Responses output item".into());
                }
                let arguments = if initial["type"] == "function_call" {
                    identity(&initial, "call_id")?;
                    identity(&initial, "name")?;
                    string(&initial, "arguments")?.to_owned()
                } else {
                    String::new()
                };
                if arguments.len() > self.limits.arguments {
                    return Err("Responses arguments exceed bound".into());
                }
                self.items.insert(
                    at,
                    Item {
                        initial,
                        done: None,
                        arguments,
                        arguments_done: false,
                        text: BTreeMap::new(),
                    },
                );
            }
            "response.function_call_arguments.delta" => {
                let argument_limit = self.limits.arguments;
                let item = self.item(&value, "function_call")?;
                let delta = string(&value, "delta")?;
                if item.arguments_done
                    || item.arguments.len().saturating_add(delta.len()) > argument_limit
                {
                    return Err("invalid Responses argument delta".into());
                }
                item.arguments.push_str(delta);
            }
            "response.function_call_arguments.done" => {
                let item = self.item(&value, "function_call")?;
                if item.arguments_done || item.arguments != string(&value, "arguments")? {
                    return Err("Responses completed arguments differ".into());
                }
                item.arguments_done = true;
            }
            "response.output_text.delta" | "response.output_text.done" => {
                let at = index(&value, "content_index", 16)?;
                if kind.ends_with(".delta") {
                    let delta = string(&value, "delta")?;
                    self.text_bytes = self.text_bytes.saturating_add(delta.len());
                    if self.text_bytes > crate::TEXT_MAX {
                        return Err("Responses text exceeds bound".into());
                    }
                    let text = self.item(&value, "message")?.text.entry(at).or_default();
                    if text.done {
                        return Err("Responses text after completion".into());
                    }
                    text.value.push_str(delta);
                } else {
                    let text = self.item(&value, "message")?.text.entry(at).or_default();
                    if text.done || text.value != string(&value, "text")? {
                        return Err("Responses completed text differs".into());
                    }
                    text.done = true;
                }
            }
            "response.output_item.done" => {
                let at = index(&value, "output_index", 64)?;
                let item = self
                    .items
                    .get_mut(&at)
                    .ok_or("unknown completed Responses item")?;
                let done = &value["item"];
                if item.done.is_some()
                    || done["id"] != item.initial["id"]
                    || done["type"] != item.initial["type"]
                {
                    return Err("Responses completed item identity differs".into());
                }
                match done["type"].as_str() {
                    Some("function_call") => {
                        if !item.arguments_done
                            || done
                                .get("status")
                                .is_some_and(|status| status != "completed")
                            || done["call_id"] != item.initial["call_id"]
                            || done["name"] != item.initial["name"]
                            || string(done, "arguments")? != item.arguments
                        {
                            return Err("Responses completed call differs".into());
                        }
                    }
                    Some("message") => {
                        if done["status"] != "completed" || done["role"] != "assistant" {
                            return Err("invalid Responses assistant message".into());
                        }
                        let content = done["content"]
                            .as_array()
                            .ok_or("invalid Responses message content")?;
                        if content.len() != item.text.len() {
                            return Err("Responses text content mismatch".into());
                        }
                        for (at, part) in content.iter().enumerate() {
                            let text = item
                                .text
                                .get(&(at as u64))
                                .ok_or("missing Responses text")?;
                            if part["type"] != "output_text"
                                || !text.done
                                || part["text"] != text.value
                            {
                                return Err("Responses text content mismatch or refusal".into());
                            }
                        }
                    }
                    Some("reasoning") => {}
                    _ => return Err("unsupported Responses completed item".into()),
                }
                item.done = Some(done.clone());
            }
            "response.completed" => {
                let response = &value["response"];
                let (id, model) = self.identity.as_ref().ok_or("missing Responses identity")?;
                if response["id"] != *id
                    || response["model"] != *model
                    || response["object"] != "response"
                    || response["store"] != false
                    || response["status"] != "completed"
                    || !response["error"].is_null()
                    || !response["incomplete_details"].is_null()
                {
                    return Err("invalid Responses terminal envelope".into());
                }
                let output = response["output"]
                    .as_array()
                    .ok_or("missing Responses terminal output")?;
                if output.len() != self.items.len() {
                    return Err("Responses terminal output count differs".into());
                }
                for (at, done) in output.iter().enumerate() {
                    if self.items.get(&(at as u64)).and_then(|i| i.done.as_ref()) != Some(done) {
                        return Err("Responses terminal output differs".into());
                    }
                }
                self.terminal = Some(output.clone());
            }
            "response.failed" | "response.incomplete" | "error" => {
                return Err("Responses did not complete successfully".into());
            }
            "response.in_progress"
            | "response.queued"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.output_text.annotation.added" => {}
            _ => return Err("unsupported Responses event".into()),
        }
        Ok(self.emit_ready())
    }

    fn emit_ready(&mut self) -> Vec<TextEvent> {
        let mut output = Vec::new();
        while let Some(item) = self.items.get_mut(&self.emit_item) {
            if self.limits.pause_at_call && item.initial["type"] == "function_call" {
                break;
            }
            if item.initial["type"] != "message" {
                if item.done.is_none() {
                    break;
                }
                self.emit_item += 1;
                self.emit_part = 0;
                continue;
            }
            if let Some(text) = item.text.get_mut(&self.emit_part) {
                if text.emitted < text.value.len() {
                    output.push(TextEvent::Delta(text.value[text.emitted..].to_owned()));
                }
                text.emitted = text.value.len();
                if !text.done {
                    break;
                }
                self.emit_part += 1;
            } else if item.done.is_some() && self.emit_part >= item.text.len() as u64 {
                let snapshot = item.text.values().fold(String::new(), |mut output, part| {
                    output.push_str(&part.value);
                    output
                });
                if !snapshot.is_empty() {
                    output.push(TextEvent::Final(snapshot));
                }
                self.emit_item += 1;
                self.emit_part = 0;
            } else {
                break;
            }
        }
        output
    }

    pub fn finish(mut self) -> Result<Turn> {
        let output = self
            .terminal
            .take()
            .ok_or("Responses stream ended without completion")?;
        let mut text = String::new();
        let mut call = None;
        for item in &output {
            if item["type"] == "message" {
                for part in item["content"]
                    .as_array()
                    .ok_or("invalid Responses message")?
                {
                    text.push_str(string(part, "text")?);
                }
            } else if item["type"] == "function_call" {
                if call.is_some() {
                    return Err("multiple Responses tool calls".into());
                }
                let id = identity(item, "call_id")?;
                let arguments = serde_json::from_str(string(item, "arguments")?)
                    .map_err(|_| "invalid Responses function arguments JSON")?;
                call = Some(FunctionCall {
                    id,
                    name: string(item, "name")?.into(),
                    arguments,
                });
            }
        }
        if call.is_none() && text.is_empty() {
            return Err("Responses completed without a result".into());
        }
        self.limits.pause_at_call = false;
        let deferred = self.emit_ready();
        Ok(Turn {
            output,
            call,
            text,
            deferred,
        })
    }
}
