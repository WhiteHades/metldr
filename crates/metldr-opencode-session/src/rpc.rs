use crate::{Result, invalid};
use serde_json::{Value, json};
use std::{collections::VecDeque, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};

const FRAME_LIMIT: usize = 1024 * 1024;
const QUEUE_LIMIT: usize = 2 * FRAME_LIMIT;
const RPC_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct Rpc {
    child: Child,
    wire: Wire,
    healthy: bool,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_rpc_and_blocked_write_kill_child_and_poison_connection() {
        for blocked_write in [false, true] {
            let mut child = Command::new("sleep")
                .arg("60")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let wire = Wire {
                stdin: child.stdin.take(),
                stdout: BufReader::new(child.stdout.take().unwrap()),
                frame: Vec::new(),
                queue: VecDeque::new(),
                queued_bytes: 0,
                sequence: 0,
            };
            let mut rpc = Rpc {
                child,
                wire,
                healthy: true,
            };
            let deadline = Duration::from_millis(30);
            if blocked_write {
                assert!(
                    timeout(deadline, rpc.send(json!({"held":"x".repeat(900_000)})))
                        .await
                        .is_err()
                );
            } else {
                assert!(
                    timeout(deadline, rpc.call("held", json!({})))
                        .await
                        .is_err()
                );
            }
            assert!(!rpc.healthy);
            assert!(rpc.call("later", json!({})).await.is_err());
            let status = timeout(Duration::from_secs(1), rpc.child.wait())
                .await
                .unwrap()
                .unwrap();
            assert!(!status.success());
        }
    }
}

struct Wire {
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    frame: Vec<u8>,
    queue: VecDeque<(Value, usize)>,
    queued_bytes: usize,
    sequence: u64,
}

struct KillOnCancel<'a> {
    child: &'a mut Child,
    armed: bool,
}
impl Drop for KillOnCancel<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.child.start_kill();
        }
    }
}

pub struct ProcessExit {
    pub code: Option<i32>,
    pub forced: bool,
    pub drained: bool,
}

#[derive(Debug)]
pub(crate) struct Rejected {
    pub code: i64,
}
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OpenCode rejected RPC request with code {}", self.code)
    }
}
impl std::error::Error for Rejected {}

impl Rpc {
    pub(crate) fn spawn(executable: &Path, workspace: &Path) -> Result<Self> {
        if !executable.is_absolute() || !workspace.is_absolute() {
            return Err(invalid(
                "OpenCode executable and workspace must be absolute",
            ));
        }
        let mut child = Command::new(executable)
            .arg("acp")
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid("OpenCode stdout missing"))?;
        Ok(Self {
            child,
            wire: Wire {
                stdin,
                stdout: BufReader::new(stdout),
                frame: Vec::new(),
                queue: VecDeque::new(),
                queued_bytes: 0,
                sequence: 0,
            },
            healthy: true,
        })
    }

    fn require_healthy(&self) -> Result<()> {
        if self.healthy {
            Ok(())
        } else {
            Err(invalid("OpenCode connection cannot be reused"))
        }
    }

    pub(crate) async fn begin(&mut self, method: &str, params: Value) -> Result<u64> {
        self.require_healthy()?;
        self.healthy = false;
        let mut guard = KillOnCancel {
            child: &mut self.child,
            armed: true,
        };
        let result = timeout(RPC_TIMEOUT, self.wire.begin(method, params))
            .await
            .map_err(|_| invalid("OpenCode RPC write deadline expired"))
            .and_then(|r| r);
        self.healthy = result.is_ok();
        guard.armed = !self.healthy;
        result
    }

    pub(crate) async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.require_healthy()?;
        self.healthy = false;
        let mut guard = KillOnCancel {
            child: &mut self.child,
            armed: true,
        };
        let result = timeout(RPC_TIMEOUT, self.wire.call(method, params))
            .await
            .map_err(|_| invalid("OpenCode RPC deadline expired"))
            .and_then(|r| r);
        self.healthy = match &result {
            Ok(_) => true,
            Err(error) => error.is::<Rejected>(),
        };
        guard.armed = !self.healthy;
        result
    }

    pub(crate) async fn send(&mut self, value: Value) -> Result<()> {
        self.require_healthy()?;
        self.healthy = false;
        let mut guard = KillOnCancel {
            child: &mut self.child,
            armed: true,
        };
        let result = timeout(RPC_TIMEOUT, self.wire.write(value))
            .await
            .map_err(|_| invalid("OpenCode write deadline expired"))
            .and_then(|r| r);
        self.healthy = result.is_ok();
        guard.armed = !self.healthy;
        result
    }

    pub(crate) async fn next(&mut self) -> Result<Value> {
        self.require_healthy()?;
        if let Some((value, size)) = self.wire.queue.pop_front() {
            self.wire.queued_bytes -= size;
            return Ok(value);
        }
        match self.wire.read().await {
            Ok((value, _)) => Ok(value),
            Err(error) => {
                self.abort();
                Err(error)
            }
        }
    }

    pub(crate) fn abort(&mut self) {
        self.healthy = false;
        let _ = self.child.start_kill();
    }

    pub(crate) async fn close(mut self) -> Result<ProcessExit> {
        self.healthy = false;
        self.wire.stdin.take();
        let drain = async {
            let mut sink = tokio::io::sink();
            tokio::io::copy(&mut self.wire.stdout, &mut sink).await
        };
        let (status, drain) = tokio::join!(
            timeout(Duration::from_secs(10), self.child.wait()),
            timeout(Duration::from_secs(10), drain)
        );
        let (status, forced) = match status {
            Ok(status) => (status?, false),
            Err(_) => {
                self.child.start_kill()?;
                (self.child.wait().await?, true)
            }
        };
        Ok(ProcessExit {
            code: status.code(),
            forced,
            drained: matches!(drain, Ok(Ok(_))),
        })
    }
}

