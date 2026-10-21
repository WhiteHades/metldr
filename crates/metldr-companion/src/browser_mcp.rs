use crate::{
    Result,
    browser_owner::Budget,
    browser_worker::{Call, Tools, decode_call, definitions},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::{interval, timeout},
};

const REQUEST_MAX: usize = 32 * 1024;
const HEADERS_MAX: usize = 16 * 1024;
const QUEUE_MAX: usize = 8;
const CONNECTION_MAX: usize = 8;
const TOOL_DEADLINE: Duration = Duration::from_secs(30);

mod progress;

pub(super) struct Server {
    listener: TcpListener,
    run: String,
    capability: String,
    address: SocketAddr,
    ready: Arc<AtomicBool>,
}

impl Server {
    pub(super) async fn bind(run: &str) -> Result<Self> {
        if !crate::model::valid_request_id(run) {
            return Err("invalid browser run id".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let mut secret = [0; 32];
        getrandom::fill(&mut secret)?;
        Ok(Self {
            listener,
            run: run.to_owned(),
            capability: hex(&secret),
            address,
            ready: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(super) fn endpoint(&self) -> (SocketAddr, String) {
        (self.address, self.capability.clone())
    }

    pub(super) fn ready(&self) -> Arc<AtomicBool> {
        self.ready.clone()
    }

    pub(super) async fn serve(
        self,
        mut tools: Tools,
        stop: Arc<AtomicBool>,
        done: Arc<AtomicBool>,
        failed: Arc<AtomicBool>,
    ) -> (Result<()>, bool) {
        let (tx, mut rx) = mpsc::channel::<Inbound>(QUEUE_MAX);
        let active = Arc::new(Calls {
            stop: stop.clone(),
            failed: failed.clone(),
            done: done.clone(),
            ready: self.ready.clone(),
            ..Calls::default()
        });
        let mut connections = JoinSet::new();
        let mut pending: VecDeque<ToolRequest> = VecDeque::new();
        let mut held = Vec::new();
        let mut accepting = true;
        let mut fatal_cancel = false;
        let mut dispatch_error = None;
        let mut poll = interval(Duration::from_millis(20));

        let serve_result: Result<()> = async {
        while accepting || !done.load(Ordering::Acquire) || !pending.is_empty() || !connections.is_empty() || !held.is_empty() {
            fatal_cancel |= active.cancelled.load(Ordering::Acquire)
                && !stop.load(Ordering::Acquire) && !done.load(Ordering::Acquire);
            if failed.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) && !done.load(Ordering::Acquire) {
                dispatch_error.get_or_insert_with(|| "Browser service failed; task halted".into());
            }
            if !stop.load(Ordering::Acquire) && !failed.load(Ordering::Acquire) && !done.load(Ordering::Acquire) && !fatal_cancel && dispatch_error.is_none() {
                if let Some(request) = pending.pop_front() {
                    if request.budget.expired() {
                        answer_tool(request, Err("browser tool deadline expired".into()));
                        continue;
                    }
                    let local_cancel = request.cancel.clone();
                    let mut call_future = Box::pin(tools.dispatch(
                        &request.identity,
                        request.call.clone(),
                        &local_cancel,
                        Some(&request.budget),
                    ));
                    let mut expired = false;
                    let result = loop {
                        tokio::select! {
                            result = &mut call_future => break result,
                            message = rx.recv() => match message {
                                Some(Inbound::Call(call)) if stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire) || done.load(Ordering::Acquire) => held.push(call),
                                Some(Inbound::Call(call)) if pending.len() < QUEUE_MAX => pending.push_back(call),
                                Some(Inbound::Call(call)) => answer_tool(call, Err("browser tool queue is full".into())),
                                None => { accepting = false; break Err("MCP request queue closed".into()); }
                            },
                            _ = poll.tick() => {
                                if active.cancelled.load(Ordering::Acquire)
                                    && !stop.load(Ordering::Acquire) && !done.load(Ordering::Acquire) {
                                    fatal_cancel = true;
                                    failed.store(true, Ordering::Release);
                                    local_cancel.store(true, Ordering::Release);
                                }
                                if stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire) || done.load(Ordering::Acquire) || fatal_cancel {
                                    local_cancel.store(true, Ordering::Release);
                                    accepting = false;
                                }
                                if request.budget.expired() {
                                    local_cancel.store(true, Ordering::Release);
                                    expired = true;
                                }
                            },
                            joined = connections.join_next(), if !connections.is_empty() => {
                                if let Some(Err(error)) = joined { dispatch_error.get_or_insert_with(|| error.to_string()); failed.store(true, Ordering::Release); }
                            }
                            accepted = self.listener.accept(), if accepting && connections.len() < CONNECTION_MAX => {
                                match accepted {
                                    Ok((stream, _)) => spawn_connection(&mut connections, stream, self.address, self.capability.clone(), self.run.clone(), tx.clone(), active.clone()),
                                    Err(error) => { dispatch_error.get_or_insert_with(|| error.to_string()); accepting = false; failed.store(true, Ordering::Release); }
                                }
                            }
                        }
                    };
                    drop(call_future);
                    if (expired || result.is_err()) && !stop.load(Ordering::Acquire) && !done.load(Ordering::Acquire) {
                        failed.store(true, Ordering::Release);
                        dispatch_error.get_or_insert_with(|| "Browser execution failed; task halted".into());
                        accepting = false;
                    }
                    if stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire) || done.load(Ordering::Acquire) {
                        held.push(request);
                    } else if expired {
                        answer_tool(request, Err("browser tool deadline expired".into()));
                    } else {
                        answer_tool(request, result);
                    }
                    continue;
                }
            }

            if stop.load(Ordering::Acquire)
                || failed.load(Ordering::Acquire)
                || done.load(Ordering::Acquire)
                || fatal_cancel
                || dispatch_error.is_some()
            {
                if fatal_cancel || dispatch_error.is_some() {
                    failed.store(true, Ordering::Release);
                }
                accepting = false;
                held.extend(pending.drain(..));
                for flag in active.pending.lock()
                    .map_err(|_| "MCP cancellation state poisoned")?
                    .values()
                {
                    flag.store(true, Ordering::Release);
                }
                if done.load(Ordering::Acquire) {
                    break;
                }
                tokio::select! {
                    _ = connections.join_next(), if !connections.is_empty() => {},
                    message = rx.recv() => if let Some(Inbound::Call(call)) = message { held.push(call); },
                    _ = poll.tick() => {}
                }
                continue;
            }

            tokio::select! {
                accepted = self.listener.accept(), if accepting && connections.len() < CONNECTION_MAX => {
                    match accepted {
                        Ok((stream, _)) => spawn_connection(&mut connections, stream, self.address, self.capability.clone(), self.run.clone(), tx.clone(), active.clone()),
                        Err(error) => { dispatch_error = Some(error.to_string()); accepting = false; failed.store(true, Ordering::Release); }
                    }
                }
                message = rx.recv() => match message {
                    Some(Inbound::Call(call)) if stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire) || done.load(Ordering::Acquire) => held.push(call),
                                Some(Inbound::Call(call)) if pending.len() < QUEUE_MAX => pending.push_back(call),
                    Some(Inbound::Call(call)) => answer_tool(call, Err("browser tool queue is full".into())),
                    None => accepting = false,
                },
                joined = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = joined { dispatch_error.get_or_insert_with(|| error.to_string()); failed.store(true, Ordering::Release); }
                }
                _ = poll.tick() => {
                    if stop.load(Ordering::Acquire) || failed.load(Ordering::Acquire) || done.load(Ordering::Acquire) { accepting = false; }
                }
            }
        }
        if fatal_cancel {
            return Err("Agent cancelled an active browser tool call".into());
        }
        if let Some(error) = dispatch_error {
            return Err(error.into());
        }
        Ok(())
        }.await;

