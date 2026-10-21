use crate::{Result, flow::Owner, model::valid_request_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Pi,
    Codex,
    Opencode,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Profile {
    Pi {
        executable: PathBuf,
        workspace: PathBuf,
        session_dir: PathBuf,
        provider: String,
        model: String,
        offline: bool,
    },
    Codex {
        executable: PathBuf,
        workspace: PathBuf,
    },
    Opencode {
        executable: PathBuf,
        workspace: PathBuf,
    },
}
impl Profile {
    fn kind(&self) -> Kind {
        match self {
            Self::Pi { .. } => Kind::Pi,
            Self::Codex { .. } => Kind::Codex,
            Self::Opencode { .. } => Kind::Opencode,
        }
    }
    fn paths(&self) -> (&PathBuf, &PathBuf, Option<&PathBuf>) {
        match self {
            Self::Pi {
                executable,
                workspace,
                session_dir,
                ..
            } => (executable, workspace, Some(session_dir)),
            Self::Codex {
                executable,
                workspace,
            }
            | Self::Opencode {
                executable,
                workspace,
            } => (executable, workspace, None),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        let (exe, workspace, session) = self.paths();
        for path in [Some(exe), Some(workspace), session].into_iter().flatten() {
            if !path.is_absolute()
                || path.as_os_str().len() > 2048
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                || path.to_string_lossy().chars().any(char::is_control)
            {
                return Err("invalid agent profile path".into());
            }
        }
        if let Self::Pi {
            provider, model, ..
        } = self
        {
            if [provider, model]
                .into_iter()
                .any(|s| s.trim().is_empty() || s.len() > 128 || s.chars().any(char::is_control))
            {
                return Err("invalid Pi model selection".into());
            }
        }
        Ok(())
    }
    fn resolve(&self) -> Result<Self> {
        self.validate()?;
        let (exe, workspace, session) = self.paths();
        let executable = exe
            .canonicalize()
            .map_err(|_| "agent executable is unavailable")?;
        let workspace = workspace
            .canonicalize()
            .map_err(|_| "agent workspace is unavailable")?;
        if !executable.is_file() || !workspace.is_dir() {
            return Err("agent executable or workspace is unavailable".into());
        }
        let session = session
            .map(|path| {
                path.canonicalize()
                    .map_err(|_| "Pi session directory is unavailable")
            })
            .transpose()?;
        if session.as_ref().is_some_and(|p| !p.is_dir()) {
            return Err("Pi session directory is unavailable".into());
        }
        Ok(match self {
            Self::Pi {
                provider,
                model,
                offline,
                ..
            } => Self::Pi {
                executable,
                workspace,
                session_dir: session.ok_or("Pi session directory missing")?,
                provider: provider.clone(),
                model: model.clone(),
                offline: *offline,
            },
            Self::Codex { .. } => Self::Codex {
                executable,
                workspace,
            },
            Self::Opencode { .. } => Self::Opencode {
                executable,
                workspace,
            },
        })
    }
    pub(super) fn native(&self) -> Result<NativeProfile> {
        let p = self.resolve()?;
        Ok(match p {
            Self::Pi {
                executable,
                workspace,
                session_dir,
                provider,
                model,
                offline,
            } => NativeProfile::Pi(crate::agent_model::Profile {
                executable,
                workspace,
                session_dir,
                provider,
                model,
                offline,
            }),
            Self::Codex {
                executable,
                workspace,
            } => NativeProfile::Codex(crate::codex_model::Profile {
                executable,
                workspace,
            }),
            Self::Opencode {
                executable,
                workspace,
            } => NativeProfile::Opencode(crate::opencode_model::Profile {
                executable,
                workspace,
            }),
        })
    }
}
pub(super) enum NativeProfile {
    Pi(crate::agent_model::Profile),
    Codex(crate::codex_model::Profile),
    Opencode(crate::opencode_model::Profile),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Selection {
    Unmanaged,
    Configured { profile: Profile },
    Removed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub request: String,
    pub expected_revision: u64,
    pub digest: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Slot {
    pub revision: u64,
    pub selection: Selection,
    pub last_change: Option<Receipt>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub slots: BTreeMap<Kind, Slot>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            slots: [Kind::Pi, Kind::Codex, Kind::Opencode]
                .into_iter()
                .map(|k| {
                    (
                        k,
                        Slot {
                            revision: 0,
                            selection: Selection::Unmanaged,
                            last_change: None,
                        },
                    )
                })
                .collect(),
        }
    }
}
impl Settings {
    pub(super) fn slot(&self, k: Kind) -> &Slot {
        self.slots.get(&k).expect("all agent slots exist")
    }
    pub(super) fn validate(&self) -> Result<()> {
        if self.slots.len() != 3 {
            return Err("invalid agent settings slots".into());
        }
        for k in [Kind::Pi, Kind::Codex, Kind::Opencode] {
            let s = self.slot(k);
            let configured = !matches!(s.selection, Selection::Unmanaged);
            if (s.revision == 0) != !configured || (s.revision == 0) != s.last_change.is_none() {
                return Err("invalid agent settings revision".into());
            }
            if let Selection::Configured { profile } = &s.selection {
                if profile.kind() != k {
                    return Err("agent profile kind mismatch".into());
                }
                profile.validate()?;
            }
            if let Some(r) = &s.last_change {
                if !valid_request_id(&r.request)
                    || r.digest.len() != 64
                    || !r
                        .digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    || r.expected_revision.checked_add(1) != Some(s.revision)
                {
                    return Err("invalid agent change receipt".into());
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Runtime {
    pub states: BTreeMap<Kind, &'static str>,
}
impl Runtime {
    pub fn public(&self, s: &Settings, busy: bool) -> Value {
        let agents: Vec<_> = [Kind::Pi, Kind::Codex, Kind::Opencode]
            .into_iter()
            .map(|agent| {
                let slot = s.slot(agent);
                let (state, profile) = match &slot.selection {
                    Selection::Unmanaged => ("unmanaged", None),
                    Selection::Removed => ("removed", None),
                    Selection::Configured { profile } => (
                        self.states.get(&agent).copied().unwrap_or("unavailable"),
                        Some(profile),
                    ),
                };
                json!({"agent":agent,"revision":slot.revision,"state":state,"profile":profile,
                    "last_request":slot.last_change.as_ref().map(|r|&r.request),
                    "last_digest":slot.last_change.as_ref().map(|r|&r.digest)})
            })
            .collect();
        json!({"busy":busy,"agents":agents})
    }
}
impl Owner {
    fn admit_agent(&self, id: &str, k: Kind, expected: u64, digest: &str) -> Result<bool> {
        let slot = self.state.agent_settings.slot(k);
        if let Some(r) = &slot.last_change {
            if r.request == id {
                if r.expected_revision == expected && r.digest == digest {
                    return Ok(false);
                }
                return Err("agent request identity already used".into());
            }
        }
        if self.operation.is_some() || self.settings.job.is_some() {
            return Err("agent settings are busy; refresh the view before retrying".into());
        }
        if slot.revision != expected {
            return Err("agent settings changed; refresh the view before retrying".into());
        }
        Ok(true)
    }
    pub(crate) fn agent_save(&mut self, id: &str, expected: u64, profile: Profile) -> Result<()> {
        profile.validate()?;
        let k = profile.kind();
        let digest = crate::agent_model::digest(
            &json!({"op":"agent_save","agent":k,"expected_revision":expected,"profile":profile}),
        )?;
        if !self.admit_agent(id, k, expected, &digest)? {
            return self.reply_view(Some(id), "view", true);
        }
        let profile = profile.resolve()?;
        let mut next = self.state.clone();
        let slot = next.agent_settings.slots.get_mut(&k).unwrap();
        slot.revision = expected
            .checked_add(1)
            .ok_or("agent revision capacity reached")?;
        slot.selection = Selection::Configured { profile };
        slot.last_change = Some(Receipt {
            request: id.into(),
            expected_revision: expected,
            digest,
        });
        self.persist(next)?;
        self.refresh_agent_runtime(k);
        self.publish()?;
        self.reply_view(Some(id), "view", true)
    }
    pub(crate) fn agent_remove(&mut self, id: &str, k: Kind, expected: u64) -> Result<()> {
        let digest = crate::agent_model::digest(
            &json!({"op":"agent_remove","agent":k,"expected_revision":expected}),
        )?;
        if !self.admit_agent(id, k, expected, &digest)? {
            return self.reply_view(Some(id), "view", true);
        }
        let mut next = self.state.clone();
        let slot = next.agent_settings.slots.get_mut(&k).unwrap();
        slot.revision = expected
            .checked_add(1)
            .ok_or("agent revision capacity reached")?;
        slot.selection = Selection::Removed;
        slot.last_change = Some(Receipt {
            request: id.into(),
            expected_revision: expected,
            digest,
        });
        self.persist(next)?;
        self.set_agent_profile(k, None);
        self.agent_runtime.states.remove(&k);
        self.publish()?;
        self.reply_view(Some(id), "view", true)
    }
    pub(crate) fn agent_refresh(&mut self, id: &str) -> Result<()> {
        if self.operation.is_some() || self.settings.job.is_some() {
            return Err("agent settings are busy".into());
        }
        self.persist(self.state.clone())?;
        self.agent_runtime.states.clear();
        for k in [Kind::Pi, Kind::Codex, Kind::Opencode] {
            self.refresh_agent_runtime(k)
        }
        self.publish()?;
        self.reply_view(Some(id), "view", true)
    }
    fn set_agent_profile(&mut self, k: Kind, p: Option<NativeProfile>) {
        match k {
            Kind::Pi => {
                self.pi_profile = p.and_then(|p| {
                    if let NativeProfile::Pi(v) = p {
                        Some(v)
                    } else {
                        None
                    }
                })
            }
            Kind::Codex => {
                self.codex_profile = p.and_then(|p| {
                    if let NativeProfile::Codex(v) = p {
                        Some(v)
                    } else {
                        None
                    }
                })
            }
            Kind::Opencode => {
                self.opencode_profile = p.and_then(|p| {
                    if let NativeProfile::Opencode(v) = p {
                        Some(v)
                    } else {
                        None
                    }
                })
            }
        }
    }
    pub(crate) fn refresh_agent_runtime(&mut self, k: Kind) {
        match &self.state.agent_settings.slot(k).selection {
            Selection::Configured { profile } => match profile.native() {
                Ok(p) => {
                    self.set_agent_profile(k, Some(p));
                    self.agent_runtime.states.insert(k, "ready");
                }
                Err(_) => {
                    self.set_agent_profile(k, None);
                    self.agent_runtime.states.insert(k, "unavailable");
                }
            },
            Selection::Removed => {
                self.set_agent_profile(k, None);
                self.agent_runtime.states.insert(k, "removed");
            }
            Selection::Unmanaged => {
                self.agent_runtime.states.insert(k, "unmanaged");
            }
        }
    }

    pub(crate) fn prepare_agent(&mut self, kind: Kind) -> Result<()> {
        if matches!(
            self.state.agent_settings.slot(kind).selection,
            Selection::Configured { .. }
        ) {
            let previous = self.agent_runtime.states.get(&kind).copied();
            self.refresh_agent_runtime(kind);
            if previous != self.agent_runtime.states.get(&kind).copied() {
                self.persist(self.state.clone())?;
                self.publish()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use crate::{flow::Owner, model::Store, store::Disk};
    use std::{
        fs,
        sync::mpsc,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn make_owner(disk: Disk, state: Store) -> Owner {
        let (transport, _) = crate::transport::Transport::captured();
        let (tx, _) = mpsc::sync_channel(8);
        Owner {
            transport,
            disk,
            settings: Default::default(),
            agent_runtime: Default::default(),
            state,
            fixture: None,
            subscribers: Default::default(),
            operation: None,
            tx,
            pi_profile: None,
            codex_profile: None,
            opencode_profile: None,
            native_permissions: Default::default(),
            pi_ui: None,
            codex_input: None,
            browser_approval: None,
            browser_io: None,
            browser_route: None,
            responses_route: None,
            google_route: None,
            chat_route: None,
        }
    }

    #[test]
    fn save_receipt_cas_busy_refusal_and_restart() {
        let root = std::env::temp_dir().join(format!(
            "metldr-agent-settings-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let state_dir = root.join("state");
        let (disk, state) = Disk::open(state_dir.clone()).unwrap();
        let mut owner = make_owner(disk, state);
        let profile = Profile::Codex {
            executable: "/bin/sh".into(),
            workspace: root.clone(),
        };
        owner.operation = Some(crate::flow::Operation {
            run: "active".into(),
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            worker: std::thread::spawn(|| {}),
        });
        assert!(
            owner
                .agent_save("blocked-task", 0, profile.clone())
                .is_err()
        );
        owner.operation.take().unwrap().worker.join().unwrap();
        owner.settings.job = Some(std::thread::spawn(|| {}));
        assert!(
            owner
                .agent_save("blocked-provider", 0, profile.clone())
                .is_err()
        );
        owner.settings.job.take().unwrap().join().unwrap();

        let missing = Profile::Codex {
            executable: "/path/that/does/not/exist".into(),
            workspace: root.clone(),
        };
        owner
            .request(crate::model::Request {
                v: crate::WIRE_VERSION,
                request: "missing".into(),
                command: crate::model::Command::AgentSave {
                    expected_revision: 0,
                    profile: missing,
                },
            })
            .expect("invalid user paths must return an error receipt without terminating the host");
        assert_eq!(owner.state.agent_settings.slot(Kind::Codex).revision, 0);

        owner.agent_save("save:1", 0, profile.clone()).unwrap();
        assert!(owner.operation.is_none());
        assert!(owner.state.view.runs.is_empty());
        let sequence = owner.state.view.sequence;
        owner.agent_save("save:1", 0, profile.clone()).unwrap();
        assert_eq!(owner.state.view.sequence, sequence);
        let changed = Profile::Codex {
            executable: "/bin/sh".into(),
            workspace: "/tmp".into(),
        };
        assert!(owner.agent_save("save:1", 0, changed).is_err());
        assert!(owner.agent_save("stale:1", 0, profile).is_err());
        drop(owner);

        let (disk, state) = Disk::open(state_dir.clone()).unwrap();
        assert_eq!(state.agent_settings.slot(Kind::Codex).revision, 1);
        let mut owner = make_owner(disk, state);
        owner.agent_remove("remove:1", Kind::Codex, 1).unwrap();
        drop(owner);
        let (disk, state) = Disk::open(state_dir).unwrap();
        assert_eq!(
            state.agent_settings.slot(Kind::Codex).selection,
            Selection::Removed
        );
        assert_eq!(state.agent_settings.slot(Kind::Codex).revision, 2);
        drop(disk);
        fs::remove_dir_all(root).unwrap();
    }
}
