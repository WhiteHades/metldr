use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Choice {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) index: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selection {
    pub(crate) control_id: String,
    pub(crate) control_label: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) previous: Option<Choice>,
    pub(crate) selected: Choice,
}

impl Choice {
    pub(crate) fn valid(&self) -> bool {
        valid_id(&self.id) && valid_label(&self.label) && self.index < 32
    }
}

impl Selection {
    pub(crate) fn valid(&self) -> bool {
        valid_id(&self.control_id)
            && valid_label(&self.control_label)
            && self.previous.as_ref().is_none_or(Choice::valid)
            && self.selected.valid()
    }
}

pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod checks {
    use super::*;
    use serde_json::json;

    #[test]
    fn selection_requires_explicit_nullable_previous_and_bounded_choices() {
        let valid = json!({
            "control_id": "select_1",
            "control_label": "Region",
            "previous": null,
            "selected": {"id":"option_1", "label":"North", "index":0}
        });
        let selection: Selection = serde_json::from_value(valid.clone()).unwrap();
        assert!(selection.valid());

        let mut missing_previous = valid.clone();
        missing_previous.as_object_mut().unwrap().remove("previous");
        assert!(serde_json::from_value::<Selection>(missing_previous).is_err());

        for invalid in [
            json!({"id":"bad id", "label":"North", "index":0}),
            json!({"id":"option_1", "label":"", "index":0}),
            json!({"id":"option_1", "label":"North", "index":32}),
        ] {
            let choice: Choice = serde_json::from_value(invalid).unwrap();
            assert!(!choice.valid());
        }
    }
}
