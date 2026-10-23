use super::runtime::FrameContext;
use crate::Result;
use crate::browser_action::{Choice, Point, ScrollKind};
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
    selects_truncated: bool,
    selects: Vec<CensusSelect>,
    scroll_targets_truncated: bool,
    frame_scroll_targets_observed: bool,
    scroll_targets: Vec<CensusScrollTarget>,
}

#[derive(Deserialize)]
struct CensusScrollTarget {
    slot: usize,
    kind: ScrollKind,
    label: String,
    x: serde_json::Number,
    y: serde_json::Number,
}

pub(super) struct ScrollTarget {
    pub(super) slot: usize,
    pub(super) kind: ScrollKind,
    pub(super) label: String,
    pub(super) before: Point,
}

#[derive(Deserialize)]
struct CensusSelect {
    slot: usize,
    label: String,
    selected: Option<usize>,
    options: Vec<CensusOption>,
}

#[derive(Deserialize)]
struct CensusOption {
    slot: usize,
    label: String,
    enabled: bool,
}

pub(super) struct SelectControl {
    pub(super) slot: usize,
    pub(super) label: String,
    options: Vec<SelectOption>,
}

struct SelectOption {
    choice: Choice,
    enabled: bool,
}

impl SelectControl {
    pub(super) fn option(&self, id: &str) -> Result<&Choice> {
        self.options
            .iter()
            .find(|option| option.enabled && option.choice.id == id)
            .map(|option| &option.choice)
            .ok_or_else(|| "invalid, disabled or expired browser option handle".into())
    }

    pub(super) fn selected(&self, index: Option<usize>) -> Result<Option<Choice>> {
        index
            .map(|index| {
                self.options
                    .get(index)
                    .map(|option| option.choice.clone())
                    .ok_or_else(|| "selected browser option is unavailable".into())
            })
            .transpose()
    }
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
    pub(super) selects: BTreeMap<String, SelectControl>,
    pub(super) scroll_targets: BTreeMap<String, ScrollTarget>,
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
        let mut selects = BTreeMap::new();
        let mut observed_selects = Vec::with_capacity(census.selects.len());
        for control in &census.selects {
            let identity = format!("{}-{generation:x}-{:x}", &run_identity[..32], control.slot);
            let handle = format!("s{identity}");
            let options: Vec<_> = control
                .options
                .iter()
                .map(|option| SelectOption {
                    choice: Choice {
                        id: format!("o{identity}-{:x}", option.slot),
                        label: option.label.clone(),
                        index: option.slot,
                    },
                    enabled: option.enabled,
                })
                .collect();
            let selected = control
                .selected
                .map(|index| public_choice(&options[index].choice));
            let choices: Vec<_> = options
                .iter()
                .filter(|option| option.enabled)
                .map(|option| public_choice(&option.choice))
                .collect();
            observed_selects.push(json!({"handle":handle,"label":control.label,"selected":selected,"options":choices}));
            selects.insert(
                handle,
                SelectControl {
                    slot: control.slot,
                    label: control.label.clone(),
                    options,
                },
            );
        }
        let mut scroll_targets = BTreeMap::new();
        let mut observed_scroll_targets = Vec::with_capacity(census.scroll_targets.len());
        for target in &census.scroll_targets {
            let handle = format!("r{}-{generation:x}-{:x}", &run_identity[..32], target.slot);
            let before = Point {
                x: target.x.clone(),
                y: target.y.clone(),
            };
            scroll_targets.insert(
                handle.clone(),
                ScrollTarget {
                    slot: target.slot,
                    kind: target.kind.clone(),
                    label: target.label.clone(),
                    before: before.clone(),
                },
            );
            observed_scroll_targets.push(json!({
                "handle": handle,
                "kind": target.kind,
                "label": target.label,
                "before": before,
            }));
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
            "selects_truncated": census.selects_truncated,
            "selects": observed_selects,
            "scroll_targets_truncated": census.scroll_targets_truncated,
            "frame_scroll_targets_observed": census.frame_scroll_targets_observed,
            "scroll_targets": observed_scroll_targets,
        });
        Ok((
            Self {
                context,
                root,
                fields,
                buttons,
                selects,
                scroll_targets,
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

    pub(super) fn select(&self, handle: &str) -> Result<&SelectControl> {
        self.selects
            .get(handle)
            .ok_or_else(|| "invalid or expired browser selection handle".into())
    }

    pub(super) fn scroll_target(&self, handle: &str) -> Result<&ScrollTarget> {
        self.scroll_targets
            .get(handle)
            .ok_or_else(|| "invalid or expired browser scroll handle".into())
    }
}

fn public_choice(choice: &Choice) -> Value {
    json!({"handle":choice.id,"label":choice.label,"index":choice.index})
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
        let mut slots = BTreeSet::new();
        if self.selects.len() > 32
            || self.selects.iter().map(|s| s.options.len()).sum::<usize>() > 64
            || self.selects.iter().any(|control| {
                control.slot >= 32
                    || !slots.insert(control.slot)
                    || !valid_label(&control.label)
                    || control.options.is_empty()
                    || control.options.len() > 32
                    || control
                        .selected
                        .is_some_and(|index| index >= control.options.len())
                    || control
                        .options
                        .iter()
                        .enumerate()
                        .any(|(index, option)| option.slot != index || !valid_label(&option.label))
            })
        {
            return Err("invalid or oversized browser selection observation".into());
        }
        if self.scroll_targets.is_empty()
            || self.scroll_targets.len() > 16
            || self.frame_scroll_targets_observed
            || self
                .scroll_targets
                .iter()
                .enumerate()
                .any(|(slot, target)| {
                    target.slot != slot
                        || !valid_label(&target.label)
                        || !numeric_position(&target.x)
                        || !numeric_position(&target.y)
                        || (slot == 0 && target.kind != ScrollKind::Page)
                        || (slot > 0 && target.kind != ScrollKind::Region)
                })
        {
            return Err("invalid browser scroll target observation".into());
        }
        Ok(())
    }
}

fn valid_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
}

fn numeric_position(value: &serde_json::Number) -> bool {
    value
        .as_f64()
        .is_some_and(|position| position.is_finite() && position.abs() <= 1_000_000_000.0)
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
            "selects_truncated":false,"selects":[],
            "scroll_targets_truncated":false,"frame_scroll_targets_observed":false,
            "scroll_targets":[{"slot":0,"kind":"page","label":"Page","x":0,"y":0}],
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
