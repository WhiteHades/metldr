use crate::{CONFIG_MAX, Result};
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BridgeConfig {
    pub(super) socket: PathBuf,
    pub(super) capability: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BootstrapConfig {
    state: PathBuf,
    #[serde(default)]
    origin: Option<String>,
    extension_origin: String,
    #[serde(default)]
    pi_profile: Option<PathBuf>,
    #[serde(default)]
    codex_profile: Option<PathBuf>,
    #[serde(default)]
    opencode_profile: Option<PathBuf>,
    #[serde(default)]
    browser_route: Option<crate::https_profile::Config>,
    #[serde(default)]
    responses_route: Option<crate::https_profile::Config>,
    #[serde(default)]
    google_route: Option<crate::https_profile::Config>,
    #[serde(default)]
    chat_route: Option<crate::https_profile::Config>,
    #[serde(default)]
    bridge: Option<BridgeConfig>,
}

#[derive(Clone)]
pub(crate) struct Fixture {
    pub(crate) origin: String,
    pub(crate) address: SocketAddr,
}

pub(super) enum Mode {
    Stdio,
    SocketOwner,
    Bridge,
}

pub(super) struct Config {
    pub(super) state: PathBuf,
    pub(super) fixture: Option<Fixture>,
    pub(super) extension_origin: String,
    pub(super) bridge: Option<BridgeConfig>,
    pub(super) mode: Mode,
    pi_profile: Option<PathBuf>,
    codex_profile: Option<PathBuf>,
    opencode_profile: Option<PathBuf>,
    pub(super) browser_route: Option<crate::https_profile::Config>,
    pub(super) responses_route: Option<crate::https_profile::Config>,
    pub(super) google_route: Option<crate::https_profile::Config>,
    pub(super) chat_route: Option<crate::https_profile::Config>,
}

pub(crate) fn parse_bootstrap_config(bytes: &[u8]) -> Result<BootstrapConfig> {
    if bytes.len() > CONFIG_MAX {
        return Err("bootstrap config exceeds bound".into());
    }
    Ok(serde_json::from_slice(bytes)?)
}

pub(crate) fn valid_parent_window_arg(args: &[String]) -> bool {
    match args {
        [] => true,
        [arg] => arg
            .strip_prefix("--parent-window=")
            .is_some_and(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())),
        _ => false,
    }
}

