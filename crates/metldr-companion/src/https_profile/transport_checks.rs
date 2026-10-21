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
            let result=conversation.next(&std::sync::atomic::AtomicBool::new(false),|chunk|{text.push_str(&chunk);Ok(())}).await;
            match expected {
                None=>{assert!(result.unwrap().is_none());assert_eq!(text,"TLS result");}
                Some(expected)=>{let error=result.err().unwrap().to_string();assert_eq!(error,expected);assert!(!error.contains("dummy-owned-browser-fixture"));}
            }
            worker.join().unwrap();
        }
    });
}
