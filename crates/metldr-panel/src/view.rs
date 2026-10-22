use crate::{
    Panel, active_run, composer, err,
    model::{BrowserActionIntent, BrowserActionOutcome, BrowserActionPhase, SourceStatus},
    results,
};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement};

pub(super) fn mount(doc: &Document) -> Result<(), JsValue> {
    doc.get_element_by_id("app")
        .ok_or_else(|| err("#app missing"))?
        .set_inner_html(include_str!("shell.html"));
    doc.get_element_by_id("composer")
        .ok_or_else(|| err("Composer missing."))?
        .set_inner_html(include_str!("composer_ui.html"));
    Ok(())
}

pub(super) fn render(panel: &Panel) -> Result<(), JsValue> {
    let doc = &panel.document;
    let snapshot = &panel.snapshot;
    let source = snapshot.as_ref().and_then(|view| view.source.as_ref());
    let title = source
        .map(|s| s.title.as_str())
        .unwrap_or("No saved source");
    set_text(doc, "source-title", title);
    set_text(doc, "source-full-title", title);
    let source_state = match snapshot
        .as_ref()
        .map(|v| v.source_status)
        .unwrap_or(SourceStatus::Checking)
    {
        SourceStatus::Current => "current document",
        SourceStatus::Changed | SourceStatus::Saved => "saved page",
        SourceStatus::Checking => "checking saved page",
    };
    set_text(
        doc,
        "source-state",
        &format!(
            "metldr | {}",
            if source.is_some() {
                source_state
            } else {
                "No saved source"
            }
        ),
    );
    let meta = source
        .map(|s| format!("Saved source | {} | {}", s.url, source_state))
        .unwrap_or_else(|| "Capture a page, then choose where to send your request.".to_string());
    set_text(doc, "source-meta", &meta);
    set_text(
        doc,
        "source-text",
        source.map(|s| s.text.as_str()).unwrap_or(""),
    );
    let profile = snapshot.as_ref().and_then(|view| view.pi_profile.as_ref());
    let profile_text = profile.map_or_else(
        || "Pi profile unavailable.".to_string(),
        |profile| format!(
            "Pi configured provider {}, model {}. Pi manages its endpoint, account, tools and permissions. The agent controls where it sends data.",
            profile.provider, profile.model
        ),
    );
    set_text(doc, "pi-profile", &profile_text);
    let codex_text = snapshot.as_ref().and_then(|view| view.codex_profile.as_ref()).map_or(
        "Codex configuration unavailable.".to_string(),
        |_| "Codex uses its native account, model, tools and permissions. The agent controls where it sends data.".to_string(),
    );
    set_text(doc, "codex-profile", &codex_text);
    let opencode_text = snapshot.as_ref().and_then(|view| view.opencode_profile.as_ref()).map_or(
        "OpenCode configuration unavailable.".to_string(),
        |_| "OpenCode uses its native provider, model, tools and permissions. The agent controls where it sends data.".to_string(),
    );
    set_text(doc, "opencode-profile", &opencode_text);
    if let Some(view) = snapshot {
        crate::provider_settings::render(
            doc,
            &view.provider_settings,
            panel.connection_generation,
            crate::conversations::settings_blocked(panel),
        )?;
        crate::agent_settings::render(
            doc,
            view,
            panel.connection_generation,
            panel.start_pending,
            crate::conversations::settings_blocked(panel),
        )?;
    }
    crate::conversations::render(panel)?;
    composer::render(panel)?;
    let connection = if panel.connected {
        "Connected"
    } else {
        "Disconnected"
    };
    set_text(doc, "connection", connection);
    if let Some(element) = doc.get_element_by_id("connection") {
        element.set_attribute(
            "data-connected",
            if panel.connected { "true" } else { "false" },
        )?;
    }
    let busy = panel.start_pending
        || active_run(snapshot).is_some()
        || crate::conversations::start_blocked(panel);
    for (action, disabled) in [
        ("capture", busy || !panel.connected),
        ("stop", active_run(snapshot).is_none()),
    ] {
        if let Ok(Some(button)) = doc.query_selector(&format!("[data-action='{action}']")) {
            if disabled {
                let _ = button.set_attribute("disabled", "");
            } else {
                let _ = button.remove_attribute("disabled");
            }
        }
    }
    status(doc, &panel.status_message);
    results::render(doc, snapshot.as_ref())
        .and_then(|()| crate::history_ui::render(panel))
        .and_then(|()| render_browser_actions(panel))
        .and_then(|()| render_native_permissions(panel))
        .and_then(|()| crate::pi_ui::render(panel))
        .and_then(|()| crate::codex_input::render(panel))
}

