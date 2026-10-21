use super::*;
use serde_json::json;

fn dynamic(id: &str, status: &str) -> Value {
    json!({"item":{"type":"dynamicToolCall","id":id,"tool":"get_weather","status":status,"arguments":{"city":"Paris"},"success":true,"contentItems":[{"type":"inputText","text":"OWNED_DYNAMIC_37"}]}})
}

#[test]
fn native_tool_details_and_outcomes_are_observations() {
    let turn = TurnId("turn-owned".into());
    let mut reports = Reports::default();
    let mut command = json!({"item":{"type":"commandExecution","id":"read-29","status":"inProgress","command":"cat owned-note.txt","cwd":"/owned","aggregatedOutput":null,"exitCode":null}});
    let started = reports.decode(&command, true, &turn).unwrap().unwrap();
    assert_eq!(started.item_id, "read-29");
    assert!(started.input.text.contains("cat owned-note.txt"));
    assert_eq!(started.state, ToolState::Started);
    command["item"]["status"] = json!("completed");
    command["item"]["exitCode"] = json!(0);
    command["item"]["aggregatedOutput"] = json!("NATIVE_NOTE_29\n");
    let completed = reports.decode(&command, false, &turn).unwrap().unwrap();
    assert_eq!(completed.item_id, started.item_id);
    assert_eq!(completed.state, ToolState::Finished(ToolOutcome::Succeeded));
    assert!(completed.output.unwrap().text.contains("NATIVE_NOTE_29"));
    let mut patch = json!({"item":{"type":"fileChange","id":"patch-31","status":"inProgress","changes":[{"path":"/owned/created-note.txt","kind":{"type":"update","move_path":"/owned/moved.txt"},"diff":"CREATED_NATIVE_31\n"}]}});
    reports.decode(&patch, true, &turn).unwrap();
    assert_eq!(
        reports.changes("patch-31").unwrap(),
        patch["item"]["changes"]
    );
    let update = json!({"itemId":"patch-31","changes":[{"path":"/owned/updated.txt","kind":{"type":"add"},"diff":"UPDATED_NATIVE_32\n"}]});
    reports.patch(&update).unwrap();
    assert_eq!(reports.changes("patch-31").unwrap(), update["changes"]);
    patch["item"]["status"] = json!("completed");
    let patch = reports.decode(&patch, false, &turn).unwrap().unwrap();
    assert!(patch.input.text.contains("/owned/moved.txt"));
    assert!(patch.output.unwrap().text.contains("CREATED_NATIVE_31"));
    assert!(reports.changes("patch-31").is_err());
    for (index, success, outcome) in [
        (0, json!(true), ToolOutcome::Succeeded),
        (1, json!(false), ToolOutcome::Failed),
        (2, Value::Null, ToolOutcome::Unspecified),
    ] {
        let mut value = dynamic(&format!("dynamic-{index}"), "completed");
        value["item"]["success"] = success;
        let report = reports.decode(&value, false, &turn).unwrap().unwrap();
        assert_eq!(report.state, ToolState::Finished(outcome));
        assert!(report.input.text.contains("Paris"));
        assert!(report.output.unwrap().text.contains("OWNED_DYNAMIC_37"));
    }
}

