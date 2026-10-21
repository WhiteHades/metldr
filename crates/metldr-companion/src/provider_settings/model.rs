use crate::{Result, https_profile::Profile, model::valid_request_id};
use metldr_native_credentials::{CredentialId, Key};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MAX_RETIRED: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Protocol {
    Messages,
    Responses,
    GoogleInteractions,
    ChatCompletions,
}

impl Protocol {
    pub const ALL: [Self; 4] = [
        Self::Messages,
        Self::Responses,
        Self::GoogleInteractions,
        Self::ChatCompletions,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Responses => "responses",
            Self::GoogleInteractions => "google_interactions",
            Self::ChatCompletions => "chat_completions",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub scope: Option<String>,
    pub slots: BTreeMap<Protocol, Slot>,
    pub pending: Option<Pending>,
    pub retired: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Slot {
    pub revision: u64,
    pub selection: Selection,
    pub last_request: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Selection {
    #[default]
    Unmanaged,
    Configured {
        connection: Connection,
    },
    Removed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Connection {
    pub endpoint: String,
    pub model: String,
    pub credential: String,
    pub trust_pem: Option<String>,
}

impl Connection {
    pub fn profile(&self, protocol: Protocol, scope: &str, revision: u64) -> Result<Profile> {
        Profile::from_keyring(
            &self.endpoint,
            &self.model,
            revision,
            CredentialId::parse(scope)?,
            CredentialId::parse(&self.credential)?,
            self.trust_pem.as_ref().map(|pem| pem.as_bytes().to_vec()),
            protocol.name(),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pending {
    pub request: String,
    pub protocol: Protocol,
    pub revision: u64,
    pub connection: Connection,
}

pub(crate) struct SecretInput(pub Key);

impl<'de> Deserialize<'de> for SecretInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Key::parse(raw)
            .map(Self)
            .map_err(|_| serde::de::Error::custom("invalid provider key"))
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum KeyChange {
    Keep,
    Replace { value: SecretInput },
}

impl Settings {
    pub fn slot(&self, protocol: Protocol) -> Slot {
        self.slots.get(&protocol).cloned().unwrap_or_default()
    }

    pub fn managed(&self, protocol: Protocol) -> bool {
        !matches!(self.slot(protocol).selection, Selection::Unmanaged)
    }

    pub fn validate(&self) -> Result<()> {
        if self.slots.len() > Protocol::ALL.len()
            || self.retired.len() + usize::from(self.pending.is_some()) > MAX_RETIRED
        {
            return Err("provider settings capacity exceeded".into());
        }
        if let Some(scope) = &self.scope {
            CredentialId::parse(scope)?;
        }
        let mut referenced = BTreeSet::new();
        for (&protocol, slot) in &self.slots {
            if slot
                .last_request
                .as_deref()
                .is_some_and(|request| !valid_request_id(request))
            {
                return Err("invalid provider request identity".into());
            }
            match &slot.selection {
                Selection::Unmanaged if slot.revision == 0 && slot.last_request.is_none() => (),
                Selection::Configured { connection }
                    if slot.revision > 0 && slot.last_request.is_some() =>
                {
                    connection.profile(
                        protocol,
                        self.scope.as_deref().ok_or("provider scope missing")?,
                        slot.revision,
                    )?;
                    if !referenced.insert(&connection.credential) {
                        return Err("shared provider credential revision".into());
                    }
                }
                Selection::Removed if slot.revision > 0 && slot.last_request.is_some() => (),
                _ => return Err("invalid provider selection revision".into()),
            }
        }
        if let Some(pending) = &self.pending {
            if !valid_request_id(&pending.request)
                || self.slot(pending.protocol).revision.checked_add(1) != Some(pending.revision)
                || !referenced.insert(&pending.connection.credential)
            {
                return Err("invalid pending provider transaction".into());
            }
            pending.connection.profile(
                pending.protocol,
                self.scope.as_deref().ok_or("provider scope missing")?,
                pending.revision,
            )?;
        }
        if !self.retired.is_empty() && self.scope.is_none() {
            return Err("retired provider scope missing".into());
        }
        for id in &self.retired {
            CredentialId::parse(id)?;
            if !referenced.insert(id) {
                return Err("invalid retired provider revision".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn settings_bound_pending_and_retired_revisions_and_redact_bad_input() {
        let scope = "a".repeat(32);
        let connection = Connection {
            endpoint: "https://example.com/v1/messages".into(),
            model: "test-model".into(),
            credential: "b".repeat(32),
            trust_pem: None,
        };
        let mut settings = Settings {
            scope: Some(scope),
            ..Settings::default()
        };
        settings.pending = Some(Pending {
            request: "save:1".into(),
            protocol: Protocol::Messages,
            revision: 1,
            connection: connection.clone(),
        });
        settings.validate().unwrap();
        settings.retired.push(connection.credential.clone());
        assert!(settings.validate().is_err());
        settings.retired.clear();
        settings.slots.insert(
            Protocol::Messages,
            Slot {
                revision: 1,
                selection: Selection::Configured { connection },
                last_request: Some("save:1".into()),
            },
        );
        assert!(settings.validate().is_err());
        settings.pending = None;
        settings.validate().unwrap();
        let invalid = br#"{"v":6,"request":"save:2","command":{"op":"provider_save","protocol":"messages","expected_revision":1,"endpoint":"https://example.com","model":"test","key":{"kind":"replace","value":"secret with space"}}}"#;
        let error = crate::wire::decode_request(invalid)
            .err()
            .unwrap()
            .to_string();
        assert_eq!(error, "invalid native request");
        assert!(!error.contains("secret"));
        let mut runtime = crate::provider_settings::Runtime {
            job: Some(std::thread::spawn(|| {})),
            started: Some(std::time::Instant::now() - std::time::Duration::from_secs(121)),
            ..Default::default()
        };
        assert!(runtime.check_deadline().is_err());
        runtime.job.take().unwrap().join().unwrap();
    }
}
