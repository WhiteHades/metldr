use crate::flow::Event;
use crate::{Result, TEXT_MAX};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};

pub(super) fn provider(
    address: SocketAddr,
    body: &Value,
    run: &str,
    stop: &AtomicBool,
    tx: &SyncSender<Event>,
) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Err("provider cancelled before connection".into());
    }
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    socket.set_write_timeout(Some(Duration::from_millis(500)))?;
    let encoded = serde_json::to_vec(body)?;
    if encoded.len() > 32_768 {
        return Err("provider request exceeds bound".into());
    }
    if stop.load(Ordering::Acquire) {
        return Err("provider cancelled before request".into());
    }
    write!(
        socket,
        "POST /flow HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        encoded.len()
    )?;
    socket.write_all(&encoded)?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut pending = Vec::new();
    let mut headers = false;
    let mut done = false;
    let mut total = 0usize;
    let mut output_bytes = 0usize;
    let mut buffer = [0; 1024];
    loop {
        if stop.load(Ordering::Acquire) {
            socket.shutdown(Shutdown::Both)?;
            return Err("local provider socket cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("provider timeout".into());
        }
        match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total > 16_384 {
                    return Err("provider stream exceeds bound".into());
                }
                pending.extend_from_slice(&buffer[..n]);
                if !headers {
                    if let Some(end) = pending.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = std::str::from_utf8(&pending[..end])?;
                        let mut lines = header.split("\r\n");
                        if lines.next() != Some("HTTP/1.1 200 OK") {
                            return Err("provider rejected context".into());
                        }
                        let mut content_type = false;
                        for line in lines {
                            let (name, value) =
                                line.split_once(':').ok_or("invalid provider header")?;
                            if name.eq_ignore_ascii_case("transfer-encoding") {
                                return Err("chunked fixture response unsupported".into());
                            }
                            if name.eq_ignore_ascii_case("content-type")
                                && value.trim() == "application/x-ndjson"
                            {
                                content_type = true;
                            }
                        }
                        if !content_type {
                            return Err("unexpected provider content type".into());
                        }
                        pending.drain(..end + 4);
                        headers = true;
                    } else if pending.len() > 4096 {
                        return Err("provider headers exceed bound".into());
                    }
                }
                if headers {
                    while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                        let line: Vec<_> = pending.drain(..=end).collect();
                        if done {
                            return Err("provider data after terminal event".into());
                        }
                        let value: Value = serde_json::from_slice(&line)?;
                        let object = value.as_object().ok_or("invalid provider event")?;
                        if object.len() == 1 && value.get("done") == Some(&Value::Bool(true)) {
                            done = true;
                        } else if object.len() == 1 {
                            let text = value
                                .get("delta")
                                .and_then(Value::as_str)
                                .ok_or("invalid provider delta")?;
                            output_bytes = output_bytes
                                .checked_add(text.len())
                                .ok_or("provider output exceeds bound")?;
                            if output_bytes > TEXT_MAX {
                                return Err("provider output exceeds bound".into());
                            }
                            tx.send(Event::Text {
                                run: run.into(),
                                change: metldr_conversation::Change::FlatAppend(text.into()),
                            })?;
                        } else {
                            return Err("unexpected provider event".into());
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                ()
            }
            Err(e) => return Err(e.into()),
        }
    }
    if !headers || !done || !pending.is_empty() {
        return Err("incomplete provider stream".into());
    }
    Ok(())
}
