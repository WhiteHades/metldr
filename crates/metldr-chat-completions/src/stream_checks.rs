use crate::stream::State;
use serde_json::{Value, json};

fn frame(state: &mut State, value: Value) -> Option<String> {
    if value.as_str() == Some("[DONE]") {
        state.frame("", "[DONE]").unwrap()
    } else {
        state.frame("", &value.to_string()).unwrap()
    }
}

fn chunk(delta: Value, finish: Option<&str>) -> Value {
    json!({"id":"chatcmpl-test","choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}

#[test]
fn ordered_calls_wait_for_done_and_keep_usage() {
    let parts = [
        json!({"id":"chatcmpl-test","usage":null,"choices":[{"index":0,"delta":{"role":"assistant","content":"o","reasoning_content":null},"finish_reason":null}]}),
        chunk(json!({"content":"k"}), None),
        chunk(
            json!({"tool_calls":[
                {"index":0,"id":"call-a","type":"function","function":{"name":"observe_page","arguments":"{"}},
                {"index":1,"id":"call-b","type":"function","function":{"name":"observe_page","arguments":"{}"}}
            ]}),
            None,
        ),
        chunk(
            json!({"tool_calls":[{"index":0,"function":{"name":"observe_page","arguments":"}"}}]}),
            Some("tool_calls"),
        ),
        json!({"id":"chatcmpl-test","choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":1}}}),
    ];
    let mut incomplete = State::default();
    let mut state = State::default();
    for part in parts {
        frame(&mut incomplete, part.clone());
        frame(&mut state, part);
    }
    assert_eq!(
        incomplete.finish().err().unwrap().to_string(),
        "Chat stream is incomplete"
    );
    frame(&mut state, json!("[DONE]"));
    let turn = state.finish().unwrap();
    assert_eq!(turn.calls.len(), 2);
    assert_eq!(turn.calls[0].id, "call-a");
    assert_eq!(turn.calls[1].id, "call-b");
    assert_eq!(turn.text, "ok");
    assert_eq!(
        turn.assistant["tool_calls"][0]["function"]["arguments"],
        "{}"
    );
    assert!(turn.assistant["reasoning_content"].is_null());
    assert_eq!(turn.usage.unwrap().cached_input_tokens, Some(2));
}

#[test]
fn malformed_sibling_truncation_and_stream_error_fail_closed() {
    let mut state = State::default();
    frame(
        &mut state,
        chunk(
            json!({"role":"assistant","tool_calls":[
                {"index":0,"id":"call-a","function":{"name":"observe_page","arguments":"{}"}},
                {"index":1,"id":"call-b","function":{"name":"missing","arguments":"{}"}}
            ]}),
            Some("tool_calls"),
        ),
    );
    frame(&mut state, json!("[DONE]"));
    assert_eq!(
        state.finish().err().unwrap().to_string(),
        "unknown browser tool"
    );

    let mut state = State::default();
    assert!(
        state
            .frame(
                "",
                &chunk(json!({"role":"assistant","content":"answer"}), Some("stop")).to_string()
            )
            .is_ok()
    );
    assert!(state.finish().is_err());

    let mut state = State::default();
    assert!(state.frame("", &json!({"id":"chatcmpl-test","error":{"message":"private upstream text"},"choices":[]}).to_string()).is_err());

    let mut state = State::default();
    frame(
        &mut state,
        chunk(json!({"role":"assistant","content":"answer"}), Some("stop")),
    );
    assert!(
        state
            .frame("", &chunk(json!({"content":"late"}), None).to_string())
            .is_err()
    );
}

#[test]
fn reasoning_merges_compatible_pieces_and_rejects_conflicting_identity() {
    let mut state = State::default();
    frame(
        &mut state,
        chunk(
            json!({"role":"assistant","content":"answer","reasoning_details":[
                {"type":"reasoning.text","id":"r1","format":"plain","text":null},
                {"type":"reasoning.text","id":null,"text":"one","summary":"retained unknown field"},
                {"type":"reasoning.text","format":"plain","text":" two"}
            ]}),
            Some("stop"),
        ),
    );
    frame(&mut state, json!("[DONE]"));
    let turn = state.finish().unwrap();
    assert_eq!(turn.assistant["reasoning_details"][0]["text"], "one two");
    assert_eq!(
        turn.assistant["reasoning_details"][0]["summary"],
        "retained unknown field"
    );
    assert_eq!(turn.assistant["reasoning_details"][0]["id"], "r1");

    let mut state = State::default();
    assert!(
        state
            .frame(
                "",
                &chunk(
                    json!({"role":"assistant","reasoning_details":[
                        {"type":"reasoning.summary","id":"r1","summary":"one"},
                        {"type":"reasoning.summary","id":"r2","summary":"two"}
                    ]}),
                    None
                )
                .to_string()
            )
            .is_ok()
    );
    frame(
        &mut state,
        chunk(json!({"role":"assistant","content":"answer"}), Some("stop")),
    );
    frame(&mut state, json!("[DONE]"));
    let turn = state.finish().unwrap();
    assert_eq!(
        turn.assistant["reasoning_details"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let mut state = State::default();
    assert!(
        state
            .frame(
                "",
                &chunk(
                    json!({"role":"assistant","reasoning_details":[
                        {"type":"reasoning.summary","id":"r1","signature":"sig1","summary":"one"},
                        {"type":"reasoning.summary","id":"r1","signature":"sig2","summary":"two"}
                    ]}),
                    None
                )
                .to_string()
            )
            .is_err()
    );
    let mut state = State::default();
    let error = state
        .frame(
            "",
            &chunk(
                json!({"role":"assistant","reasoning_details":[
                    {"type":"reasoning.text","id":"r1","index":0,"text":"one"},
                    {"type":"reasoning.text","id":"r2","index":1,"text":"two"},
                    {"type":"reasoning.text","id":"r1","index":2,"text":"three"}
                ]}),
                None,
            )
            .to_string(),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "conflicting Chat reasoning metadata");
}

#[test]
fn encrypted_reasoning_allows_tool_calls_with_stop() {
    for content in [Value::Null, json!("")] {
        let mut state = State::default();
        frame(
            &mut state,
            chunk(
                json!({"role":"assistant","content":content,"reasoning_details":[{"type":"reasoning.encrypted","data":"opaque"}],"tool_calls":[
                {"index":0,"id":"call-a","function":{"name":"observe_page","arguments":"{}"}}
                ]}),
                Some("stop"),
            ),
        );
        frame(
            &mut state,
            json!({"id":"chatcmpl-test","choices":[],"usage":{"prompt_tokens":7,"completion_tokens":null,"prompt_cache_hit_tokens":2,"completion_tokens_details":null}}),
        );
        frame(&mut state, json!("[DONE]"));
        let turn = state.finish().unwrap();
        assert_eq!(turn.calls.len(), 1);
        assert_eq!(turn.assistant["content"], content);
        assert_eq!(turn.usage.unwrap().cached_input_tokens, Some(2));
    }
    let mut state = State::default();
    frame(
        &mut state,
        chunk(
            json!({"role":"assistant","tool_calls":[
                {"index":0,"id":"call-a","function":{"name":"observe_page","arguments":"{}"}}
            ]}),
            Some("stop"),
        ),
    );
    frame(&mut state, json!("[DONE]"));
    assert_eq!(
        state.finish().err().unwrap().to_string(),
        "Chat stop with tool calls lacks encrypted reasoning"
    );
}
