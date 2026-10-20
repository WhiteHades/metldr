use crate::{Result, agent_model::digest};
use metldr_native_credentials::{CredentialId, Vault};
use reqwest::{Certificate, Url, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fs::File, io::Read, net::SocketAddr, path::PathBuf};

pub(crate) const FIXTURE_MODEL: &str = "fixture-browser-model";

#[cfg(test)]
pub(crate) mod transport_checks;
#[cfg(all(test, target_os = "linux"))]
mod vault_checks;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Config {
    Fixture,
    Https {
        endpoint: String,
        model: String,
        revision: u64,
        credential_env: String,
        trust_ca: Option<PathBuf>,
    },
    Keyring {
        endpoint: String,
        model: String,
        revision: u64,
        scope: String,
        credential: String,
        trust_ca: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub endpoint: String,
    pub model: String,
    pub revision: u64,
    pub fingerprint: String,
}

impl Binding {
    pub fn validate(&self) -> Result<()> {
        validate_endpoint(&self.endpoint)?;
        if self.revision == 0
            || self.model.is_empty()
            || self.model.len() > 128
            || !self.model.bytes().all(|b| b.is_ascii_graphic())
            || self.fingerprint.len() != 64
            || !self.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid HTTPS provider binding".into());
        }
        Ok(())
    }

    pub fn public_google(&self) -> serde_json::Value {
        let mut value = self.public();
        value["kind"] = json!("https_google_interactions");
        value
    }

    pub fn public_responses(&self) -> serde_json::Value {
        let mut value = self.public();
        value["kind"] = json!("https_responses");
        value
    }

    pub fn public_chat_completions(&self) -> serde_json::Value {
        let mut value = self.public();
        value["kind"] = json!("https_chat_completions");
        value
    }

    pub fn public(&self) -> serde_json::Value {
        json!({"kind":"https_messages", "endpoint":self.endpoint, "model":self.model,
            "revision":self.revision, "fingerprint":self.fingerprint})
    }
}

#[derive(Clone)]
pub(crate) enum Profile {
    Fixture {
        endpoint: String,
    },
    Https {
        binding: Binding,
        credential: Credential,
        trust_pem: Option<Vec<u8>>,
    },
}

#[derive(Clone)]
pub(crate) enum Credential {
    Environment(String),
    Pinned(std::sync::Arc<HeaderValue>),
    Keyring {
        scope: CredentialId,
        id: CredentialId,
    },
}

fn validate_endpoint(endpoint: &str) -> Result<Url> {
    let url = Url::parse(endpoint).map_err(|_| "invalid HTTPS provider endpoint")?;
    if endpoint.len() > 1024
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err("invalid HTTPS provider endpoint".into());
    }
    Ok(url)
}

impl Config {
    pub fn bind(self, address: Option<SocketAddr>) -> Result<Profile> {
        self.bind_protocol(address, "messages")
    }

    pub fn bind_responses(self, address: Option<SocketAddr>) -> Result<Profile> {
        if matches!(self, Self::Fixture) {
            return Err("Responses requires a configured HTTPS route".into());
        }
        self.bind_protocol(address, "responses")
    }

    pub fn bind_google(self, address: Option<SocketAddr>) -> Result<Profile> {
        if matches!(self, Self::Fixture) {
            return Err("Google requires a configured HTTPS route".into());
        }
        self.bind_protocol(address, "google_interactions")
    }

    pub fn bind_chat_completions(self, address: Option<SocketAddr>) -> Result<Profile> {
        if matches!(self, Self::Fixture) {
            return Err("Chat Completions requires a configured HTTPS route".into());
        }
        self.bind_protocol(address, "chat_completions")
    }

