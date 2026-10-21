use super::*;
use metldr_browser_tools::Call;
use serde_json::json;

fn events(tool: bool) -> Vec<Value> {
    let mut v = vec![
        json!({"event_type":"interaction.created","interaction":{"id":"owned-turn","model":"owned-model","status":"in_progress"}}),
    ];
    v.extend([
        json!({"event_type":"step.start","index":0,"step":{"type":"thought","signature":"partial"}}),
        json!({"event_type":"step.delta","index":0,"delta":{"type":"thought_summary","content":{"type":"text","text":"Check the live page."}}}),
        json!({"event_type":"step.delta","index":0,"delta":{"type":"thought_signature","signature":"final-opaque-signature"}}),
        json!({"event_type":"step.stop","index":0}),
        json!({"event_type":"step.start","index":1,"step":{"type":"model_output","content":[]}}),
        json!({"event_type":"step.delta","index":1,"delta":{"type":"text","text":"North "}}),
        json!({"event_type":"step.delta","index":1,"delta":{"type":"text","text":"🌍 ready"}}),
        json!({"event_type":"step.stop","index":1}),
    ]);
    if tool {
        v.extend([
            json!({"event_type":"step.start","index":2,"step":{"type":"function_call","id":"owned-call","name":"observe_page","arguments":{}}}),
            json!({"event_type":"step.delta","index":2,"delta":{"type":"arguments_delta","arguments":"{"}}),
            json!({"event_type":"step.delta","index":2,"delta":{"type":"arguments_delta","arguments":"}"}}),
            json!({"event_type":"step.stop","index":2}),
        ]);
    }
    v.push(json!({"event_type":"interaction.completed","interaction":{"id":"owned-turn","model":"owned-model","status":if tool {"requires_action"} else {"completed"}}}));
    v
}

fn replay(v: &[Value], size: usize) -> Result<(stream::Turn, String)> {
    let mut wire = String::new();
    for value in v {
        wire.push_str(&format!(
            "event: {}\r\ndata: {value}\r\n\r\n",
            value["event_type"].as_str().unwrap()
        ));
    }
    let mut sse = sse::Sse::default();
    let mut state = stream::State::default();
    let mut text = String::new();
    for chunk in wire.as_bytes().chunks(size) {
        for &byte in chunk {
            if let Some((event, data)) = sse.byte(byte)? {
                if let Some(delta) = state.frame(&event, &data)? {
                    text.push_str(&delta);
                }
            }
        }
    }
    sse.finish()?;
    Ok((state.finish()?, text))
}

fn client() -> Client {
    Client::new(
        "https://example.com/v1/interactions",
        "owned-model",
        "owned-key",
        None,
    )
    .unwrap()
}
fn prompt() -> Prompt<'static> {
    Prompt {
        instruction: "observe",
        title: "North",
        url: "https://example.com/report",
        text: "17 units",
    }
}

#[test]
fn streams_unicode_and_retains_complete_stateless_steps() {
    for size in [1, 2, 7, 127, 4096] {
        let (turn, text) = replay(&events(true), size).unwrap();
        assert_eq!(text, "North 🌍 ready");
        assert_eq!(turn.text, text);
        assert_eq!(turn.call.as_ref().unwrap().call, Call::Observe);
        let mut conversation = client().start(prompt()).unwrap();
        conversation.phase = Phase::Pending(turn);
        let result = json!({"kind":"page_observation","untrusted":true});
        conversation.submit_tool_result(&result).unwrap();
        assert!(conversation.submit_tool_result(&result).is_err());
        let input = &conversation.request["input"];
        assert_eq!(
            input[1],
            json!({"type":"thought","signature":"final-opaque-signature","summary":[{"type":"text","text":"Check the live page."}]})
        );
        assert_eq!(
            input[2],
            json!({"type":"model_output","content":[{"type":"text","text":"North 🌍 ready"}]})
        );
        assert_eq!(input[3]["id"], "owned-call");
        assert_eq!(
            input[4],
            json!({"type":"function_result","name":"observe_page","call_id":"owned-call","result":[{"type":"text","text":result.to_string()}]})
        );
        let (turn, text) = replay(&events(false), size).unwrap();
        assert!(turn.call.is_none());
        assert_eq!(text, turn.text);
    }
}

