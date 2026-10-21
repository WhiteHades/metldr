type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const FRAME_MAX: usize = 65_536;
const STORE_MAX: usize = 49_152;
const TEXT_MAX: usize = 8_192;
const CONFIG_MAX: usize = 4_096;
const WIRE_VERSION: u8 = 5;

mod agent;
mod agent_model;
mod agent_owner;
mod agent_settings;
mod browser_action;
mod browser_io;
mod browser_mcp;
mod browser_owner;
mod browser_worker;
mod codex;
mod codex_model;
mod codex_owner;
mod flow;
mod https_profile;
mod model;
mod native;
mod native_permission;
mod opencode;
mod opencode_model;
mod opencode_owner;
mod provider;
mod provider_settings;
mod public_view;
mod recovery;
mod store;
mod tool_report;
mod transport;
mod wire;

#[cfg(test)]
mod browser_checks;
#[cfg(test)]
mod tests;

pub fn run() -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    native::run()
}