        if serve_result.is_err() {
            failed.store(true, Ordering::Release);
            for request in pending.drain(..) {
                answer_tool(request, Err("browser tool server failed".into()));
            }
            if let Ok(flags) = active.pending.lock() {
                for flag in flags.values() {
                    flag.store(true, Ordering::Release);
                }
            }
        }
        connections.abort_all();
        while connections.join_next().await.is_some() {}
        let close = tools.close().await;
        let released = close.is_ok();
        let result = match (serve_result, close) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => {
                failed.store(true, Ordering::Release);
                Err(error)
            }
            (Ok(()), Ok(())) if fatal_cancel => {
                Err("Agent cancelled an active browser tool call".into())
            }
            (Ok(()), Ok(())) => Ok(()),
        };
        (result, released)
    }
}

struct ToolRequest {
    identity: String,
    call: Call,
    budget: Budget,
    cancel: Arc<AtomicBool>,
    reply: oneshot::Sender<std::result::Result<Value, String>>,
}

enum Inbound {
    Call(ToolRequest),
}

#[derive(Default)]
struct Calls {
    pending: Mutex<HashMap<String, Arc<AtomicBool>>>,
    cancelled: AtomicBool,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
}

impl Calls {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
            || self.failed.load(Ordering::Acquire)
            || self.cancelled.load(Ordering::Acquire)
    }

    async fn fence_response(&self) -> bool {
        if !self.stopping() {
            return false;
        }
        while !self.done.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        true
    }
}

