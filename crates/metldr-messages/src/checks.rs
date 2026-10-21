use super::*;
use metldr_browser_tools::{BROWSER_POLICY, Call};

fn initial() -> Value {
    json!({"type":"message_start","message":{"id":"msg_1","type":"message",
        "role":"assistant","model":"fixture-model","content":[],"stop_reason":null,
        "stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":0}}})
}

fn stop(reason: &str) -> Vec<Value> {
    vec![
        json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null},"usage":{"output_tokens":12}}),
        json!({"type":"message_stop"}),
    ]
}

fn text_and_call() -> Vec<Value> {
    let mut events = vec![
        initial(),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","metadata":"kept"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private thought"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque-"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signature"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"opaque encrypted bytes"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Read বাংলা 🦀"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"call_1","name":"observe_page","input":{}}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{"}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"}"}}),
        json!({"type":"content_block_stop","index":3}),
    ];
    events.extend(stop("tool_use"));
    events
}

fn replay(events: &[Value], chunk: usize) -> Result<(stream::Turn, String)> {
    let bytes: Vec<u8> = events
        .iter()
        .flat_map(|e| {
            format!(
                "event: {}\r\ndata: {e}\r\n\r\n",
                e["type"].as_str().unwrap()
            )
            .into_bytes()
        })
        .collect();
    let mut parser = sse::Sse::default();
    let mut state = stream::State::new("fixture-model");
    let mut visible = String::new();
    for bytes in bytes.chunks(chunk) {
        for &b in bytes {
            if let Some((event, data)) = parser.byte(b)? {
                if let Some(text) = state.frame(&event, &data)? {
                    visible.push_str(&text);
                }
            }
        }
    }
    parser.finish()?;
    let turn = state.finish()?;
    assert_eq!(turn.text, visible);
    Ok((turn, visible))
}

fn client() -> Client {
    Client::new(
        "https://example.com/v1/messages",
        "fixture-model",
        "dummy-key",
        None,
    )
    .unwrap()
}

fn prompt() -> Prompt<'static> {
    Prompt {
        instruction: "inspect",
        title: "Title",
        url: "https://example.org/page",
        text: "untrusted page",
    }
}

#[test]
fn streams_unicode_and_preserves_exact_tool_continuation() {
    for chunk in [1, 2, 7, 127, 4096] {
        let (turn, text) = replay(&text_and_call(), chunk).unwrap();
        assert_eq!(text, "Read বাংলা 🦀");
        assert_eq!(turn.call.as_ref().unwrap().call, Call::Observe);
        let expected = json!([
            {"type":"thinking","thinking":"private thought","signature":"opaque-signature","metadata":"kept"},
            {"type":"redacted_thinking","data":"opaque encrypted bytes"},
            {"type":"text","text":"Read বাংলা 🦀"},
            {"type":"tool_use","id":"call_1","name":"observe_page","input":{}}
        ]);
        assert_eq!(json!(turn.content), expected);
        let mut conversation = client().start(prompt()).unwrap();
        conversation.phase = Phase::Pending(turn);
        let result = json!({"kind":"page_observation","untrusted":true});
        conversation.submit_tool_result(&result).unwrap();
        assert_eq!(
            conversation.request["messages"][1],
            json!({"role":"assistant","content":expected})
        );
        assert_eq!(
            conversation.request["messages"][2],
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":result.to_string()}]})
        );
        assert!(conversation.submit_tool_result(&result).is_err());
    }
}

