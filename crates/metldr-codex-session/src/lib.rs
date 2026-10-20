#![doc = include_str!("../README.md")]

mod event;
mod report;
mod rpc;

pub use event::{Event, ToolCall, TurnStatus};
pub use report::{Preview, ToolOutcome, ToolReport, ToolState};
pub use rpc::ProcessExit;
use serde_json::{Value, json};
use std::{
    error::Error,
    io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
pub(crate) fn invalid(message: &'static str) -> Box<dyn Error + Send + Sync> {
    io::Error::other(message).into()
}
pub(crate) fn field(value: &Value, name: &str) -> Result<String> {
    value[name]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| invalid("Codex response field missing or invalid"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadId(String);
impl ThreadId {
    pub fn saved(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(invalid("Invalid saved Codex thread identity"));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnId(String);
impl TurnId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct ThreadInfo {
    pub id: ThreadId,
    pub session_id: String,
    pub model: String,
    pub provider: String,
    pub cwd: String,
}

pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub enum Thread {
    New { tools: Vec<ToolSpec> },
    Resume(ThreadId),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Interrupt {
    Acknowledged,
    Rejected { code: i64 },
}

pub struct CodexSession {
    rpc: rpc::Rpc,
    info: ThreadInfo,
    turn: Option<TurnId>,
    connection: u64,
    pending_tools: Vec<Value>,
    reports: report::Reports,
    failed: bool,
}

impl CodexSession {
    pub async fn open(executable: &Path, workspace: &Path, thread: Thread) -> Result<Self> {
        let mut rpc = rpc::Rpc::spawn(executable, workspace)?;
        rpc.call("initialize", json!({"clientInfo":{"name":"metldr","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
        rpc.send(json!({"method":"initialized","params":{}}))
            .await?;
        let expected_id = match &thread {
            Thread::Resume(id) => Some(id.clone()),
            _ => None,
        };
        let (method, params) = match thread {
            Thread::New { tools } => {
                if tools.len() > 32 {
                    return Err(invalid("Too many Codex client tools"));
                }
                let mut definitions = Vec::new();
                for tool in tools {
                    if tool.name.is_empty()
                        || tool.name.len() > 64
                        || !tool
                            .name
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                        || tool.description.len() > 4096
                        || tool.input_schema["type"] != "object"
                        || definitions.iter().any(|v: &Value| v["name"] == tool.name)
                    {
                        return Err(invalid("Invalid Codex client tool definition"));
                    }
                    definitions.push(json!({"type":"function","name":tool.name,"description":tool.description,"inputSchema":tool.input_schema}));
                }
                (
                    "thread/start",
                    json!({"cwd":workspace,"dynamicTools":definitions}),
                )
            }
            Thread::Resume(id) => ("thread/resume", json!({"threadId":id.0,"cwd":workspace})),
        };
        let response = rpc.call(method, params).await?;
        let info = ThreadInfo {
            id: ThreadId::saved(&field(&response["thread"], "id")?)?,
            session_id: field(&response["thread"], "sessionId")?,
            model: field(&response, "model")?,
            provider: field(&response, "modelProvider")?,
            cwd: field(&response, "cwd")?,
        };
        if expected_id.as_ref().is_some_and(|id| id != &info.id)
            || Path::new(&info.cwd) != workspace
        {
            return Err(invalid("Codex returned a different thread or workspace"));
        }
        static CONNECTION: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            rpc,
            info,
            turn: None,
            connection: CONNECTION.fetch_add(1, Ordering::Relaxed),
            pending_tools: Vec::new(),
            reports: report::Reports::default(),
            failed: false,
        })
    }

    pub fn info(&self) -> &ThreadInfo {
        &self.info
    }

    pub async fn prompt(&mut self, text: &str) -> Result<TurnId> {
        if self.failed || self.turn.is_some() || text.is_empty() || text.len() > 32 * 1024 {
            return Err(invalid("Codex prompt is invalid or session is busy"));
        }
        let response = self
            .rpc
            .call(
                "turn/start",
                json!({"threadId":self.info.id.0,"input":[{"type":"text","text":text}]}),
            )
            .await?;
        let turn = TurnId(field(&response["turn"], "id")?);
        self.turn = Some(turn.clone());
        self.reports.clear();
        Ok(turn)
    }

    pub async fn next_event(&mut self) -> Result<Event> {
        if self.failed || self.turn.is_none() {
            return Err(invalid("No active Codex turn"));
        }
        loop {
            let value = self.rpc.next().await?;
            match self.event(value).await {
                Ok(Some(event)) => return Ok(event),
                Ok(None) => (),
                Err(error) => {
                    self.failed = true;
                    self.rpc.abort();
                    return Err(error);
                }
            }
        }
    }

    pub async fn reply_tool(&mut self, call: ToolCall, text: &str, success: bool) -> Result<()> {
        if self.failed
            || call.connection != self.connection
            || self.turn.as_ref() != Some(&call.turn)
            || text.len() > 32 * 1024
        {
            return Err(invalid("Codex tool reply is invalid or stale"));
        }
        let index = self
            .pending_tools
            .iter()
            .position(|id| id == &call.request)
            .ok_or_else(|| invalid("Codex tool reply identity is not pending"))?;
        self.rpc.send(json!({"id":call.request,"result":{"success":success,"contentItems":[{"type":"inputText","text":text}]}})).await?;
        self.pending_tools.swap_remove(index);
        Ok(())
    }

    pub async fn interrupt(&mut self) -> Result<Interrupt> {
        if self.failed {
            return Err(invalid("Codex session has failed"));
        }
        let turn = self
            .turn
            .as_ref()
            .ok_or_else(|| invalid("No active Codex turn to interrupt"))?;
        let result = self
            .rpc
            .call(
                "turn/interrupt",
                json!({"threadId":self.info.id.0,"turnId":turn.0}),
            )
            .await;
        match result {
            Ok(_) => Ok(Interrupt::Acknowledged),
            Err(error) => match error.downcast_ref::<rpc::Rejected>() {
                Some(rejected) => Ok(Interrupt::Rejected {
                    code: rejected.code,
                }),
                None => Err(error),
            },
        }
    }

    pub async fn close(self) -> Result<ProcessExit> {
        self.rpc.close().await
    }
}
