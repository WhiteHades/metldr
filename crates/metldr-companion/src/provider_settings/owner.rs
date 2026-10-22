use super::{Connection, KeyChange, Pending, Protocol, Selection, Slot};
use crate::{Result, flow::Owner, https_profile::Profile};
use metldr_native_credentials::CredentialId;
use serde_json::{Value, json};
use std::{collections::BTreeMap, thread::JoinHandle};

#[derive(Default)]
pub(crate) struct Runtime {
    pub job: Option<JoinHandle<()>>,
    pub started: Option<std::time::Instant>,
    pub states: BTreeMap<Protocol, &'static str>,
    pub notice: Option<&'static str>,
}

pub(crate) enum Completion {
    Saved {
        pending: Pending,
        result: std::result::Result<Profile, &'static str>,
    },
    Loaded {
        request: Option<String>,
        profiles: Vec<(Protocol, std::result::Result<Profile, &'static str>)>,
    },
    Cleaned {
        removed: Vec<String>,
        failed: bool,
    },
}

impl Runtime {
    pub fn check_deadline(&self) -> Result<()> {
        if self.job.is_some()
            && self
                .started
                .is_some_and(|start| start.elapsed() >= std::time::Duration::from_secs(120))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "provider settings worker timed out; restart to recover",
            )
            .into());
        }
        Ok(())
    }

    pub fn public(&self, settings: &super::Settings) -> Value {
        let connections: Vec<_> = Protocol::ALL.into_iter().map(|protocol| {
            let slot = settings.slot(protocol);
            let (state, endpoint, model) = match &slot.selection {
                Selection::Unmanaged => ("unmanaged", None, None),
                Selection::Removed => ("removed", None, None),
                Selection::Configured { connection } => (
                    self.states.get(&protocol).copied().unwrap_or("loading"),
                    Some(connection.endpoint.as_str()), Some(connection.model.as_str()),
                ),
            };
            json!({"protocol":protocol,"revision":slot.revision,"state":state,"endpoint":endpoint,"model":model})
        }).collect();
        json!({"busy":self.job.is_some(),"pending":settings.pending.as_ref().map(|p|p.protocol),
            "cleanup_pending":!settings.retired.is_empty(),"notice":self.notice,"connections":connections})
    }
}

impl Owner {
    pub(crate) fn provider_route(&self, protocol: Protocol) -> Option<&Profile> {
        match protocol {
            Protocol::Messages => self.browser_route.as_ref(),
            Protocol::Responses => self.responses_route.as_ref(),
            Protocol::GoogleInteractions => self.google_route.as_ref(),
            Protocol::ChatCompletions => self.chat_route.as_ref(),
        }
    }

    pub(crate) fn set_provider_route(&mut self, protocol: Protocol, profile: Option<Profile>) {
        *match protocol {
            Protocol::Messages => &mut self.browser_route,
            Protocol::Responses => &mut self.responses_route,
            Protocol::GoogleInteractions => &mut self.google_route,
            Protocol::ChatCompletions => &mut self.chat_route,
        } = profile;
    }

    fn provider_admit(&mut self, request: &str, protocol: Protocol, revision: u64) -> Result<bool> {
        self.refresh_saved_settings()?;
        let slot = self.state.providers.slot(protocol);
        if slot.last_request.as_deref() == Some(request) {
            self.reply_view(Some(request), "view", true)?;
            return Ok(false);
        }
        if self.settings.job.is_some() || self.state.providers.pending.is_some() {
            return Err("provider settings are busy; refresh the view before retrying".into());
        }
        if slot.revision != revision {
            return Err("provider connection changed; refresh the view before saving".into());
        }
        if self.state.providers.retired.len() >= super::model::MAX_RETIRED {
            return Err("provider cleanup capacity reached; refresh to retry cleanup".into());
        }
        if self
            .state
            .providers
            .slots
            .values()
            .any(|slot| slot.last_request.as_deref() == Some(request))
        {
            return Err("provider request identity already used".into());
        }
        Ok(true)
    }

    pub(crate) fn provider_save(
        &mut self,
        request: &str,
        protocol: Protocol,
        expected: u64,
        endpoint: String,
        model: String,
        key: KeyChange,
    ) -> Result<()> {
        if !self.provider_admit(request, protocol, expected)? {
            return Ok(());
        }
        let slot = self.state.providers.slot(protocol);
        let previous = match &slot.selection {
            Selection::Configured { connection } => Some(connection.credential.clone()),
            _ => None,
        };
        if matches!(key, KeyChange::Keep) && previous.is_none() {
            return Err("a new connection requires an API key".into());
        }
        let revision = expected
            .checked_add(1)
            .ok_or("provider revision capacity reached")?;
        let mut next = self.state.clone();
        let scope = match &next.providers.scope {
            Some(scope) => scope.clone(),
            None => CredentialId::generate()?.as_str().to_owned(),
        };
        next.providers.scope = Some(scope.clone());
        let trust_pem = match &slot.selection {
            Selection::Configured { connection } => connection.trust_pem.clone(),
            _ => self
                .provider_route(protocol)
                .and_then(Profile::trust_pem)
                .map(String::from_utf8)
                .transpose()
                .map_err(|_| "provider trust certificate must be UTF-8 PEM")?,
        };
        let connection = Connection {
            endpoint,
            model,
            credential: CredentialId::generate()?.as_str().to_owned(),
            trust_pem,
        };
        connection.profile(protocol, &scope, revision)?;
        let pending = Pending {
            request: request.into(),
            protocol,
            revision,
            connection,
        };
        next.providers.pending = Some(pending.clone());
        self.persist_settings(next)?;
        self.settings.notice = None;
        self.settings.started = Some(std::time::Instant::now());
        self.settings.job = Some(super::worker::save(
            self.disk.lease(),
            self.tx.clone(),
            scope,
            pending,
            previous,
            key,
        ));
        self.publish()
    }

