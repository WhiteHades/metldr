use super::transport_checks::{Protocol, text_server};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

const MODEL: &str = "text-check-model";
const KEY: &str = "dummy-owned-browser-fixture";
const SYSTEM: &str = "Summarize only the supplied text.";
const INPUT: &str = "বাংলা page 🦀";
const OUTPUT: &str = "শুভেচ্ছা 🌍";

fn frames(protocol: Protocol, tool: bool, complete: bool) -> String {
    let events: Vec<(String, Value)> = match protocol {
        Protocol::Messages => {
            let mut v = vec![(
                "message_start".into(),
                json!({"type":"message_start","message":{"type":"message","id":"msg_1","model":MODEL,"role":"assistant","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
            )];
            if tool {
                v.push(("content_block_start".into(), json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_1","name":"observe_page","input":{}}})));
                v.push((
                    "content_block_stop".into(),
                    json!({"type":"content_block_stop","index":0}),
                ));
            } else {
                v.push(("content_block_start".into(), json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})));
                v.push(("content_block_delta".into(), json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":OUTPUT}})));
                v.push((
                    "content_block_stop".into(),
                    json!({"type":"content_block_stop","index":0}),
                ));
            }
            if complete {
                v.push(("message_delta".into(),json!({"type":"message_delta","delta":{"stop_reason":if tool {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":2}})));
                v.push(("message_stop".into(), json!({"type":"message_stop"})));
            }
            v
        }
        Protocol::Responses => {
            let item = if tool {
                json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"observe_page","arguments":"{}","status":"completed"})
            } else {
                json!({"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":OUTPUT,"annotations":[]}]})
            };
            let mut added = item.clone();
            added["status"] = json!("in_progress");
            if tool {
                added["arguments"] = json!("");
            } else {
                added["content"] = json!([]);
            }
            let mut events = vec![
                json!({"type":"response.created","response":{"id":"resp_1","object":"response","store":false,"model":MODEL,"status":"in_progress"}}),
                json!({"type":"response.output_item.added","output_index":0,"item":added}),
            ];
            if tool {
                events.push(json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","delta":"{}"}));
                events.push(json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","arguments":"{}"}));
            } else {
                events.push(json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg_1","content_index":0,"delta":OUTPUT}));
                events.push(json!({"type":"response.output_text.done","output_index":0,"item_id":"msg_1","content_index":0,"text":OUTPUT}));
            }
            events.push(json!({"type":"response.output_item.done","output_index":0,"item":item}));
            if complete {
                events.push(json!({"type":"response.completed","response":{"id":"resp_1","object":"response","store":false,"model":MODEL,"status":"completed","output":[item]}}));
            }
            return events
                .iter_mut()
                .enumerate()
                .map(|(sequence, event)| {
                    event["sequence_number"] = json!(sequence);
                    format!(
                        "event: {}\ndata: {event}\n\n",
                        event["type"].as_str().unwrap()
                    )
                })
                .collect();
        }
        Protocol::Google => {
            let mut v = vec![
                json!({"event_type":"interaction.created","interaction":{"id":"ix_1","model":MODEL,"status":"in_progress"}}),
            ];
            if tool {
                v.push(json!({"event_type":"step.start","index":0,"step":{"type":"function_call","id":"call_1","name":"observe_page","arguments":{}}}));
            } else {
                v.push(json!({"event_type":"step.start","index":0,"step":{"type":"model_output","content":[]}}));
                v.push(json!({"event_type":"step.delta","index":0,"delta":{"type":"text","text":OUTPUT}}));
            }
            if complete {
                v.push(json!({"event_type":"step.stop","index":0}));
                v.push(json!({"event_type":"interaction.completed","interaction":{"id":"ix_1","model":MODEL,"status":if tool {"requires_action"} else {"completed"}}}));
            }
            return v
                .iter()
                .map(|e| {
                    format!(
                        "event: {}\ndata: {e}\n\n",
                        e["event_type"].as_str().unwrap()
                    )
                })
                .collect();
        }
        Protocol::Chat => {
            let delta = if tool {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"observe_page","arguments":"{}"}}]})
            } else {
                json!({"role":"assistant","content":OUTPUT})
            };
            let finish = if tool { "tool_calls" } else { "stop" };
            let mut v = vec![(
                String::new(),
                json!({"id":"chatcmpl_1","choices":[{"index":0,"delta":delta,"finish_reason":finish}]}),
            )];
            if complete {
                v.push((
                    String::new(),
                    json!({"id":"chatcmpl_1","choices":[],"usage":{"completion_tokens":2}}),
                ));
            }
            return v
                .iter()
                .map(|(_, e)| format!("data: {e}\n\n"))
                .collect::<String>()
                + if complete { "data: [DONE]\n\n" } else { "" };
        }
    };
    events
        .iter()
        .map(|(event, value)| format!("event: {event}\ndata: {value}\n\n"))
        .collect()
}

async fn call(
    protocol: Protocol,
    endpoint: &str,
    stop: &AtomicBool,
    emit: impl FnMut(metldr_conversation::TextEvent) -> crate::Result<()>,
) -> crate::Result<()> {
    match protocol {
        Protocol::Messages => {
            metldr_messages::Client::new(
                endpoint,
                MODEL,
                KEY,
                Some(include_bytes!("../../test-tls/ca.pem")),
            )?
            .text(SYSTEM, INPUT, stop, emit)
            .await
        }
        Protocol::Responses => {
            metldr_responses::Client::new(
                endpoint,
                MODEL,
                KEY,
                Some(include_bytes!("../../test-tls/ca.pem")),
            )?
            .text(SYSTEM, INPUT, stop, emit)
            .await
        }
        Protocol::Google => {
            metldr_google_interactions::Client::new(
                endpoint,
                MODEL,
                KEY,
                Some(include_bytes!("../../test-tls/ca.pem")),
            )?
            .text(SYSTEM, INPUT, stop, emit)
            .await
        }
        Protocol::Chat => {
            metldr_chat_completions::Client::new(
                endpoint,
                MODEL,
                KEY,
                Some(include_bytes!("../../test-tls/ca.pem")),
            )?
            .text(SYSTEM, INPUT, stop, emit)
            .await
        }
    }
}

fn assert_request(protocol: Protocol, request: &Value) {
    let expected = match protocol {
        Protocol::Messages => json!({"model":MODEL,"system":SYSTEM,"max_tokens":2048,"stream":true,
            "messages":[{"role":"user","content":[{"type":"text","text":INPUT}]}]}),
        Protocol::Responses => json!({"model":MODEL,"instructions":SYSTEM,
            "input":[{"role":"user","content":[{"type":"input_text","text":INPUT}]}],
            "stream":true,"store":false,"truncation":"disabled","max_output_tokens":2048}),
        Protocol::Google => json!({"model":MODEL,"system_instruction":SYSTEM,
            "input":[{"type":"user_input","content":[{"type":"text","text":INPUT}]}],
            "stream":true,"store":false,"generation_config":{"max_output_tokens":2048}}),
        Protocol::Chat => json!({"model":MODEL,
            "messages":[{"role":"system","content":SYSTEM},{"role":"user","content":INPUT}],
            "stream":true,"stream_options":{"include_usage":true},"max_tokens":4096}),
    };
    assert_eq!(request, &expected);
}

#[test]
fn all_native_text_clients_accept_only_complete_tool_free_responses_over_tls() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for protocol in [
            Protocol::Messages,
            Protocol::Responses,
            Protocol::Google,
            Protocol::Chat,
        ] {
            let (endpoint, server) = text_server(&frames(protocol, false, true), protocol);
            let mut output = String::new();
            let mut finals = Vec::new();
            call(protocol, &endpoint, &AtomicBool::new(false), |part| {
                match part {
                    metldr_conversation::TextEvent::Delta(part) => output.push_str(&part),
                    metldr_conversation::TextEvent::Final(part) => finals.push(part),
                }
                Ok(())
            })
            .await
            .unwrap();
            assert_eq!(output, OUTPUT);
            assert_eq!(finals, [OUTPUT]);
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 1);
            assert_request(protocol, &requests[0]);

            let (endpoint, server) = text_server(&frames(protocol, true, true), protocol);
            let error = call(protocol, &endpoint, &AtomicBool::new(false), |_| Ok(()))
                .await
                .unwrap_err()
                .to_string();
            assert_eq!(
                error,
                format!(
                    "{} text request returned a tool call",
                    match protocol {
                        Protocol::Messages => "Messages",
                        Protocol::Responses => "Responses",
                        Protocol::Google => "Google Interactions",
                        Protocol::Chat => "Chat",
                    }
                )
            );
            assert_eq!(server.join().unwrap().len(), 1);

            let (incomplete, empty, cancelled) = match protocol {
                Protocol::Messages => (
                    "Messages stream ended without completion",
                    "Messages stop reason and output disagree",
                    "Messages request cancelled",
                ),
                Protocol::Responses => (
                    "Responses stream ended without completion",
                    "Responses completed without a result",
                    "Responses request cancelled",
                ),
                Protocol::Google => (
                    "Google stream ended without completion",
                    "Google terminal status and output disagree",
                    "Google Interactions request cancelled",
                ),
                Protocol::Chat => (
                    "Chat stream is incomplete",
                    "Chat stopped without an answer",
                    "Chat request cancelled",
                ),
            };
            let (endpoint, server) = text_server(&frames(protocol, false, false), protocol);
            let mut partial = String::new();
            let mut finals = Vec::new();
            let error = call(protocol, &endpoint, &AtomicBool::new(false), |part| {
                match part {
                    metldr_conversation::TextEvent::Delta(part) => partial.push_str(&part),
                    metldr_conversation::TextEvent::Final(part) => finals.push(part),
                }
                Ok(())
            })
            .await
            .unwrap_err();
            assert_eq!(error.to_string(), incomplete);
            assert_eq!(partial, OUTPUT);
            // Only the Responses fixture closes its message before omitting the response terminal.
            let expected_finals = if matches!(protocol, Protocol::Responses) {
                vec![OUTPUT]
            } else {
                Vec::new()
            };
            assert_eq!(finals, expected_finals);
            assert_eq!(server.join().unwrap().len(), 1);

            let (endpoint, server) =
                text_server(&frames(protocol, false, true).replace(OUTPUT, ""), protocol);
            let error = call(protocol, &endpoint, &AtomicBool::new(false), |_| Ok(()))
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), empty);
            assert_eq!(server.join().unwrap().len(), 1);

            let oversized = frames(protocol, false, true).replace(OUTPUT, &"x".repeat(8193));
            let (endpoint, server) = text_server(&oversized, protocol);
            let mut output = String::new();
            let error = call(protocol, &endpoint, &AtomicBool::new(false), |part| {
                if let metldr_conversation::TextEvent::Delta(part) = part {
                    output.push_str(&part);
                }
                Ok(())
            })
            .await
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                match protocol {
                    Protocol::Messages => "Messages text exceeds bound",
                    Protocol::Responses => "Responses text exceeds bound",
                    Protocol::Google => "Google text exceeds bound",
                    Protocol::Chat => "Chat text exceeds bound",
                }
            );
            assert!(output.is_empty());
            assert_eq!(server.join().unwrap().len(), 1);

            let (endpoint, server) = text_server("", protocol);
            let stop = AtomicBool::new(true);
            let error = call(protocol, &endpoint, &stop, |_| Ok(()))
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), cancelled);
            assert!(server.join().unwrap().is_empty());
        }
    });
}
