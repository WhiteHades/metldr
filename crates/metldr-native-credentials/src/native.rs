use crate::{CredentialId, Error, SERVICE};
use keyring_core::{Entry, Error as KeyringError, api::CredentialStoreApi};

#[cfg(target_os = "windows")]
fn entry(scope: &CredentialId, id: &CredentialId, part: &str) -> Result<Entry, Error> {
    let store = windows_native_keyring_store::Store::new().map_err(map_error)?;
    let user = format!("{}:{}:{}", scope.as_str(), id.as_str(), part);
    CredentialStoreApi::build(&*store, SERVICE, &user, None).map_err(map_error)
}

#[cfg(target_os = "macos")]
fn entry(scope: &CredentialId, id: &CredentialId, part: &str) -> Result<Entry, Error> {
    let store = apple_native_keyring_store::keychain::Store::new().map_err(map_error)?;
    let user = format!("{}:{}:{}", scope.as_str(), id.as_str(), part);
    CredentialStoreApi::build(&*store, SERVICE, &user, None).map_err(map_error)
}

pub(crate) fn read(
    scope: &CredentialId,
    id: &CredentialId,
    part: &str,
) -> Result<Option<Vec<u8>>, Error> {
    match entry(scope, id, part)?.get_secret() {
        Ok(value) => Ok(Some(value)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(map_error(error)),
    }
}

pub(crate) fn put(
    scope: &CredentialId,
    id: &CredentialId,
    part: &str,
    value: &[u8],
) -> Result<(), Error> {
    let entry = entry(scope, id, part)?;
    match entry.get_secret() {
        Ok(mut existing) => {
            let same = existing == value;
            zeroize::Zeroize::zeroize(&mut existing);
            return if same { Ok(()) } else { Err(Error::Conflict) };
        }
        Err(KeyringError::NoEntry) => {}
        Err(error) => return Err(map_error(error)),
    }
    entry.set_secret(value).map_err(map_error)
}

pub(crate) fn remove(scope: &CredentialId, id: &CredentialId, part: &str) -> Result<(), Error> {
    match entry(scope, id, part)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

fn map_error(error: KeyringError) -> Error {
    match error {
        KeyringError::NoEntry => Error::Missing,
        KeyringError::NoStorageAccess(_) => Error::Locked,
        KeyringError::Invalid(_, _) | KeyringError::TooLong(_, _) => Error::Invalid,
        KeyringError::Ambiguous(_)
        | KeyringError::BadEncoding(_)
        | KeyringError::BadDataFormat(_, _) => Error::Corrupt,
        _ => Error::Unavailable,
    }
}
