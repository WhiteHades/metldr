use serde::{Deserialize, Serialize};

use super::selection::{valid_id, valid_label};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckboxState {
    pub checked: bool,
    pub indeterminate: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RadioMember {
    pub index: u8,
    pub label: String,
    pub hidden: bool,
    pub disabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RadioGroup {
    pub member_count: u8,
    #[serde(deserialize_with = "Option::deserialize")]
    pub prior_selected: Option<RadioMember>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Control {
    Checkbox {
        control_id: String,
        control_label: String,
        before: CheckboxState,
        checked: bool,
    },
    Radio {
        control_id: String,
        control_label: String,
        index: u8,
        group: RadioGroup,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ControlReceipt {
    Checkbox {
        checked: bool,
        indeterminate: bool,
    },
    Radio {
        selected: RadioMember,
        member_count: u8,
        #[serde(deserialize_with = "Option::deserialize")]
        cleared_peer: Option<RadioMember>,
    },
}

impl RadioMember {
    pub(crate) fn valid(&self, count: u8) -> bool {
        self.index < count && valid_label(&self.label)
    }
}

impl RadioGroup {
    pub(crate) fn valid(&self, target: u8) -> bool {
        (1..=32).contains(&self.member_count)
            && target < self.member_count
            && self
                .prior_selected
                .as_ref()
                .is_none_or(|peer| peer.index != target && peer.valid(self.member_count))
    }
}

impl Control {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Checkbox {
                control_id,
                control_label,
                before,
                checked,
            } => valid_id(control_id) && valid_label(control_label) && before.checked != *checked,
            Self::Radio {
                control_id,
                control_label,
                index,
                group,
            } => valid_id(control_id) && valid_label(control_label) && group.valid(*index),
        }
    }

    pub(crate) fn matches(&self, receipt: &ControlReceipt) -> bool {
        match (self, receipt) {
            (
                Self::Checkbox { checked, .. },
                ControlReceipt::Checkbox {
                    checked: observed,
                    indeterminate: false,
                },
            ) => checked == observed,
            (
                Self::Radio {
                    control_label,
                    index,
                    group,
                    ..
                },
                ControlReceipt::Radio {
                    selected,
                    member_count,
                    cleared_peer,
                },
            ) => {
                *member_count == group.member_count
                    && selected.index == *index
                    && &selected.label == control_label
                    && !selected.hidden
                    && !selected.disabled
                    && cleared_peer == &group.prior_selected
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn checked_control_receipt_matches_the_whole_approved_effect() {
        let checkbox = Control::Checkbox {
            control_id: "control_1".into(),
            control_label: "Email updates".into(),
            before: CheckboxState {
                checked: false,
                indeterminate: true,
            },
            checked: true,
        };
        assert!(checkbox.valid());
        assert!(checkbox.matches(&ControlReceipt::Checkbox {
            checked: true,
            indeterminate: false,
        }));
        assert!(!checkbox.matches(&ControlReceipt::Checkbox {
            checked: true,
            indeterminate: true,
        }));
        let selected = RadioMember {
            index: 1,
            label: "Express".into(),
            hidden: false,
            disabled: false,
        };
        let peer = RadioMember {
            index: 0,
            label: "Standard".into(),
            hidden: true,
            disabled: true,
        };
        let radio = Control::Radio {
            control_id: "control_2".into(),
            control_label: "Express".into(),
            index: 1,
            group: RadioGroup {
                member_count: 2,
                prior_selected: Some(peer.clone()),
            },
        };
        assert!(radio.valid());
        assert!(radio.matches(&ControlReceipt::Radio {
            selected: selected.clone(),
            member_count: 2,
            cleared_peer: Some(peer),
        }));
        assert!(!radio.matches(&ControlReceipt::Radio {
            selected,
            member_count: 2,
            cleared_peer: None,
        }));
        assert!(
            !RadioGroup {
                member_count: 33,
                prior_selected: None,
            }
            .valid(0)
        );
    }
}
