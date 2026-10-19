use crate::{Invocation, Result};
use metldr_browser_tools::decode_call;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) struct Turn {
    pub steps: Vec<Value>,
    pub call: Option<Invocation>,
    pub text: String,
}

struct Step {
    value: Value,
    arguments: String,
    text: String,
    emitted: usize,
    closed: bool,
}

#[derive(Default)]
pub(super) struct State {
    identity: Option<(String, Option<String>)>,
    steps: BTreeMap<u64, Step>,
    terminal: Option<String>,
    announced_terminal: Option<String>,
    done: bool,
    events: usize,
    text_bytes: usize,
    cursor: u64,
}

fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| "invalid Google Interactions string".into())
}

fn identity(v: &Value, key: &str) -> Result<String> {
    let s = string(v, key)?;
    if s.is_empty() || s.len() > 256 || s.chars().any(char::is_control) {
        return Err("invalid Google Interactions identity".into());
    }
    Ok(s.into())
}

fn parts_text(value: &Value) -> Result<String> {
    let mut text = String::new();
    for part in value.as_array().ok_or("invalid Google text content")? {
        if part["type"] != "text" {
            return Err("unsupported Google content".into());
        }
        text.push_str(string(part, "text")?);
        if text.len() > 16_384 {
            return Err("Google text content exceeds bound".into());
        }
    }
    Ok(text)
}