fn answer_tool(request: ToolRequest, result: Result<Value>) {
    let _ = request
        .reply
        .send(result.map_err(|error| error.to_string()));
}

fn spawn_connection(
    tasks: &mut JoinSet<()>,
    stream: TcpStream,
    address: SocketAddr,
    capability: String,
    run: String,
    tx: mpsc::Sender<Inbound>,
    active: Arc<Calls>,
) {
    tasks.spawn(async move {
        let _ = handle_connection(stream, address, &capability, &run, tx, active).await;
    });
}

async fn handle_connection(
    mut stream: TcpStream,
    address: SocketAddr,
    capability: &str,
    run: &str,
    tx: mpsc::Sender<Inbound>,
    active: Arc<Calls>,
) -> Result<()> {
    let mut parsed = timeout(Duration::from_secs(5), read_request(&mut stream)).await??;
    if parsed.headers.get("host").map(String::as_str) != Some(address.to_string().as_str())
        || parsed.headers.contains_key("origin")
        || parsed.headers.get("authorization").is_none_or(|header| {
            !constant_time_eq(header.as_bytes(), format!("Bearer {capability}").as_bytes())
        })
    {
        return write_http(&mut stream, 403, "").await;
    }
    if parsed.method != "POST" {
        return write_http(&mut stream, 405, "").await;
    }
    if parsed.headers.get("content-type").is_none_or(|value| {
        !value
            .split(';')
            .next()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
    }) {
        return write_http(&mut stream, 400, "").await;
    }
    if parsed.path != "/mcp" {
        return write_http(&mut stream, 404, "").await;
    }
    if parsed.headers.contains_key("transfer-encoding") {
        return write_http(&mut stream, 400, "").await;
    }
    let Some(length) = parsed.content_length else {
        return write_http(&mut stream, 400, "").await;
    };
    if length > REQUEST_MAX {
        return write_http(&mut stream, 413, "").await;
    }
    parsed.body = read_body(&mut stream, length).await?;
    let message: Value = match serde_json::from_slice(&parsed.body) {
        Ok(value) => value,
        Err(_) => return write_http(&mut stream, 400, "").await,
    };
    let initialize = message.get("method").and_then(Value::as_str) == Some("initialize");
    if !initialize
        && parsed
            .headers
            .get("mcp-protocol-version")
            .map(String::as_str)
            != Some("2025-11-25")
    {
        return write_http(&mut stream, 400, "").await;
    }
    let tool_call = message["method"] == "tools/call";
    let discovery = message["method"] == "tools/list";
    let streaming = tool_call
        && parsed
            .headers
            .get("accept")
            .is_some_and(|value| progress::accepted(value));
    let (updates, mut receiver) = mpsc::channel(1);
    let mut result_future = Box::pin(process_message(
        message,
        run,
        tx,
        active.clone(),
        streaming.then_some(updates),
    ));
    let mut events = progress::Stream::default();
    let result = loop {
        tokio::select! {
            biased;
            result = &mut result_future => break result,
            Some(update) = receiver.recv(), if streaming => {
                if !active.stopping() && !active.done.load(Ordering::Acquire) {
                    events.send(&mut stream, &update).await;
                }
            }
        }
    };
    if tool_call && active.fence_response().await {
        return Ok(());
    }
    if events.started() {
        let body = match result {
            Ok(Some(body)) => body,
            Ok(None) => return Err("streaming tool response absent".into()),
            Err(error) => json_error(error),
        };
        events.send(&mut stream, &body).await;
        return Ok(());
    }
    let response = match result {
        Ok(Some(body)) => {
            let sent = write_http(&mut stream, 200, &body).await;
            if discovery && sent.is_ok() {
                active.ready.store(true, Ordering::Release);
            }
            sent
        }
        Ok(None) => write_http(&mut stream, 202, "").await,
        Err(error) => write_http(&mut stream, 400, &json_error(error)).await,
    };
    response
}

