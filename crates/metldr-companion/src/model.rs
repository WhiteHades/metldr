use crate::{Result, TEXT_MAX};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    pub(super) tab: i32,
    pub(super) document: String,
    pub(super) url: String,
    pub(super) title: String,
    pub(super) text: String,
}

pub(super) fn validate_page_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|_| "invalid page URL")?;
    if value.len() > 2048
        || value.chars().any(char::is_control)
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() == Some(0)
        || url.as_str() != value
    {
        return Err("invalid page URL".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Source {
    pub(super) id: String,
    #[serde(flatten)]
    pub(super) page: Page,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Running,
    StopRequested,
    Complete,
    Cancelled,
    Failed,
    Interrupted,
}
impl Status {
    pub(super) fn terminal(self) -> bool {
        !matches!(self, Self::Running | Self::StopRequested)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Run {
    pub(super) id: String,
    pub(super) source: String,
    pub(super) kind: String,
    pub(super) question: Option<String>,
    pub(super) status: Status,
    pub(super) output: metldr_conversation::Content,
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct View {
    pub(super) sequence: u64,
    pub(super) store: String,
    pub(super) source: Option<Source>,
    pub(super) runs: Vec<Run>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Store {
    pub(super) schema: u8,
    pub(super) view: View,
    pub(super) next: u64,
    pub(super) starts: BTreeMap<String, String>,
    pub(super) launches: BTreeMap<String, crate::agent_model::Launch>,
    pub(super) workflows: BTreeMap<String, crate::agent_model::Workflow>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) codex_workflows: BTreeMap<String, crate::codex_model::Workflow>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) opencode_workflows: BTreeMap<String, crate::opencode_model::Workflow>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) browser_actions: BTreeMap<String, crate::browser_action::Action>,
    pub(super) providers: crate::provider_settings::Settings,
    pub(super) agent_settings: crate::agent_settings::Settings,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub(super) v: u8,
    pub(super) request: String,
    pub(super) command: Command,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    #[serde(skip_serializing)]
    ProviderSave {
        protocol: crate::provider_settings::Protocol,
        expected_revision: u64,
        endpoint: String,
        model: String,
        key: crate::provider_settings::KeyChange,
    },
    ProviderRemove {
        protocol: crate::provider_settings::Protocol,
        expected_revision: u64,
    },
    ProviderRefresh,
    AgentSave {
        expected_revision: u64,
        profile: crate::agent_settings::Profile,
    },
    AgentRemove {
        agent: crate::agent_settings::Kind,
        expected_revision: u64,
    },
    AgentRefresh,
    GoogleTask {
        source: String,
        text: String,
        fingerprint: String,
    },
    ResponsesTask {
        source: String,
        text: String,
        fingerprint: String,
    },
    ChatTask {
        source: String,
        text: String,
        fingerprint: String,
    },
    BrowserTask {
        source: String,
        text: String,
        fingerprint: Option<String>,
    },
    BrowserReply {
        call: u64,
        run: String,
        result: crate::browser_io::WireReply,
    },
    BrowserEvent {
        run: String,
        tab: i32,
        session: Option<String>,
        method: String,
        params: serde_json::Value,
    },
    BrowserDecision {
        action: String,
        revision: u64,
        approve: bool,
    },
    View,
    Summarize {
        page: Page,
    },
    Capture {
        expected_store: String,
        #[serde(deserialize_with = "Option::deserialize")]
        expected_source: Option<String>,
        page: Page,
    },
    Ask {
        source: String,
        text: String,
    },
    Watch {
        run: String,
    },
    Stop {
        run: String,
    },
    PiPrompt {
        source: String,
        text: String,
    },
    PiFollowUp {
        workflow: String,
        revision: u64,
        text: String,
    },
    CodexPrompt {
        source: String,
        text: String,
    },
    CodexFollowUp {
        workflow: String,
        revision: u64,
        text: String,
    },
    #[serde(rename = "opencode_prompt")]
    OpenCodePrompt {
        source: String,
        text: String,
    },
    #[serde(rename = "opencode_follow_up")]
    OpenCodeFollowUp {
        workflow: String,
        revision: u64,
        text: String,
    },
    #[serde(rename = "opencode_permission")]
    OpenCodePermission {
        run: String,
        request: String,
        option: Option<String>,
    },
}

impl Store {
    pub(super) fn seal_outputs(&mut self) -> Result<bool> {
        let mut changed = false;
        for run in &mut self.view.runs {
            if run.status.terminal() {
                changed |= run.output.end(if run.status == Status::Complete {
                    metldr_conversation::Ending::Complete
                } else {
                    metldr_conversation::Ending::Interrupted
                })?;
            }
        }
        Ok(changed)
    }
    pub(super) fn validate(&self) -> Result<()> {
        self.providers.validate()?;
        self.agent_settings.validate()?;
        let view = &self.view;
        if view.store.len() != 32
            || !view.store.bytes().all(|b| b.is_ascii_hexdigit())
            || view.runs.len() > 16
            || self.starts.len() > 16
            || self.next == 0
        {
            return Err("invalid saved store".into());
        }
        let prefix = format!("{}:", view.store);
        let valid_id = |s: &str| {
            s.strip_prefix(&prefix)
                .and_then(|n| n.parse::<u64>().ok())
                .is_some_and(|n| n > 0 && n < self.next)
        };
        if let Some(source) = &view.source {
            if !valid_id(&source.id) {
                return Err("invalid saved source identity".into());
            }
            check_page_fields(&source.page)?;
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut previous_run = 0;
        for run in &view.runs {
            run.output.validate()?;
            if !valid_id(&run.id)
                || !valid_id(&run.source)
                || !ids.insert(&run.id)
                || !matches!(
                    run.kind.as_str(),
                    "summary" | "answer" | "agent" | "browser"
                )
                || run.output.len() > TEXT_MAX
                || run
                    .question
                    .as_ref()
                    .is_some_and(|s| s.is_empty() || s.len() > 1024)
                || run.error.as_ref().is_some_and(|s| s.len() > 256)
            {
                return Err("invalid saved run".into());
            }
            let number = run
                .id
                .strip_prefix(&prefix)
                .ok_or("invalid run prefix")?
                .parse::<u64>()?;
            if number <= previous_run {
                return Err("saved runs are not in creation order".into());
            }
            previous_run = number;
        }
        for (request, run) in &self.starts {
            if !valid_request_id(request) || !view.runs.iter().any(|r| &r.id == run) {
                return Err("invalid saved start identity".into());
            }
        }
        crate::browser_action::validate(self, valid_id)?;
        crate::codex_model::validate(self)?;
        crate::opencode_model::validate(self)?;
        crate::agent_model::validate(self)
    }
    pub(super) fn id(&mut self) -> Result<String> {
        let local = self.next;
        self.next = local.checked_add(1).ok_or("identity capacity reached")?;
        Ok(format!("{}:{local}", self.view.store))
    }
}

pub(super) fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))
}
pub(super) fn check_page_fields(page: &Page) -> Result<()> {
    if page.tab < 0
        || page.document.is_empty()
        || page.document.len() > 128
        || page.url.len() > 2048
        || page.title.is_empty()
        || page.title.len() > 256
        || page.text.is_empty()
        || page.text.len() > TEXT_MAX
    {
        return Err("invalid or oversized page".into());
    }
    validate_page_url(&page.url)?;
    Ok(())
}
