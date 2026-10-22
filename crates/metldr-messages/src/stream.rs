use crate::{Invocation, Result, TextEvent};
use metldr_browser_tools::decode_call;
use serde_json::{Value, json};

pub(super) struct Turn {
    pub content: Vec<Value>,
    pub call: Option<Invocation>,
    pub text: String,
}

struct Block {
    value: Value,
    arguments: Option<String>,
    emitted: usize,
    closed: bool,
}

pub(super) struct State {
    model: String,
    started: bool,
    blocks: Vec<Block>,
    reason: Option<String>,
    stopped: bool,
    usage_seen: bool,
    output_tokens: u64,
    events: usize,
    text_bytes: usize,
    cursor: usize,
}

fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| "invalid Messages string".into())
}

fn identity(v: &Value, key: &str) -> Result<String> {
    let s = string(v, key)?;
    if s.is_empty() || s.len() > 128 || s.chars().any(char::is_control) {
        return Err("invalid Messages identity".into());
    }
    Ok(s.into())
}

fn append(v: &mut Value, key: &str, extra: &str) -> Result<()> {
    let mut text = v
        .get(key)
        .map(|_| string(v, key))
        .transpose()?
        .unwrap_or("")
        .to_owned();
    if text.len().saturating_add(extra.len()) > 16_384 {
        return Err("Messages block exceeds bound".into());
    }
    text.push_str(extra);
    v[key] = json!(text);
    Ok(())
}

