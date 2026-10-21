use super::{ApprovalKind, Result, invalid};
use serde_json::{Value, json};

pub(super) fn decode(kind: ApprovalKind, params: &Value) -> Result<(Vec<String>, Vec<Value>)> {
    if kind == ApprovalKind::Permissions {
        let permissions = params
            .get("permissions")
            .filter(|p| p.is_object())
            .ok_or_else(|| invalid("Codex permission profile missing"))?;
        if permissions
            .as_object()
            .unwrap()
            .keys()
            .any(|k| !matches!(k.as_str(), "fileSystem" | "network"))
        {
            return Err(invalid("Codex permission profile unsupported"));
        }
        return Ok((
            vec![
                "Allow for this turn".into(),
                "Allow for this Codex session".into(),
                "Decline".into(),
            ],
            vec![
                json!({"permissions":permissions,"scope":"turn"}),
                json!({"permissions":permissions,"scope":"session"}),
                json!({"permissions":{},"scope":"turn"}),
            ],
        ));
    }
    let defaults = json!(["accept", "acceptForSession", "decline", "cancel"]);
    let offered = if kind == ApprovalKind::Command {
        match params.get("availableDecisions") {
            None | Some(Value::Null) => &defaults,
            Some(value) => value,
        }
    } else {
        &defaults
    };
    let offered = offered
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 16)
        .ok_or_else(|| invalid("Codex approval choices missing or oversized"))?;
    let mut names = Vec::new();
    let mut responses = Vec::new();
    for (index, decision) in offered.iter().enumerate() {
        if offered[..index].contains(decision) {
            return Err(invalid("Repeated Codex approval choice"));
        }
        let name = match decision.as_str() {
            Some("accept") => "Allow once",
            Some("acceptForSession") => "Allow for this Codex session",
            Some("decline") => "Decline",
            Some("cancel") => "Cancel task",
            Some(_) => return Err(invalid("Unsupported Codex approval choice")),
            None if decision.as_object().is_some_and(|o| o.len() == 1) => {
                if let Some(value) = decision.get("acceptWithExecpolicyAmendment") {
                    if !value.as_object().is_some_and(|o| o.len() == 1)
                        || !value["execpolicy_amendment"].as_array().is_some_and(|a| {
                            !a.is_empty()
                                && a.iter().all(|s| s.as_str().is_some_and(|s| !s.is_empty()))
                        })
                    {
                        return Err(invalid("Codex command rule invalid"));
                    }
                    "Allow and save this command rule"
                } else if let Some(value) = decision.get("applyNetworkPolicyAmendment") {
                    let rule = &value["network_policy_amendment"];
                    if !value.as_object().is_some_and(|o| o.len() == 1)
                        || !rule.as_object().is_some_and(|o| o.len() == 2)
                        || !rule["host"].as_str().is_some_and(|s| !s.is_empty())
                    {
                        return Err(invalid("Codex network rule invalid"));
                    }
                    match rule["action"].as_str() {
                        Some("allow") => "Allow host and save rule",
                        Some("deny") => "Deny host and save rule",
                        _ => return Err(invalid("Codex network rule action invalid")),
                    }
                } else {
                    return Err(invalid("Unsupported Codex approval choice"));
                }
            }
            None => return Err(invalid("Codex approval choice malformed")),
        };
        names.push(name.into());
        responses.push(json!({"decision":decision}));
    }
    Ok((names, responses))
}
