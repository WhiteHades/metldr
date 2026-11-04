use serde::{Deserialize, Serialize};

use crate::{Identity, Result, valid_token};

pub const WIRE_VERSION: u8 = 19;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    pub v: u8,
    pub request: String,
    pub kind: String,
    pub identity: Identity,
    pub offset: u32,
    pub total: u32,
    pub text: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next: Option<u32>,
}

impl Page {
    pub fn validate_header(&self, request: &str) -> Result<()> {
        if self.v != WIRE_VERSION
            || self.kind != "detail"
            || self.request != request
            || !valid_token(&self.request, 96)
        {
            return Err(crate::Error::InvalidPage);
        }
        self.identity.validate()
    }
}
