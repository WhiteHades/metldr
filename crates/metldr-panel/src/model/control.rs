use super::{valid_control_id, valid_selection_label};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckboxState {
    pub(crate) checked: bool,
    pub(crate) indeterminate: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RadioMember {
    pub(crate) index: u8,
    pub(crate) label: String,
    pub(crate) hidden: bool,
    pub(crate) disabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RadioGroup {
    pub(crate) member_count: u8,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) prior_selected: Option<RadioMember>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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
    fn valid(&self, count: u8) -> bool {
        self.index < count && valid_selection_label(&self.label)
    }
}

impl Control {
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Checkbox {
                control_id,
                control_label,
                before,
                checked,
            } => {
                valid_control_id(control_id)
                    && valid_selection_label(control_label)
                    && before.checked != *checked
            }
            Self::Radio {
                control_id,
                control_label,
                index,
                group,
            } => {
                valid_control_id(control_id)
                    && valid_selection_label(control_label)
                    && (1..=32).contains(&group.member_count)
                    && *index < group.member_count
                    && group
                        .prior_selected
                        .as_ref()
                        .is_none_or(|peer| peer.valid(group.member_count) && peer.index != *index)
            }
        }
    }
}

impl ControlReceipt {
    pub(super) fn matches(&self, control: &Control) -> bool {
        match (self, control) {
            (
                Self::Checkbox {
                    checked,
                    indeterminate: false,
                },
                Control::Checkbox {
                    checked: requested, ..
                },
            ) => checked == requested,
            (
                Self::Radio {
                    selected,
                    member_count,
                    cleared_peer,
                },
                Control::Radio {
                    control_label,
                    index,
                    group,
                    ..
                },
            ) => {
                *member_count == group.member_count
                    && selected.index == *index
                    && selected.label == *control_label
                    && !selected.hidden
                    && !selected.disabled
                    && selected.valid(*member_count)
                    && cleared_peer == &group.prior_selected
            }
            _ => false,
        }
    }
}
