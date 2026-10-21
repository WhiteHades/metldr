#![doc = include_str!("../README.md")]

mod mcp;
mod process;
mod rpc;

use std::{error::Error, path::PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub use mcp::McpServer;

pub struct PiProfile {
    pub executable: PathBuf,
    pub workspace: PathBuf,
    pub provider: String,
    pub model: String,
    pub session_dir: PathBuf,
    pub resume: Option<PathBuf>,
    pub offline: bool,
    pub mcp: Option<McpServer>,
}

pub struct PiSession(process::Session);

pub struct ProcessExit {
    pub code: Option<i32>,
    pub forced: bool,
    pub drained: bool,
}

impl PiSession {
    pub fn spawn(profile: PiProfile) -> Result<Self> {
        process::Session::spawn(profile).map(Self)
    }

    pub fn send(&mut self, value: serde_json::Value) -> Result<()> {
        self.0.send(value)
    }

    pub fn recv(&self, timeout: std::time::Duration) -> Result<serde_json::Value> {
        self.0.recv(timeout)
    }

    pub fn id(&self) -> u32 {
        self.0.id()
    }

    pub fn close(&mut self) -> Result<ProcessExit> {
        self.0.close()
    }
}
