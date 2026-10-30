use super::*;
use serde_json::Value;
use std::{
    io::{Cursor, Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn server(status: &str, body: &str) -> (String, thread::JoinHandle<Vec<Value>>) {
    server_key(status, body, "dummy-owned-browser-fixture")
}

pub(crate) fn server_key(
    status: &str,
    body: &str,
    key: &str,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    scripted(vec![(status.to_owned(), body.to_owned())], key, false)
}

fn scripted(
    replies: Vec<(String, String)>,
    key: &str,
    chat: bool,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    scripted_protocol(
        replies,
        key,
        if chat {
            Protocol::Chat
        } else {
            Protocol::Messages
        },
    )
}

fn scripted_responses(
    replies: Vec<(String, String)>,
    key: &str,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    scripted_protocol(replies, key, Protocol::Responses)
}

#[test]
fn generated_answers_stream_over_tls_and_close_on_callback_or_terminal_failure() {
    let partial = r#"{"surfaceId":"answer","initialHeight":null,"css":"","html":["<button>Count</button>"],"jsFunctions":"function init(){"#;
    let arguments = format!(r#"{partial}}}","jsExpressions":["init()"]}}"#);
    let initial = json!({"type":"function_call","id":"fc_answer","call_id":"call_answer",
        "name":"generateSandboxedUi","arguments":"","status":"in_progress"});
    let mut final_call = initial.clone();
    final_call["arguments"] = json!(arguments);
    final_call["status"] = json!("completed");
    let events = vec![
        json!({"type":"response.created","response":{"id":"resp_answer","object":"response","model":"model","store":false}}),
        json!({"type":"response.output_item.added","output_index":0,"item":initial}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_answer","delta":partial}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_answer","delta":&arguments[partial.len()..]}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_answer","arguments":arguments}),
        json!({"type":"response.output_item.done","output_index":0,"item":final_call}),
        json!({"type":"response.completed","response":{"id":"resp_answer","object":"response","model":"model","store":false,"status":"completed","error":null,"incomplete_details":null,"output":[final_call]}}),
    ];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (complete, reject_callback) in [(true, false), (false, false), (true, true)] {
        let count = events.len() - usize::from(!complete);
        let body: String = events[..count]
            .iter()
            .enumerate()
            .map(|(sequence, event)| {
                let mut event = event.clone();
                event["sequence_number"] = json!(sequence);
                format!(
                    "event: {}\ndata: {}\n\n",
                    event["type"].as_str().unwrap(),
                    event
                )
            })
            .collect();
        let (endpoint, server) =
            scripted_responses(vec![("200 OK".into(), body)], "dummy-owned-browser-fixture");
        let client = metldr_responses::Client::new(
            &endpoint,
            "model",
            "dummy-owned-browser-fixture",
            Some(include_bytes!("../../test-tls/ca.pem")),
        )
        .unwrap();
        let mut conversation = client
            .start_presentation("present", "show an interactive answer")
            .unwrap();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let mut updates = Vec::new();
        let result = runtime.block_on(conversation.next(
            &stop,
            |_| Err("unexpected text".into()),
            |progress| {
                assert_eq!(progress.call_id, "call_answer");
                assert_eq!(progress.surface_id, "answer");
                updates.push(serde_json::to_value(progress.content.view()).unwrap());
                if reject_callback {
                    Err("owned reader stopped".into())
                } else {
                    Ok(())
                }
            },
        ));
        assert!(!updates.is_empty());
        assert!(updates[0].get("jsFunctions").is_none());
        assert!(updates.iter().all(|source| source["generating"] == true));
        if complete && !reject_callback {
            let invocation = result.unwrap().unwrap();
            let metldr_responses::PresentationContent::Generated(source) =
                invocation.presentation.content
            else {
                panic!("expected generated source")
            };
            assert_eq!(
                serde_json::to_value(source.view()).unwrap()["generating"],
                false
            );
            assert_eq!(updates.len(), 2);
        } else {
            assert!(result.is_err());
        }
        assert!(
            runtime
                .block_on(conversation.next(&stop, |_| Ok(()), |_| Ok(())))
                .is_err()
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|tool| tool["name"] == "generateSandboxedUi")
                .count(),
            1
        );
        assert_eq!(requests[0]["store"], false);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Protocol {
    Messages,
    Responses,
    Google,
    Chat,
}

pub(super) fn text_server(
    body: &str,
    protocol: Protocol,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    scripted_protocol(
        vec![("200 OK".into(), body.into())],
        "dummy-owned-browser-fixture",
        protocol,
    )
}

fn scripted_protocol(
    replies: Vec<(String, String)>,
    key: &str,
    protocol: Protocol,
) -> (String, thread::JoinHandle<Vec<Value>>) {
    let api_key = key.to_owned();
    let certs = rustls_pemfile::certs(&mut Cursor::new(include_bytes!(
        "../../test-tls/server.pem"
    )))
    .collect::<std::result::Result<Vec<_>, _>>()
    .unwrap();
    let key = rustls_pemfile::private_key(&mut Cursor::new(include_bytes!(
        "../../test-tls/server-key.pem"
    )))
    .unwrap()
    .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let path = match protocol {
        Protocol::Messages => "/v1/messages",
        Protocol::Responses => "/v1/responses",
        Protocol::Google => "/v1/interactions",
        Protocol::Chat => "/v1/chat/completions",
    };
    let endpoint = format!("https://{}{path}", listener.local_addr().unwrap());
    let location = endpoint.clone();
    let worker = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in replies {
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nLocation: {location}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let deadline = Instant::now() + Duration::from_secs(4);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => return requests,
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut stream = rustls::StreamOwned::new(
                rustls::ServerConnection::new(Arc::new(config.clone())).unwrap(),
                stream,
            );
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if stream.read_exact(&mut byte).is_err() {
                    return requests;
                }
                headers.push(byte[0]);
                assert!(headers.len() < 8192);
            }
            let headers = String::from_utf8(headers).unwrap();
            let (expected_header, expected_key) = match protocol {
                Protocol::Messages => ("x-api-key", api_key.clone()),
                Protocol::Responses | Protocol::Chat => {
                    ("authorization", format!("Bearer {api_key}"))
                }
                Protocol::Google => ("x-goog-api-key", api_key.clone()),
            };
            assert!(headers.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case(expected_header) && value.trim() == expected_key
                })
            }));
            assert!(headers.starts_with(&format!("POST {path} HTTP/1.1\r\n")));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            assert!(length <= 131_072);
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            requests.push(serde_json::from_slice(&body).unwrap());
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.flush();
        }
        requests
    });
    (endpoint, worker)
}

