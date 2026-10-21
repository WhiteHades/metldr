use super::*;
use serde_json::{Map, json};

pub(super) fn questions(p: &Value) -> Result<InputPrompt> {
    let item = bounded_string(&p["itemId"], 256, true)?;
    if !controlled_id(&item, 256) {
        return Err(invalid("Unsupported Codex question input"));
    }
    if p["isBlocking"].as_bool().is_none() {
        return Err(invalid("Unsupported Codex question input"));
    }
    let raw = p["questions"]
        .as_array()
        .filter(|a| (1..=4).contains(&a.len()))
        .ok_or_else(|| invalid("Unsupported Codex question input"))?;
    let mut seen = BTreeSet::new();
    let mut questions = Vec::new();
    for q in raw {
        if q.get("isSecret").is_some_and(|v| v != false)
            || q.get("isOther").is_some_and(|v| !v.is_boolean())
        {
            return Err(invalid("Unsupported Codex question input"));
        }
        let id = bounded_string(&q["id"], 128, true)?;
        let header = bounded_string(&q["header"], 256, false)?;
        let text = bounded_string(&q["question"], 4096, false)?;
        if !controlled_id(&id, 128) || !seen.insert(id.clone()) {
            return Err(invalid("Unsupported Codex question input"));
        }
        let options = q["options"]
            .as_array()
            .filter(|a| (1..=16).contains(&a.len()))
            .ok_or_else(|| invalid("Unsupported Codex question input"))?;
        let mut labels = BTreeSet::new();
        let mut parsed = Vec::new();
        for o in options {
            let label = bounded_string(&o["label"], 1024, true)?;
            if !controlled_id(&label, 1024) || !labels.insert(label.clone()) {
                return Err(invalid("Unsupported Codex question input"));
            }
            parsed.push(QuestionOption {
                label,
                description: bounded_string(&o["description"], 2048, false)?,
            });
        }
        questions.push(Question {
            id,
            header,
            text,
            options: parsed,
            other: q["isOther"].as_bool().unwrap_or(false),
        });
    }
    Ok(InputPrompt::Questions {
        item,
        is_blocking: p["isBlocking"].as_bool().unwrap(),
        questions,
    })
}

pub(super) fn elicitation(p: &Value) -> Result<InputPrompt> {
    if p["mode"] != "form" {
        return Err(invalid("Unsupported MCP elicitation mode"));
    }
    if p.get("_meta")
        .is_some_and(|m| !m.is_null() && !m.is_object())
    {
        return Err(invalid("Unsupported MCP elicitation metadata"));
    }
    let server = bounded_string(&p["serverName"], 256, true)?;
    let message = bounded_string(&p["message"], 4096, false)?;
    if p.get("_meta")
        .is_some_and(|m| m["codex_approval_kind"] == "mcp_tool_call")
    {
        let meta = p
            .get("_meta")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("Unsupported MCP tool approval"))?;
        let mut persist = BTreeSet::new();
        let persist_ok = meta
            .get("persist")
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values.iter().all(|v| {
                    v.as_str().is_some_and(|s| {
                        matches!(s, "session" | "always") && persist.insert(s.to_owned())
                    })
                })
            });
        if p["requestedSchema"] != json!({"type":"object","properties":{}})
            || !meta
                .get("tool_description")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty() && s.len() <= 4096)
            || !meta.get("tool_params").is_some_and(Value::is_object)
            || !meta.get("tool_params_display").is_some_and(Value::is_array)
            || !persist_ok
        {
            return Err(invalid("Unsupported MCP tool approval"));
        }
        let review = serde_json::to_string_pretty(p.get("_meta").unwrap())?;
        if review.len() > 16 * 1024 {
            return Err(invalid("MCP tool review exceeds bound"));
        }
        return Ok(InputPrompt::McpToolApproval {
            server,
            message,
            review,
        });
    }
    if p.get("_meta")
        .and_then(Value::as_object)
        .is_some_and(|m| m.contains_key("codex_approval_kind"))
    {
        return Err(invalid("Unsupported MCP elicitation metadata"));
    }
    Ok(InputPrompt::McpForm {
        server,
        message,
        fields: parse_fields(&p["requestedSchema"])?,
    })
}