async fn process_message(
    value: Value,
    run: &str,
    tx: mpsc::Sender<Inbound>,
    active: Arc<Calls>,
    updates: Option<mpsc::Sender<String>>,
) -> Result<Option<String>> {
    let object = value.as_object().ok_or("invalid JSON-RPC message")?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("invalid JSON-RPC version".into());
    }
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .filter(|m| m.len() <= 128)
        .ok_or("invalid MCP method")?;
    let id = object.get("id").cloned();
    if let Some(id) = &id {
        validate_id(id)?;
    }
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    let Some(id) = id else {
        match method {
            "notifications/initialized" => return Ok(None),
            "notifications/cancelled" => {
                let request = params
                    .get("requestId")
                    .ok_or("missing cancelled request id")?;
                validate_id(request)?;
                let identity = request_identity(run, request);
                let guard = active
                    .pending
                    .lock()
                    .map_err(|_| "MCP cancellation state poisoned")?;
                if let Some(flag) = guard.get(&identity) {
                    flag.store(true, Ordering::Release);
                    active.cancelled.store(true, Ordering::Release);
                }
                return Ok(None);
            }
            _ => return Err("unsupported MCP notification".into()),
        }
    };
    let result = match method {
        "initialize" => {
            if params.get("protocolVersion").and_then(Value::as_str) != Some("2025-11-25") {
                return Ok(Some(rpc_error(
                    id,
                    -32602,
                    "unsupported MCP protocol version",
                )));
            }
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"metldr-browser","version":"0.1.0"}})
        }
        "tools/list" => {
            json!({"tools":definitions().into_iter().map(|definition| json!({"name":definition.name,"description":definition.description,"inputSchema":definition.input_schema})).collect::<Vec<_>>() })
        }
        "tools/call" => {
            let token = progress::token(&params)?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or("missing MCP tool name")?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if serde_json::to_vec(&arguments)?.len() > 8 * 1024 {
                return Ok(Some(rpc_error(
                    id,
                    -32602,
                    "tool arguments exceed size limit",
                )));
            }
            let call = match decode_call(name, arguments) {
                Ok(call) => call,
                Err(error) => return Ok(Some(tool_error(id, error.to_string()))),
            };
            let identity = request_identity(run, &id);
            let (reply, response) = oneshot::channel();
            let call_cancel = Arc::new(AtomicBool::new(false));
            let budget = Budget::new(TOOL_DEADLINE);
            {
                let mut active = active
                    .pending
                    .lock()
                    .map_err(|_| "MCP cancellation state poisoned")?;
                if active.contains_key(&identity) {
                    return Ok(Some(tool_error(
                        id,
                        "browser tool request id is already active".into(),
                    )));
                }
                active.insert(identity.clone(), call_cancel.clone());
            }
            let request = ToolRequest {
                identity: identity.clone(),
                call,
                budget: budget.clone(),
                cancel: call_cancel.clone(),
                reply,
            };
            if tx.try_send(Inbound::Call(request)).is_err() {
                active
                    .pending
                    .lock()
                    .map_err(|_| "MCP cancellation state poisoned")?
                    .remove(&identity);
                return Ok(Some(tool_error(id, "MCP browser queue is full".into())));
            }
            let mut response = response;
            let mut poll = interval(Duration::from_millis(20));
            let mut last_progress: Option<Duration> = None;
            let response = loop {
                tokio::select! {
                    biased;
                    result = &mut response => break Some(result),
                    _ = poll.tick() => {
                        if budget.expired() { break None; }
                        if let (Some(elapsed), Some(token), Some(updates)) = (budget.review_elapsed(), token.as_ref(), updates.as_ref())
                            && !active.stopping() && !active.done.load(Ordering::Acquire)
                            && last_progress.is_none_or(|last| elapsed >= last + Duration::from_secs(5))
                            && updates.try_send(progress::notification(token, elapsed)).is_ok() {
                            last_progress = Some(elapsed);
                        }
                    }
                }
            };
            active
                .pending
                .lock()
                .map_err(|_| "MCP cancellation state poisoned")?
                .remove(&identity);
            match response {
                Some(Ok(Ok(value))) => {
                    json!({"content":[{"type":"text","text":serde_json::to_string(&value)?}]})
                }
                Some(Ok(Err(error))) => return Ok(Some(tool_error(id, error))),
                _ => {
                    call_cancel.store(true, Ordering::Release);
                    active.failed.store(true, Ordering::Release);
                    return Ok(Some(tool_error(id, "browser tool deadline expired".into())));
                }
            }
        }
        _ => return Ok(Some(rpc_error(id, -32601, "method not found"))),
    };
    Ok(Some(
        json!({"jsonrpc":"2.0","id":id,"result":result}).to_string(),
    ))
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    content_length: Option<usize>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    use tokio::io::AsyncReadExt;
    let mut head = Vec::with_capacity(1024);
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() == HEADERS_MAX {
            return Err("MCP headers exceed size limit".into());
        }
        let mut byte = [0];
        stream.read_exact(&mut byte).await?;
        head.push(byte[0]);
    }
    let text = std::str::from_utf8(&head)?;
    let mut lines = text[..text.len() - 4].split("\r\n");
    let mut first = lines
        .next()
        .ok_or("missing HTTP request line")?
        .split_ascii_whitespace();
    let method = first.next().ok_or("missing HTTP method")?.to_owned();
    let path = first.next().ok_or("missing HTTP path")?.to_owned();
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("invalid HTTP request line".into());
    }
    let mut headers = HashMap::new();
    for line in lines {
        let (key, value) = line.split_once(':').ok_or("invalid HTTP header")?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return Err("invalid HTTP header name".into());
        }
        if value.bytes().any(|b| b.is_ascii_control() && b != b'\t') {
            return Err("invalid HTTP header value".into());
        }
        let value = value.trim();
        if headers
            .insert(key.to_ascii_lowercase(), value.to_owned())
            .is_some()
        {
            return Err("duplicate HTTP header".into());
        }
    }
    let content_length = headers.get("content-length").and_then(|s| s.parse().ok());
    Ok(Request {
        method,
        path,
        headers,
        content_length,
        body: Vec::new(),
    })
}

