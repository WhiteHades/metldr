use crate::{Result, TurnId, invalid};
use serde_json::Value;
use std::{collections::BTreeMap, io::Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolOutcome {
    Succeeded,
    Failed,
    Declined,
    Unspecified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolState {
    Started,
    Finished(ToolOutcome),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolReport {
    pub turn: TurnId,
    pub item_id: String,
    pub tool: String,
    pub state: ToolState,
    pub at_ms: Option<i64>,
    pub title: Preview,
    pub input: Preview,
    pub output: Option<Preview>,
}

#[derive(Default)]
pub(crate) struct Reports(BTreeMap<String, (String, ToolReport)>);

impl Reports {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    pub(crate) fn decode(
        &mut self,
        params: &Value,
        started: bool,
        turn: &TurnId,
    ) -> Result<Option<ToolReport>> {
        let item = &params["item"];
        let kind = text(item, "type")?;
        if !matches!(kind, "commandExecution" | "fileChange" | "dynamicToolCall") {
            return Ok(None);
        }
        let item_id = identity(item, "id")?.to_owned();
        let status = text(item, "status")?;
        if (status == "inProgress") != started {
            return Err(invalid("Codex item phase and status disagree"));
        }
        let outcome = match status {
            "inProgress" => None,
            "failed" => Some(ToolOutcome::Failed),
            "declined" if kind != "dynamicToolCall" => Some(ToolOutcome::Declined),
            "completed" => Some(ToolOutcome::Succeeded),
            _ => return Err(invalid("Codex item status invalid")),
        };
        let state = outcome.map_or(ToolState::Started, ToolState::Finished);
        let mut report = ToolReport {
            turn: turn.clone(),
            item_id,
            tool: String::new(),
            state,
            at_ms: optional_i64(
                params,
                if started {
                    "startedAtMs"
                } else {
                    "completedAtMs"
                },
            )?,
            title: preview("", 256),
            input: preview("", 1024),
            output: None,
        };
        match kind {
            "commandExecution" => {
                let command = text(item, "command")?;
                let cwd = text(item, "cwd")?;
                let output = optional_text(item, "aggregatedOutput")?;
                let exit = optional_i64(item, "exitCode")?;
                report.tool = "Command".into();
                report.title = preview(command, 256);
                let mut input = Sink::new(1024);
                input.push("Command: ");
                input.push(command);
                input.push("\nDirectory: ");
                input.push(cwd);
                report.input = input.finish();
                if status == "completed" {
                    report.state = ToolState::Finished(match exit {
                        Some(0) => ToolOutcome::Succeeded,
                        Some(_) => ToolOutcome::Failed,
                        None => ToolOutcome::Unspecified,
                    });
                }
                if !started {
                    let mut result = result_sink(report.state);
                    if let Some(exit) = exit {
                        result.push(&format!("Exit code: {exit}\n"));
                    }
                    if let Some(output) = output {
                        result.push(output);
                    }
                    report.output = Some(result.finish());
                }
            }
            "fileChange" => {
                let changes = item["changes"]
                    .as_array()
                    .ok_or_else(|| invalid("Codex file changes missing"))?;
                let mut input = Sink::new(1024);
                let mut output = result_sink(report.state);
                for change in changes {
                    let path = text(change, "path")?;
                    let diff = text(change, "diff")?;
                    let kind = text(&change["kind"], "type")?;
                    if !matches!(kind, "add" | "delete" | "update") {
                        return Err(invalid("Codex file change kind invalid"));
                    }
                    let destination = optional_text(&change["kind"], "move_path")?;
                    input.push(kind);
                    input.push(" ");
                    input.push(path);
                    if let Some(destination) = destination {
                        input.push(" to ");
                        input.push(destination);
                    }
                    input.push("\n");
                    output.push(path);
                    output.push("\n");
                    output.push(diff);
                    output.push("\n");
                }
                report.tool = "File change".into();
                report.title = preview("File changes", 256);
                report.input = input.finish();
                if !started {
                    report.output = Some(output.finish());
                }
            }
            "dynamicToolCall" => {
                report.tool = identity(item, "tool")?.into();
                report.title = preview(&report.tool, 256);
                let mut input = Sink::new(1024);
                if let Some(namespace) = optional_text(item, "namespace")? {
                    input.push("Namespace: ");
                    input.push(namespace);
                    input.push("\n");
                }
                serde_json::to_writer_pretty(
                    &mut input,
                    item.get("arguments")
                        .ok_or_else(|| invalid("Codex item arguments missing"))?,
                )?;
                report.input = input.finish();
                let success = match item.get("success") {
                    None | Some(Value::Null) => None,
                    Some(Value::Bool(value)) => Some(*value),
                    _ => return Err(invalid("Codex tool success invalid")),
                };
                if status == "completed" {
                    report.state = ToolState::Finished(match success {
                        Some(true) => ToolOutcome::Succeeded,
                        Some(false) => ToolOutcome::Failed,
                        None => ToolOutcome::Unspecified,
                    });
                }
                let mut output = result_sink(report.state);
                match item.get("contentItems") {
                    None | Some(Value::Null) => (),
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            match text(part, "type")? {
                                "inputText" => {
                                    output.push(text(part, "text")?);
                                    output.push("\n");
                                }
                                "inputImage" => {
                                    text(part, "imageUrl")?;
                                    output.truncated = true;
                                }
                                "inputAudio" => {
                                    text(part, "audioUrl")?;
                                    output.truncated = true;
                                }
                                _ => return Err(invalid("Codex tool output content invalid")),
                            }
                        }
                    }
                    _ => return Err(invalid("Codex tool output invalid")),
                }
                if !started {
                    report.output = Some(output.finish());
                }
            }
            _ => unreachable!(),
        }
        if let Some((previous_kind, previous)) = self.0.get(&report.item_id) {
            if previous_kind != kind {
                return Err(invalid("Codex native item kind changed"));
            }
            if previous == &report {
                return Ok(None);
            }
            if !matches!(
                (previous.state, report.state),
                (ToolState::Started, ToolState::Finished(_))
            ) {
                return Err(invalid("Codex native item lifecycle conflict"));
            }
        } else if self.0.len() >= 32 {
            return Err(invalid("Codex native item limit exceeded"));
        }
        self.0
            .insert(report.item_id.clone(), (kind.into(), report.clone()));
        Ok(Some(report))
    }
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| invalid("Codex item text missing or invalid"))
}
fn identity<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    let value = text(value, name)?;
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(invalid("Codex native item identity invalid"));
    }
    Ok(value)
}
fn optional_text<'a>(value: &'a Value, name: &str) -> Result<Option<&'a str>> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(invalid("Codex optional item text invalid")),
    }
}
fn optional_i64(value: &Value, name: &str) -> Result<Option<i64>> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| invalid("Codex optional item number invalid")),
    }
}
fn result_sink(state: ToolState) -> Sink {
    let mut sink = Sink::new(2048);
    sink.push(match state {
        ToolState::Started => "",
        ToolState::Finished(ToolOutcome::Succeeded) => "Reported outcome: succeeded\n",
        ToolState::Finished(ToolOutcome::Failed) => "Reported outcome: failed\n",
        ToolState::Finished(ToolOutcome::Declined) => "Reported outcome: declined\n",
        ToolState::Finished(ToolOutcome::Unspecified) => "Reported outcome: unspecified\n",
    });
    sink
}
fn preview(text: &str, limit: usize) -> Preview {
    let mut sink = Sink::new(limit);
    sink.push(text);
    sink.finish()
}

struct Sink {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
}
impl Sink {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            truncated: false,
        }
    }
    fn push(&mut self, text: &str) {
        self.write_all(text.as_bytes())
            .expect("bounded preview write is infallible");
    }
    fn finish(mut self) -> Preview {
        while std::str::from_utf8(&self.bytes).is_err() {
            self.bytes.pop();
        }
        Preview {
            text: String::from_utf8(self.bytes).expect("validated UTF8"),
            truncated: self.truncated,
        }
    }
}
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let keep = bytes.len().min(self.limit - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..keep]);
        self.truncated |= keep != bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod checks;
