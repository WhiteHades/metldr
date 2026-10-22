mod assembler;
mod identity;
mod snapshot;
mod wire;

pub use assembler::Assembler;
pub use identity::{Counter, DocumentKind, Identity, Scope};
pub use snapshot::Snapshot;

#[derive(Debug)]
pub enum Error {
    InvalidIdentity,
    InvalidRequest,
    Serialization,
    FrameTooLarge,
    InvalidFrame,
    InvalidPage,
    Stale,
    Incomplete,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidIdentity => "invalid identity",
            Self::InvalidRequest => "invalid request",
            Self::Serialization => "record serialization failed or exceeded its limit",
            Self::FrameTooLarge => "encoded frame exceeds its limit",
            Self::InvalidFrame => "invalid frame",
            Self::InvalidPage => "invalid page",
            Self::Stale => "page identity is stale",
            Self::Incomplete => "record assembly is incomplete",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) const MAX_RECORD: usize = 2 * 1024 * 1024;
pub(crate) const MAX_FRAME: usize = 65_536;
pub(crate) const FRAGMENT_BYTES: usize = 2_048;

pub(crate) fn valid_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.:".contains(&byte))
}