#[test]
fn chat_rejects_malformed_siblings_and_changed_reused_ids_before_yielding() {
    fn response(id: &str, calls: Value) -> (String, String) {
        let chunk = json!({"id":id,"choices":[{"index":0,"delta":{
            "role":"assistant","content":null,"tool_calls":calls
        },"finish_reason":"tool_calls"}]});
        (
            "200 OK".into(),
            format!("data: {chunk}\n\ndata: [DONE]\n\n"),
        )
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for malformed in [true, false] {
            let observe = json!({"index":0,"id":"same","type":"function","function":{"name":"observe_page","arguments":"{}"}});
            let replies = if malformed {
                vec![response("chat-one", json!([observe,
                    {"index":1,"id":"bad","type":"function","function":{"name":"missing","arguments":"{}"}}
                ]))]
            } else {
                vec![response("chat-one", json!([observe])), response("chat-two", json!([
                    {"index":0,"id":"fresh","type":"function","function":{"name":"observe_page","arguments":"{}"}},
                    {"index":1,"id":"same","type":"function","function":{"name":"navigate_page","arguments":"{\"url\":\"https://example.org/other\"}"}}
                ]))]
            };
            let (endpoint, worker) = scripted(replies, "dummy-owned-browser-fixture", true);
            let client = metldr_chat_completions::Client::new(&endpoint, "model", "dummy-owned-browser-fixture",
                Some(include_bytes!("../../test-tls/ca.pem"))).unwrap();
            let mut conversation = client.start(metldr_chat_completions::Prompt {
                instruction:"inspect",title:"Title",url:"https://example.org/",text:"Text"
            }).unwrap();
            let stop = std::sync::atomic::AtomicBool::new(false);
            if !malformed {
                let first = conversation.next(&stop, 8, |_| Ok(())).await.unwrap().unwrap();
                assert_eq!(first.id, "same");
                assert_eq!(first.call, metldr_browser_tools::Call::Observe);
                conversation.submit_tool_result(&json!({"observed":"actual result"})).unwrap();
            }
            let error = conversation.next(&stop, 7, |_| Ok(())).await.err().unwrap();
            assert_eq!(error.to_string(), if malformed { "unknown browser tool" } else { "Chat reused a tool identity with changed arguments" });
            assert_eq!(conversation.next(&stop, 7, |_| Ok(())).await.err().unwrap().to_string(), "Chat conversation is closed");
            let requests = worker.join().unwrap();
            assert_eq!(requests.len(), if malformed {1} else {2});
            if !malformed {
                let messages = requests[1]["messages"].as_array().unwrap();
                assert_eq!(messages.len(), 4);
                assert_eq!(messages[2]["tool_calls"][0]["id"], "same");
                assert_eq!(messages[3], json!({"role":"tool","tool_call_id":"same","content":"{\"observed\":\"actual result\"}"}));
            }
        }
    });
}