    fn bind_protocol(self, address: Option<SocketAddr>, protocol: &str) -> Result<Profile> {
        let (endpoint, model, revision, credential, trust_ca) = match self {
            Self::Fixture => {
                let address = address.ok_or("Messages fixture requires a configured fixture")?;
                if !matches!(address, SocketAddr::V4(a) if a.ip().is_loopback() && a.port() != 0) {
                    return Err("Messages fixture must use IPv4 loopback".into());
                }
                return Ok(Profile::Fixture {
                    endpoint: format!("https://{address}/v1/messages"),
                });
            }
            Self::Https {
                endpoint,
                model,
                revision,
                credential_env,
                trust_ca,
            } => {
                if credential_env.is_empty()
                    || credential_env.len() > 128
                    || !credential_env.bytes().enumerate().all(|(i, b)| {
                        b == b'_' || b.is_ascii_uppercase() || (i > 0 && b.is_ascii_digit())
                    })
                {
                    return Err("invalid native credential reference".into());
                }
                (
                    endpoint,
                    model,
                    revision,
                    Credential::Environment(credential_env),
                    trust_ca,
                )
            }
            Self::Keyring {
                endpoint,
                model,
                revision,
                scope,
                credential,
                trust_ca,
            } => (
                endpoint,
                model,
                revision,
                Credential::Keyring {
                    scope: CredentialId::parse(&scope)?,
                    id: CredentialId::parse(&credential)?,
                },
                trust_ca,
            ),
        };
        let pem = trust_ca
            .map(|path| -> Result<Vec<u8>> {
                let mut bytes = Vec::new();
                File::open(path)
                    .map_err(|_| "Provider trust certificate unavailable")?
                    .take(16_385)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Provider trust certificate unreadable")?;
                if bytes.len() > 16_384 {
                    return Err("Provider trust certificate exceeds bound".into());
                }
                Ok(bytes)
            })
            .transpose()?;
        bind_parts(endpoint, model, revision, credential, pem, protocol)
    }
}

fn bind_parts(
    endpoint: String,
    model: String,
    revision: u64,
    credential: Credential,
    pem: Option<Vec<u8>>,
    protocol: &str,
) -> Result<Profile> {
    let endpoint = validate_endpoint(&endpoint)?.to_string();
    if pem.as_ref().is_some_and(|bytes| bytes.len() > 16_384) {
        return Err("Provider trust certificate exceeds bound".into());
    }
    if let Some(pem) = &pem {
        let certificates =
            Certificate::from_pem_bundle(pem).map_err(|_| "invalid Provider trust certificate")?;
        if certificates.is_empty() {
            return Err("empty Provider trust certificate".into());
        }
    }
    let mut identity = json!({"protocol":protocol, "endpoint":endpoint,
                    "model":model,"revision":revision,"trust_pem":pem});
    match &credential {
        Credential::Pinned(_) => return Err("pinned credential cannot define a binding".into()),
        Credential::Environment(variable) => identity["credential_env"] = json!(variable),
        Credential::Keyring { scope, id } => {
            identity["credential_keyring"] = json!({"scope":scope.as_str(), "id":id.as_str()})
        }
    }
    let fingerprint = digest(&identity)?;
    let binding = Binding {
        endpoint,
        model,
        revision,
        fingerprint,
    };
    binding.validate()?;
    Ok(Profile::Https {
        binding,
        credential,
        trust_pem: pem,
    })
}

impl Profile {
    pub fn from_keyring(
        endpoint: &str,
        model: &str,
        revision: u64,
        scope: CredentialId,
        id: CredentialId,
        pem: Option<Vec<u8>>,
        protocol: &str,
    ) -> Result<Self> {
        bind_parts(
            endpoint.into(),
            model.into(),
            revision,
            Credential::Keyring { scope, id },
            pem,
            protocol,
        )
    }

    pub fn pin(mut self) -> Result<Self> {
        let header = self.credential()?;
        match &mut self {
            Self::Https { credential, .. } => {
                *credential = Credential::Pinned(std::sync::Arc::new(header))
            }
            Self::Fixture { .. } => return Err("fixture profile cannot be managed".into()),
        }
        Ok(self)
    }