impl State {
    pub fn frame(&mut self, event: &str, data: &str) -> Result<Option<String>> {
        self.events += 1;
        if self.events > 4096 {
            return Err("Google event count exceeds bound".into());
        }
        if data == "[DONE]" {
            if self.terminal.is_none() || self.done || !matches!(event, "" | "done") {
                return Err("unexpected Google DONE trailer".into());
            }
            self.done = true;
            return Ok(None);
        }
        if self.terminal.is_some() {
            return Err("Google event after completion".into());
        }
        let value: Value = serde_json::from_str(data).map_err(|_| "invalid Google event JSON")?;
        let kind = string(&value, "event_type")?;
        if !event.is_empty() && event != kind {
            return Err("Google event envelope mismatch".into());
        }
        if kind == "interaction.created" {
            if self.identity.is_some() {
                return Err("duplicate Google interaction creation".into());
            }
            let interaction = &value["interaction"];
            let model = interaction
                .get("model")
                .map(|_| identity(interaction, "model"))
                .transpose()?;
            if interaction
                .get("status")
                .is_some_and(|v| !matches!(v.as_str(), Some("in_progress" | "queued")))
            {
                return Err("invalid Google creation status".into());
            }
            self.identity = Some((identity(interaction, "id")?, model));
            return Ok(None);
        }
        if self.identity.is_none() {
            return Err("Google event before creation".into());
        }
        match kind {
            "interaction.status_update" => {
                self.check_identity(&value, "interaction_id")?;
                let status = string(&value, "status")?;
                if !matches!(
                    status,
                    "in_progress" | "queued" | "requires_action" | "completed"
                ) || self
                    .announced_terminal
                    .as_ref()
                    .is_some_and(|previous| previous != status)
                {
                    return Err(
                        "Google interaction status changed after terminal announcement".into(),
                    );
                }
                if matches!(status, "completed" | "requires_action") {
                    self.announced_terminal = Some(status.into());
                }
            }
            "step.start" => {
                let at = value["index"].as_u64().ok_or("invalid Google step index")?;
                if at != self.steps.len() as u64 || at >= 64 {
                    return Err("duplicate or unordered Google step".into());
                }
                let step = value["step"].clone();
                if step
                    .get("signature")
                    .is_some_and(|v| v.as_str().is_none_or(|s| s.len() > 16_384))
                {
                    return Err("invalid Google signature".into());
                }
                let mut text = String::new();
                let mut arguments = String::new();
                match string(&step, "type")? {
                    "model_output" => {
                        if let Some(content) = step.get("content") {
                            text = parts_text(content)?;
                        }
                        self.text_bytes = self.text_bytes.saturating_add(text.len());
                        if self.text_bytes > crate::TEXT_MAX {
                            return Err("Google text exceeds bound".into());
                        }
                    }
                    "thought" => {
                        if let Some(summary) = step.get("summary") {
                            parts_text(summary)?;
                        }
                    }
                    "function_call" => {
                        let id = identity(&step, "id")?;
                        identity(&step, "name")?;
                        if self
                            .steps
                            .values()
                            .any(|s| s.value["type"] == "function_call" && s.value["id"] == id)
                        {
                            return Err("duplicate Google call identity".into());
                        }
                        if let Some(value) = step.get("arguments") {
                            let object = value
                                .as_object()
                                .ok_or("invalid Google initial arguments")?;
                            if !object.is_empty() {
                                arguments = value.to_string();
                            }
                        }
                        if arguments.len() > 16_384 {
                            return Err("Google arguments exceed bound".into());
                        }
                    }
                    _ => return Err("unsupported Google step".into()),
                }
                self.steps.insert(
                    at,
                    Step {
                        value: step,
                        arguments,
                        text,
                        emitted: 0,
                        closed: false,
                    },
                );
            }
            "step.delta" => {
                let at = value["index"]
                    .as_u64()
                    .ok_or("invalid Google delta index")?;
                let step = self.steps.get_mut(&at).ok_or("unknown Google step")?;
                if step.closed {
                    return Err("Google delta after step stop".into());
                }
                let delta = &value["delta"];
                match (string(&step.value, "type")?, string(delta, "type")?) {
                    ("model_output", "text") => {
                        let text = string(delta, "text")?;
                        self.text_bytes = self.text_bytes.saturating_add(text.len());
                        if self.text_bytes > crate::TEXT_MAX {
                            return Err("Google text exceeds bound".into());
                        }
                        step.text.push_str(text);
                        if step.value.get("content").is_none() {
                            step.value["content"] = json!([]);
                        }
                        let content = step.value["content"]
                            .as_array_mut()
                            .ok_or("invalid Google content")?;
                        if let Some(last) = content.last_mut() {
                            let mut rendered = string(last, "text")?.to_owned();
                            rendered.push_str(text);
                            last["text"] = json!(rendered);
                        } else {
                            content.push(json!({"type":"text","text":text}));
                        }
                    }
                    ("thought", "thought_signature") => {
                        if let Some(signature) = delta.get("signature") {
                            let signature = signature
                                .as_str()
                                .filter(|s| s.len() <= 16_384)
                                .ok_or("invalid Google signature delta")?;
                            step.value["signature"] = json!(signature);
                        }
                    }
                    ("thought", "thought_summary") => {
                        if let Some(content) = delta.get("content") {
                            parts_text(&json!([content]))?;
                            if step.value.get("summary").is_none() {
                                step.value["summary"] = json!([]);
                            }
                            step.value["summary"]
                                .as_array_mut()
                                .ok_or("invalid Google thought summary")?
                                .push(content.clone());
                        }
                    }
                    ("function_call", "arguments_delta") => {
                        if let Some(arguments) = delta.get("arguments") {
                            let arguments =
                                arguments.as_str().ok_or("invalid Google arguments delta")?;
                            if step.arguments.len().saturating_add(arguments.len()) > 16_384 {
                                return Err("Google arguments exceed bound".into());
                            }
                            step.arguments.push_str(arguments);
                        }
                    }
                    _ => return Err("unsupported or mismatched Google delta".into()),
                }
            }
            "step.stop" => {
                let at = value["index"].as_u64().ok_or("invalid Google stop index")?;
                let step = self
                    .steps
                    .get_mut(&at)
                    .ok_or("unknown Google stopped step")?;
                if step.closed {
                    return Err("duplicate Google step stop".into());
                }
                if step.value["type"] == "function_call" {
                    let arguments: Value = if step.arguments.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&step.arguments)
                            .map_err(|_| "invalid Google function arguments JSON")?
                    };
                    if !arguments.is_object() {
                        return Err("Google arguments are not an object".into());
                    }
                    step.value["arguments"] = arguments;
                }
                step.closed = true;
            }
            "interaction.completed" => {
                let interaction = &value["interaction"];
                self.check_identity(interaction, "id")?;
                if let Some(model) = interaction.get("model") {
                    let model = model.as_str().ok_or("invalid Google completed model")?;
                    if self
                        .identity
                        .as_ref()
                        .and_then(|(_, m)| m.as_deref())
                        .is_some_and(|m| m != model)
                    {
                        return Err("Google completed model changed".into());
                    }
                }
                if self.steps.values().any(|step| !step.closed) {
                    return Err("unfinished Google step at completion".into());
                }
                if let Some(steps) = interaction.get("steps") {
                    let reconstructed: Vec<_> =
                        self.steps.values().map(|step| step.value.clone()).collect();
                    if *steps != json!(reconstructed) {
                        return Err("Google terminal steps differ".into());
                    }
                }
                for key in [
                    "total_tokens",
                    "total_input_tokens",
                    "total_output_tokens",
                    "total_cached_tokens",
                    "total_thought_tokens",
                    "total_tool_use_tokens",
                ] {
                    if interaction["usage"]
                        .get(key)
                        .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
                    {
                        return Err("invalid Google usage count".into());
                    }
                }
                let status = string(interaction, "status")?;
                if self
                    .announced_terminal
                    .as_ref()
                    .is_some_and(|previous| previous != status)
                {
                    return Err("Google terminal status contradicts announcement".into());
                }
                if !matches!(status, "completed" | "requires_action") {
                    return Err("Google interaction did not complete successfully".into());
                }
                self.terminal = Some(status.into());
            }
            "error" => return Err("Google endpoint reported an error".into()),
            _ => return Err("unsupported Google event".into()),
        }
        Ok(self.emit_ready())
    }

    fn check_identity(&self, value: &Value, key: &str) -> Result<()> {
        if identity(value, key)? != self.identity.as_ref().ok_or("missing Google identity")?.0 {
            return Err("Google interaction identity changed".into());
        }
        Ok(())
    }

    fn emit_ready(&mut self) -> Option<String> {
        let mut text = String::new();
        while let Some(step) = self.steps.get_mut(&self.cursor) {
            if step.value["type"] == "model_output" {
                text.push_str(&step.text[step.emitted..]);
                step.emitted = step.text.len();
                if !step.closed {
                    break;
                }
            }
            self.cursor += 1;
        }
        (!text.is_empty()).then_some(text)
    }

    pub fn finish(self) -> Result<Turn> {
        let status = self
            .terminal
            .ok_or("Google stream ended without completion")?;
        let mut call = None;
        let mut text = String::new();
        let mut steps = Vec::new();
        for step in self.steps.into_values() {
            text.push_str(&step.text);
            if step.value["type"] == "function_call" {
                if call.is_some() {
                    return Err("multiple Google browser calls".into());
                }
                call = Some(Invocation {
                    id: identity(&step.value, "id")?,
                    call: decode_call(
                        string(&step.value, "name")?,
                        step.value["arguments"].clone(),
                    )?,
                });
            }
            steps.push(step.value);
        }
        if (status == "requires_action") != call.is_some() || (call.is_none() && text.is_empty()) {
            return Err("Google terminal status and output disagree".into());
        }
        Ok(Turn { steps, call, text })
    }
}