fn render_native_permissions(panel: &Panel) -> Result<(), JsValue> {
    let section = panel
        .document
        .get_element_by_id("native-permissions")
        .ok_or_else(|| err("native permission list is missing."))?;
    let permissions = panel
        .snapshot
        .as_ref()
        .map(|view| view.native_permissions.as_slice())
        .unwrap_or(&[]);
    let cards: Vec<_> = (0..section.children().length())
        .filter_map(|i| section.children().item(i))
        .collect();
    let mut restore_focus = false;
    let mut attention = None;
    for card in &cards {
        if !permissions.iter().any(|item| {
            card.get_attribute("data-native-run").as_deref() == Some(item.run.as_str())
                && card.get_attribute("data-native-request").as_deref()
                    == Some(item.request.as_str())
        }) {
            restore_focus |= contains_focus(&panel.document, card);
            section.remove_child(card)?;
        }
    }
    for (position, permission) in permissions.iter().enumerate() {
        let card = match cards.iter().find(|card| {
            card.get_attribute("data-native-run").as_deref() == Some(permission.run.as_str())
                && card.get_attribute("data-native-request").as_deref()
                    == Some(permission.request.as_str())
        }) {
            Some(card) => card.clone(),
            None => {
                let card = create_native_permission_card(&panel.document, permission)?;
                attention.get_or_insert_with(|| card.clone());
                card
            }
        };
        update_native_permission_card(panel, &card, permission)?;
        let current = section.children().item(position as u32);
        if !current
            .as_ref()
            .is_some_and(|node| node.is_same_node(Some(&card)))
        {
            section.insert_before(&card, current.as_ref().map(|node| node.as_ref()))?;
        }
    }
    if permissions.is_empty() {
        section.set_attribute("hidden", "")?;
    } else {
        section.remove_attribute("hidden")?;
    }
    if let Some(card) = attention {
        card.scroll_into_view_with_bool(true);
    }
    if restore_focus {
        if let Some(status) = panel
            .document
            .get_element_by_id("status")
            .and_then(|e| e.dyn_into::<HtmlElement>().ok())
        {
            let _ = status.focus();
        }
    }
    Ok(())
}

fn create_native_permission_card(
    doc: &Document,
    permission: &crate::model::NativePermissionView,
) -> Result<Element, JsValue> {
    let card = doc.create_element("article")?;
    card.set_attribute("data-native-run", permission.run.as_str())?;
    card.set_attribute("data-native-request", permission.request.as_str())?;
    let heading = doc.create_element("h2")?;
    heading.set_attribute("data-part", "title")?;
    card.append_child(&heading)?;
    for (part, label) in [("agent", "Agent"), ("tool", "Tool"), ("input", "Input")] {
        let name = doc.create_element("strong")?;
        name.set_text_content(Some(&format!("{label}: ")));
        if part == "input" {
            let row = doc.create_element("div")?;
            row.append_child(&name)?;
            let value = doc.create_element("pre")?;
            value.set_attribute("data-part", part)?;
            row.append_child(&value)?;
            card.append_child(&row)?;
            continue;
        }
        let row = doc.create_element("p")?;
        row.append_child(&name)?;
        let value = doc.create_element("span")?;
        value.set_attribute("data-part", part)?;
        row.append_child(&value)?;
        card.append_child(&row)?;
    }
    let controls = doc.create_element("div")?;
    controls.set_attribute("data-part", "controls")?;
    for option in &permission.options {
        let button = doc.create_element("button")?;
        button.set_attribute("type", "button")?;
        button.set_attribute("data-action", "native-permission")?;
        button.set_attribute("data-native-option", &option.id)?;
        button.set_text_content(Some(&option.name));
        controls.append_child(&button)?;
    }
    let cancel = doc.create_element("button")?;
    cancel.set_attribute("type", "button")?;
    cancel.set_attribute("data-action", "native-permission")?;
    cancel.set_text_content(Some("Cancel"));
    controls.append_child(&cancel)?;
    card.append_child(&controls)?;
    Ok(card)
}

