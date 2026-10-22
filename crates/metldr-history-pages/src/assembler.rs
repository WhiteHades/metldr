use crate::{
    Error, FRAGMENT_BYTES, Identity, MAX_FRAME, MAX_RECORD, Result, valid_token, wire::Page,
};

pub struct Assembler {
    identity: Identity,
    bytes: Vec<u8>,
    total: Option<u32>,
    boundaries: Vec<u32>,
}

impl Assembler {
    pub fn new(identity: Identity) -> Result<Self> {
        identity.validate()?;
        Ok(Self {
            identity,
            bytes: Vec::new(),
            total: None,
            boundaries: vec![0],
        })
    }

    pub fn next_offset(&self) -> u32 {
        self.bytes.len() as u32
    }

    pub fn push(&mut self, bytes: &[u8], request: &str) -> Result<bool> {
        if bytes.len() > MAX_FRAME || !valid_token(request, 96) {
            return Err(Error::InvalidFrame);
        }
        let page: Page = serde_json::from_slice(bytes).map_err(|_| Error::InvalidFrame)?;
        page.validate_header(request)?;
        if page.identity != self.identity {
            return Err(Error::Stale);
        }
        let total = page.total as usize;
        let start = page.offset as usize;
        let end = start
            .checked_add(page.text.len())
            .ok_or(Error::InvalidPage)?;
        if total == 0
            || total > MAX_RECORD
            || start >= total
            || end > total
            || page.text.is_empty()
            || page.text.len() > FRAGMENT_BYTES
            || (end < total && page.text.len() < FRAGMENT_BYTES - 3)
            || page.next != (end < total).then_some(end as u32)
        {
            return Err(Error::InvalidPage);
        }
        let fragment = page.text.as_bytes();
        if end <= self.bytes.len() {
            let exact_page = self
                .boundaries
                .binary_search(&page.offset)
                .ok()
                .is_some_and(|index| self.boundaries.get(index + 1) == Some(&(end as u32)));
            if exact_page
                && self.total == Some(page.total)
                && self.bytes.get(start..end) == Some(fragment)
            {
                return Ok(self.bytes.len() == total);
            }
            return Err(Error::InvalidPage);
        }
        if start != self.bytes.len() || self.total.is_some_and(|known| known != page.total) {
            return Err(Error::InvalidPage);
        }
        if self.total.is_none() {
            self.total = Some(page.total);
            self.bytes.reserve(total);
        }
        self.bytes.extend_from_slice(fragment);
        self.boundaries.push(end as u32);
        Ok(self.bytes.len() == total)
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        if self.total.map(|total| total as usize) != Some(self.bytes.len()) {
            return Err(Error::Incomplete);
        }
        Ok(self.bytes)
    }
}