pub(super) fn valid_extension_origin(origin: &str) -> bool {
    origin
        .strip_prefix("chrome-extension://")
        .and_then(|s| s.strip_suffix('/'))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((CONFIG_MAX + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > CONFIG_MAX {
        return Err("configuration exceeds bound".into());
    }
    Ok(bytes)
}

impl Config {
    pub(super) fn load() -> Result<Self> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let (bootstrap, mode) = match args.as_slice() {
            [flag, path] if flag == "--owner-config" => (
                parse_bootstrap_config(&read_bounded(Path::new(path))?)?,
                Mode::SocketOwner,
            ),
            [actual, tail @ ..] if actual.starts_with("chrome-extension://") => {
                if !valid_parent_window_arg(tail) {
                    return Err("unexpected Chrome native host arguments".into());
                }
                let config = match std::env::var_os("METLDR_FIRST_FLOW_CONFIG") {
                    Some(path) => parse_bootstrap_config(&read_bounded(Path::new(&path))?)?,
                    None => BootstrapConfig {
                        state: super::installation::select_state(actual)?,
                        extension_origin: actual.clone(),
                        origin: None,
                        pi_profile: None,
                        codex_profile: None,
                        opencode_profile: None,
                        bridge: None,
                        browser_route: None,
                        responses_route: None,
                        google_route: None,
                        chat_route: None,
                    },
                };
                if actual != &config.extension_origin {
                    return Err("extension origin not authorized".into());
                }
                let mode = if config.bridge.is_some() {
                    Mode::Bridge
                } else {
                    Mode::Stdio
                };
                (config, mode)
            }
            [
                state_flag,
                state,
                origin_flag,
                origin,
                extension_flag,
                expected,
                actual,
                tail @ ..,
            ] if state_flag == "--state"
                && origin_flag == "--origin"
                && extension_flag == "--extension-origin" =>
            {
                if expected != actual {
                    return Err("extension origin not authorized".into());
                }
                let pi_profile = match tail {
                    [flag, path, rest @ ..]
                        if flag == "--pi-profile" && valid_parent_window_arg(rest) =>
                    {
                        Some(PathBuf::from(path))
                    }
                    rest if valid_parent_window_arg(rest) => None,
                    _ => return Err("unexpected native host arguments".into()),
                };
                (
                    BootstrapConfig {
                        state: state.into(),
                        origin: Some(origin.clone()),
                        extension_origin: expected.clone(),
                        pi_profile,
                        codex_profile: None,
                        opencode_profile: None,
                        bridge: None,
                        browser_route: None,
                        responses_route: None,
                        google_route: None,
                        chat_route: None,
                    },
                    Mode::Stdio,
                )
            }
            _ => {
                return Err(
                    "expected owned state, loopback origin and extension origin arguments".into(),
                );
            }
        };
        let fixture = bootstrap
            .origin
            .as_deref()
            .map(|origin| -> Result<Fixture> {
                let url = reqwest::Url::parse(origin).map_err(|_| "invalid fixture origin")?;
                let address: SocketAddr = origin
                    .strip_prefix("http://")
                    .ok_or("fixture must use loopback HTTP")?
                    .parse()?;
                if !address.ip().is_loopback()
                    || !address.is_ipv4()
                    || address.port() == 0
                    || origin != format!("http://{address}")
                    || url.path() != "/"
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err("fixture must use IPv4 loopback".into());
                }
                Ok(Fixture {
                    origin: origin.to_owned(),
                    address,
                })
            })
            .transpose()?;
        if !valid_extension_origin(&bootstrap.extension_origin) {
            return Err("extension origin not authorized".into());
        }
        if let Some(bridge) = &bootstrap.bridge {
            bridge.validate(&bootstrap.state)?;
        }
        if matches!(mode, Mode::SocketOwner) && bootstrap.bridge.is_none() {
            return Err("owner requires bridge configuration".into());
        }
        Ok(Self {
            state: bootstrap.state,
            fixture,
            extension_origin: bootstrap.extension_origin,
            bridge: bootstrap.bridge,
            mode,
            pi_profile: bootstrap.pi_profile,
            codex_profile: bootstrap.codex_profile,
            opencode_profile: bootstrap.opencode_profile,
            browser_route: bootstrap.browser_route,
            google_route: bootstrap.google_route,
            chat_route: bootstrap.chat_route,
            responses_route: bootstrap.responses_route,
        })
    }

    pub(super) fn profile(&self) -> Result<Option<crate::agent_model::Profile>> {
        let Some(path) = &self.pi_profile else {
            return Ok(None);
        };
        let mut profile: crate::agent_model::Profile =
            serde_json::from_slice(&read_bounded(path)?)?;
        profile.validate()?;
        profile.executable = profile.executable.canonicalize()?;
        profile.workspace = profile.workspace.canonicalize()?;
        std::fs::create_dir_all(&profile.session_dir)?;
        profile.session_dir = profile.session_dir.canonicalize()?;
        if !profile.executable.is_file() || !profile.workspace.is_dir() {
            return Err("Pi profile executable or workspace is unavailable".into());
        }
        Ok(Some(profile))
    }

    pub(super) fn codex_profile(&self) -> Result<Option<crate::codex_model::Profile>> {
        let Some(path) = &self.codex_profile else {
            return Ok(None);
        };
        let mut profile: crate::codex_model::Profile =
            serde_json::from_slice(&read_bounded(path)?)?;
        profile.validate()?;
        profile.executable = profile.executable.canonicalize()?;
        profile.workspace = profile.workspace.canonicalize()?;
        if !profile.executable.is_file() || !profile.workspace.is_dir() {
            return Err("Codex profile executable or workspace is unavailable".into());
        }
        Ok(Some(profile))
    }

    pub(super) fn opencode_profile(&self) -> Result<Option<crate::opencode_model::Profile>> {
        let Some(path) = &self.opencode_profile else {
            return Ok(None);
        };
        let mut profile: crate::opencode_model::Profile =
            serde_json::from_slice(&read_bounded(path)?)?;
        profile.validate()?;
        profile.executable = profile.executable.canonicalize()?;
        profile.workspace = profile.workspace.canonicalize()?;
        if !profile.executable.is_file() || !profile.workspace.is_dir() {
            return Err("OpenCode profile executable or workspace is unavailable".into());
        }
        Ok(Some(profile))
    }
}

impl BridgeConfig {
    fn validate(&self, state: &Path) -> Result<()> {
        if !state.is_absolute()
            || !self.socket.is_absolute()
            || self.socket.as_os_str().len() > 100
            || self.socket.file_name().is_none()
            || self.capability.len() != 64
            || !self
                .capability
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid bridge configuration".into());
        }
        let parent = self
            .socket
            .parent()
            .ok_or("bridge socket parent absent")?
            .canonicalize()?;
        if state
            .parent()
            .ok_or("state parent absent")?
            .canonicalize()?
            != parent
        {
            return Err("bridge socket is outside state parent".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(parent)?.permissions().mode() & 0o077 != 0 {
                return Err("bridge parent is not private".into());
            }
        }
        Ok(())
    }
}