fn update_native_permission_card(
    panel: &Panel,
    card: &Element,
    permission: &crate::model::NativePermissionView,
) -> Result<(), JsValue> {
    card.set_attribute("data-native-run", permission.run.as_str())?;
    card.set_attribute("data-native-request", permission.request.as_str())?;
    for (part, value) in [
        ("agent", permission.agent.label()),
        ("title", permission.title.as_str()),
        ("tool", permission.tool.as_str()),
        ("input", permission.input.as_str()),
    ] {
        card.query_selector(&format!("[data-part='{part}']"))?
            .ok_or_else(|| err("native permission detail is missing."))?
            .set_text_content(Some(value));
    }
    let disabled = !panel.connected
        || active_run(&panel.snapshot) != Some(&permission.run)
        || panel.native_permission_pending.is_some();
    let buttons = card.query_selector_all("[data-action='native-permission']")?;
    for index in 0..buttons.length() {
        if let Some(button) = buttons
            .item(index)
            .and_then(|node| node.dyn_into::<Element>().ok())
        {
            button.set_attribute("data-native-run", permission.run.as_str())?;
            button.set_attribute("data-native-request", permission.request.as_str())?;
            if disabled {
                button.set_attribute("disabled", "")?;
            } else {
                button.remove_attribute("disabled")?;
            }
        }
    }
    Ok(())
}

fn render_browser_actions(panel: &Panel) -> Result<(), JsValue> {
    let section = panel
        .document
        .get_element_by_id("browser-actions")
        .ok_or_else(|| err("Browser approval list is missing."))?;
    let current_actions = panel
        .document
        .get_element_by_id("browser-current")
        .ok_or_else(|| err("Current browser actions are missing."))?;
    let receipts = panel
        .document
        .get_element_by_id("browser-receipts")
        .ok_or_else(|| err("Browser action results are missing."))?;
    let receipt_rows = panel
        .document
        .get_element_by_id("browser-receipt-rows")
        .ok_or_else(|| err("Browser action result rows are missing."))?;
    let actions = panel
        .snapshot
        .as_ref()
        .map(|view| view.browser_actions.as_slice())
        .unwrap_or(&[]);
    let children = section.query_selector_all("article[data-browser-action]")?;
    let cards: Vec<_> = (0..children.length())
        .filter_map(|index| {
            children
                .item(index)
                .and_then(|node| node.dyn_into::<Element>().ok())
        })
        .collect();
    let mut restore_focus = false;
    let mut attention = None;
    for card in &cards {
        if !actions.iter().any(|action| {
            card.get_attribute("data-browser-action").as_deref() == Some(action.id.as_str())
        }) {
            restore_focus |= contains_focus(&panel.document, card);
            card.remove();
        }
    }
    let mut ordered: Vec<_> = actions.iter().collect();
    ordered.sort_by_key(|action| action.phase != BrowserActionPhase::Pending);
    let mut current_position = 0;
    let mut receipt_position = 0;
    for action in ordered {
        let card = match cards.iter().find(|card| {
            card.get_attribute("data-browser-action").as_deref() == Some(action.id.as_str())
        }) {
            Some(card) => card.clone(),
            None => create_action_card(&panel.document, action)?,
        };
        if action.phase == BrowserActionPhase::Pending
            && card.get_attribute("data-phase").as_deref() != Some("pending")
        {
            attention.get_or_insert_with(|| card.clone());
        }
        update_action_card(panel, &card, action, &mut restore_focus)?;
        let (parent, position) = if action.phase == BrowserActionPhase::Settled
            && matches!(
                action.outcome,
                Some(
                    BrowserActionOutcome::Verified
                        | BrowserActionOutcome::ClickObserved
                        | BrowserActionOutcome::SelectionObserved
                )
            ) {
            (&receipt_rows, &mut receipt_position)
        } else {
            (&current_actions, &mut current_position)
        };
        let current = parent.children().item(*position);
        if !current
            .as_ref()
            .is_some_and(|node| node.is_same_node(Some(&card)))
        {
            parent.insert_before(&card, current.as_ref().map(|node| node.as_ref()))?;
        }
        *position += 1;
    }
    set_text(
        &panel.document,
        "browser-receipts-count",
        &format!("Browser action results ({receipt_position})"),
    );
    if receipt_position == 0 {
        restore_focus |= contains_focus(&panel.document, &receipts);
        receipts.set_attribute("hidden", "")?;
    } else {
        receipts.remove_attribute("hidden")?;
    }
    if actions.is_empty() {
        section.set_attribute("hidden", "")?;
    } else {
        section.remove_attribute("hidden")?;
    }
    if let Some(card) = attention {
        card.scroll_into_view_with_bool(true);
    }
    if restore_focus {
        if let Some(status) = panel
            .document
            .get_element_by_id("status")
            .and_then(|element| element.dyn_into::<HtmlElement>().ok())
        {
            let _ = status.focus();
        }
    }
    Ok(())
}

