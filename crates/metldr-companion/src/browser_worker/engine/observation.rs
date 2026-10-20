use super::runtime::FrameContext;
use crate::Result;
use chromiumoxide::cdp::js_protocol::runtime::RemoteObjectId;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Census {
    url: String,
    title: String,
    text: String,
    text_truncated: bool,
    fields_truncated: bool,
    fields: Vec<CensusField>,
    buttons_truncated: bool,
    buttons: Vec<CensusField>,
}

#[derive(Deserialize)]
struct CensusField {
    slot: usize,
    label: String,
    check_label: String,
}

pub(super) struct Field {
    pub(super) slot: usize,
    pub(super) label: String,
    pub(super) check_label: String,
}

pub(super) struct Snapshot {
    pub(super) context: FrameContext,
    pub(super) root: RemoteObjectId,
    pub(super) fields: BTreeMap<String, Field>,
    pub(super) buttons: BTreeMap<String, Field>,
}

impl Snapshot {
    pub(super) fn from_census(
        context: FrameContext,
        root: RemoteObjectId,
        run: &str,
        generation: u64,
        value: Value,
    ) -> Result<(Self, Value)> {
        let census: Census = serde_json::from_value(value)?;
        census.validate()?;
        let run_identity = crate::agent_model::digest(&run)?;
        let mut fields = BTreeMap::new();
        let mut observed_fields = Vec::with_capacity(census.fields.len());
        for field in &census.fields {
            let handle = format!("h{}-{generation:x}-{:x}", &run_identity[..32], field.slot);
            fields.insert(
                handle.clone(),
                Field {
                    slot: field.slot,
                    label: field.label.clone(),
                    check_label: field.check_label.clone(),
                },
            );
            observed_fields.push(json!({ "handle": handle, "label": field.label.clone() }));
        }
        let mut buttons = BTreeMap::new();
        let mut observed_buttons = Vec::with_capacity(census.buttons.len());
        for button in &census.buttons {
            let handle = format!("b{}-{generation:x}-{:x}", &run_identity[..32], button.slot);
            buttons.insert(
                handle.clone(),
                Field {
                    slot: button.slot,
                    label: button.label.clone(),
                    check_label: button.check_label.clone(),
                },
            );
            observed_buttons.push(json!({"handle":handle,"label":button.label}));
        }
        let result = json!({
            "kind": "page_observation",
            "untrusted": true,
            "url": census.url,
            "title": census.title,
            "text": census.text,
            "text_truncated": census.text_truncated,
            "fields_truncated": census.fields_truncated,
            "fields": observed_fields,
            "buttons_truncated": census.buttons_truncated,
            "buttons": observed_buttons,
        });
        Ok((
            Self {
                context,
                root,
                fields,
                buttons,
            },
            result,
        ))
    }

    pub(super) fn field(&self, handle: &str) -> Result<&Field> {
        self.fields
            .get(handle)
            .ok_or_else(|| "invalid or expired browser field handle".into())
    }
    pub(super) fn button(&self, handle: &str) -> Result<&Field> {
        self.buttons
            .get(handle)
            .ok_or_else(|| "invalid or expired browser button handle".into())
    }
}

impl Census {
    fn validate(&self) -> Result<()> {
        if self.url.is_empty()
            || self.url.len() > 2048
            || self.title.len() > 128
            || self.text.len() > 4096
            || [&self.fields, &self.buttons].into_iter().any(|controls| {
                let mut slots = BTreeSet::new();
                controls.len() > 32
                    || controls.iter().any(|field| {
                        field.slot >= 32
                            || !slots.insert(field.slot)
                            || field.label.is_empty()
                            || field.label.len() > 128
                            || field.label.chars().any(char::is_control)
                            || field.check_label.len() > 2048
                    })
            })
        {
            return Err("invalid or oversized browser observation".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn observation_boundary_rejects_duplicate_slots_and_oversized_text() {
        let mut census: Census = serde_json::from_value(json!({
            "url":"https://example.test/", "title":"Example", "text":"Live text",
            "text_truncated":false, "fields_truncated":false,
            "buttons_truncated":false,"buttons":[],
            "fields":[{"slot":0,"label":"Report title","check_label":"Report title"}]
        }))
        .unwrap();
        census.validate().unwrap();
        census.fields.push(CensusField {
            slot: 0,
            label: "Decoy".into(),
            check_label: "Decoy".into(),
        });
        assert!(census.validate().is_err());
        census.fields.pop();
        census.text = "x".repeat(4097);
        assert!(census.validate().is_err());
    }
}