#[test]
fn rejects_truncated_contradictory_or_incomplete_action_streams() {
    let events = text_and_call();
    for end in 0..events.len() {
        assert!(replay(&events[..end], 1).is_err());
    }
    for replacement in [
        json!({"type":"content_block_stop","index":9}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"wrong parent"}}),
    ] {
        let mut bad = events.clone();
        bad[4] = replacement;
        assert!(replay(&bad, 2).is_err());
    }
    for reason in [
        "end_turn",
        "max_tokens",
        "pause_turn",
        "refusal",
        "model_context_window_exceeded",
    ] {
        let mut bad = events.clone();
        bad[15]["delta"]["stop_reason"] = json!(reason);
        assert!(replay(&bad, 7).is_err());
    }
    let mut bad = events.clone();
    bad.insert(16,json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":13}}));
    assert!(replay(&bad, 7).is_err());
    let mut bad = events.clone();
    bad[13]["delta"]["partial_json"] = json!("broken}");
    assert!(replay(&bad, 1).is_err());
    let mut bad = events.clone();
    bad[0]["message"]["model"] = json!("different");
    assert!(replay(&bad, 1).is_err());
    let mut bad = events.clone();
    bad.insert(
        16,
        json!({"type":"error","error":{"message":"private details"}}),
    );
    assert!(replay(&bad, 7).is_err());
    let mut bad = events.clone();
    bad.push(events[8].clone());
    assert!(replay(&bad, 127).is_err());
    let mut future = events.clone();
    future.insert(1, json!({"type":"future_event","data":"ignored"}));
    future.push(json!({"type":"ping"}));
    assert!(replay(&future, 2).is_ok());
}

#[test]
fn emits_visible_text_before_terminal_and_in_content_order() {
    let mut state = stream::State::new("fixture-model");
    let events = text_and_call();
    let mut partial = String::new();
    for e in &events[..15] {
        if let Some(s) = state
            .frame(e["type"].as_str().unwrap(), &e.to_string())
            .unwrap()
        {
            partial.push_str(&s);
        }
    }
    assert_eq!(partial, "Read বাংলা 🦀");
    assert!(state.finish().is_err());
    let mut events = vec![
        initial(),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"A"}}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"X"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Y"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"B"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_stop","index":0}),
    ];
    events.extend(stop("end_turn"));
    assert_eq!(replay(&events, 1).unwrap().1, "ABXY");
}

#[test]
fn constructs_native_request_and_enforces_conversation_order() {
    let mut c = client().start(prompt()).unwrap();
    let q = &c.request;
    assert_eq!(q["stream"], true);
    assert_eq!(q["max_tokens"], 2048);
    assert_eq!(q["system"], BROWSER_POLICY);
    assert_eq!(q.as_object().unwrap().len(), 6);
    assert_eq!(q["tools"].as_array().unwrap().len(), 5);
    assert!(!q.to_string().contains("dummy-key"));
    assert_eq!(
        q["tools"][0]["input_schema"],
        metldr_browser_tools::definitions()[0].input_schema
    );
    assert_eq!(q["tools"][4]["name"], "select_option");
    assert_eq!(
        q["tools"][4]["input_schema"],
        metldr_browser_tools::definitions()[4].input_schema
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    c.phase = Phase::Pending(replay(&text_and_call(), 7).unwrap().0);
    rt.block_on(async {
        assert!(c.next(&AtomicBool::new(false), |_| Ok(())).await.is_err());
    });
    assert!(matches!(c.phase, Phase::Pending(_)));
    c.submit_tool_result(&json!({"kind":"verified"})).unwrap();
    rt.block_on(async {
        assert!(c.next(&AtomicBool::new(true), |_| Ok(())).await.is_err());
    });
    assert!(matches!(c.phase, Phase::Closed));
    assert!(c.submit_tool_result(&json!({})).is_err());
    for endpoint in [
        "http://127.0.0.1:1/v1/messages",
        "https://u@example.com/",
        "https://example.com/?key=x",
        "https://example.com/#x",
        "https://example.com:0/",
    ] {
        assert!(Client::new(endpoint, "model", "key", None).is_err());
    }
    assert!(Client::new("https://example.com/", "model", "bad\nkey", None).is_err());
    assert!(Client::new("https://example.com/", "model", "key", Some(b"invalid PEM")).is_err());
}
