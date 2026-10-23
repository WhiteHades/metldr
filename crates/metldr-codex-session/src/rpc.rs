use crate::{Result, invalid};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{timeout, timeout_at},
};

const FRAME_LIMIT: usize = 1024 * 1024;
const QUEUE_LIMIT: usize = 2 * FRAME_LIMIT;
const RPC_TIMEOUT: Duration = Duration::from_secs(15);
const STEER_WRITE_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) fn clean_eof(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::UnexpectedEof)
}

pub(crate) struct Rpc {
    child: Child,
    wire: Wire,
    healthy: bool,
}

#[cfg(all(test, unix))]
#[path = "rpc/checks.rs"]
mod checks;

struct Wire {
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    frame: Vec<u8>,
    queue: VecDeque<(Value, usize)>,
    queued_bytes: usize,
    sequence: u64,
    pending: Option<PendingResponse>,
}

struct PendingResponse {
    id: u64,
    deadline: Instant,
    received: bool,
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
        write!(f, "Codex rejected RPC request with code {}", self.code)
    }
}
impl std::error::Error for Rejected {}

impl Rpc {
    pub(crate) fn spawn(executable: &Path, workspace: &Path) -> Result<Self> {
        if !executable.is_absolute() || !workspace.is_absolute() {
            return Err(invalid("Codex executable and workspace must be absolute"));
        }
        let mut child = Command::new(executable)
            .args(["app-server", "--listen", "stdio://"])
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
            .ok_or_else(|| invalid("Codex stdout missing"))?;
        Ok(Self {
            child,
            wire: Wire {
                stdin,
                stdout: BufReader::new(stdout),
                frame: Vec::new(),
                queue: VecDeque::new(),
                queued_bytes: 0,
                sequence: 0,
                pending: None,
            },
            healthy: true,
        })
    }

    fn require_healthy(&self) -> Result<()> {
        if self.healthy {
            Ok(())
        } else {
            Err(invalid("Codex connection cannot be reused"))
        }
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
            .map_err(|_| invalid("Codex RPC deadline expired"))
            .and_then(|r| r);
        self.healthy = match &result {
            Ok(_) => true,
            Err(error) => error.is::<Rejected>(),
        };
        guard.armed = !self.healthy;
        result
    }

    pub(crate) async fn begin_steer(&mut self, params: Value) -> Result<u64> {
        self.require_healthy()?;
        if self.wire.pending.is_some() {
            return Err(invalid("Codex steering response is pending"));
        }
        let id = self.wire.request_id()?;
        self.wire.pending = Some(PendingResponse {
            id,
            deadline: Instant::now() + RPC_TIMEOUT,
            received: false,
        });
        self.healthy = false;
        let mut guard = KillOnCancel {
            child: &mut self.child,
            armed: true,
        };
        let result = timeout(
            STEER_WRITE_TIMEOUT,
            self.wire
                .write(json!({"id":id,"method":"turn/steer","params":params})),
        )
        .await
        .map_err(|_| invalid("Codex steering write deadline expired"))
        .and_then(|r| r);
        self.healthy = result.is_ok();
        guard.armed = !self.healthy;
        result.map(|()| id)
    }

    pub(crate) fn finish_steer(&mut self, id: u64) -> Result<()> {
        if !self
            .wire
            .pending
            .as_ref()
            .is_some_and(|p| p.id == id && p.received)
        {
            return Err(invalid("Codex steering response identity mismatch"));
        }
        self.wire.pending = None;
        Ok(())
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
            .map_err(|_| invalid("Codex write deadline expired"))
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
        match self.wire.next().await {
            Ok((value, _)) => Ok(value),
            Err(error) if clean_eof(error.as_ref()) => Err(error),
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
        // Drain while waiting so a full stdout pipe cannot prevent graceful exit.
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
    fn request_id(&mut self) -> Result<u64> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("Codex request identities exhausted"))?;
        Ok(self.sequence)
    }

    fn accept_pending(&mut self, value: &Value) -> Result<()> {
        let pending = self
            .pending
            .as_mut()
            .filter(|p| !p.received && value["id"].as_u64() == Some(p.id))
            .ok_or_else(|| invalid("Codex response identity mismatch"))?;
        match response(value) {
            Ok(_) => (),
            Err(error) if error.is::<Rejected>() => (),
            Err(error) => return Err(error),
        }
        pending.received = true;
        Ok(())
    }

    async fn read_with_deadline(&mut self) -> Result<(Value, usize)> {
        if let Some(pending) = self.pending.as_ref().filter(|p| !p.received) {
            timeout_at(pending.deadline.into(), self.read())
                .await
                .map_err(|_| invalid("Codex steering response deadline expired"))?
        } else {
            self.read().await
        }
    }

    async fn next(&mut self) -> Result<(Value, usize)> {
        let (value, size) = self.read_with_deadline().await?;
        if value.get("method").is_none() {
            self.accept_pending(&value)?;
        }
        Ok((value, size))
    }

    fn enqueue(&mut self, value: Value, size: usize) -> Result<()> {
        if self.queued_bytes + size > QUEUE_LIMIT || self.queue.len() >= 256 {
            return Err(invalid("Codex event queue exceeds limit"));
        }
        self.queued_bytes += size;
        self.queue.push_back((value, size));
        Ok(())
    }

    async fn write(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        if bytes.len() > FRAME_LIMIT {
            return Err(invalid("Codex command exceeds frame limit"));
        }
        bytes.push(b'\n');
        self.stdin
            .as_mut()
            .ok_or_else(|| invalid("Codex stdin closed"))?
            .write_all(&bytes)
            .await?;
        Ok(())
    }

    async fn read(&mut self) -> Result<(Value, usize)> {
        // The buffer survives cancelled reads, so Stop can interrupt a pending event wait.
        loop {
            let bytes = self.stdout.fill_buf().await?;
            if bytes.is_empty() {
                return Err(if self.frame.is_empty() {
                    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Codex stdout closed")
                        .into()
                } else {
                    invalid("Codex stdout closed during a frame")
                });
            }
            let end = bytes.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let count = end.unwrap_or(bytes.len());
            if self.frame.len() + count > FRAME_LIMIT {
                return Err(invalid("Codex output exceeds frame limit"));
            }
            self.frame.extend_from_slice(&bytes[..count]);
            self.stdout.consume(count);
            if end.is_some() {
                let count = self.frame.len();
                let parsed = serde_json::from_slice(&self.frame);
                self.frame.clear();
                return Ok((parsed?, count));
            }
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.request_id()?;
        {
            self.write(json!({"id":id,"method":method,"params":params}))
                .await?;
            loop {
                let (value, size) = self.read_with_deadline().await?;
                if value.get("method").is_none() {
                    if value["id"].as_u64() == Some(id) {
                        return response(&value);
                    }
                    self.accept_pending(&value)?;
                }
                self.enqueue(value, size)?;
            }
        }
    }
}

pub(crate) fn response(value: &Value) -> Result<Value> {
    if let Some(error) = value.get("error") {
        let code = error["code"]
            .as_i64()
            .filter(|_| error["message"].is_string() && value.get("result").is_none())
            .ok_or_else(|| invalid("Codex RPC error is malformed"))?;
        return Err(Box::new(Rejected { code }));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| invalid("Codex result missing"))
}
