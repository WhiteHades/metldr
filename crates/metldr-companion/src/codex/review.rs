use metldr_codex_session::ApprovalKind;
use serde_json::{Map, Value};

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        value => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn append_field(output: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value.filter(|value| !value.is_null()) {
        output.push_str(label);
        output.push_str(":\n");
        output.push_str(&value_text(value));
        output.push('\n');
    }
}

fn append_object(output: &mut String, label: &str, object: Map<String, Value>) {
    if !object.is_empty() {
        append_field(output, label, Some(&Value::Object(object)));
    }
}

fn file_changes(output: &mut String, changes: &[Value]) {
    output.push_str("File changes:\n");
    for (index, change) in changes.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        append_field(output, "Path", change.get("path"));

        let mut kind_details = change
            .get("kind")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let kind = kind_details
            .remove("type")
            .or_else(|| change.get("kind").filter(|kind| !kind.is_object()).cloned());
        append_field(output, "Kind", kind.as_ref());
        let move_path = kind_details.remove("move_path").or_else(|| {
            change
                .get("move_path")
                .or_else(|| change.get("movePath"))
                .cloned()
        });
        append_field(output, "Move to", move_path.as_ref());
        append_object(output, "Other change kind details", kind_details);

        if let Some(diff) = change.get("diff") {
            output.push_str("Diff:\n");
            output.push_str(&value_text(diff));
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }

        if let Some(details) = change.as_object() {
            let mut details = details.clone();
            for key in ["path", "kind", "move_path", "movePath", "diff"] {
                details.remove(key);
            }
            append_object(output, "Other change details", details);
        }
    }
}

fn command_effects(output: &mut String, review: &Value) {
    let decisions = review.get("availableDecisions").and_then(Value::as_array);
    let has_offered_exec = decisions.is_some_and(|decisions| {
        decisions.iter().any(|decision| {
            decision
                .get("acceptWithExecpolicyAmendment")
                .and_then(|value| value.get("execpolicy_amendment"))
                == review.get("proposedExecpolicyAmendment")
        })
    });
    if let Some(amendment) = review.get("proposedExecpolicyAmendment")
        && !has_offered_exec
    {
        append_field(output, "Proposed saved command rule", Some(amendment));
    }
    let has_offered_network = decisions.is_some_and(|decisions| {
        decisions.iter().any(|decision| {
            decision
                .get("applyNetworkPolicyAmendment")
                .and_then(|value| value.get("network_policy_amendment"))
                == review.get("proposedNetworkPolicyAmendment")
        })
    });
    if let Some(amendment) = review.get("proposedNetworkPolicyAmendment")
        && !has_offered_network
    {
        append_field(output, "Proposed saved network rule", Some(amendment));
    }
    if let Some(decisions) = decisions {
        for decision in decisions {
            if let Some(amendment) = decision
                .get("acceptWithExecpolicyAmendment")
                .and_then(|value| value.get("execpolicy_amendment"))
            {
                append_field(output, "Allow and save this command rule", Some(amendment));
            }
            if let Some(amendment) = decision
                .get("applyNetworkPolicyAmendment")
                .and_then(|value| value.get("network_policy_amendment"))
            {
                let label = match amendment.get("action").and_then(Value::as_str) {
                    Some("allow") => "Allow host and save rule",
                    Some("deny") => "Deny host and save rule",
                    _ => "Saved network rule offered",
                };
                append_field(output, label, Some(amendment));
            }
        }
    }
}