#[test]
fn native_reports_bound_text_and_reject_conflicting_lifecycles() {
    let turn = TurnId("turn-owned".into());
    let mut reports = Reports::default();
    let mut value = dynamic("call-owned", "inProgress");
    value["item"]["arguments"] = json!({"text":"é".repeat(2000)});
    let report = reports.decode(&value, true, &turn).unwrap().unwrap();
    assert!(report.input.truncated && report.input.text.len() <= 1024);
    assert!(reports.decode(&value, true, &turn).unwrap().is_none());
    value["item"]["status"] = json!("completed");
    value["item"]["contentItems"] = json!([{"type":"inputImage","imageUrl":"owned:omitted"},{"type":"inputText","text":"é".repeat(3000)}]);
    let report = reports.decode(&value, false, &turn).unwrap().unwrap();
    let output = report.output.unwrap();
    assert!(output.truncated && output.text.len() <= 2048);
    assert!(reports.decode(&value, false, &turn).unwrap().is_none());
    value["item"]["status"] = json!("inProgress");
    assert!(reports.decode(&value, true, &turn).is_err());
    reports.clear();
    for n in 0..32 {
        reports
            .decode(&dynamic(&format!("call-{n}"), "completed"), false, &turn)
            .unwrap();
    }
    assert!(
        reports
            .decode(&dynamic("overflow", "completed"), false, &turn)
            .is_err()
    );
    for bad in [
        json!({"item":{"type":"dynamicToolCall"}}),
        json!({"item":{"type":"fileChange","id":"p","status":"completed","changes":[{"path":"x","diff":7,"kind":{"type":"add"}}]}}),
        json!({"item":{"type":"commandExecution","id":"c","status":"completed","command":"x","cwd":"/owned","exitCode":"zero"}}),
    ] {
        assert!(Reports::default().decode(&bad, false, &turn).is_err());
    }
    assert!(
        Reports::default()
            .decode(&dynamic("bad\nidentity", "completed"), false, &turn)
            .is_err()
    );
    assert!(
        Reports::default()
            .decode(&dynamic("bad-phase", "completed"), true, &turn)
            .is_err()
    );
    let declined =
        json!({"item":{"type":"fileChange","id":"declined","status":"declined","changes":[]}});
    assert_eq!(
        Reports::default()
            .decode(&declined, false, &turn)
            .unwrap()
            .unwrap()
            .state,
        ToolState::Finished(ToolOutcome::Declined)
    );
}

#[test]
fn command_output_progress_is_bounded_and_keeps_lifecycle_authority() {
    let turn = TurnId("turn-owned".into());
    let mut reports = Reports::default();
    let command = json!({"item":{"type":"commandExecution","id":"command-progress","status":"inProgress","command":"read","cwd":"/owned","aggregatedOutput":null,"exitCode":null}});
    assert!(
        reports
            .progress(&json!({"itemId":"unknown","delta":"ignored"}), &turn)
            .unwrap()
            .is_none()
    );
    reports
        .decode(&dynamic("dynamic-progress", "inProgress"), true, &turn)
        .unwrap();
    assert!(
        reports
            .progress(
                &json!({"itemId":"dynamic-progress","delta":"ignored"}),
                &turn
            )
            .unwrap()
            .is_none()
    );
    let started = reports.decode(&command, true, &turn).unwrap().unwrap();
    assert!(
        reports
            .progress(&json!({"itemId":"command-progress","delta":""}), &turn)
            .unwrap()
            .is_none()
    );
    let first = reports
        .progress(&json!({"itemId":"command-progress","delta":"one"}), &turn)
        .unwrap()
        .unwrap();
    assert_eq!(first.1.text, "one");
    let repeated = reports
        .progress(&json!({"itemId":"command-progress","delta":"one"}), &turn)
        .unwrap()
        .unwrap();
    assert_eq!(repeated.1.text, "oneone");
    let ordered = reports
        .progress(&json!({"itemId":"command-progress","delta":"-then"}), &turn)
        .unwrap()
        .unwrap();
    assert_eq!(ordered.1.text, "oneone-then");
    assert!(reports.decode(&command, true, &turn).unwrap().is_none());
    let bounded = reports
        .progress(
            &json!({"itemId":"command-progress","delta":format!("{}é", "x".repeat(2036))}),
            &turn,
        )
        .unwrap()
        .unwrap();
    assert_eq!(bounded.1.text.len(), 2047);
    assert!(bounded.1.truncated);
    assert!(
        reports
            .progress(
                &json!({"itemId":"command-progress","delta":"after cap"}),
                &turn
            )
            .unwrap()
            .is_none()
    );
    let mut completed = command;
    completed["item"]["status"] = json!("completed");
    completed["item"]["exitCode"] = json!(0);
    completed["item"]["aggregatedOutput"] = json!("final output");
    let report = reports.decode(&completed, false, &turn).unwrap().unwrap();
    assert_eq!(report.item_id, started.item_id);
    assert_eq!(
        report.output.unwrap().text,
        "Reported outcome: succeeded\nExit code: 0\nfinal output"
    );
    assert!(
        reports
            .progress(&json!({"itemId":"command-progress","delta":"late"}), &turn)
            .unwrap()
            .is_none()
    );
    assert!(reports.decode(&completed, false, &turn).unwrap().is_none());
}
