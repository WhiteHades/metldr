use crate::{Result, model::Source};
use redb::{ReadableDatabase, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

pub(super) const FORMAT: u8 = 11;
const RECORD_MAX: usize = 2 * 1024 * 1024;
pub(super) const META: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
pub(super) const CONVERSATIONS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("conversations");
pub(super) const CONVERSATION_ORDER: TableDefinition<u64, &str> =
    TableDefinition::new("conversation_order");
pub(super) const SELECTIONS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("selection_receipts");
pub(super) const SOURCES: TableDefinition<&str, &[u8]> = TableDefinition::new("sources");
pub(super) const RUNS: TableDefinition<&str, &[u8]> = TableDefinition::new("runs");
pub(super) const LAUNCHES: TableDefinition<&str, &[u8]> = TableDefinition::new("launches");
pub(super) const STARTS: TableDefinition<&str, &[u8]> = TableDefinition::new("starts");
pub(super) const ACTIVE_RUNS: TableDefinition<&str, ()> = TableDefinition::new("active_runs");
pub(super) const CONTEXT: TableDefinition<(&str, u64), &str> =
    TableDefinition::new("completed_context");
pub(super) const WORKFLOW_RUNS: TableDefinition<(&str, u64), &str> =
    TableDefinition::new("workflow_runs");
pub(super) const NATIVE_TARGETS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("native_targets");
pub(super) const NAVIGATION_HEADS: TableDefinition<&str, &str> =
    TableDefinition::new("navigation_heads");
pub(super) const PI: TableDefinition<&str, &[u8]> = TableDefinition::new("pi_workflows");
pub(super) const CODEX: TableDefinition<&str, &[u8]> = TableDefinition::new("codex_workflows");
pub(super) const OPENCODE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("opencode_workflows");
pub(super) const ACTIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("actions");
pub(super) const RUN_ORDER: TableDefinition<(&str, u64), &str> = TableDefinition::new("run_order");
pub(super) const ACTION_ORDER: TableDefinition<(&str, u64, &str), &str> =
    TableDefinition::new("action_order");
pub(super) const ACTION_BY_TOOL: TableDefinition<(&str, &str), &str> =
    TableDefinition::new("action_by_tool");
pub(super) const OPEN_ACTIONS: TableDefinition<&str, &str> = TableDefinition::new("open_actions");
pub(super) const NAVIGATION_GRANTS: TableDefinition<(&str, u64), &str> =
    TableDefinition::new("navigation_grants");
pub(super) const UNCERTAIN_RUNS: TableDefinition<&str, &str> =
    TableDefinition::new("uncertain_runs");

pub(super) fn require_base_tables(read: &redb::ReadTransaction) -> Result<()> {
    for definition in [
        META,
        CONVERSATIONS,
        SOURCES,
        RUNS,
        LAUNCHES,
        STARTS,
        NATIVE_TARGETS,
        PI,
        CODEX,
        OPENCODE,
        ACTIONS,
    ] {
        read.open_table(definition)?;
    }
    for definition in [CONTEXT, WORKFLOW_RUNS, RUN_ORDER, NAVIGATION_GRANTS] {
        read.open_table(definition)?;
    }
    for definition in [NAVIGATION_HEADS, OPEN_ACTIONS, UNCERTAIN_RUNS] {
        read.open_table(definition)?;
    }
    read.open_table(ACTIVE_RUNS)?;
    read.open_table(ACTION_ORDER)?;
    read.open_table(ACTION_BY_TOOL)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    pub format: u8,
    pub store: String,
    pub next: u64,
    pub sequence: u64,
    pub selected: String,
    pub epoch: u64,
    pub legacy_sha256: String,
    pub providers: crate::provider_settings::Settings,
    pub agents: crate::agent_settings::Settings,
}

impl Profile {
    pub(super) fn validate(&self) -> Result<()> {
        if self.format != FORMAT
            || self.store.len() != 32
            || !self.store.bytes().all(|b| b.is_ascii_hexdigit())
            || self.next == 0
            || self.epoch == 0
        {
            return Err("unsupported or invalid history profile".into());
        }
        self.providers.validate()?;
        self.agents.validate()
    }
}

pub(super) fn validate_origin(profile: &Profile, original: &[u8]) -> Result<()> {
    if original.len() > crate::STORE_MAX {
        return Err("history origin exceeds bound".into());
    }
    let legacy: crate::model::Store = serde_json::from_slice(original)?;
    legacy.validate()?;
    if profile.legacy_sha256 != format!("{:x}", Sha256::digest(original))
        || profile.store != legacy.view.store
        || profile.next <= legacy.next
        || profile.sequence < legacy.view.sequence
    {
        return Err("history origin does not match profile".into());
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Conversation {
    pub id: String,
    pub title: String,
    pub source: Option<String>,
}

pub(super) fn conversation_number(profile: &Profile, id: &str) -> Result<u64> {
    let number = id
        .strip_prefix(&format!("{}:", profile.store))
        .and_then(|suffix| suffix.parse::<u64>().ok())
        .filter(|number| *number > 0 && *number < profile.next)
        .ok_or("invalid conversation identity")?;
    if id != format!("{}:{number}", profile.store) {
        return Err("noncanonical conversation identity".into());
    }
    Ok(number)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record<T> {
    pub conversation: String,
    pub revision: u64,
    pub value: T,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeTarget {
    pub workflow: String,
    pub initial: crate::browser_action::Target,
    pub current: crate::browser_action::Target,
    pub navigation: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Provenance {
    Known { source: Source },
    LegacyUnavailable { id: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Start {
    pub conversation: String,
    pub run: String,
    pub format: ReceiptFormat,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReceiptFormat {
    LegacyUnscoped,
    Scoped { epoch: u64, digest: String },
}

pub(super) fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > RECORD_MAX - self.0.len() {
                return Err(io::Error::other("history record exceeds bound"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new());
    serde_json::to_writer(&mut output, value)?;
    Ok(output.0)
}

pub(super) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > RECORD_MAX {
        return Err("saved history record exceeds bound".into());
    }
    Ok(serde_json::from_slice(bytes)?)
}
