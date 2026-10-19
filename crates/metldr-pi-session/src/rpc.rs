use crate::Result;
use serde_json::Value;
use std::{
    io::{self, BufRead, BufReader, Read},
    sync::mpsc::Sender,
};

const MAX_LINE: usize = 64 * 1024;
const MAX_TOTAL: usize = 1024 * 1024;
const MAX_RECORDS: usize = 2048;

pub(crate) fn read_stdout(reader: impl Read, tx: Sender<Result<Value>>) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut total = 0;
    let mut records = 0;
    loop {
        line.clear();
        let mut ended = false;
        while !ended {
            let available = match reader.fill_buf() {
                Ok(buf) => buf,
                Err(error) => {
                    let _ = tx.send(Err(error.into()));
                    return;
                }
            };
            if available.is_empty() {
                if line.is_empty() {
                    return;
                }
                let _ = tx.send(Err(invalid("Pi RPC output ended inside a frame")));
                return;
            }
            let take = available
                .iter()
                .position(|&b| b == b'\n')
                .map_or(available.len(), |i| i + 1);
            if line.len() + take > MAX_LINE || total + take > MAX_TOTAL {
                let _ = tx.send(Err(invalid("Pi RPC output exceeded its size limit")));
                return;
            }
            ended = available[take - 1] == b'\n';
            line.extend_from_slice(&available[..take]);
            reader.consume(take);
        }
        total += line.len();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        records += 1;
        if records > MAX_RECORDS {
            let _ = tx.send(Err(invalid("Pi RPC output exceeded its record limit")));
            return;
        }
        let parsed = std::str::from_utf8(&line)
            .map_err(|e| invalid(e.to_string()))
            .and_then(|s| serde_json::from_str::<Value>(s).map_err(|e| invalid(e.to_string())))
            .and_then(|v| {
                if v.is_object() && v["type"].as_str().is_some_and(|s| !s.is_empty()) {
                    Ok(v)
                } else {
                    Err(invalid("Pi RPC record must have a type"))
                }
            });
        let failed = parsed.is_err();
        if tx.send(parsed).is_err() || failed {
            return;
        }
    }
}

fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_or_incomplete_frames_end_the_stream() {
        for input in [
            b"bad\n{\"type\":\"later\"}\n".as_slice(),
            b"{\"type\":\"partial\"}",
            b"{}\n",
            b"\xff\n",
        ] {
            let (tx, rx) = std::sync::mpsc::channel();
            read_stdout(input, tx);
            assert!(rx.recv().unwrap().is_err());
            assert!(rx.recv().is_err());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        read_stdout(b"{\"type\":\"ready\"}\r\n".as_slice(), tx);
        assert_eq!(rx.recv().unwrap().unwrap()["type"], "ready");
        let (tx, rx) = std::sync::mpsc::channel();
        read_stdout(vec![b'x'; MAX_LINE + 1].as_slice(), tx);
        assert!(rx.recv().unwrap().is_err());
    }
}
