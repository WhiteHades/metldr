use super::{
    FlowError, MAX_FRAME, MAX_PAGE_TEXT, SourceId, SourceView, WireSource, parse_identity,
};

impl WireSource {
    pub(super) fn into_view(self, store: &str) -> Result<SourceView, FlowError> {
        if !parse_identity(self.id.as_str()).is_ok_and(|(saved, _)| saved == store)
            || self.tab < 0
            || self.document.is_empty()
            || self.document.len() > 128
            || self.url.len() > 2048
            || self.title.is_empty()
            || self.title.len() > 256
            || self.text.is_empty()
            || self.text.len() > MAX_PAGE_TEXT
        {
            return Err(FlowError::Protocol("invalid or oversized source".into()));
        }
        Ok(SourceView {
            id: self.id,
            tab: self.tab,
            document: self.document,
            url: self.url,
            title: self.title,
            text: self.text,
        })
    }
}

impl SourceView {
    pub(crate) fn decode_record(bytes: &[u8], expected: &SourceId) -> Result<Self, FlowError> {
        if bytes.len() > MAX_FRAME {
            return Err(FlowError::Protocol("source exceeds 65536 bytes".into()));
        }
        let source: WireSource = serde_json::from_slice(bytes)
            .map_err(|error| FlowError::Protocol(error.to_string()))?;
        if source.id != *expected {
            return Err(FlowError::Protocol("saved source identity changed".into()));
        }
        let (store, _) = parse_identity(expected.as_str()).map_err(FlowError::Protocol)?;
        source.into_view(store)
    }
}
