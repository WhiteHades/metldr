use super::{element, select, token::Kind};
use crate::{Panel, active_run, err};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, HtmlElement, HtmlOptionElement};

#[derive(Clone)]
pub(super) enum Action {
    Model,
    Capture,
    Insert(Kind),
}

#[derive(Clone)]
pub(super) struct Row {
    pub(super) value: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) disabled: bool,
    pub(super) action: Action,
}

pub(super) fn models(panel: &Panel) -> Result<Vec<Row>, JsValue> {
    let select = select(&panel.document)?;
    (0..select.children().length())
        .filter_map(|index| select.children().item(index))
        .map(|element| {
            let option = element.dyn_into::<HtmlOptionElement>()?;
            Ok(Row {
                value: option.value(),
                name: option.text(),
                description: if option.disabled() {
                    "Unavailable"
                } else if option.has_attribute("data-session") {
                    "Continue"
                } else {
                    ""
                }
                .into(),
                disabled: option.disabled(),
                action: Action::Model,
            })
        })
        .collect()
}

pub(super) fn suggestions(panel: &Panel, kind: Kind, query: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    if kind == Kind::Source {
        rows.push(Row {
            value: "capture-page".into(),
            name: "Capture current page".into(),
            description: "Use the selected browser tab".into(),
            disabled: !panel.connected
                || panel.start_pending
                || active_run(&panel.snapshot).is_some(),
            action: Action::Capture,
        });
        if let Some(source) = panel
            .snapshot
            .as_ref()
            .and_then(|view| view.source.as_ref())
        {
            rows.push(Row {
                value: "saved-source".into(),
                name: source.title.clone(),
                description: "Saved page".into(),
                disabled: false,
                action: Action::Insert(kind),
            });
        }
        rows.retain(|row| row.name.to_lowercase().contains(query));
    } else {
        for (name, description) in [
            ("/compare", "Compare the saved source"),
            ("/draft-email", "Draft an email from this source"),
            ("/summarize", "Summarize the saved source"),
        ] {
            if name[1..].starts_with(query) {
                rows.push(Row {
                    value: name.into(),
                    name: name.into(),
                    description: description.into(),
                    disabled: false,
                    action: Action::Insert(kind),
                });
            }
        }
    }
    rows
}

pub(super) fn paint(
    doc: &Document,
    id: &str,
    rows: &[Row],
    models: bool,
    selected: &str,
) -> Result<bool, JsValue> {
    let container = element(doc, id)?;
    let signature = serde_json::to_string(&(
        selected,
        rows.iter()
            .map(|row| (&row.value, &row.name, &row.description, row.disabled))
            .collect::<Vec<_>>(),
    ))
    .map_err(|error| err(&error.to_string()))?;
    if container.get_attribute("data-rows").as_deref() == Some(signature.as_str()) {
        return Ok(false);
    }
    container.set_attribute("data-rows", &signature)?;
    container.set_text_content(None);
    for (index, row) in rows.iter().enumerate() {
        let button = doc.create_element("button")?;
        button.set_attribute("type", "button")?;
        button.set_attribute("role", "option")?;
        button.set_attribute("data-menu-index", &index.to_string())?;
        button.set_attribute(
            if models {
                "data-execution-value"
            } else {
                "data-suggestion"
            },
            &row.value,
        )?;
        button.set_attribute("title", &row.name)?;
        if row.disabled {
            button.set_attribute("disabled", "")?;
        }
        if models {
            button.set_attribute(
                "aria-selected",
                if row.value == selected {
                    "true"
                } else {
                    "false"
                },
            )?;
        } else if !matches!(row.action, Action::Insert(Kind::Command)) {
            let icon = doc.create_element("span")?;
            icon.set_attribute("aria-hidden", "true")?;
            icon.set_inner_html(SOURCE_ICON);
            button.append_child(&icon)?;
        }
        for (text, class) in [
            (
                &row.name,
                if models {
                    "composer-model-name"
                } else {
                    "composer-row-name"
                },
            ),
            (
                &row.description,
                if models {
                    "composer-model-tag"
                } else {
                    "composer-row-description"
                },
            ),
        ] {
            let span = doc.create_element("span")?;
            span.set_class_name(class);
            span.set_text_content(Some(text));
            button.append_child(&span)?;
        }
        if models {
            let check = doc.create_element("span")?;
            check.set_class_name("composer-model-check");
            check.set_attribute("aria-hidden", "true")?;
            if row.value != selected {
                check.set_attribute("style", "visibility:hidden")?;
            }
            check.set_inner_html(CHECK_ICON);
            button.append_child(&check)?;
        }
        container.append_child(&button)?;
    }
    Ok(true)
}

pub(super) fn highlight(
    doc: &Document,
    id: &str,
    rows: &str,
    index: Option<usize>,
    show: bool,
) -> Result<(), JsValue> {
    let highlight = element(doc, id)?.dyn_into::<HtmlElement>()?;
    let row = index
        .and_then(|index| element(doc, rows).ok()?.children().item(index as u32))
        .and_then(|row| row.dyn_into::<HtmlElement>().ok());
    let style = highlight.style();
    if let Some(row) = row {
        style.set_property("--row-block", &format!("{}px", row.offset_top()))?;
        style.set_property("--row-size", &format!("{}px", row.offset_height()))?;
    }
    style.set_property("--row-opacity", if show { "1" } else { "0" })
}

pub(super) fn position(doc: &Document) -> Result<(), JsValue> {
    let menu = element(doc, "composer-model-menu")?.dyn_into::<HtmlElement>()?;
    let anchor = doc
        .query_selector("#composer .composer-anchor")?
        .ok_or_else(|| err("Composer anchor missing."))?
        .get_bounding_client_rect();
    let trigger = element(doc, "composer-model")?.get_bounding_client_rect();
    menu.style().set_property(
        "--menu-inline",
        &format!(
            "{}px",
            (trigger.left() - anchor.left())
                .max(0.0)
                .min((anchor.width() - 176.0).max(0.0))
        ),
    )?;
    menu.style().set_property(
        "--menu-max-block",
        &format!("{}px", (trigger.top() - 8.0).max(0.0)),
    )?;
    menu.style().set_property(
        "--menu-block",
        &format!("{}px", anchor.bottom() - trigger.top() + 8.0),
    )
}

const SOURCE_ICON: &str = r#"<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>"#;
const CHECK_ICON: &str = r#"<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>"#;
