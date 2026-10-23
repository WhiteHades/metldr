use crate::{
    Result,
    browser_action::{CheckboxState, Control, RadioGroup},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::browser_worker) enum NativeState {
    Checkbox {
        checked: bool,
        indeterminate: bool,
    },
    Radio {
        checked: bool,
        index: u8,
        group: RadioGroup,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(in super::super) struct NativeControl {
    pub(in super::super) slot: usize,
    pub(in super::super) label: String,
    pub(in super::super) state: NativeState,
}

impl NativeState {
    pub(in super::super) fn checked(&self) -> bool {
        match self {
            Self::Checkbox { checked, .. } | Self::Radio { checked, .. } => *checked,
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::Checkbox { .. } => true,
            Self::Radio {
                checked,
                index,
                group,
            } => {
                (1..=32).contains(&group.member_count)
                    && *index < group.member_count
                    && group
                        .prior_selected
                        .as_ref()
                        .is_none_or(|peer| peer.valid(group.member_count))
                    && *checked
                        == group
                            .prior_selected
                            .as_ref()
                            .is_some_and(|peer| peer.index == *index)
            }
        }
    }
}

impl NativeControl {
    pub(in super::super) fn valid(&self) -> bool {
        self.slot < 32
            && !self.label.trim().is_empty()
            && self.label.len() <= 128
            && !self.label.chars().any(char::is_control)
            && self.state.valid()
    }

    pub(in super::super) fn proposal(
        &self,
        handle: String,
        checked: bool,
    ) -> Result<Option<Control>> {
        if self.state.checked() == checked {
            return Ok(None);
        }
        let control = match &self.state {
            NativeState::Checkbox {
                checked: before_checked,
                indeterminate,
            } => Control::Checkbox {
                control_id: handle,
                control_label: self.label.clone(),
                before: CheckboxState {
                    checked: *before_checked,
                    indeterminate: *indeterminate,
                },
                checked,
            },
            NativeState::Radio { checked: true, .. } if !checked => {
                return Err("unchecking a checked native radio is unsupported".into());
            }
            NativeState::Radio { index, group, .. } => Control::Radio {
                control_id: handle,
                control_label: self.label.clone(),
                index: *index,
                group: group.clone(),
            },
        };
        Ok(Some(control))
    }

    pub(in super::super) fn public(&self, handle: &str) -> Value {
        let mut value = serde_json::to_value(&self.state).expect("control state serializes");
        value["handle"] = json!(handle);
        value["label"] = json!(self.label);
        value
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn proposals_preserve_no_op_and_radio_transition_rules() {
        let unchecked = NativeControl {
            slot: 0,
            label: "Updates".into(),
            state: NativeState::Checkbox {
                checked: false,
                indeterminate: true,
            },
        };
        let proposal = unchecked.proposal("c1-1-0".into(), true).unwrap().unwrap();
        assert!(proposal.valid());
        assert!(
            unchecked
                .proposal("c1-1-0".into(), false)
                .unwrap()
                .is_none()
        );

        let selected = NativeControl {
            slot: 1,
            label: "Weekly".into(),
            state: NativeState::Radio {
                checked: true,
                index: 1,
                group: RadioGroup {
                    member_count: 2,
                    prior_selected: Some(crate::browser_action::RadioMember {
                        index: 1,
                        label: "Weekly".into(),
                        hidden: false,
                        disabled: false,
                    }),
                },
            },
        };
        assert!(selected.proposal("c1-1-1".into(), true).unwrap().is_none());
        assert!(selected.proposal("c1-1-1".into(), false).is_err());
    }
}
