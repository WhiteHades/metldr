use super::{Binding, Control, Proposal, Scroll, Selection, bounded};
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Intent {
    Field {
        field_id: String,
        field_label: String,
        previous_value: String,
        value: String,
    },
    Navigate {
        destination: String,
    },
    ActivateButton {
        button_id: String,
        button_label: String,
    },
    SelectOption {
        selection: Selection,
    },
    Scroll {
        scroll: Scroll,
    },
    SetNativeControl {
        control: Control,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireProposal {
    run: String,
    tool_use_id: String,
    binding: Binding,
    url: String,
    intent: Option<Intent>,
    field_id: Option<String>,
    field_label: Option<String>,
    previous_value: Option<String>,
    value: Option<String>,
}

impl TryFrom<WireProposal> for Proposal {
    type Error = String;

    fn try_from(wire: WireProposal) -> std::result::Result<Self, String> {
        let legacy = wire.field_id.is_some()
            || wire.field_label.is_some()
            || wire.previous_value.is_some()
            || wire.value.is_some();
        let intent = match wire.intent {
            Some(intent) if !legacy => intent,
            Some(_) => return Err("mixed browser proposal formats".into()),
            None => Intent::Field {
                field_id: wire.field_id.ok_or("missing field handle")?,
                field_label: wire.field_label.unwrap_or_else(|| "Text field".into()),
                previous_value: wire.previous_value.ok_or("missing previous value")?,
                value: wire.value.ok_or("missing field value")?,
            },
        };
        Ok(Proposal {
            run: wire.run,
            tool_use_id: wire.tool_use_id,
            binding: wire.binding,
            url: wire.url,
            intent,
        })
    }
}

impl Intent {
    pub(super) fn validate(&self, current_url: &str) -> Result<()> {
        match self {
            Self::Field {
                field_id,
                field_label,
                previous_value,
                value,
            } if bounded(field_id, 64)
                && bounded(field_label, 128)
                && field_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                && previous_value.len() <= 1024
                && value.len() <= 1024 =>
            {
                Ok(())
            }
            Self::Navigate { destination } if valid_destination(current_url, destination) => Ok(()),
            Self::ActivateButton {
                button_id,
                button_label,
            } if bounded(button_id, 64)
                && button_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                && bounded(button_label, 128) =>
            {
                Ok(())
            }
            Self::SelectOption { selection } if selection.valid() => Ok(()),
            Self::Scroll { scroll } if scroll.valid() => Ok(()),
            Self::SetNativeControl { control } if control.valid() => Ok(()),
            _ => Err("invalid browser action intent".into()),
        }
    }
}

pub(crate) fn valid_destination(current_url: &str, destination: &str) -> bool {
    let (Ok(current), Ok(next)) = (
        reqwest::Url::parse(current_url),
        reqwest::Url::parse(destination),
    ) else {
        return false;
    };
    bounded(destination, 2048)
        && next.as_str() == destination
        && next.origin() == current.origin()
        && next.scheme() == "http"
        && next.host_str() == Some("127.0.0.1")
        && next.port().is_some()
        && next.username().is_empty()
        && next.password().is_none()
        && next.query().is_none()
        && next.fragment().is_none()
        && matches!(next.path(), "/article" | "/decoy" | "/redirect")
}
