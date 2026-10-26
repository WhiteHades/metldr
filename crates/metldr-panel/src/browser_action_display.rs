use crate::model::{
    BrowserActionIntent, BrowserActionOutcome, BrowserActionPhase, BrowserActionView, Control,
    ControlReceipt, ScrollDirection,
};

pub(crate) struct Display {
    pub(crate) title: &'static str,
    pub(crate) status: String,
    pub(crate) receipt: bool,
    pub(crate) details: Vec<Detail>,
}

pub(crate) struct Detail {
    pub(crate) part: &'static str,
    pub(crate) label: &'static str,
    pub(crate) value: String,
}

pub(crate) fn display(action: &BrowserActionView) -> Display {
    let pending = action.phase == BrowserActionPhase::Pending;
    let title = match &action.intent {
        BrowserActionIntent::Navigate { .. } if pending => "Approve navigation",
        BrowserActionIntent::Navigate { .. } => "Navigation",
        BrowserActionIntent::ActivateButton { .. } if pending => "Approve button click",
        BrowserActionIntent::ActivateButton { .. } => "Button action",
        BrowserActionIntent::Field { .. } => "Browser action",
        BrowserActionIntent::SelectOption { .. } if pending => "Approve option selection",
        BrowserActionIntent::SelectOption { .. } => "Option selection",
        BrowserActionIntent::Scroll { .. } if pending => "Approve scroll",
        BrowserActionIntent::Scroll { .. } => "Page scroll",
        BrowserActionIntent::SetNativeControl { .. } if pending => "Approve control change",
        BrowserActionIntent::SetNativeControl { .. } => "Control change",
    };
    let mut details = Vec::new();
    let mut add = |part, label, value: String| details.push(Detail { part, label, value });
    match &action.intent {
        BrowserActionIntent::Field {
            field_label,
            previous_value,
            value,
            ..
        } => {
            add("destination", "Destination", action.url.clone());
            add("field", "Field", field_label.clone());
            add("previous-value", "Previous value", previous_value.clone());
            add("value", "Proposed value", value.clone());
        }
        BrowserActionIntent::Navigate { destination } => {
            add(
                "destination",
                "Destination",
                format!("{} → {destination}", action.url),
            );
        }
        BrowserActionIntent::ActivateButton { button_label, .. } => {
            add("destination", "Source", action.url.clone());
            add("button", "Button", button_label.clone());
        }
        BrowserActionIntent::SelectOption { selection } => {
            add("destination", "Destination", action.url.clone());
            add("field", "Control", selection.control_label.clone());
            add(
                "previous-value",
                "Previous selection",
                selection.previous.as_ref().map_or_else(
                    || "No selection".to_owned(),
                    |choice| format!("{} (option {})", choice.label, choice.index + 1),
                ),
            );
            add(
                "value",
                "Chosen option",
                format!(
                    "{} (option {})",
                    selection.selected.label,
                    selection.selected.index + 1
                ),
            );
        }
        BrowserActionIntent::Scroll { scroll } => {
            add("destination", "Destination", action.url.clone());
            add("scroll-target", "Target", scroll.label.clone());
            add(
                "scroll-motion",
                "Scroll",
                format!(
                    "{} {} px",
                    match scroll.direction {
                        ScrollDirection::Up => "Up",
                        ScrollDirection::Down => "Down",
                        ScrollDirection::Left => "Left",
                        ScrollDirection::Right => "Right",
                    },
                    scroll.pixels
                ),
            );
            add(
                "scroll-before",
                "Before",
                format!("x: {}, y: {}", scroll.before.x, scroll.before.y),
            );
            if let Some(point) = &action.observed_scroll {
                add(
                    "scroll-after",
                    "After",
                    format!("x: {}, y: {}", point.x, point.y),
                );
            }
        }
        BrowserActionIntent::SetNativeControl { control } => {
            add("destination", "Destination", action.url.clone());
            let (label, previous, requested, group, peer) = match control {
                Control::Checkbox {
                    control_label,
                    before,
                    checked,
                    ..
                } => (
                    control_label.as_str(),
                    checkbox_state(before.checked, before.indeterminate).to_owned(),
                    checkbox_state(*checked, false).to_owned(),
                    String::new(),
                    String::new(),
                ),
                Control::Radio {
                    control_label,
                    index,
                    group,
                    ..
                } => (
                    control_label.as_str(),
                    "Not checked".to_owned(),
                    format!("Select {} (option {})", control_label, index + 1),
                    if group.member_count == 1 {
                        "1 option".to_owned()
                    } else {
                        format!("{} options", group.member_count)
                    },
                    group
                        .prior_selected
                        .as_ref()
                        .map_or_else(|| "None selected".to_owned(), radio_choice),
                ),
            };
            add("control-target", "Control", label.to_owned());
            add("control-previous", "Previous state", previous);
            add("control-requested", "Requested state", requested);
            if matches!(control, Control::Radio { .. }) {
                add("control-group", "Radio group", group);
                add("control-peer", "Previous radio choice", peer);
            }
            if let Some(receipt) = &action.observed_control {
                add(
                    "control-observed",
                    "Observed local state",
                    match receipt {
                        ControlReceipt::Checkbox {
                            checked,
                            indeterminate,
                        } => checkbox_state(*checked, *indeterminate).to_owned(),
                        ControlReceipt::Radio {
                            selected,
                            cleared_peer,
                            ..
                        } => format!(
                            "Selected {} (option {}). Cleared {}.",
                            selected.label,
                            selected.index + 1,
                            cleared_peer
                                .as_ref()
                                .map_or("no other choice".to_owned(), radio_choice),
                        ),
                    },
                );
            }
        }
    }
    let status = match action.outcome {
        Some(BrowserActionOutcome::ClickObserved) => "Button click observed".to_owned(),
        Some(BrowserActionOutcome::SelectionObserved) => "Selection observed".to_owned(),
        Some(BrowserActionOutcome::ScrollObserved) if action.observed_scroll.as_ref().is_some_and(|after| {
            matches!(&action.intent, BrowserActionIntent::Scroll { scroll } if scroll.before == *after)
        }) => "No movement observed".to_owned(),
        Some(BrowserActionOutcome::ScrollObserved) => "Scroll position observed".to_owned(),
        Some(BrowserActionOutcome::ControlObserved) => "Control state observed on this page".to_owned(),
        _ => format!("{}{}{}", action.phase,
            action.outcome.map(|outcome| format!(" ({outcome})")).unwrap_or_default(),
            action.reason.as_ref().map(|reason| format!(": {reason}")).unwrap_or_default()),
    };
    Display {
        title,
        status,
        receipt: action.phase == BrowserActionPhase::Settled
            && matches!(
                action.outcome,
                Some(
                    BrowserActionOutcome::Verified
                        | BrowserActionOutcome::ClickObserved
                        | BrowserActionOutcome::SelectionObserved
                        | BrowserActionOutcome::ScrollObserved
                        | BrowserActionOutcome::ControlObserved
                )
            ),
        details,
    }
}

fn checkbox_state(checked: bool, indeterminate: bool) -> &'static str {
    if indeterminate {
        "Mixed"
    } else if checked {
        "Checked"
    } else {
        "Not checked"
    }
}

fn radio_choice(member: &crate::model::RadioMember) -> String {
    format!(
        "{} (option {}{}{})",
        member.label,
        member.index + 1,
        if member.hidden { ", hidden" } else { "" },
        if member.disabled { ", disabled" } else { "" }
    )
}
