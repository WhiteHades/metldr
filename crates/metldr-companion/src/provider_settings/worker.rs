use super::{Completion, KeyChange, Pending, Protocol, Selection, Settings};
use crate::{flow::Event, https_profile::Profile};
use metldr_native_credentials::{CredentialId, Error, Vault};
use std::{
    fs::File,
    sync::{Arc, mpsc::SyncSender},
    thread::{self, JoinHandle},
};

fn state(error: &(dyn std::error::Error + 'static)) -> &'static str {
    match error.downcast_ref::<Error>() {
        Some(Error::Missing) => "missing",
        Some(Error::Locked) => "locked",
        Some(Error::Corrupt) => "corrupt",
        _ => "unavailable",
    }
}

pub(super) fn save(
    lease: Arc<File>,
    tx: SyncSender<Event>,
    scope: String,
    pending: Pending,
    previous: Option<String>,
    change: KeyChange,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let _lease = lease;
        let result = (|| -> crate::Result<Profile> {
            let vault = Vault::new(CredentialId::parse(&scope)?);
            let id = CredentialId::parse(&pending.connection.credential)?;
            let key = match change {
                KeyChange::Keep => vault.read(&CredentialId::parse(
                    previous.as_deref().ok_or("previous provider key missing")?,
                )?)?,
                KeyChange::Replace { value } => value.0,
            };
            vault.put(&id, &key)?;
            pending
                .connection
                .profile(pending.protocol, &scope, pending.revision)?
                .pin()
        })()
        .map_err(|error| state(error.as_ref()));
        let _ = tx.send(Event::Provider(Completion::Saved { pending, result }));
    })
}

pub(super) fn load(
    lease: Arc<File>,
    tx: SyncSender<Event>,
    settings: Settings,
    request: Option<String>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let _lease = lease;
        let profiles = Protocol::ALL
            .into_iter()
            .filter_map(|protocol| {
                let slot = settings.slot(protocol);
                let Selection::Configured { connection } = slot.selection else {
                    return None;
                };
                let result = (|| -> crate::Result<Profile> {
                    connection
                        .profile(
                            protocol,
                            settings.scope.as_deref().ok_or("provider scope missing")?,
                            slot.revision,
                        )?
                        .pin()
                })()
                .map_err(|error| state(error.as_ref()));
                Some((protocol, result))
            })
            .collect();
        let _ = tx.send(Event::Provider(Completion::Loaded { request, profiles }));
    })
}

pub(super) fn cleanup(
    lease: Arc<File>,
    tx: SyncSender<Event>,
    scope: String,
    retired: Vec<String>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let _lease = lease;
        let count = retired.len();
        let mut removed = Vec::new();
        for id in retired {
            let result = CredentialId::parse(&scope).and_then(|scope| {
                CredentialId::parse(&id).and_then(|id| Vault::new(scope).remove(&id))
            });
            if result.is_ok() {
                removed.push(id);
            }
        }
        let failed = removed.len() < count;
        let _ = tx.send(Event::Provider(Completion::Cleaned { removed, failed }));
    })
}
