#![doc = include_str!("../README.md")]

mod mcp;
mod permission;
mod rpc;

pub use mcp::McpServer;
pub use permission::Permission;
pub use rpc::ProcessExit;
use rpc::Rpc;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn invalid(message: &str) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionId(String);
impl SessionId {
    pub fn saved(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(invalid("invalid OpenCode session identity"));
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub enum Session {
    New,
    Resume(SessionId),
}

pub enum Event {
    Update(Value),
    Permission(Permission),
    Finished { stop_reason: String },
}

pub struct OpenCodeSession {
    rpc: Rpc,
    session: SessionId,
    agent: Value,
    options: Vec<Value>,
    active: Option<u64>,
    origin: Arc<()>,
    pending: BTreeMap<String, Arc<()>>,
}

impl OpenCodeSession {
    pub async fn open(
        executable: &Path,
        workspace: &Path,
        session: Session,
        servers: &[McpServer],
    ) -> Result<Self> {
        let servers = mcp::declarations(servers)?;
        let cwd = workspace
            .to_str()
            .ok_or_else(|| invalid("OpenCode workspace must be UTF8"))?;
        let mut rpc = Rpc::spawn(executable, workspace)?;
        let result = async {
            let init = rpc
                .call(
                    "initialize",
                    json!({"protocolVersion":1,"clientCapabilities":{},
                "clientInfo":{"name":"metldr","version":"0.1.0"}}),
                )
                .await?;
            if init["protocolVersion"] != 1 || init["agentInfo"]["name"] != "OpenCode" {
                return Err(invalid("unsupported OpenCode ACP handshake"));
            }
            let attached = match &session {
                Session::New => {
                    rpc.call("session/new", json!({"cwd":cwd,"mcpServers":servers}))
                        .await?
                }
                Session::Resume(id) => {
                    if !init["agentCapabilities"]["sessionCapabilities"]["resume"].is_object() {
                        return Err(invalid("OpenCode does not support session resume"));
                    }
                    rpc.call(
                        "session/resume",
                        json!({"cwd":cwd,"sessionId":id.as_str(),"mcpServers":servers}),
                    )
                    .await?
                }
            };
            let id = match &session {
                Session::New => SessionId::saved(
                    attached["sessionId"]
                        .as_str()
                        .ok_or_else(|| invalid("OpenCode session identity missing"))?,
                )?,
                Session::Resume(id) => {
                    if attached
                        .get("sessionId")
                        .is_some_and(|value| value != id.as_str())
                    {
                        return Err(invalid("OpenCode resumed a different session"));
                    }
                    id.clone()
                }
            };
            let options = attached["configOptions"]
                .as_array()
                .cloned()
                .ok_or_else(|| invalid("OpenCode session configuration missing"))?;
            Ok((id, init["agentInfo"].clone(), options))
        }
        .await;
        match result {
            Ok((session, agent, options)) => Ok(Self {
                rpc,
                session,
                agent,
                options,
                active: None,
                origin: Arc::new(()),
                pending: BTreeMap::new(),
            }),
            Err(error) => {
                let _ = rpc.close().await;
                Err(error)
            }
        }
    }

    pub fn id(&self) -> &SessionId {
        &self.session
    }
    pub fn agent_info(&self) -> &Value {
        &self.agent
    }
    pub fn config_options(&self) -> &[Value] {
        &self.options
    }

    pub async fn prompt(&mut self, text: &str) -> Result<()> {
        if self.active.is_some() {
            return Err(invalid("OpenCode prompt is already active"));
        }
        let params = prompt_params(self.session.as_str(), text)?;
        let id = self.rpc.begin("session/prompt", params).await?;
        self.active = Some(id);
        Ok(())
    }

    pub async fn cancel(&mut self) -> Result<()> {
        if self.active.is_none() {
            return Err(invalid("OpenCode has no active prompt"));
        }
        self.rpc
            .send(json!({"jsonrpc":"2.0","method":"session/cancel",
            "params":{"sessionId":self.session.as_str()}}))
            .await
    }

    pub async fn reply_permission(
        &mut self,
        request: &Permission,
        option: Option<&str>,
    ) -> Result<()> {
        if !request.is_pending(&self.origin, self.pending.get(&request.key)) {
            return Err(invalid(
                "OpenCode permission belongs to another connection or has already been answered",
            ));
        }
        let result = request.response(option)?;
        self.rpc
            .send(json!({"jsonrpc":"2.0","id":request.id,"result":result}))
            .await?;
        self.pending.remove(&request.key);
        Ok(())
    }

    pub async fn next_event(&mut self) -> Result<Event> {
        loop {
            let value = self.rpc.next().await?;
            match self.decode(&value) {
                Ok(Some(event)) => return Ok(event),
                Ok(None) => {
                    if value.get("id").is_some() {
                        self.rpc
                            .send(json!({"jsonrpc":"2.0","id":value["id"],
                            "error":{"code":-32601,"message":"Unsupported client method"}}))
                            .await?;
                    }
                }
                Err(error) => {
                    self.rpc.abort();
                    return Err(error);
                }
            }
        }
    }

    fn decode(&mut self, value: &Value) -> Result<Option<Event>> {
        if let Some(method) = value["method"].as_str() {
            if method == "session/update" && value.get("id").is_none() {
                let params = &value["params"];
                if params["sessionId"] != self.session.as_str()
                    || params["update"]["sessionUpdate"]
                        .as_str()
                        .is_none_or(|s| s.is_empty() || s.len() > 64)
                {
                    return Err(invalid("OpenCode update has wrong session or missing kind"));
                }
                return Ok(Some(Event::Update(params["update"].clone())));
            }
            if method == "session/request_permission" {
                let key = permission::request_key(&value["id"])?;
                if self.pending.len() >= 16 || self.pending.contains_key(&key) {
                    return Err(invalid(
                        "OpenCode pending permission limit or repeated identity",
                    ));
                }
                let request = Permission::parse(value, self.session.as_str(), self.origin.clone())?;
                self.pending.insert(key, request.identity.clone());
                return Ok(Some(Event::Permission(request)));
            }
            return Ok(None);
        }
        if self.active.is_none_or(|id| value["id"] != id) || value.get("error").is_some() {
            return Err(invalid(
                "OpenCode prompt response failed or has wrong identity",
            ));
        }
        let reason = value["result"]["stopReason"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
            .ok_or_else(|| invalid("OpenCode prompt settlement is malformed"))?
            .to_owned();
        self.active = None;
        Ok(Some(Event::Finished {
            stop_reason: reason,
        }))
    }

    pub async fn close(mut self) -> Result<ProcessExit> {
        let result = if self.active.is_none() {
            self.rpc
                .call("session/close", json!({"sessionId":self.session.as_str()}))
                .await
                .map(|_| ())
        } else {
            Ok(())
        };
        let exit = self.rpc.close().await?;
        result?;
        Ok(exit)
    }
}

fn prompt_params(session: &str, text: &str) -> Result<Value> {
    if text.trim().is_empty() || text.len() > 128 * 1024 {
        return Err(invalid("OpenCode prompt is empty or exceeds 128 KiB"));
    }
    Ok(json!({"sessionId":session,"prompt":[{"type":"text","text":text}]}))
}

#[cfg(test)]
mod prompt_tests {
    use super::*;

    #[test]
    fn complete_context_fits_the_prompt_and_rpc_frame_bounds() {
        let text = "\0".repeat(128 * 1024);
        let params = prompt_params("ses_owned", &text).unwrap();
        assert_eq!(params["prompt"][0]["text"], text);
        assert_eq!(params["sessionId"], "ses_owned");
        assert!(serde_json::to_vec(&params).unwrap().len() < 1024 * 1024 - 256);
        assert!(prompt_params("ses_owned", &(text + "a")).is_err());
        assert!(prompt_params("ses_owned", " \n").is_err());
    }
}