impl Wire {
    async fn write(&mut self, mut value: Value) -> Result<()> {
        let envelope = value
            .as_object_mut()
            .ok_or_else(|| invalid("OpenCode message must be a JSON-RPC object"))?;
        if envelope
            .get("jsonrpc")
            .is_some_and(|version| version.as_str() != Some("2.0"))
        {
            return Err(invalid("OpenCode message must use JSON-RPC 2.0"));
        }
        envelope.insert("jsonrpc".into(), Value::String("2.0".into()));
        let mut bytes = serde_json::to_vec(&value)?;
        if bytes.len() > FRAME_LIMIT {
            return Err(invalid("OpenCode command exceeds frame limit"));
        }
        bytes.push(b'\n');
        self.stdin
            .as_mut()
            .ok_or_else(|| invalid("OpenCode stdin closed"))?
            .write_all(&bytes)
            .await?;
        Ok(())
    }

    async fn read(&mut self) -> Result<(Value, usize)> {
        loop {
            let bytes = self.stdout.fill_buf().await?;
            if bytes.is_empty() {
                return Err(invalid("OpenCode stdout closed"));
            }
            let end = bytes.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let count = end.unwrap_or(bytes.len());
            if self.frame.len() + count > FRAME_LIMIT {
                return Err(invalid("OpenCode output exceeds frame limit"));
            }
            self.frame.extend_from_slice(&bytes[..count]);
            self.stdout.consume(count);
            if end.is_some() {
                let count = self.frame.len();
                let parsed = serde_json::from_slice(&self.frame)?;
                self.frame.clear();
                validate_envelope(&parsed)?;
                return Ok((parsed, count));
            }
        }
    }

    async fn begin(&mut self, method: &str, params: Value) -> Result<u64> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("OpenCode RPC request ID exhausted"))?;
        let id = self.sequence;
        self.write(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        Ok(id)
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.begin(method, params).await?;
        loop {
            let (value, size) = self.read().await?;
            if value.get("method").is_none() {
                if value["id"] != id {
                    return Err(invalid("OpenCode response identity mismatch"));
                }
                if let Some(error) = value.get("error") {
                    let code = error["code"]
                        .as_i64()
                        .filter(|_| error["message"].is_string() && value.get("result").is_none())
                        .ok_or_else(|| invalid("OpenCode RPC error is malformed"))?;
                    return Err(Box::new(Rejected { code }));
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| invalid("OpenCode result missing"));
            }
            if self.queued_bytes + size > QUEUE_LIMIT || self.queue.len() >= 256 {
                return Err(invalid("OpenCode event queue exceeds limit"));
            }
            self.queued_bytes += size;
            self.queue.push_back((value, size));
        }
    }
}

fn validate_envelope(value: &Value) -> Result<()> {
    if !value.is_object() || value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid("OpenCode frame is not a JSON-RPC 2.0 envelope"));
    }
    if let Some(method) = value.get("method") {
        if !method.is_string()
            || value.get("result").is_some()
            || value.get("error").is_some()
            || value
                .get("params")
                .is_some_and(|params| !(params.is_object() || params.is_array()))
            || value
                .get("id")
                .is_some_and(|id| !(id.is_string() || id.is_number() || id.is_null()))
        {
            return Err(invalid("OpenCode request envelope is malformed"));
        }
        return Ok(());
    }
    if !value
        .get("id")
        .is_some_and(|id| id.is_string() || id.is_number() || id.is_null())
        || value.get("result").is_some() == value.get("error").is_some()
    {
        return Err(invalid("OpenCode response envelope is malformed"));
    }
    if let Some(error) = value.get("error")
        && (!error.is_object()
            || !error.get("code").is_some_and(Value::is_i64)
            || !error.get("message").is_some_and(Value::is_string))
    {
        return Err(invalid("OpenCode RPC error is malformed"));
    }
    Ok(())
}
