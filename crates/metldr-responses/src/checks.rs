use super::*;
use metldr_browser_tools::Call;
use stream::Turn;

fn events(tool: bool) -> Vec<Value> {
    let response = json!({"id":"resp_owned","object":"response","model":"resolved-model-2026",
        "store":false,"status":"in_progress"});
    let mut events = vec![json!({"type":"response.created","response":response})];
    let reasoning = json!({"type":"reasoning","id":"rs_owned","summary":[],
        "encrypted_content":"opaque-final-owned"});
    events.push(json!({"type":"response.output_item.added","output_index":0,
        "item":{"type":"reasoning","id":"rs_owned","summary":[],"encrypted_content":"partial"}}));
    events.push(json!({"type":"response.output_item.done","output_index":0,"item":reasoning}));
    let item = if tool {
        let arguments = "{}";
        let item = json!({"type":"function_call","id":"fc_owned","call_id":"call_owned",
            "name":"observe_page","arguments":arguments,"status":"completed"});
        let mut initial = item.clone();
        initial["status"] = json!("in_progress");
        initial["arguments"] = json!("");
        events.push(json!({"type":"response.output_item.added","output_index":1,"item":initial}));
        events.push(
            json!({"type":"response.function_call_arguments.delta","output_index":1,
            "item_id":"fc_owned","delta":"{"}),
        );
        events.push(
            json!({"type":"response.function_call_arguments.delta","output_index":1,
            "item_id":"fc_owned","delta":"}"}),
        );
        events.push(
            json!({"type":"response.function_call_arguments.done","output_index":1,
            "item_id":"fc_owned","arguments":arguments}),
        );
        item
    } else {
        let item = json!({"type":"message","id":"msg_owned","role":"assistant",
            "status":"completed","phase":"final_answer",
            "content":[{"type":"output_text","text":"North 🌍 ready","annotations":[]}]});
        let mut initial = item.clone();
        initial["status"] = json!("in_progress");
        initial["content"] = json!([]);
        events.push(json!({"type":"response.output_item.added","output_index":1,"item":initial}));
        for delta in ["North ", "🌍 ready"] {
            events.push(json!({"type":"response.output_text.delta","output_index":1,
                "item_id":"msg_owned","content_index":0,"delta":delta}));
        }
        events.push(json!({"type":"response.output_text.done","output_index":1,
            "item_id":"msg_owned","content_index":0,"text":"North 🌍 ready"}));
        item
    };
    events.push(json!({"type":"response.output_item.done","output_index":1,"item":item}));
    let mut complete = response;
    complete["status"] = json!("completed");
    complete["output"] = json!([reasoning, item]);
    events.push(json!({"type":"response.completed","response":complete}));
    for (at, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(at * 2);
    }
    events
}