    pub fn trust_pem(&self) -> Option<Vec<u8>> {
        match self {
            Self::Https { trust_pem, .. } => trust_pem.clone(),
            Self::Fixture { .. } => None,
        }
    }

    pub fn messages_client(
        &self,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<metldr_messages::Client> {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Messages request cancelled".into());
        }
        let (endpoint, model, pem) = match self {
            Self::Fixture { endpoint } => (
                endpoint.as_str(),
                FIXTURE_MODEL,
                Some(include_bytes!("../test-tls/ca.pem").as_slice()),
            ),
            Self::Https {
                binding, trust_pem, ..
            } => (
                binding.endpoint.as_str(),
                binding.model.as_str(),
                trust_pem.as_deref(),
            ),
        };
        let key = self.credential()?;
        let client = metldr_messages::Client::new(
            endpoint,
            model,
            key.to_str().map_err(|_| "invalid Messages credential")?,
            pem,
        )?;
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Messages request cancelled".into());
        }
        Ok(client)
    }

    pub fn responses_client(
        &self,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<metldr_responses::Client> {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Responses request cancelled".into());
        }
        let Self::Https {
            binding, trust_pem, ..
        } = self
        else {
            return Err("Responses requires HTTPS".into());
        };
        let key = self.credential()?;
        let client = metldr_responses::Client::new(
            &binding.endpoint,
            &binding.model,
            key.to_str().map_err(|_| "invalid Responses credential")?,
            trust_pem.as_deref(),
        )?;
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Responses request cancelled".into());
        }
        Ok(client)
    }

    pub fn google_client(
        &self,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<metldr_google_interactions::Client> {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Google Interactions request cancelled".into());
        }
        let Self::Https {
            binding, trust_pem, ..
        } = self
        else {
            return Err("Google Interactions requires HTTPS".into());
        };
        let key = self.credential()?;
        let client = metldr_google_interactions::Client::new(
            &binding.endpoint,
            &binding.model,
            key.to_str()
                .map_err(|_| "invalid Google Interactions credential")?,
            trust_pem.as_deref(),
        )?;
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Google Interactions request cancelled".into());
        }
        Ok(client)
    }

    pub fn chat_client(
        &self,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<metldr_chat_completions::Client> {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Chat request cancelled".into());
        }
        let Self::Https {
            binding, trust_pem, ..
        } = self
        else {
            return Err("Chat Completions requires HTTPS".into());
        };
        let key = self.credential()?;
        let client = metldr_chat_completions::Client::new(
            &binding.endpoint,
            &binding.model,
            key.to_str().map_err(|_| "invalid Chat credential")?,
            trust_pem.as_deref(),
        )?;
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Chat request cancelled".into());
        }
        Ok(client)
    }

    pub fn public(&self) -> serde_json::Value {
        match self {
            Self::Fixture { .. } => json!({"kind":"messages","model":FIXTURE_MODEL}),
            Self::Https { binding, .. } => binding.public(),
        }
    }

    pub fn credential(&self) -> Result<HeaderValue> {
        let mut header = match self {
            Self::Fixture { .. } => HeaderValue::from_static("dummy-owned-browser-fixture"),
            Self::Https {
                credential: Credential::Pinned(header),
                ..
            } => return Ok(header.as_ref().clone()),
            Self::Https {
                credential: Credential::Environment(variable),
                ..
            } => {
                let value =
                    std::env::var(variable).map_err(|_| "Provider credential unavailable")?;
                if value.is_empty()
                    || value.len() > 4096
                    || !value.bytes().all(|b| b.is_ascii_graphic())
                {
                    return Err("invalid Provider credential".into());
                }
                HeaderValue::from_str(&value).map_err(|_| "invalid Provider credential")?
            }
            Self::Https {
                credential: Credential::Keyring { scope, id },
                ..
            } => {
                let key = Vault::new(scope.clone()).read(id)?;
                HeaderValue::from_str(key.expose()).map_err(|_| "invalid Provider credential")?
            }
        };
        header.set_sensitive(true);
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_binding_is_redacted_and_does_not_open_the_vault() {
        let bind = |scope: &str, credential: &str| {
            Config::Keyring {
                endpoint: "https://example.com/v1/messages".into(),
                model: "resolved-model-id".into(),
                revision: 1,
                scope: scope.into(),
                credential: credential.into(),
                trust_ca: None,
            }
            .bind(Some("127.0.0.1:1".parse().unwrap()))
        };
        let scope = "a".repeat(32);
        let id = "b".repeat(32);
        let first = bind(&scope, &id).unwrap().public();
        assert!(!first.to_string().contains(&scope));
        assert!(!first.to_string().contains(&id));
        assert_ne!(
            first["fingerprint"],
            bind(&scope, &"c".repeat(32)).unwrap().public()["fingerprint"]
        );
        assert!(bind("not-an-id", &id).is_err());
        assert!(bind(&scope, &"B".repeat(32)).is_err());
    }

    #[test]
    fn binds_identity_without_resolving_credentials() {
        let bind = |endpoint: &str, revision, credential_env: &str| {
            Config::Https {
                endpoint: endpoint.into(),
                model: "resolved-model-id".into(),
                revision,
                credential_env: credential_env.into(),
                trust_ca: None,
            }
            .bind(Some("127.0.0.1:1".parse().unwrap()))
        };
        for endpoint in [
            "http://example.com/v1/messages",
            "https://user@example.com/v1/messages",
            "https://example.com/?key=secret",
            "https://example.com/#secret",
            "https://example.com:0/",
        ] {
            assert!(bind(endpoint, 1, "METLDR_TEST_KEY").is_err());
        }
        let first = bind("https://example.com/v1/messages", 1, "METLDR_TEST_KEY")
            .unwrap()
            .public();
        assert!(
            Config::Https {
                endpoint: "https://example.com/v1/messages".into(),
                model: "resolved-model-id".into(),
                revision: 1,
                credential_env: "METLDR_TEST_KEY".into(),
                trust_ca: None,
            }
            .bind(None)
            .is_ok()
        );
        for other in [
            bind("https://example.net/v1/messages", 1, "METLDR_TEST_KEY"),
            bind("https://example.com/v1/messages", 2, "METLDR_TEST_KEY"),
            bind("https://example.com/v1/messages", 1, "METLDR_OTHER_KEY"),
        ] {
            assert_ne!(first["fingerprint"], other.unwrap().public()["fingerprint"]);
        }
        let responses = Config::Https {
            endpoint: "https://example.com/v1/messages".into(),
            model: "resolved-model-id".into(),
            revision: 1,
            credential_env: "METLDR_TEST_KEY".into(),
            trust_ca: None,
        }
        .bind_responses(Some("127.0.0.1:1".parse().unwrap()))
        .unwrap();
        let Profile::Https { binding, .. } = responses else {
            panic!("HTTPS required")
        };
        assert_ne!(
            first["fingerprint"],
            binding.public_responses()["fingerprint"]
        );
        assert_eq!(binding.public_responses()["kind"], "https_responses");
        assert!(
            Config::Fixture
                .bind_responses(Some("127.0.0.1:1".parse().unwrap()))
                .is_err()
        );
        assert!(!first.to_string().contains("METLDR_TEST_KEY"));
        assert!(bind("https://example.com/v1/messages", 0, "METLDR_TEST_KEY").is_err());
        assert!(bind("https://example.com/v1/messages", 1, "invalid-env").is_err());
        let path = std::env::temp_dir().join(format!("metldr-invalid-ca-{}", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        drop(file);
        std::fs::write(&path, b"x").unwrap();
        let result = Config::Https {
            endpoint: "https://example.com/v1/messages".into(),
            model: "resolved-model-id".into(),
            revision: 1,
            credential_env: "METLDR_TEST_KEY".into(),
            trust_ca: Some(path.clone()),
        }
        .bind(Some("127.0.0.1:1".parse().unwrap()));
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
    }
}