    pub(crate) fn provider_remove(
        &mut self,
        request: &str,
        protocol: Protocol,
        expected: u64,
    ) -> Result<()> {
        if !self.provider_admit(request, protocol, expected)? {
            return Ok(());
        }
        let slot = self.state.providers.slot(protocol);
        let mut next = self.state.clone();
        if let Selection::Configured { connection } = slot.selection {
            next.providers.retired.push(connection.credential);
        }
        next.providers.slots.insert(
            protocol,
            Slot {
                revision: expected
                    .checked_add(1)
                    .ok_or("provider revision capacity reached")?,
                selection: Selection::Removed,
                last_request: Some(request.into()),
            },
        );
        self.persist_settings(next)?;
        self.set_provider_route(protocol, None);
        self.settings.notice = None;
        self.settings.states.remove(&protocol);
        self.provider_cleanup()?;
        self.publish()?;
        self.reply_view(Some(request), "view", true)
    }

    pub(crate) fn provider_load(&mut self, request: Option<String>) -> Result<()> {
        self.refresh_saved_settings()?;
        if self.settings.job.is_some() {
            return Err("provider settings are busy".into());
        }
        self.persist_settings(self.state.clone())?;
        for protocol in Protocol::ALL {
            if self.state.providers.managed(protocol) {
                self.set_provider_route(protocol, None);
                self.settings.states.insert(protocol, "loading");
            }
        }
        self.settings.notice = None;
        self.settings.started = Some(std::time::Instant::now());
        self.settings.job = Some(super::worker::load(
            self.disk.lease(),
            self.tx.clone(),
            self.state.providers.clone(),
            request,
        ));
        self.publish()
    }

    fn provider_cleanup(&mut self) -> Result<()> {
        if self.state.providers.retired.is_empty() {
            return Ok(());
        }
        if self.settings.job.is_some() || self.state.providers.pending.is_some() {
            return Err("provider cleanup cannot overlap a write".into());
        }
        let scope = self
            .state
            .providers
            .scope
            .clone()
            .ok_or("provider cleanup scope missing")?;
        self.settings.started = Some(std::time::Instant::now());
        self.settings.job = Some(super::worker::cleanup(
            self.disk.lease(),
            self.tx.clone(),
            scope,
            self.state.providers.retired.clone(),
        ));
        Ok(())
    }

    pub(crate) fn provider_completed(&mut self, completion: Completion) -> Result<()> {
        self.refresh_saved_settings()?;
        self.settings
            .job
            .take()
            .ok_or("provider completion without worker")?
            .join()
            .map_err(|_| "provider settings worker panicked")?;
        self.settings.started = None;
        match completion {
            Completion::Saved { pending, result } => {
                let current = self
                    .state
                    .providers
                    .pending
                    .as_ref()
                    .ok_or("provider transaction missing")?;
                if current.request != pending.request
                    || current.connection.credential != pending.connection.credential
                {
                    return Err("provider transaction completion mismatch".into());
                }
                let mut next = self.state.clone();
                next.providers.pending = None;
                let (profile, error) = match result {
                    Ok(profile) => {
                        if let Selection::Configured { connection } =
                            next.providers.slot(pending.protocol).selection
                        {
                            next.providers.retired.push(connection.credential);
                        }
                        next.providers.slots.insert(
                            pending.protocol,
                            Slot {
                                revision: pending.revision,
                                selection: Selection::Configured {
                                    connection: pending.connection,
                                },
                                last_request: Some(pending.request.clone()),
                            },
                        );
                        (Some(profile), None)
                    }
                    Err(error) => {
                        next.providers.retired.push(pending.connection.credential);
                        (None, Some(error))
                    }
                };
                self.persist_settings(next)?;
                if let Some(profile) = profile {
                    self.set_provider_route(pending.protocol, Some(profile));
                    self.settings.states.insert(pending.protocol, "ready");
                }
                self.settings.notice = error;
                self.provider_cleanup()?;
                self.publish()?;
                if let Some(error) = error {
                    self.emit(json!({"v":crate::WIRE_VERSION,"request":pending.request,"kind":"error","message":error}))
                } else {
                    self.reply_view(Some(&pending.request), "view", true)
                }
            }
            Completion::Loaded { request, profiles } => {
                self.persist_settings(self.state.clone())?;
                for (protocol, result) in profiles {
                    match result {
                        Ok(profile) => {
                            self.set_provider_route(protocol, Some(profile));
                            self.settings.states.insert(protocol, "ready");
                        }
                        Err(state) => {
                            self.set_provider_route(protocol, None);
                            self.settings.states.insert(protocol, state);
                        }
                    }
                }
                self.provider_cleanup()?;
                self.publish()?;
                if let Some(request) = request {
                    self.reply_view(Some(&request), "view", true)?;
                }
                Ok(())
            }
            Completion::Cleaned { removed, failed } => {
                let mut next = self.state.clone();
                next.providers.retired.retain(|id| !removed.contains(id));
                self.persist_settings(next)?;
                if failed {
                    self.settings.notice = Some("Credential cleanup is pending. Refresh to retry.");
                }
                self.publish()
            }
        }
    }
}
