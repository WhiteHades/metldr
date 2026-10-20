use crate::Result;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const DETAIL_MAX: usize = 65_536;

pub(super) fn append(
    target: &mut Vec<Value>,
    identities: &mut BTreeMap<String, Map<String, Value>>,
    incoming: &Value,
) -> Result<()> {
    let entries = incoming
        .as_array()
        .ok_or("invalid Chat reasoning details")?;
    for entry in entries {
        let next = entry.as_object().ok_or("invalid Chat reasoning detail")?;
        if serde_json::to_vec(next)?.len() > DETAIL_MAX {
            return Err("Chat reasoning detail exceeds bound".into());
        }
        validate_detail(next)?;
        if let Some(id) = next.get("id").and_then(Value::as_str) {
            let metadata = identities.entry(id.to_owned()).or_default();
            for field in ["type", "index", "format", "signature"] {
                if let Some(value) = non_null(next.get(field)) {
                    if non_null(metadata.get(field)).is_some_and(|previous| previous != value) {
                        return Err("conflicting Chat reasoning metadata".into());
                    }
                    metadata.insert(field.into(), value.clone());
                }
            }
        }
        let kind = next["type"].as_str().expect("validated reasoning type");
        let body = match kind {
            "reasoning.text" => Some("text"),
            "reasoning.summary" => Some("summary"),
            _ => None,
        };
        if let Some(body) = body {
            if let Some(last) = target.last_mut().and_then(Value::as_object_mut)
                && last.get("type").and_then(Value::as_str) == Some(kind)
                && compatible(last, next)?
            {
                merge_metadata(last, next)?;
                if let Some(piece) = next.get(body).and_then(Value::as_str) {
                    match last.get_mut(body) {
                        Some(Value::String(current)) => current.push_str(piece),
                        None | Some(Value::Null) => {
                            last.insert(body.into(), Value::String(piece.to_owned()));
                        }
                        _ => return Err("invalid accumulated Chat reasoning text".into()),
                    }
                }
                if !last.contains_key(body) && next.contains_key(body) {
                    last.insert(body.into(), Value::Null);
                }
                merge_unknown(last, next, body)?;
                if serde_json::to_vec(last)?.len() > DETAIL_MAX {
                    return Err("Chat reasoning detail exceeds bound".into());
                }
                continue;
            }
        }
        target.push(Value::Object(next.clone()));
    }
    Ok(())
}

fn validate_detail(detail: &Map<String, Value>) -> Result<()> {
    match detail.get("type").and_then(Value::as_str) {
        Some("reasoning.text") => validate_optional_string(detail, "text")?,
        Some("reasoning.summary") => validate_optional_string(detail, "summary")?,
        Some("reasoning.encrypted") if detail.get("data").and_then(Value::as_str).is_some() => (),
        Some("reasoning.encrypted") => return Err("invalid Chat encrypted reasoning detail".into()),
        _ => return Err("unsupported Chat reasoning detail".into()),
    }
    validate_optional_string(detail, "id")?;
    validate_optional_string(detail, "format")?;
    validate_optional_string(detail, "signature")?;
    if detail
        .get("index")
        .is_some_and(|index| !index.is_null() && !index.is_u64())
    {
        return Err("invalid Chat reasoning index".into());
    }
    Ok(())
}

fn validate_optional_string(detail: &Map<String, Value>, field: &str) -> Result<()> {
    if detail
        .get(field)
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err("invalid Chat reasoning metadata".into());
    }
    Ok(())
}

fn compatible(target: &Map<String, Value>, source: &Map<String, Value>) -> Result<bool> {
    let target_id = non_null(target.get("id"));
    let source_id = non_null(source.get("id"));
    if target_id.is_some() && source_id.is_some() && target_id != source_id {
        return Ok(false);
    }
    if target_id.is_some() && source_id.is_some() {
        for field in ["index", "format", "signature"] {
            let left = non_null(target.get(field));
            let right = non_null(source.get(field));
            if left.is_some() && right.is_some() && left != right {
                return Err("conflicting Chat reasoning metadata".into());
            }
        }
    }
    let left_index = non_null(target.get("index"));
    let right_index = non_null(source.get("index"));
    if left_index.is_some() && right_index.is_some() && left_index != right_index {
        return Ok(false);
    }
    let left_format = non_null(target.get("format"));
    let right_format = non_null(source.get("format"));
    if left_format.is_some() && right_format.is_some() && left_format != right_format {
        return Ok(false);
    }
    let left_signature = non_null(target.get("signature"));
    let right_signature = non_null(source.get("signature"));
    if left_signature.is_some() && right_signature.is_some() && left_signature != right_signature {
        return Ok(false);
    }
    Ok(true)
}

fn non_null(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

fn merge_metadata(target: &mut Map<String, Value>, source: &Map<String, Value>) -> Result<()> {
    for field in ["id", "index", "format", "signature"] {
        if let Some(value) = source.get(field) {
            if value.is_null() {
                target.entry(field).or_insert(Value::Null);
                continue;
            }
            if non_null(target.get(field)).is_some_and(|old| old != value) {
                return Err("conflicting Chat reasoning metadata".into());
            }
            target.insert(field.into(), value.clone());
        }
    }
    Ok(())
}

fn merge_unknown(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    body: &str,
) -> Result<()> {
    for (key, value) in source {
        if key == body
            || matches!(
                key.as_str(),
                "type" | "id" | "index" | "format" | "signature"
            )
        {
            continue;
        }
        if target.get(key).is_some_and(|old| old != value) {
            return Err("conflicting Chat reasoning detail field".into());
        }
        target.entry(key.clone()).or_insert_with(|| value.clone());
    }
    Ok(())
}
