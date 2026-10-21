use super::*;
use tokio::io::AsyncWriteExt;

pub(super) fn accepted(header: &str) -> bool {
    header.split(',').any(|range| {
        let mut parts = range.split(';');
        if !parts
            .next()
            .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
        {
            return false;
        }
        let mut quality = None;
        for parameter in parts {
            if let Some((name, value)) = parameter.split_once('=')
                && name.trim().eq_ignore_ascii_case("q")
            {
                if quality.is_some() {
                    return false;
                }
                quality = Some(value.trim().parse::<f32>().unwrap_or(0.0));
            }
        }
        quality.is_none_or(|value| value > 0.0 && value <= 1.0)
    })
}

pub(super) fn token(params: &Value) -> Result<Option<Value>> {
    let Some(meta) = params.get("_meta") else {
        return Ok(None);
    };
    let meta = meta.as_object().ok_or("invalid MCP metadata")?;
    let Some(token) = meta.get("progressToken") else {
        return Ok(None);
    };
    validate_id(token)?;
    Ok(Some(token.clone()))
}

pub(super) fn notification(token: &Value, elapsed: Duration) -> String {
    json!({"jsonrpc":"2.0","method":"notifications/progress","params":{
        "progressToken":token,"progress":elapsed.as_secs_f64(),
        "message":"Waiting for your browser approval. Progress counts seconds spent waiting."
    }})
    .to_string()
}

#[derive(Default)]
pub(super) struct Stream {
    started: bool,
    disconnected: bool,
}

impl Stream {
    pub(super) fn started(&self) -> bool {
        self.started
    }

    pub(super) async fn send(&mut self, stream: &mut TcpStream, body: &str) {
        if self.disconnected {
            return;
        }
        let mut frame = String::new();
        if !self.started {
            frame.push_str("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
            self.started = true;
        }
        frame.push_str("event: message\ndata: ");
        frame.push_str(body);
        frame.push_str("\n\n");
        self.disconnected = !matches!(
            timeout(Duration::from_secs(5), stream.write_all(frame.as_bytes())).await,
            Ok(Ok(()))
        );
        // The negotiated MCP version requires explicit cancellation after disconnect.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn progress_stream_keeps_the_requested_token_and_final_response() {
        assert!(accepted("application/json, text/event-stream;q=1"));
        assert!(accepted("text/event-stream"));
        assert!(!accepted("application/json, text/event-stream;q=0"));
        assert!(!accepted("text/event-stream;q=invalid"));
        assert!(token(&json!({"_meta":{"progressToken":{}}})).is_err());
        assert!(token(&json!({"_meta":{"progressToken":1.5}})).is_err());
        assert!(token(&json!({})).unwrap().is_none());
        let token = token(&json!({"_meta":{"progressToken":"review-7"}}))
            .unwrap()
            .unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(headers.contains("Content-Type: text/event-stream"));
            let messages: Vec<Value> = body
                .split("\n\n")
                .filter(|frame| !frame.is_empty())
                .map(|frame| {
                    serde_json::from_str(frame.strip_prefix("event: message\ndata: ").unwrap())
                        .unwrap()
                })
                .collect();
            assert_eq!(messages.len(), 3);
            assert_eq!(messages[0]["params"]["progressToken"], token);
            assert_eq!(messages[1]["params"]["progressToken"], token);
            assert!(
                messages[1]["params"]["progress"].as_f64().unwrap()
                    > messages[0]["params"]["progress"].as_f64().unwrap()
            );
            assert!(messages[0]["params"].get("total").is_none());
            assert_eq!(messages[2]["id"], 7);
            assert_eq!(messages[2]["result"], json!({"content":[]}));
        };
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut events = Stream::default();
            events
                .send(&mut socket, &notification(&token, Duration::from_secs(5)))
                .await;
            events
                .send(&mut socket, &notification(&token, Duration::from_secs(10)))
                .await;
            events
                .send(
                    &mut socket,
                    &json!({"jsonrpc":"2.0","id":7,"result":{"content":[]}}).to_string(),
                )
                .await;
        };
        timeout(Duration::from_secs(2), async {
            tokio::join!(client, server);
        })
        .await
        .unwrap();
    }
}