pub(super) fn render(kind: ApprovalKind, review: &Value) -> String {
    let mut output = String::new();
    let mut remaining = review.as_object().cloned().unwrap_or_default();

    match kind {
        ApprovalKind::Command => {
            append_field(&mut output, "Command", remaining.get("command"));
            append_field(&mut output, "Working directory", remaining.get("cwd"));
            append_field(&mut output, "Reason", remaining.get("reason"));
            command_effects(&mut output, review);
            for key in [
                "command",
                "cwd",
                "reason",
                "availableDecisions",
                "proposedExecpolicyAmendment",
                "proposedNetworkPolicyAmendment",
            ] {
                remaining.remove(key);
            }
        }
        ApprovalKind::FileChange => {
            if let Some(changes) = remaining.get("changes").and_then(Value::as_array) {
                file_changes(&mut output, changes);
                remaining.remove("changes");
            }
            append_field(&mut output, "Reason", remaining.get("reason"));
            append_field(&mut output, "Grant root", remaining.get("grantRoot"));
            remaining.remove("reason");
            remaining.remove("grantRoot");
        }
        ApprovalKind::Permissions => {
            append_field(
                &mut output,
                "Proposed permission profile",
                remaining.get("permissions"),
            );
            remaining.remove("permissions");
        }
    }

    append_object(&mut output, "Other review details", remaining);
    if output.is_empty() {
        value_text(review)
    } else {
        output
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_review_leads_with_action_and_shows_saved_effects_and_extra_fields() {
        let review = json!({
            "command":"/bin/sh -c 'printf ok'",
            "cwd":"/owned/work",
            "reason":"Create the requested marker.",
            "environmentId":"local",
            "proposedExecpolicyAmendment":["/bin/sh", "-c", "printf ok"],
            "availableDecisions":[
                "accept",
                {"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["/bin/sh", "-c", "printf ok"]}},
                {"applyNetworkPolicyAmendment":{"network_policy_amendment":{"action":"allow","host":"example.test"}}}
            ]
        });
        let text = render(ApprovalKind::Command, &review);
        let command = text.find("Command:\n").unwrap();
        let cwd = text.find("Working directory:\n").unwrap();
        let reason = text.find("Reason:\n").unwrap();
        assert!(command < cwd && cwd < reason);
        assert!(text.contains("Allow and save this command rule:\n"));
        assert!(text.contains("Allow host and save rule:\n"));
        assert!(text.contains("example.test"));
        assert!(text.contains("environmentId"));
        assert!(!text.contains("proposedExecpolicyAmendment"));
    }

    #[test]
    fn file_review_keeps_paths_moves_and_diff_as_readable_lines() {
        let review = json!({
            "changes":[{
                "path":"/owned/old.txt",
                "kind":{"type":"update","move_path":"/owned/new.txt"},
                "diff":"first line\nsecond line\n",
                "futureDetail":"kept"
            }],
            "reason":null,
            "grantRoot":null
        });
        let text = render(ApprovalKind::FileChange, &review);
        assert!(text.contains("Path:\n/owned/old.txt\n"));
        assert!(text.contains("Kind:\nupdate\n"));
        assert!(text.contains("Move to:\n/owned/new.txt\n"));
        assert!(text.contains("Diff:\nfirst line\nsecond line\n"));
        assert!(text.contains("futureDetail"));
        assert!(!text.contains("first line\\nsecond line"));
        assert!(!text.contains("Reason:\nnull"));
        assert!(!text.contains("Grant root:\nnull"));
    }

    #[test]
    fn file_review_shows_nonempty_optional_fields() {
        let review = json!({
            "changes":[],
            "reason":"Review the generated change.",
            "grantRoot":"/owned/work"
        });
        let text = render(ApprovalKind::FileChange, &review);
        assert!(text.contains("Reason:\nReview the generated change.\n"));
        assert!(text.contains("Grant root:\n/owned/work\n"));
        assert!(!text.contains("Other review details"));
    }

    #[test]
    fn permission_review_shows_complete_profile_and_unknown_details() {
        let review = json!({
            "permissions":{"fileSystem":{"write":["/owned/output"]},"network":{"allow":["example.test"]}},
            "scope":"session",
            "futureDetail":"kept"
        });
        let text = render(ApprovalKind::Permissions, &review);
        assert!(text.contains("Proposed permission profile:\n"));
        assert!(text.contains("/owned/output"));
        assert!(text.contains("example.test"));
        assert!(text.contains("scope"));
        assert!(text.contains("futureDetail"));
    }
}