#[test]
fn rejects_partial_failed_and_contradictory_streams() {
    let valid = events(true);
    for end in 0..valid.len() {
        assert!(replay(&valid[..end], 1).is_err());
    }
    for mutation in 0..8 {
        let mut bad = valid.clone();
        let last = bad.len() - 1;
        match mutation {
            0 => bad[last]["interaction"]["id"] = json!("wrong"),
            1 => bad[last]["interaction"]["status"] = json!("completed"),
            2 => bad[last]["interaction"]["status"] = json!("incomplete"),
            3 => bad[last]["interaction"]["model"] = json!("changed"),
            4 => bad[10]["index"] = json!(99),
            5 => bad[12]["event_type"] = json!("step.delta"),
            6 => bad[last]["interaction"]["steps"] = json!([]),
            _ => bad[9]["step"]["name"] = json!("unsupported_browser_tool"),
        }
        assert!(replay(&bad, 7).is_err(), "mutation {mutation}");
    }
    for (tool, status) in [(true, "completed"), (false, "requires_action")] {
        let mut bad = events(tool);
        bad.insert(bad.len()-1,json!({"event_type":"interaction.status_update","interaction_id":"owned-turn","status":status}));
        assert!(replay(&bad, 1).is_err());
    }
    let mut wrong_parent = valid.clone();
    wrong_parent.insert(12,json!({"event_type":"step.delta","index":2,"delta":{"type":"thought_signature","signature":"misplaced"}}));
    assert!(replay(&wrong_parent, 1).is_err());
    let mut no_op = valid.clone();
    no_op.insert(
        12,
        json!({"event_type":"step.delta","index":2,"delta":{"type":"arguments_delta"}}),
    );
    assert!(replay(&no_op, 1).is_ok());
    let mut state = stream::State::default();
    assert!(state.frame("done", "[DONE]").is_err());
    let mut state = stream::State::default();
    for v in &valid {
        state
            .frame(v["event_type"].as_str().unwrap(), &v.to_string())
            .unwrap();
    }
    state.frame("done", "[DONE]").unwrap();
    assert!(state.frame("done", "[DONE]").is_err());
    assert!(state.frame("interaction.status_update", "{}").is_err());
}

#[test]
fn interleaved_model_steps_follow_step_order() {
    let mut v = events(false)[..5].to_vec();
    v.extend([
        json!({"event_type":"step.start","index":1,"step":{"type":"model_output","content":[]}}),
        json!({"event_type":"step.start","index":2,"step":{"type":"model_output","content":[]}}),
    ]);
    for (index, text) in [(1, "A"), (2, "X"), (1, "B"), (2, "Y")] {
        v.push(
            json!({"event_type":"step.delta","index":index,"delta":{"type":"text","text":text}}),
        );
    }
    for index in [2, 1] {
        v.push(json!({"event_type":"step.stop","index":index}));
    }
    v.push(json!({"event_type":"interaction.completed","interaction":{"id":"owned-turn","status":"completed"}}));
    let (turn, text) = replay(&v, 1).unwrap();
    assert_eq!(text, "ABXY");
    assert_eq!(turn.text, text);
}

#[tokio::test(flavor = "current_thread")]
async fn encodes_native_request_and_enforces_conversation_order() {
    let mut conversation = client().start(prompt()).unwrap();
    let request = &conversation.request;
    assert_eq!(request["store"], false);
    assert_eq!(request["stream"], true);
    assert_eq!(
        request["generation_config"],
        json!({"max_output_tokens":2048,"tool_choice":"validated"})
    );
    assert_eq!(request["input"][0]["type"], "user_input");
    assert_eq!(
        request["tools"][0]["parameters"],
        json!({"type":"object","properties":{},"additionalProperties":false})
    );
    assert_eq!(request["tools"].as_array().unwrap().len(), 5);
    assert_eq!(request["tools"][4]["name"], "select_option");
    assert_eq!(
        request["tools"][4]["parameters"],
        metldr_browser_tools::definitions()[4].input_schema
    );
    assert!(request.get("previous_interaction_id").is_none());
    assert!(
        request["system_instruction"]
            .as_str()
            .unwrap()
            .contains("untrusted")
    );
    assert!(!request.to_string().contains("owned-key"));
    for tool in request["tools"].as_array().unwrap() {
        assert!(tool.get("strict").is_none());
        assert_eq!(tool["type"], "function");
    }
    for endpoint in [
        "http://example.com/",
        "https://user@example.com/",
        "https://example.com/?key=secret",
        "https://example.com/#fragment",
    ] {
        assert!(Client::new(endpoint, "model", "key", None).is_err());
    }
    assert!(conversation.submit_tool_result(&json!({})).is_err());
    let stop = AtomicBool::new(false);
    conversation.phase = Phase::Pending(replay(&events(true), 7).unwrap().0);
    assert!(conversation.next(&stop, |_| Ok(())).await.is_err());
    conversation
        .submit_tool_result(&json!({"kind":"owned"}))
        .unwrap();
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(conversation.next(&stop, |_| Ok(())).await.is_err());
    stop.store(false, std::sync::atomic::Ordering::Release);
    assert!(conversation.next(&stop, |_| Ok(())).await.is_err());
}