#[test]
fn verifies_public_messages_tls_and_does_not_follow_redirects() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for (trusted,status,expected) in [(true,"200 OK",None),(false,"200 OK",Some("Messages transport failed")),(true,"302 Found",Some("Messages endpoint returned an error"))] {
            let events=[
                json!({"type":"message_start","message":{"type":"message","id":"msg_tls","model":FIXTURE_MODEL,"role":"assistant","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"TLS result"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
                json!({"type":"message_stop"}),
            ];
            let body=events.iter().map(|e|format!("event: {}\ndata: {e}\n\n",e["type"].as_str().unwrap())).collect::<String>();
            let (endpoint,worker)=server(status,&body);
            let model=metldr_messages::Client::new(&endpoint,FIXTURE_MODEL,"dummy-owned-browser-fixture",if trusted {Some(include_bytes!("../../test-tls/ca.pem").as_slice())} else {None}).unwrap();
            let mut conversation=model.start(metldr_messages::Prompt {instruction:"inspect",title:"Title",url:"https://example.org/",text:"Text"}).unwrap();
            let mut text=String::new();
            let result=conversation.next(&std::sync::atomic::AtomicBool::new(false),|chunk|{if let metldr_conversation::TextEvent::Delta(chunk)=chunk {text.push_str(&chunk)};Ok(())}).await;
            match expected {
                None=>{assert!(result.unwrap().is_none());assert_eq!(text,"TLS result");}
                Some(expected)=>{let error=result.err().unwrap().to_string();assert_eq!(error,expected);assert!(!error.contains("dummy-owned-browser-fixture"));}
            }
            worker.join().unwrap();
        }
    });
}