impl State {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.into(),
            started: false,
            blocks: Vec::new(),
            reason: None,
            stopped: false,
            usage_seen: false,
            output_tokens: 0,
            events: 0,
            text_bytes: 0,
            cursor: 0,
        }
    }

    pub fn frame(&mut self, event: &str, data: &str) -> Result<Option<TextEvent>> {
        self.events += 1;
        if self.events > 4096 {
            return Err("Messages event count exceeds bound".into());
        }
        let value: Value = serde_json::from_str(data).map_err(|_| "invalid Messages event JSON")?;
        let kind = identity(&value, "type")?;
        if !event.is_empty() && event != kind {
            return Err("Messages event envelope mismatch".into());
        }
        if kind == "error" {
            return Err("Messages endpoint reported an error".into());
        }
        if !matches!(
            kind.as_str(),
            "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "message_stop"
        ) {
            return Ok(None);
        }
        if self.stopped {
            return Err("Messages event after completion".into());
        }
        if kind == "message_start" {
            let message = &value["message"];
            if self.started
                || message["type"] != "message"
                || message["role"] != "assistant"
                || message["model"] != self.model
                || message["content"] != json!([])
                || !message["stop_reason"].is_null()
                || !message["stop_sequence"].is_null()
            {
                return Err("invalid Messages start".into());
            }
            identity(message, "id")?;
            message["usage"]["input_tokens"]
                .as_u64()
                .ok_or("invalid Messages input usage")?;
            self.usage(&message["usage"])?;
            self.started = true;
            return Ok(None);
        }
        if !self.started {
            return Err("Messages event before start".into());
        }
        match kind.as_str() {
            "content_block_start" => {
                if self.reason.is_some() {
                    return Err("Messages block after stop reason".into());
                }
                let at = value["index"]
                    .as_u64()
                    .ok_or("invalid Messages block index")?;
                if at != self.blocks.len() as u64 || at >= 64 {
                    return Err("duplicate or unordered Messages block".into());
                }
                let block = value["content_block"].clone();
                match string(&block, "type")? {
                    "text" => {
                        let text = string(&block, "text")?;
                        self.text_bytes = self.text_bytes.saturating_add(text.len());
                        if self.text_bytes > crate::TEXT_MAX {
                            return Err("Messages text exceeds bound".into());
                        }
                    }
                    "thinking" => {
                        if string(&block, "thinking")?.len() > 16_384
                            || block
                                .get("signature")
                                .is_some_and(|v| v.as_str().is_none_or(|s| s.len() > 16_384))
                        {
                            return Err("invalid Messages thinking block".into());
                        }
                    }
                    "redacted_thinking" => {
                        if string(&block, "data")?.len() > 16_384 {
                            return Err("Messages redacted data exceeds bound".into());
                        }
                    }
                    "tool_use" => {
                        let id = identity(&block, "id")?;
                        identity(&block, "name")?;
                        if !block["input"].is_object()
                            || block["input"].to_string().len() > 16_384
                            || self
                                .blocks
                                .iter()
                                .any(|b| b.value["type"] == "tool_use" && b.value["id"] == id)
                        {
                            return Err("invalid Messages tool block".into());
                        }
                    }
                    _ => return Err("unsupported Messages content block".into()),
                }
                self.blocks.push(Block {
                    value: block,
                    arguments: None,
                    emitted: 0,
                    closed: false,
                });
            }
            "content_block_delta" => {
                if self.reason.is_some() {
                    return Err("Messages delta after stop reason".into());
                }
                let at = value["index"]
                    .as_u64()
                    .ok_or("invalid Messages delta index")?;
                let block = usize::try_from(at)
                    .ok()
                    .and_then(|at| self.blocks.get_mut(at))
                    .ok_or("unknown Messages block")?;
                if block.closed {
                    return Err("Messages delta after block stop".into());
                }
                let delta = &value["delta"];
                match (string(&block.value, "type")?, string(delta, "type")?) {
                    ("text", "text_delta") => {
                        let text = string(delta, "text")?;
                        self.text_bytes = self.text_bytes.saturating_add(text.len());
                        if self.text_bytes > crate::TEXT_MAX {
                            return Err("Messages text exceeds bound".into());
                        }
                        append(&mut block.value, "text", text)?;
                    }
                    ("thinking", "thinking_delta") => {
                        append(&mut block.value, "thinking", string(delta, "thinking")?)?
                    }
                    ("thinking", "signature_delta") => {
                        append(&mut block.value, "signature", string(delta, "signature")?)?
                    }
                    ("tool_use", "input_json_delta") => {
                        if !block.value["input"]
                            .as_object()
                            .is_some_and(|o| o.is_empty())
                        {
                            return Err("Messages tool input mixed with fragments".into());
                        }
                        let text = string(delta, "partial_json")?;
                        let arguments = block.arguments.get_or_insert_with(String::new);
                        if arguments.len().saturating_add(text.len()) > 16_384 {
                            return Err("Messages arguments exceed bound".into());
                        }
                        arguments.push_str(text);
                    }
                    _ => return Err("unsupported or mismatched Messages delta".into()),
                }
            }
            "content_block_stop" => {
                if self.reason.is_some() {
                    return Err("Messages block stop after reason".into());
                }
                let at = value["index"]
                    .as_u64()
                    .ok_or("invalid Messages stop index")?;
                let block = usize::try_from(at)
                    .ok()
                    .and_then(|at| self.blocks.get_mut(at))
                    .ok_or("unknown stopped Messages block")?;
                if block.closed {
                    return Err("duplicate Messages block stop".into());
                }
                if let Some(arguments) = block.arguments.take() {
                    let input: Value = serde_json::from_str(&arguments)
                        .map_err(|_| "invalid Messages tool JSON")?;
                    if !input.is_object() {
                        return Err("Messages tool input is not an object".into());
                    }
                    block.value["input"] = input;
                }
                if block.value["type"] == "thinking"
                    && string(&block.value, "signature")?.is_empty()
                {
                    return Err("Messages thinking signature missing".into());
                }
                block.closed = true;
            }
            "message_delta" => {
                if self.blocks.iter().any(|b| !b.closed) {
                    return Err("Messages delta before block completion".into());
                }
                if let Some(reason) = value["delta"].get("stop_reason").filter(|v| !v.is_null()) {
                    let reason = reason.as_str().ok_or("invalid Messages stop reason")?;
                    if !matches!(reason, "end_turn" | "stop_sequence" | "tool_use")
                        || self.reason.as_ref().is_some_and(|old| old != reason)
                    {
                        return Err("Messages did not complete successfully".into());
                    }
                    self.reason = Some(reason.into());
                }
                if value["delta"]
                    .get("stop_sequence")
                    .is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|s| s.len() > 16_384))
                {
                    return Err("invalid Messages stop sequence".into());
                }
                self.usage(&value["usage"])?;
                self.usage_seen = true;
            }
            "message_stop" => {
                if self.reason.is_none()
                    || !self.usage_seen
                    || self.blocks.iter().any(|b| !b.closed)
                {
                    return Err("Messages stopped before completion".into());
                }
                self.stopped = true;
            }
            _ => unreachable!(),
        }
        Ok(self.emit_ready())
    }

    fn usage(&mut self, value: &Value) -> Result<()> {
        let output = value["output_tokens"]
            .as_u64()
            .ok_or("invalid Messages output usage")?;
        if output < self.output_tokens {
            return Err("Messages output usage decreased".into());
        }
        for key in [
            "input_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ] {
            if value.get(key).is_some_and(|v| v.as_u64().is_none()) {
                return Err("invalid Messages usage count".into());
            }
        }
        self.output_tokens = output;
        Ok(())
    }

    fn emit_ready(&mut self) -> Option<TextEvent> {
        let mut text = String::new();
        while let Some(block) = self.blocks.get_mut(self.cursor) {
            if block.value["type"] == "text" {
                let current = block.value["text"].as_str().expect("validated text block");
                text.push_str(&current[block.emitted..]);
                block.emitted = current.len();
                if !block.closed {
                    break;
                }
            } else if !block.closed {
                break;
            }
            self.cursor += 1;
        }
        (!text.is_empty()).then_some(TextEvent::Delta(text))
    }

    pub fn finish(self) -> Result<Turn> {
        if !self.stopped {
            return Err("Messages stream ended without completion".into());
        }
        let mut content = Vec::new();
        let mut call = None;
        let mut text = String::new();
        for block in self.blocks {
            match block.value["type"].as_str() {
                Some("text") => text.push_str(string(&block.value, "text")?),
                Some("tool_use") => {
                    if call.is_some() {
                        return Err("multiple Messages browser calls".into());
                    }
                    call = Some(Invocation {
                        id: identity(&block.value, "id")?,
                        call: decode_call(
                            string(&block.value, "name")?,
                            block.value["input"].clone(),
                        )?,
                    });
                }
                _ => {}
            }
            content.push(block.value);
        }
        if (self.reason.as_deref() == Some("tool_use")) != call.is_some()
            || (call.is_none() && text.is_empty())
        {
            return Err("Messages stop reason and output disagree".into());
        }
        Ok(Turn {
            content,
            call,
            text,
        })
    }
}