fn replay(events: &[Value], chunk_size: usize) -> Result<(Turn, String)> {
    let stream: String = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\r\ndata: {}\r\n\r\n",
                event["type"].as_str().unwrap(),
                event
            )
        })
        .collect();
    let mut sse = sse::Sse::default();
    let mut state = stream::State::default();
    let mut text = String::new();
    for chunk in stream.as_bytes().chunks(chunk_size) {
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

#[test]
fn streams_unicode_and_preserves_finalized_opaque_tool_history() {
    for size in [1, 2, 7, 127, 4096] {
        let (turn, text) = replay(&events(false), size).unwrap();
        assert_eq!(text, "North 🌍 ready");
        assert_eq!(turn.text, text);
        assert!(turn.call.is_none());
        assert_eq!(turn.output[1]["phase"], "final_answer");
        let (tool, text) = replay(&events(true), size).unwrap();
        assert!(text.is_empty());
        assert_eq!(tool.call.as_ref().unwrap().call, Call::Observe);
        let mut conversation = client().start(prompt()).unwrap();
        conversation.phase = Phase::Pending(tool);
        let result = json!({"kind":"page_observation","untrusted":true});
        conversation.submit_tool_result(&result).unwrap();
        assert!(conversation.submit_tool_result(&result).is_err());
        let request = conversation.request;
        assert_eq!(
            request["input"][1]["encrypted_content"],
            "opaque-final-owned"
        );
        assert_eq!(request["input"][2]["id"], "fc_owned");
        assert_eq!(request["input"][3]["call_id"], "call_owned");
        assert_ne!(request["input"][3]["call_id"], request["input"][2]["id"]);
        assert_eq!(
            serde_json::from_str::<Value>(request["input"][3]["output"].as_str().unwrap()).unwrap(),
            result
        );
    }
    let mut optional_status = events(true);
    optional_status[7]["item"]
        .as_object_mut()
        .unwrap()
        .remove("status");
    optional_status[8]["response"]["output"][1]
        .as_object_mut()
        .unwrap()
        .remove("status");
    assert!(replay(&optional_status, 7).is_ok());
}

#[test]
fn rejects_partial_calls_identity_changes_and_false_completion() {
    let valid = events(true);
    for at in 0..valid.len() {
        assert!(replay(&valid[..at], 1).is_err());
    }
    for mutation in 0..8 {
        let mut bad = valid.clone();
        let last = bad.len() - 1;
        match mutation {
            0 => bad[4]["item_id"] = json!("other"),
            1 => bad[6]["arguments"] = json!("{\"extra\":true}"),
            2 => bad[7]["item"]["call_id"] = json!("different"),
            3 => bad[last]["response"]["status"] = json!("incomplete"),
            4 => bad[last]["response"]["id"] = json!("other"),
            5 => bad[last]["response"]["output"][0]["encrypted_content"] = json!("contradiction"),
            6 => bad[last]["sequence_number"] = json!(0),
            _ => bad[last]["type"] = json!("response.failed"),
        }
        assert!(replay(&bad, 7).is_err(), "mutation {mutation}");
    }
    let mut after = valid.clone();
    after.push(json!({"type":"response.in_progress","sequence_number":999}));
    assert!(replay(&after, 4096).is_err());
    let mut sse = sse::Sse::default();
    for &byte in b"data: {}" {
        sse.byte(byte).unwrap();
    }
    assert!(sse.finish().is_err());
}

#[test]
fn interleaved_parts_follow_output_order() {
    let original = events(false);
    let mut interleaved = original[..4].to_vec();
    for (part, delta) in [(0, "A"), (1, "X"), (0, "B"), (1, "Y")] {
        interleaved.push(json!({"type":"response.output_text.delta","output_index":1,
            "item_id":"msg_owned","content_index":part,"delta":delta}));
    }
    for (part, text) in [(1, "XY"), (0, "AB")] {
        interleaved.push(json!({"type":"response.output_text.done","output_index":1,
            "item_id":"msg_owned","content_index":part,"text":text}));
    }
    let mut done = original[7].clone();
    done["item"]["content"] = json!([
        {"type":"output_text","text":"AB","annotations":[]},
        {"type":"output_text","text":"XY","annotations":[]}]);
    let mut terminal = original[8].clone();
    terminal["response"]["output"][1] = done["item"].clone();
    interleaved.extend([done, terminal]);
    for (at, event) in interleaved.iter_mut().enumerate() {
        event["sequence_number"] = json!(at);
    }
    let (turn, rendered) = replay(&interleaved, 1).unwrap();
    assert_eq!(turn.text, "ABXY");
    assert_eq!(rendered, turn.text);
}

#[test]
fn constructs_direct_stateless_request_without_provider_credentials() {
    let conversation = client().start(prompt()).unwrap();
    let request = conversation.request;
    assert_eq!(request["model"], "configured-alias");
    assert_eq!(request["stream"], true);
    assert_eq!(request["store"], false);
    assert_eq!(request["parallel_tool_calls"], false);
    assert_eq!(request["include"], json!(["reasoning.encrypted_content"]));
    assert!(request.get("previous_response_id").is_none());
    assert!(!request.to_string().contains("Bearer owned"));
    assert_eq!(request["tools"].as_array().unwrap().len(), 3);
    for tool in request["tools"].as_array().unwrap() {
        assert_eq!(tool["strict"], true);
        assert!(tool["parameters"]["required"].is_array());
        assert_eq!(tool["parameters"]["additionalProperties"], false);
    }
}

fn client() -> Client {
    Client::new(
        "https://example.com/v1/responses",
        "configured-alias",
        "owned",
        None,
    )
    .unwrap()
}

fn prompt() -> Prompt<'static> {
    Prompt {
        instruction: "observe",
        title: "North",
        url: "https://example.com/article",
        text: "17 units",
    }
}

#[tokio::test(flavor = "current_thread")]
async fn enforces_order_and_rejects_unsafe_configuration() {
    for endpoint in [
        "http://example.com/v1/responses",
        "https://user@example.com/",
        "https://example.com/?key=secret",
        "https://example.com/#fragment",
        "https://example.com:0/",
    ] {
        assert!(Client::new(endpoint, "model", "owned", None).is_err());
    }
    for (model, key) in [
        ("", "owned"),
        ("invalid model", "owned"),
        ("model", ""),
        ("model", "invalid\r\nkey"),
    ] {
        assert!(Client::new("https://example.com/", model, key, None).is_err());
    }
    assert!(Client::new("https://example.com/", "model", "owned", Some(b"invalid")).is_err());
    let large = "x".repeat(131_073);
    assert!(
        client()
            .start(Prompt {
                text: &large,
                ..prompt()
            })
            .is_err()
    );
    let stop = AtomicBool::new(false);
    let mut conversation = client().start(prompt()).unwrap();
    assert!(conversation.submit_tool_result(&json!({})).is_err());
    let (turn, _) = replay(&events(true), 1).unwrap();
    conversation.phase = Phase::Pending(turn);
    assert!(conversation.next(&stop, |_| Ok(())).await.is_err());
    conversation
        .submit_tool_result(&json!({"kind":"owned_result"}))
        .unwrap();
    assert!(conversation.submit_tool_result(&json!({})).is_err());
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(conversation.next(&stop, |_| Ok(())).await.is_err());
    stop.store(false, std::sync::atomic::Ordering::Release);
    assert!(conversation.next(&stop, |_| Ok(())).await.is_err());
    assert!(conversation.submit_tool_result(&json!({})).is_err());
}