fn create_action_card(
    doc: &Document,
    action: &crate::model::BrowserActionView,
) -> Result<Element, JsValue> {
    let card = doc.create_element("article")?;
    card.set_attribute("data-browser-action", action.id.as_str())?;
    let heading = doc.create_element("h2")?;
    heading.set_text_content(Some("Browser action"));
    card.append_child(&heading)?;
    for (part, label) in [
        ("destination", "Destination"),
        ("button", "Button"),
        ("field", "Field"),
        ("previous-value", "Previous value"),
        ("value", "Proposed value"),
        ("status", "Status"),
    ] {
        let detail = doc.create_element("p")?;
        detail.set_attribute("data-row", part)?;
        let name = doc.create_element("strong")?;
        name.set_text_content(Some(&format!("{label}: ")));
        detail.append_child(&name)?;
        let value = doc.create_element("span")?;
        value.set_attribute("data-part", part)?;
        detail.append_child(&value)?;
        card.append_child(&detail)?;
    }
    Ok(card)
}

fn update_action_card(
    panel: &Panel,
    card: &Element,
    action: &crate::model::BrowserActionView,
    restore_focus: &mut bool,
) -> Result<(), JsValue> {
    card.set_attribute("data-browser-action", action.id.as_str())?;
    card.set_attribute("data-phase", &action.phase.to_string())?;
    let heading = card
        .query_selector("h2")?
        .ok_or_else(|| err("Browser action heading is missing."))?;
    heading.set_text_content(Some(match &action.intent {
        BrowserActionIntent::Navigate { .. } if action.phase == BrowserActionPhase::Pending => {
            "Approve navigation"
        }
        BrowserActionIntent::Navigate { .. } => "Navigation",
        BrowserActionIntent::ActivateButton { .. }
            if action.phase == BrowserActionPhase::Pending =>
        {
            "Approve button click"
        }
        BrowserActionIntent::ActivateButton { .. } => "Button action",
        BrowserActionIntent::Field { .. } => "Browser action",
        BrowserActionIntent::SelectOption { .. } if action.phase == BrowserActionPhase::Pending => {
            "Approve option selection"
        }
        BrowserActionIntent::SelectOption { .. } => "Option selection",
    }));
    match &action.intent {
        BrowserActionIntent::Field {
            field_label,
            previous_value,
            value,
            ..
        } => {
            update_detail(card, "destination", &action.url)?;
            update_detail(card, "field", field_label)?;
            update_detail(card, "previous-value", previous_value)?;
            update_detail(card, "value", value)?;
            show_rows(
                card,
                &["destination", "field", "previous-value", "value", "status"],
            )?;
        }
        BrowserActionIntent::Navigate { destination } => {
            update_detail(
                card,
                "destination",
                &format!("{} → {destination}", action.url),
            )?;
            show_rows(card, &["destination", "status"])?;
        }
        BrowserActionIntent::ActivateButton { button_label, .. } => {
            update_row_label(card, "destination", "Source")?;
            update_detail(card, "destination", &action.url)?;
            update_detail(card, "button", button_label)?;
            show_rows(card, &["destination", "button", "status"])?;
        }
        BrowserActionIntent::SelectOption { selection } => {
            update_detail(card, "destination", &action.url)?;
            update_row_label(card, "field", "Control")?;
            update_detail(card, "field", &selection.control_label)?;
            update_row_label(card, "previous-value", "Previous selection")?;
            update_detail(
                card,
                "previous-value",
                &selection.previous.as_ref().map_or_else(
                    || "No selection".to_owned(),
                    |choice| format!("{} (option {})", choice.label, choice.index + 1),
                ),
            )?;
            update_row_label(card, "value", "Chosen option")?;
            update_detail(
                card,
                "value",
                &format!(
                    "{} (option {})",
                    selection.selected.label,
                    selection.selected.index + 1
                ),
            )?;
            show_rows(
                card,
                &["destination", "field", "previous-value", "value", "status"],
            )?;
        }
    }
    let status = match action.outcome {
        Some(BrowserActionOutcome::ClickObserved) => "Button click observed".to_owned(),
        Some(BrowserActionOutcome::SelectionObserved) => "Selection observed".to_owned(),
        _ => format!(
            "{}{}{}",
            action.phase,
            action
                .outcome
                .map(|outcome| format!(" ({outcome})"))
                .unwrap_or_default(),
            action
                .reason
                .as_ref()
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        ),
    };
    update_detail(card, "status", &status)?;
    let controls = card.query_selector("[data-part='controls']")?;
    if action.phase != BrowserActionPhase::Pending {
        if let Some(controls) = controls {
            *restore_focus |= contains_focus(&panel.document, &controls);
            card.remove_child(&controls)?;
        }
        return Ok(());
    }
    let controls = match controls {
        Some(controls) => controls,
        None => {
            let controls = panel.document.create_element("div")?;
            controls.set_attribute("data-part", "controls")?;
            for (label, command) in [
                ("Approve", "browser-approve"),
                ("Decline", "browser-decline"),
            ] {
                let button = panel.document.create_element("button")?;
                button.set_attribute("type", "button")?;
                button.set_attribute("data-action", command)?;
                button.set_text_content(Some(label));
                controls.append_child(&button)?;
            }
            card.append_child(&controls)?;
            controls
        }
    };
    let connected = panel.connected && panel.browser_decision_pending.is_none();
    let current_document = connected
        && panel
            .snapshot
            .as_ref()
            .is_some_and(|view| view.can_approve(action, panel.flow.source_epoch()));
    for (command, label, available) in [
        ("browser-approve", "Approve", current_document),
        ("browser-decline", "Decline", connected),
    ] {
        let button = controls
            .query_selector(&format!("[data-action='{command}']"))?
            .ok_or_else(|| err("Browser decision button is missing."))?;
        button.set_attribute("data-browser-action", action.id.as_str())?;
        button.set_attribute("data-revision", &action.revision.to_string())?;
        button.set_attribute(
            "aria-label",
            &format!("{label} browser action for {}", action.url),
        )?;
        if !available {
            button.set_attribute("disabled", "")?;
        } else {
            button.remove_attribute("disabled")?;
        }
    }
    Ok(())
}

