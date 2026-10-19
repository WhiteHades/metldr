use crate::{CredentialId, Error, SERVICE};
use dbus_secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;

fn service() -> Result<SecretService, Error> {
    SecretService::connect_with_max_prompt_timeout(EncryptionType::Dh, 0).map_err(map_error)
}

fn item<'a>(
    collection: &'a dbus_secret_service::Collection<'_>,
    scope: &CredentialId,
    id: &CredentialId,
    part: &str,
) -> Result<Vec<dbus_secret_service::Item<'a>>, Error> {
    let account = format!("{}:{}:{}", scope.as_str(), id.as_str(), part);
    collection
        .search_items(HashMap::from([
            ("application", SERVICE),
            ("account", account.as_str()),
        ]))
        .map_err(map_error)
}

pub(crate) fn read(
    scope: &CredentialId,
    id: &CredentialId,
    part: &str,
) -> Result<Option<Vec<u8>>, Error> {
    let service = service()?;
    let collection = service.get_default_collection().map_err(map_error)?;
    if collection.is_locked().map_err(map_error)? {
        return Err(Error::Locked);
    }
    let mut found = item(&collection, scope, id, part)?;
    if found.len() > 1 {
        return Err(Error::Corrupt);
    }
    let Some(found) = found.pop() else {
        return Ok(None);
    };
    if found.is_locked().map_err(map_error)? {
        return Err(Error::Locked);
    }
    found.get_secret().map(Some).map_err(map_error)
}

pub(crate) fn put(
    scope: &CredentialId,
    id: &CredentialId,
    part: &str,
    value: &[u8],
) -> Result<(), Error> {
    let service = service()?;
    let collection = service.get_default_collection().map_err(map_error)?;
    if collection.is_locked().map_err(map_error)? {
        return Err(Error::Locked);
    }
    let account = format!("{}:{}:{}", scope.as_str(), id.as_str(), part);
    let found = item(&collection, scope, id, part)?;
    if !found.is_empty() {
        return Err(Error::Conflict);
    }
    collection
        .create_item(
            "MetLDR credential",
            HashMap::from([("application", SERVICE), ("account", account.as_str())]),
            value,
            false,
            "application/octet-stream",
        )
        .map_err(map_error)?;
    Ok(())
}

pub(crate) fn remove(scope: &CredentialId, id: &CredentialId, part: &str) -> Result<(), Error> {
    let service = service()?;
    let collection = service.get_default_collection().map_err(map_error)?;
    if collection.is_locked().map_err(map_error)? {
        return Err(Error::Locked);
    }
    let mut found = item(&collection, scope, id, part)?;
    if found.len() > 1 {
        return Err(Error::Corrupt);
    }
    if let Some(entry) = found.pop() {
        if entry.is_locked().map_err(map_error)? {
            return Err(Error::Locked);
        }
        entry.delete().map_err(map_error)?;
    }
    Ok(())
}

fn map_error(error: dbus_secret_service::Error) -> Error {
    use dbus_secret_service::Error as E;
    match error {
        E::NoResult => Error::Unavailable,
        E::Locked | E::Prompt => Error::Locked,
        E::Unavailable => Error::Unavailable,
        _ => Error::Unavailable,
    }
}