#[test]
fn public_responses_presentation_preserves_text_and_opaque_tool_continuation_over_tls() {
    fn sse(mut events: Vec<Value>) -> String {
        for (sequence, event) in events.iter_mut().enumerate() {
            event["sequence_number"] = json!(sequence);
        }
        events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap()
                )
            })
            .collect()
    }

    let args = json!({"surfaceId":"surface-owned","catalogId":"copilotkit://open-generative-ui-tables",
        "components":[{"id":"root","component":"Table","title":"Quarterly sales",
            "columns":["Quarter","Sales"],"rows":[["Q1","$100"],["Q2","$250"]],
            "source":"User-provided data"}]});
    let reasoning = json!({"type":"reasoning","id":"rs_owned","summary":[],
        "encrypted_content":"opaque-final-owned"});
    let before = json!({"type":"message","id":"msg_before","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":"Before the table.","annotations":[]}]});
    let call = json!({"type":"function_call","id":"fc_item_owned","call_id":"call_owned",
        "name":"render_a2ui","arguments":args.to_string(),"status":"completed"});
    let after = json!({"type":"message","id":"msg_after","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":" After the table.","annotations":[]}]});
    let mut first = vec![
        json!({"type":"response.created","response":{"id":"resp_first",
        "object":"response","model":"model","store":false,"status":"in_progress"}}),
    ];
    first.extend([
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"type":"reasoning","id":"rs_owned","summary":[],"encrypted_content":"partial"}}),
        json!({"type":"response.output_item.done","output_index":0,"item":reasoning.clone()}),
        json!({"type":"response.output_item.added","output_index":1,
            "item":{"type":"message","id":"msg_before","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.output_text.delta","output_index":1,"item_id":"msg_before","content_index":0,"delta":"Before the table."}),
        json!({"type":"response.output_text.done","output_index":1,"item_id":"msg_before","content_index":0,"text":"Before the table."}),
        json!({"type":"response.output_item.done","output_index":1,"item":before.clone()}),
        json!({"type":"response.output_item.added","output_index":2,
            "item":{"type":"function_call","id":"fc_item_owned","call_id":"call_owned","name":"render_a2ui","arguments":"","status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_item_owned","delta":args.to_string()}),
        json!({"type":"response.function_call_arguments.done","output_index":2,"item_id":"fc_item_owned","arguments":args.to_string()}),
        json!({"type":"response.output_item.done","output_index":2,"item":call.clone()}),
        json!({"type":"response.output_item.added","output_index":3,
            "item":{"type":"message","id":"msg_after","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.output_text.delta","output_index":3,"item_id":"msg_after","content_index":0,"delta":" After the table."}),
        json!({"type":"response.output_text.done","output_index":3,"item_id":"msg_after","content_index":0,"text":" After the table."}),
        json!({"type":"response.output_item.done","output_index":3,"item":after.clone()}),
    ]);
    let completed = json!({"type":"response.completed","response":{"id":"resp_first",
        "object":"response","model":"model","store":false,"status":"completed",
        "error":null,"incomplete_details":null,"output":[reasoning,before,call,after]}});
    first.push(completed);

    let final_message = json!({"type":"message","id":"msg_final","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":" Finished.","annotations":[]}]});
    let second = vec![
        json!({"type":"response.created","response":{"id":"resp_second",
        "object":"response","model":"model","store":false,"status":"in_progress"}}),
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"type":"message","id":"msg_final","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg_final","content_index":0,"delta":" Finished."}),
        json!({"type":"response.output_text.done","output_index":0,"item_id":"msg_final","content_index":0,"text":" Finished."}),
        json!({"type":"response.output_item.done","output_index":0,"item":final_message.clone()}),
        json!({"type":"response.completed","response":{"id":"resp_second","object":"response",
            "model":"model","store":false,"status":"completed","error":null,
            "incomplete_details":null,"output":[final_message]}}),
    ];
    let first_body = sse(first);
    let replies = vec![
        ("200 OK".into(), first_body.clone()),
        ("200 OK".into(), sse(second)),
    ];
    let (endpoint, worker) = scripted_responses(replies, "dummy-owned-browser-fixture");
    let client = metldr_responses::Client::new(
        &endpoint,
        "model",
        "dummy-owned-browser-fixture",
        Some(include_bytes!("../../test-tls/ca.pem")),
    )
    .unwrap();
    let mut conversation = client.start_presentation("present", "show sales").unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut rendered = String::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let invocation = conversation
            .next(
                &stop,
                |event| {
                    if let metldr_conversation::TextEvent::Delta(text) = event {
                        rendered.push_str(&text);
                    }
                    Ok(())
                },
                |_| Err("unexpected generated answer".into()),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rendered, "Before the table.");
        assert_eq!(invocation.id, "call_owned");
        assert_eq!(invocation.presentation.surface_id, "surface-owned");
        let metldr_responses::PresentationContent::Table(table) = invocation.presentation.content
        else {
            panic!("expected table")
        };
        assert_eq!(table.title(), "Quarterly sales");
        assert_eq!(table.columns(), ["Quarter", "Sales"]);
        assert_eq!(table.rows()[1], ["Q2", "$250"]);
        assert_eq!(table.source(), "User-provided data");
        assert_eq!(
            conversation
                .next(&stop, |_| Ok(()), |_| Ok(()))
                .await
                .err()
                .unwrap()
                .to_string(),
            "Responses tool result missing"
        );
        conversation
            .submit_tool_result(
                &json!({"status":"saved","artifactId":"artifact-owned"}),
                |event| {
                    if let metldr_conversation::TextEvent::Delta(text) = event {
                        rendered.push_str(&text);
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(rendered, "Before the table. After the table.");
        assert!(
            conversation
                .next(
                    &stop,
                    |event| {
                        if let metldr_conversation::TextEvent::Delta(text) = event {
                            rendered.push_str(&text);
                        }
                        Ok(())
                    },
                    |_| Err("unexpected generated answer".into())
                )
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(rendered, "Before the table. After the table. Finished.");
    });
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 2);
    let continuation = requests[1]["input"].as_array().unwrap();
    assert_eq!(continuation[1]["encrypted_content"], "opaque-final-owned");
    assert_eq!(continuation[3]["id"], "fc_item_owned");
    assert_eq!(continuation[3]["call_id"], "call_owned");
    assert_ne!(continuation[3]["id"], continuation[3]["call_id"]);
    assert_eq!(continuation[2]["content"][0]["text"], "Before the table.");
    assert_eq!(continuation[4]["content"][0]["text"], " After the table.");
    assert_eq!(continuation[5]["call_id"], "call_owned");
    assert_eq!(
        serde_json::from_str::<Value>(continuation[5]["output"].as_str().unwrap()).unwrap(),
        json!({"status":"saved","artifactId":"artifact-owned"})
    );

    let (endpoint, worker) = scripted_responses(
        vec![("200 OK".into(), first_body.clone())],
        "dummy-owned-browser-fixture",
    );
    let client = metldr_responses::Client::new(
        &endpoint,
        "model",
        "dummy-owned-browser-fixture",
        Some(include_bytes!("../../test-tls/ca.pem")),
    )
    .unwrap();
    let mut conversation = client.start_presentation("present", "show sales").unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let error = runtime
        .block_on(conversation.next(
            &stop,
            |event| {
                if matches!(event, metldr_conversation::TextEvent::Delta(_)) {
                    stop.store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(())
            },
            |_| Err("unexpected generated answer".into()),
        ))
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "Responses request cancelled");
    assert_eq!(
        runtime
            .block_on(conversation.next(
                &std::sync::atomic::AtomicBool::new(false),
                |_| Ok(()),
                |_| Ok(())
            ))
            .err()
            .unwrap()
            .to_string(),
        "Responses conversation is closed"
    );
    assert_eq!(worker.join().unwrap().len(), 1);

    let (endpoint, server) = scripted_responses(
        vec![("200 OK".into(), first_body)],
        "dummy-owned-browser-fixture",
    );
    let client = metldr_responses::Client::new(
        &endpoint,
        "model",
        "dummy-owned-browser-fixture",
        Some(include_bytes!("../../test-tls/ca.pem")),
    )
    .unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let worker = thread::spawn(move || {
        let mut output = crate::direct_output::DirectOutput::new("owned-run", &tx);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(crate::presentation::run(
                client,
                "present",
                "show sales",
                "owned-run",
                &tx,
                &worker_stop,
                &mut output,
            ))
            .map_err(|error| error.to_string())
    });
    let mut prefix = String::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            crate::flow::Event::Text {
                change: metldr_conversation::Change::Append { text, .. },
                ..
            } => prefix.push_str(&text),
            crate::flow::Event::Text {
                change: metldr_conversation::Change::Final { .. },
                ..
            } => (),
            crate::flow::Event::Presentation {
                ack,
                update: crate::presentation::Update::Complete(_),
                ..
            } => {
                stop.store(true, std::sync::atomic::Ordering::Release);
                let _ = ack.try_send(Ok(metldr_conversation::ArtifactId::new(
                    "owned-artifact".into(),
                )
                .unwrap()));
                break;
            }
            _ => panic!("unexpected presentation event"),
        }
    }
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        "presentation cancelled"
    );
    assert_eq!(prefix, "Before the table.");
    assert!(rx.try_iter().next().is_none());
    assert_eq!(server.join().unwrap().len(), 1);
}