fn show_rows(card: &Element, visible: &[&str]) -> Result<(), JsValue> {
    for row in [
        "destination",
        "button",
        "field",
        "previous-value",
        "value",
        "status",
    ] {
        let element = card
            .query_selector(&format!("[data-row='{row}']"))?
            .ok_or_else(|| err("Browser action row is missing."))?;
        if visible.contains(&row) {
            element.remove_attribute("hidden")?;
        } else {
            element.set_attribute("hidden", "")?;
        }
    }
    Ok(())
}

fn update_detail(card: &Element, name: &str, value: &str) -> Result<(), JsValue> {
    let element = card
        .query_selector(&format!("[data-part='{name}']"))?
        .ok_or_else(|| err("Browser action field is missing."))?;
    write_text(&element, value);
    Ok(())
}

fn update_row_label(card: &Element, name: &str, label: &str) -> Result<(), JsValue> {
    let element = card
        .query_selector(&format!("[data-row='{name}'] strong"))?
        .ok_or_else(|| err("Browser action label is missing."))?;
    element.set_text_content(Some(&format!("{label}: ")));
    Ok(())
}

fn contains_focus(doc: &Document, element: &Element) -> bool {
    doc.active_element()
        .is_some_and(|active| element.contains(Some(active.as_ref())))
}

fn set_text(doc: &Document, id: &str, value: &str) {
    if let Some(element) = doc.get_element_by_id(id) {
        write_text(&element, value);
    }
}

fn write_text(element: &Element, value: &str) {
    if element.text_content().as_deref() != Some(value) {
        element.set_text_content(Some(value));
    }
}

pub(super) fn status(doc: &Document, value: &str) {
    set_text(doc, "status", value);
}
