use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlSelectElement};

use crate::state::{Choice, Setup, Status};

pub fn element(document: &Document, id: &str) -> Result<Element, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("Missing control {id}")))
}

pub fn text(document: &Document, id: &str, value: &str) -> Result<(), JsValue> {
    element(document, id)?.set_text_content(Some(value));
    Ok(())
}

fn flag(document: &Document, id: &str, attribute: &str, enabled: bool) -> Result<(), JsValue> {
    let element = element(document, id)?;
    if enabled {
        element.set_attribute(attribute, "")
    } else {
        element.remove_attribute(attribute)
    }
}

pub fn models(document: &Document, names: &[String], selected: &str) -> Result<(), JsValue> {
    let select = element(document, "model-choice")?;
    select.set_text_content(None);
    for name in names {
        let option = document.create_element("option")?;
        option.set_attribute("value", name)?;
        option.set_text_content(Some(name));
        select.append_child(&option)?;
    }
    select.dyn_into::<HtmlSelectElement>()?.set_value(selected);
    Ok(())
}

pub fn render(document: &Document, setup: &Setup, downloading: bool) -> Result<(), JsValue> {
    let local = matches!(setup.choice, Choice::Chrome | Choice::Ollama);
    for (id, selected) in [
        ("choose-local", local),
        ("choose-provider", setup.choice == Choice::Provider),
        ("choose-agent", setup.choice == Choice::Agent),
        ("choose-chrome", setup.choice == Choice::Chrome),
        ("choose-ollama", setup.choice == Choice::Ollama),
    ] {
        element(document, id)?
            .set_attribute("aria-pressed", if selected { "true" } else { "false" })?;
        flag(document, id, "disabled", setup.saving)?;
    }
    for (id, visible) in [
        ("local-options", local),
        ("chrome-controls", setup.choice == Choice::Chrome),
        ("ollama-controls", setup.choice == Choice::Ollama),
        ("provider-note", setup.choice == Choice::Provider),
        ("agent-note", setup.choice == Choice::Agent),
        ("check-button", local),
        ("download-button", setup.status == Status::Downloadable),
        ("cancel-button", downloading),
        ("download-progress", setup.status == Status::Downloading),
        ("model-choice", !setup.models.is_empty()),
        ("model-choice-label", !setup.models.is_empty()),
    ] {
        flag(document, id, "hidden", !visible)?;
    }
    for (id, disabled) in [
        (
            "check-button",
            setup.saving || setup.status == Status::Checking || downloading,
        ),
        (
            "download-button",
            setup.saving || downloading || setup.status != Status::Downloadable,
        ),
        ("cancel-button", !downloading),
        (
            "continue-button",
            setup.loading || setup.saving || setup.theme_saving || !setup.ready(),
        ),
        (
            "explore-button",
            setup.loading || setup.saving || setup.theme_saving,
        ),
        ("theme-choice", setup.saving),
        ("model-choice", setup.saving || setup.models.is_empty()),
    ] {
        flag(document, id, "disabled", disabled)?;
    }
    let heading = if setup.saving {
        "Saving setup"
    } else {
        match setup.status {
            Status::Idle => match setup.choice {
                Choice::Chrome => "Chrome AI selected",
                Choice::Ollama => "Ollama selected",
                Choice::Provider => "Provider setup unavailable",
                Choice::Agent => "Agent setup unavailable",
            },
            Status::Checking => "Checking connection",
            Status::Ready => "Connection ready",
            Status::Downloadable => "Model download available",
            Status::Downloading => "Model download in progress",
            Status::Unavailable => "Connection unavailable",
            Status::Error => "Connection check failed",
        }
    };
    text(document, "status-text", heading)?;
    text(document, "connection-detail", &setup.detail)?;
    let status = match setup.status {
        Status::Ready => "ready",
        Status::Error | Status::Unavailable => "error",
        _ => "idle",
    };
    element(document, "status-indicator")?.set_attribute("data-status", status)?;
    let progress = element(document, "download-progress")?;
    match setup.progress {
        Some(value) => progress.set_attribute("value", &(value * 100.0).to_string())?,
        None => progress.remove_attribute("value")?,
    }
    document
        .document_element()
        .ok_or_else(|| JsValue::from_str("Document root unavailable"))?
        .set_attribute("data-theme", &setup.theme)?;
    element(document, "theme-choice")?
        .dyn_into::<HtmlSelectElement>()?
        .set_value(&setup.theme);
    Ok(())
}
