#![forbid(unsafe_code)]

use std::fmt;
use zeroize::{Zeroize, Zeroizing};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native;

const PART_SIZE: usize = 2048;
const HEADER_SIZE: usize = 2;
const SERVICE: &str = "com.metldr.credentials.v1";
// Platform stores lack create-only writes. The native owner also holds its process store lock.
static WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialId(String);

impl CredentialId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Invalid);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn generate() -> Result<Self, Error> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
        let mut value = String::with_capacity(32);
        for byte in bytes {
            use fmt::Write;
            write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct Key(String);

impl Key {
    pub fn parse(mut value: String) -> Result<Self, Error> {
        let valid = (1..=4096).contains(&value.len())
            && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte));
        if !valid {
            value.zeroize();
            return Err(Error::Invalid);
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Missing,
    Locked,
    Unavailable,
    Conflict,
    Corrupt,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid credential input",
            Self::Missing => "credential is missing",
            Self::Locked => "credential store is locked",
            Self::Unavailable => "credential store is unavailable",
            Self::Conflict => "credential revision conflicts with stored data",
            Self::Corrupt => "credential record is corrupt",
        })
    }
}

impl std::error::Error for Error {}

pub struct Vault {
    scope: CredentialId,
}

impl Vault {
    pub fn new(scope: CredentialId) -> Self {
        Self { scope }
    }

    pub fn put(&self, id: &CredentialId, key: &Key) -> Result<(), Error> {
        let _write = WRITES.lock().map_err(|_| Error::Unavailable)?;
        match self.read(id) {
            Ok(existing) => {
                return if existing.expose() == key.expose() {
                    Ok(())
                } else {
                    Err(Error::Conflict)
                };
            }
            Err(Error::Missing) => (),
            Err(error) => return Err(error),
        }
        if key.0.len() <= PART_SIZE
            && let Some(mut orphan) = backend::read(&self.scope, id, "tail")?
        {
            orphan.zeroize();
            return Err(Error::Conflict);
        }
        let mut head = Vec::with_capacity(HEADER_SIZE + PART_SIZE);
        let count = if key.0.len() > PART_SIZE { 2 } else { 1 };
        head.extend_from_slice(&[1, count]);
        head.extend_from_slice(&key.0.as_bytes()[..key.0.len().min(PART_SIZE)]);
        let tail = (count == 2).then(|| &key.0.as_bytes()[PART_SIZE..]);

        let result = (|| {
            if let Some(tail) = tail {
                self.write_part(id, "tail", tail)?;
            }
            self.write_part(id, "head", &head)
        })();
        head.zeroize();
        result
    }

    fn write_part(&self, id: &CredentialId, part: &str, expected: &[u8]) -> Result<(), Error> {
        let existing = backend::read(&self.scope, id, part)?;
        match existing {
            Some(mut raw) => {
                let same = raw == expected;
                raw.zeroize();
                if same { Ok(()) } else { Err(Error::Conflict) }
            }
            None => {
                backend::put(&self.scope, id, part, expected)?;
                let mut actual = backend::read(&self.scope, id, part)?.ok_or(Error::Corrupt)?;
                let same = actual == expected;
                actual.zeroize();
                if same { Ok(()) } else { Err(Error::Conflict) }
            }
        }
    }

    pub fn read(&self, id: &CredentialId) -> Result<Key, Error> {
        let mut head = backend::read(&self.scope, id, "head")?.ok_or(Error::Missing)?;
        let parsed = (|| {
            if head.len() < HEADER_SIZE
                || head.len() > HEADER_SIZE + PART_SIZE
                || head[0] != 1
                || !matches!(head[1], 1 | 2)
            {
                return Err(Error::Corrupt);
            }
            let mut bytes = Zeroizing::new(head[HEADER_SIZE..].to_vec());
            let result = if head[1] == 1 {
                if bytes.is_empty() || bytes.len() > PART_SIZE {
                    Err(Error::Corrupt)
                } else {
                    key_from_bytes(std::mem::take(&mut *bytes))
                }
            } else if bytes.len() != PART_SIZE {
                Err(Error::Corrupt)
            } else {
                match backend::read(&self.scope, id, "tail")? {
                    Some(mut tail) if (1..=PART_SIZE).contains(&tail.len()) => {
                        bytes.extend_from_slice(&tail);
                        tail.zeroize();
                        key_from_bytes(std::mem::take(&mut *bytes))
                    }
                    Some(mut tail) => {
                        tail.zeroize();
                        Err(Error::Corrupt)
                    }
                    None => Err(Error::Corrupt),
                }
            };
            result
        })();
        head.zeroize();
        parsed
    }

    pub fn remove(&self, id: &CredentialId) -> Result<(), Error> {
        let _write = WRITES.lock().map_err(|_| Error::Unavailable)?;
        backend::remove(&self.scope, id, "head")?;
        backend::remove(&self.scope, id, "tail")
    }
}

fn key_from_bytes(bytes: Vec<u8>) -> Result<Key, Error> {
    match String::from_utf8(bytes) {
        Ok(value) => Key::parse(value).map_err(|_| Error::Corrupt),
        Err(error) => {
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            Err(Error::Corrupt)
        }
    }
}

#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use native as backend;

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
compile_error!("metldr-native-credentials supports Linux, Windows, and macOS only");

#[cfg(test)]
mod checks;