async fn read_body(stream: &mut TcpStream, length: usize) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut body = vec![0; length];
    timeout(Duration::from_secs(5), stream.read_exact(&mut body)).await??;
    Ok(body)
}

async fn write_http(stream: &mut TcpStream, status: u16, body: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        _ => "Service Unavailable",
    };
    let content_type = if body.is_empty() {
        ""
    } else {
        "Content-Type: application/json\r\n"
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n{content_type}\r\n{body}",
        body.len()
    );
    timeout(
        Duration::from_secs(5),
        stream.write_all(response.as_bytes()),
    )
    .await??;
    Ok(())
}

fn rpc_error(id: Value, code: i32, message: &str) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}).to_string()
}
fn tool_error(id: Value, message: String) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":message}],"isError":true}}).to_string()
}
fn json_error(error: impl ToString) -> String {
    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":error.to_string()}})
        .to_string()
}

fn request_identity(run: &str, id: &Value) -> String {
    let mut hash = Sha256::new();
    hash.update(run.as_bytes());
    hash.update([0]);
    hash.update(if id.is_string() { b"s" } else { b"n" });
    hash.update(id.to_string().as_bytes());
    format!("mcp-{}", hex(&hash.finalize()))
}

fn validate_id(id: &Value) -> Result<()> {
    if !(id.is_string()
        && id
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
        || id.as_number().is_some_and(|n| n.is_i64() || n.is_u64()))
    {
        return Err("invalid JSON-RPC request id".into());
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for (index, expected) in right.iter().enumerate() {
        difference |= (left.get(index).copied().unwrap_or(0) ^ expected) as usize;
    }
    difference == 0
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 15) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn native_http_authenticates_methods_and_negotiates_tools() {
        let server = Server::bind("0123456789abcdef0123456789abcdef:2")
            .await
            .unwrap();
        let (address, capability) = server.endpoint();
        assert_eq!(address, server.address);
        assert_eq!(capability, server.capability);
        assert!(!server.ready().load(Ordering::Acquire));
        let (tx, _rx) = mpsc::channel(8);
        let active = Arc::new(Calls {
            ready: server.ready(),
            ..Calls::default()
        });
        for (method, authorized, origin, version, body, status) in [
            ("GET", false, false, false, "", 403),
            ("GET", true, false, false, "", 405),
            ("DELETE", true, false, false, "", 405),
            (
                "POST",
                true,
                true,
                true,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                403,
            ),
            (
                "POST",
                true,
                false,
                false,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                400,
            ),
            (
                "POST",
                true,
                false,
                false,
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
                200,
            ),
            (
                "POST",
                true,
                false,
                true,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                200,
            ),
        ] {
            let mut request = format!("{method} /mcp HTTP/1.1\r\nHost: {}\r\n", server.address);
            if authorized {
                request.push_str(&format!("Authorization: Bearer {}\r\n", server.capability));
            }
            if origin {
                request.push_str("Origin: https://untrusted.test\r\n");
            }
            if version {
                request.push_str("Mcp-Protocol-Version: 2025-11-25\r\n");
            }
            if method == "POST" {
                request.push_str(&format!(
                    "Content-Type: application/json\r\nContent-Length: {}\r\n",
                    body.len()
                ));
            }
            request.push_str("\r\n");
            request.push_str(body);
            let client = async {
                let mut socket = TcpStream::connect(server.address).await.unwrap();
                socket.write_all(request.as_bytes()).await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(socket.read_u8().await.unwrap());
                    assert!(head.len() <= HEADERS_MAX);
                }
                let mut reply = String::from_utf8(head).unwrap();
                let length = reply
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .trim()
                    .parse::<usize>()
                    .unwrap();
                let mut body_bytes = vec![0; length];
                socket.read_exact(&mut body_bytes).await.unwrap();
                reply.push_str(std::str::from_utf8(&body_bytes).unwrap());
                assert!(
                    reply.starts_with(&format!("HTTP/1.1 {status} ")),
                    "unexpected HTTP status"
                );
                if body.contains("tools/list") && status == 200 {
                    let value: Value =
                        serde_json::from_str(reply.split_once("\r\n\r\n").unwrap().1).unwrap();
                    assert_eq!(
                        value["result"]["tools"].as_array().unwrap().len(),
                        definitions().len()
                    );
                    assert_eq!(value["result"]["tools"][0]["name"], definitions()[0].name);
                }
            };
            let service = async {
                let (socket, _) = server.listener.accept().await.unwrap();
                handle_connection(
                    socket,
                    server.address,
                    &server.capability,
                    &server.run,
                    tx.clone(),
                    active.clone(),
                )
                .await
                .unwrap();
            };
            timeout(Duration::from_secs(2), async {
                tokio::join!(client, service);
            })
            .await
            .unwrap();
            assert_eq!(
                server.ready().load(Ordering::Acquire),
                body.contains("tools/list") && status == 200
            );
        }
    }
    #[tokio::test]
    async fn explicit_cancellation_remains_latched_after_reply_removes_active_call() {
        let active = Arc::new(Calls::default());
        let identity = request_identity("owned:2", &json!(7));
        let flag = Arc::new(AtomicBool::new(false));
        active
            .pending
            .lock()
            .unwrap()
            .insert(identity.clone(), flag.clone());
        let (tx, _rx) = mpsc::channel(1);
        process_message(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}),
            "owned:2",
            tx,
            active.clone(),
            None,
        )
        .await
        .unwrap();
        active.pending.lock().unwrap().remove(&identity);
        assert!(flag.load(Ordering::Acquire));
        assert!(active.cancelled.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn failed_browser_execution_halts_the_native_route_after_confirmed_detach() {
        use crate::{
            browser_io::{Client, Operation},
            flow::Event,
            model::Page,
        };
        let server = Server::bind("0123456789abcdef0123456789abcdef:2")
            .await
            .unwrap();
        let address = server.address;
        let capability = server.capability.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let owner = std::thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let Event::BrowserIo(call) = event else {
                    panic!("unexpected owner event")
                };
                match call.operation {
                    Operation::Attach { .. } => {
                        call.ack
                            .send(Err("owned attachment refusal".into()))
                            .unwrap();
                    }
                    Operation::Detach { .. } => {
                        call.ack.send(Ok(json!({"released":true}))).unwrap();
                        return;
                    }
                    _ => panic!("unexpected browser operation"),
                }
            }
            panic!("browser release was not requested");
        });
        let (_notice, events) = mpsc::channel(1);
        let tools = Tools::new(
            Client {
                run: server.run.clone(),
                tx,
            },
            crate::browser_action::Target::from(&Page {
                tab: 7,
                document: "owned-document".into(),
                url: "http://127.0.0.1:1/article".into(),
                title: "Owned page".into(),
                text: "Owned source".into(),
            }),
            events,
            Arc::new(AtomicBool::new(false)),
        );
        let done = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let client = async {
            let mut socket = TcpStream::connect(address).await.unwrap();
            let body=json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"observe_page","arguments":{}}}).to_string();
            let request = format!(
                "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {capability}\r\nContent-Type: application/json\r\nMcp-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            tokio::select! {
                _ = socket.read_u8() => panic!("tool result escaped before native cancellation settled"),
                _ = async { while !failed.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(5)).await; } } => (),
            }
            done.store(true, Ordering::Release);
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            assert!(response.is_empty());
        };
        let service = server.serve(
            tools,
            Arc::new(AtomicBool::new(false)),
            done.clone(),
            failed.clone(),
        );
        let (_, (result, released)) = timeout(Duration::from_secs(3), async {
            tokio::join!(client, service)
        })
        .await
        .unwrap();
        assert!(result.is_err());
        assert!(released);
        assert!(failed.load(Ordering::Acquire));
        owner.join().unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn shutdown_fence_outlives_http_deadlines_until_native_completion() {
        let calls = Calls::default();
        calls.stop.store(true, Ordering::Release);
        let fence = calls.fence_response();
        tokio::pin!(fence);
        assert!(timeout(Duration::from_secs(40), &mut fence).await.is_err());
        calls.done.store(true, Ordering::Release);
        assert!(
            timeout(Duration::from_millis(50), &mut fence)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn externally_failed_service_waits_for_native_completion_without_browser_dispatch() {
        let server = Server::bind("0123456789abcdef0123456789abcdef:2")
            .await
            .unwrap();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let (_notice, events) = mpsc::channel(1);
        let tools = Tools::new(
            crate::browser_io::Client {
                run: server.run.clone(),
                tx,
            },
            crate::browser_action::Target::from(&crate::model::Page {
                tab: 7,
                document: "owned-document".into(),
                url: "http://127.0.0.1:1/article".into(),
                title: "Owned page".into(),
                text: "Owned source".into(),
            }),
            events,
            Arc::new(AtomicBool::new(false)),
        );
        let done = Arc::new(AtomicBool::new(false));
        let service = server.serve(
            tools,
            Arc::new(AtomicBool::new(false)),
            done.clone(),
            Arc::new(AtomicBool::new(true)),
        );
        tokio::pin!(service);
        assert!(
            timeout(Duration::from_millis(40), &mut service)
                .await
                .is_err()
        );
        assert!(matches!(
            rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        done.store(true, Ordering::Release);
        let (result, released) = timeout(Duration::from_secs(1), service).await.unwrap();
        assert!(result.unwrap_err().to_string().contains("service failed"));
        assert!(released);
    }
}
