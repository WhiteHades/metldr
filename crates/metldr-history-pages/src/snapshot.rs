use serde::Serialize;
use std::io::{self, Write};

use crate::{
    Error, FRAGMENT_BYTES, Identity, MAX_FRAME, MAX_RECORD, Result, valid_token,
    wire::{Page, WIRE_VERSION},
};

pub struct Snapshot {
    identity: Identity,
    text: String,
}

struct Bounded(Vec<u8>);

impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_RECORD {
            return Err(io::Error::other("record exceeds maximum size"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Snapshot {
    pub fn new(identity: Identity, value: &impl Serialize) -> Result<Self> {
        identity.validate()?;
        let mut writer = Bounded(Vec::new());
        serde_json::to_writer(&mut writer, value).map_err(|_| Error::Serialization)?;
        let text = String::from_utf8(writer.0).map_err(|_| Error::Serialization)?;
        Ok(Self { identity, text })
    }

    pub fn page(&self, request: &str, offset: u32) -> Result<Vec<u8>> {
        if !valid_token(request, 96) || offset as usize >= self.text.len() {
            return Err(Error::InvalidRequest);
        }
        let start = offset as usize;
        if !self.text.is_char_boundary(start) {
            return Err(Error::InvalidPage);
        }
        let mut end = (start + FRAGMENT_BYTES).min(self.text.len());
        end = self.text.floor_char_boundary(end);
        let next = (end < self.text.len()).then_some(end as u32);
        let page = Page {
            v: WIRE_VERSION,
            request: request.to_owned(),
            kind: "detail".to_owned(),
            identity: self.identity.clone(),
            offset,
            total: self.text.len() as u32,
            text: self.text[start..end].to_owned(),
            next,
        };
        let encoded = serde_json::to_vec(&page).map_err(|_| Error::Serialization)?;
        if encoded.len() > MAX_FRAME {
            return Err(Error::FrameTooLarge);
        }
        Ok(encoded)
    }
}