fn parse_fields(schema: &Value) -> Result<Vec<FormField>> {
    let obj = schema
        .as_object()
        .filter(|o| {
            o.get("type").is_some_and(|v| v == "object")
                && o.keys()
                    .all(|k| ["type", "properties", "required"].contains(&k.as_str()))
        })
        .ok_or_else(|| invalid("Unsupported MCP form schema"))?;
    let props = obj
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("Unsupported MCP form schema"))?;
    if props.len() > 8 {
        return Err(invalid("Unsupported MCP form schema"));
    }
    let mut required = BTreeSet::new();
    if let Some(raw) = obj.get("required") {
        for v in raw
            .as_array()
            .ok_or_else(|| invalid("Unsupported MCP form schema"))?
        {
            let key = v
                .as_str()
                .ok_or_else(|| invalid("Unsupported MCP form schema"))?;
            if !required.insert(key.to_owned()) {
                return Err(invalid("Unsupported MCP form schema"));
            }
        }
    } else if !props.is_empty() {
        return Err(invalid("Unsupported MCP form schema"));
    }
    if required.len() != props.len() || props.keys().any(|k| !required.contains(k)) {
        return Err(invalid("Unsupported MCP form schema"));
    }
    let mut fields = Vec::new();
    for (name, raw) in props {
        if !controlled_id(name, 128) {
            return Err(invalid("Unsupported MCP form schema"));
        }
        let o = raw
            .as_object()
            .ok_or_else(|| invalid("Unsupported MCP form field"))?;
        if o.keys().any(|k| {
            ![
                "type",
                "title",
                "description",
                "minLength",
                "maxLength",
                "enum",
                "default",
            ]
            .contains(&k.as_str())
        }) || o.get("type").is_none_or(|v| v != "string")
        {
            return Err(invalid("Unsupported MCP form field"));
        }
        let title = o
            .get("title")
            .map(|v| bounded_string(v, 256, false))
            .transpose()?
            .unwrap_or_else(|| name.clone());
        let description = o
            .get("description")
            .map(|v| bounded_string(v, 2048, false))
            .transpose()?;
        let min = length(o.get("minLength"), 0)?;
        let max = length(o.get("maxLength"), 4096)?;
        if min > max {
            return Err(invalid("Unsupported MCP form field"));
        }
        let choices = o
            .get("enum")
            .map(|v| {
                let a = v
                    .as_array()
                    .filter(|a| (1..=16).contains(&a.len()))
                    .ok_or_else(|| invalid("Unsupported MCP form field"))?;
                let mut seen = BTreeSet::new();
                let mut result = Vec::new();
                for value in a {
                    let s = bounded_string(value, 1024, true)?;
                    if !seen.insert(s.clone()) {
                        return Err(invalid("Unsupported MCP form field"));
                    }
                    result.push(s);
                }
                Ok(result)
            })
            .transpose()?;
        if choices.is_some() && (o.contains_key("minLength") || o.contains_key("maxLength")) {
            return Err(invalid("Unsupported MCP form field"));
        }
        let default = o
            .get("default")
            .map(|v| bounded_string(v, 4096, false))
            .transpose()?;
        if let Some(d) = &default {
            if choices.as_ref().is_some_and(|c| !c.contains(d))
                || d.chars().count() < min
                || d.chars().count() > max
            {
                return Err(invalid("Unsupported MCP form default"));
            }
        }
        fields.push(FormField {
            name: name.clone(),
            title,
            description,
            min_chars: min,
            max_chars: max,
            choices,
            default,
        });
    }
    Ok(fields)
}
fn length(v: Option<&Value>, default: usize) -> Result<usize> {
    match v {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| *n <= 4096)
            .map(|n| n as usize)
            .ok_or_else(|| invalid("Unsupported MCP form length")),
    }
}

pub(super) fn encode(prompt: &InputPrompt, answer: &InputAnswer) -> Result<Value> {
    let result = encode_inner(prompt, answer)?;
    if serde_json::to_vec(&result)?.len() > 16 * 1024 {
        return Err(invalid("Codex input answer exceeds bound"));
    }
    Ok(result)
}

fn encode_inner(prompt: &InputPrompt, answer: &InputAnswer) -> Result<Value> {
    match (prompt, answer) {
        (InputPrompt::Questions { questions, .. }, InputAnswer::Questions(answers)) => {
            let known: BTreeMap<_, _> = questions.iter().map(|q| (q.id.as_str(), q)).collect();
            if answers.keys().any(|k| !known.contains_key(k.as_str())) {
                return Err(invalid("Question answer is invalid"));
            }
            let mut encoded = Map::new();
            for (id, values) in answers {
                let q = known[id.as_str()];
                if values.is_empty()
                    || values.len() > 16
                    || values.iter().any(|v| {
                        if q.options.iter().any(|o| o.label == *v) {
                            false
                        } else {
                            !q.other || v.chars().count() > 4096 || v.is_empty()
                        }
                    })
                {
                    return Err(invalid("Question answer is invalid"));
                }
                encoded.insert(id.clone(), json!({"answers":values}));
            }
            Ok(json!({"answers":encoded}))
        }
        (InputPrompt::McpForm { fields, .. }, InputAnswer::Accept(values)) => {
            if values.len() != fields.len()
                || fields.iter().any(|f| {
                    values.get(&f.name).is_none_or(|v| {
                        v.chars().count() < f.min_chars
                            || v.chars().count() > f.max_chars
                            || f.choices.as_ref().is_some_and(|c| !c.contains(v))
                    })
                })
            {
                return Err(invalid("MCP form answer is invalid"));
            }
            Ok(json!({"action":"accept","content":values}))
        }
        (InputPrompt::McpToolApproval { .. }, InputAnswer::Accept(values)) if values.is_empty() => {
            Ok(json!({"action":"accept","content":{}}))
        }
        (
            InputPrompt::McpForm { .. } | InputPrompt::McpToolApproval { .. },
            InputAnswer::Decline,
        ) => Ok(json!({"action":"decline","content":null})),
        (
            InputPrompt::McpForm { .. } | InputPrompt::McpToolApproval { .. },
            InputAnswer::Cancel,
        ) => Ok(json!({"action":"cancel","content":null})),
        _ => Err(invalid("Answer does not match Codex input")),
    }
}
