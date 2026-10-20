use crate::{err, model::ActivityView};
use wasm_bindgen::JsValue;
use web_sys::{Document, Element};

pub(super) fn reconcile(
    doc: &Document,
    group: &Element,
    ids: &[&str],
    reports: &[ActivityView],
    existing: &[Element],
) -> Result<(), JsValue> {
    use crate::model::ToolPreviewView;

    struct Tool {
        id: String,
        name: Option<String>,
        phase: Option<String>,
        phase_kind: Option<String>,
        title: Option<ToolPreviewView>,
        input: Option<ToolPreviewView>,
        output: Option<ToolPreviewView>,
    }

    let mut tools = Vec::new();
    for id in ids {
        if tools.iter().any(|tool: &Tool| tool.id == *id) {
            continue;
        }
        tools.push(Tool {
            id: (*id).to_owned(),
            name: None,
            phase: None,
            phase_kind: None,
            title: None,
            input: None,
            output: None,
        });
    }
    for report in reports {
        let Some(call) = report.call.as_ref() else {
            continue;
        };
        let Some(tool) = tools.iter_mut().find(|tool| tool.id == call.id) else {
            continue;
        };
        tool.name = Some(report.tool.clone());
        tool.phase = Some(
            report
                .outcome
                .map_or_else(|| report.phase.to_string(), |outcome| outcome.to_string()),
        );
        tool.phase_kind = Some(report.phase.to_string());
        if call.title.is_some() {
            tool.title.clone_from(&call.title);
        }
        if call.input.is_some() {
            tool.input.clone_from(&call.input);
        }
        if call.output.is_some() {
            tool.output.clone_from(&call.output);
        }
    }

    let calls = group
        .query_selector("[data-part='tool-calls']")?
        .ok_or_else(|| err("Tool call list is missing."))?;
    let count = group
        .query_selector("[data-part='tool-count']")?
        .ok_or_else(|| err("Tool count is missing."))?;
    write_text(
        &count,
        &format!(
            "{} {}",
            tools.len(),
            if tools.len() == 1 { "tool" } else { "tools" }
        ),
    );
    if tools.is_empty() {
        group.set_attribute("hidden", "")?;
    } else {
        group.remove_attribute("hidden")?;
    }

    for child in existing {
        if !tools
            .iter()
            .any(|tool| child.get_attribute("data-tool-call").as_deref() == Some(&tool.id))
            && child
                .parent_node()
                .is_some_and(|parent| parent.is_same_node(Some(calls.as_ref())))
        {
            calls.remove_child(child)?;
        }
    }

    for (position, tool) in tools.iter().enumerate() {
        let details = existing
            .iter()
            .find(|child| child.get_attribute("data-tool-call").as_deref() == Some(&tool.id))
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| create_tool(doc, &tool.id))?;
        update_tool(
            &details,
            tool.name.as_deref(),
            tool.phase.as_deref(),
            tool.phase_kind.as_deref(),
            tool.title.as_ref(),
            tool.input.as_ref(),
            tool.output.as_ref(),
        )?;
        let current = calls.children().item(position as u32);
        if !current
            .as_ref()
            .is_some_and(|current| current.is_same_node(Some(&details)))
        {
            calls.insert_before(&details, current.as_ref().map(|node| node.as_ref()))?;
        }
    }
    if tools.is_empty() {
        calls.set_attribute("hidden", "")?;
    } else {
        calls.remove_attribute("hidden")?;
    }
    Ok(())
}

fn create_tool(doc: &Document, id: &str) -> Result<Element, JsValue> {
    let details = doc.create_element("details")?;
    details.set_class_name("tool-call");
    details.set_attribute("data-tool-call", id)?;
    let summary = doc.create_element("summary")?;
    let chevron = doc.create_element("span")?;
    chevron.set_attribute("data-part", "tool-chevron")?;
    chevron.set_attribute("aria-hidden", "true")?;
    chevron.set_inner_html("<svg viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M6 9l6 6 6-6\"/></svg>");
    let name = doc.create_element("span")?;
    name.set_attribute("data-part", "tool-name")?;
    let phase = doc.create_element("span")?;
    phase.set_attribute("data-part", "tool-phase")?;
    summary.append_child(&chevron)?;
    summary.append_child(&name)?;
    summary.append_child(&phase)?;
    details.append_child(&summary)?;

    let body = doc.create_element("div")?;
    body.set_attribute("data-part", "tool-body")?;
    let title = doc.create_element("p")?;
    title.set_attribute("data-part", "tool-title")?;
    title.set_attribute("hidden", "")?;
    let title_incomplete = doc.create_element("p")?;
    title_incomplete.set_attribute("data-part", "tool-title-incomplete")?;
    title_incomplete.set_attribute("data-preview-incomplete", "")?;
    write_text(&title_incomplete, "Preview incomplete.");
    title_incomplete.set_attribute("hidden", "")?;
    let unavailable = doc.create_element("p")?;
    unavailable.set_attribute("data-part", "tool-unavailable")?;
    write_text(&unavailable, "Details unavailable.");
    let native_id = doc.create_element("p")?;
    native_id.set_attribute("data-part", "tool-id")?;
    let input = create_preview(doc, "tool-input", "Input")?;
    let output = create_preview(doc, "tool-output", "Output")?;
    body.append_child(&title)?;
    body.append_child(&title_incomplete)?;
    body.append_child(&unavailable)?;
    body.append_child(&native_id)?;
    body.append_child(&input)?;
    body.append_child(&output)?;
    details.append_child(&body)?;
    Ok(details)
}

fn create_preview(doc: &Document, part_name: &str, label: &str) -> Result<Element, JsValue> {
    let section = doc.create_element("section")?;
    section.set_attribute("data-part", part_name)?;
    let heading = doc.create_element("strong")?;
    write_text(&heading, label);
    let text = doc.create_element("pre")?;
    text.set_attribute("data-part", "preview-text")?;
    text.set_attribute("data-preview-text", "")?;
    let incomplete = doc.create_element("p")?;
    incomplete.set_attribute("data-part", "preview-incomplete")?;
    incomplete.set_attribute("data-preview-incomplete", "")?;
    write_text(&incomplete, "Preview incomplete.");
    section.append_child(&heading)?;
    section.append_child(&text)?;
    section.append_child(&incomplete)?;
    Ok(section)
}

fn update_tool(
    details: &Element,
    name_value: Option<&str>,
    phase_value: Option<&str>,
    phase_kind: Option<&str>,
    title: Option<&crate::model::ToolPreviewView>,
    input: Option<&crate::model::ToolPreviewView>,
    output: Option<&crate::model::ToolPreviewView>,
) -> Result<(), JsValue> {
    let name = part(details, "tool-name")?;
    let phase = part(details, "tool-phase")?;
    let unavailable = part(details, "tool-unavailable")?;
    match (name_value, phase_value) {
        (Some(name_value), Some(phase_value)) => {
            write_text(&name, name_value);
            write_text(&phase, phase_value);
            if let Some(phase_kind) = phase_kind {
                phase.set_attribute("data-phase", phase_kind)?;
            }
            unavailable.set_attribute("hidden", "")?;
        }
        _ => {
            write_text(&name, "Tool observed");
            write_text(&phase, "Details unavailable");
            phase.remove_attribute("data-phase")?;
            unavailable.remove_attribute("hidden")?;
        }
    }
    write_text(
        &part(details, "tool-id")?,
        details
            .get_attribute("data-tool-call")
            .as_deref()
            .unwrap_or(""),
    );
    let title_element = part(details, "tool-title")?;
    if let Some(title) = title {
        write_text(&title_element, &title.text);
        title_element.remove_attribute("hidden")?;
    } else {
        write_text(&title_element, "");
        title_element.set_attribute("hidden", "")?;
    }
    update_title_incomplete(details, title)?;
    update_preview(&part(details, "tool-input")?, input)?;
    update_preview(&part(details, "tool-output")?, output)?;
    Ok(())
}

fn update_title_incomplete(
    details: &Element,
    title: Option<&crate::model::ToolPreviewView>,
) -> Result<(), JsValue> {
    let incomplete = part(details, "tool-title-incomplete")?;
    if title.is_some_and(|title| title.truncated) {
        incomplete.remove_attribute("hidden")?;
    } else {
        incomplete.set_attribute("hidden", "")?;
    }
    Ok(())
}

fn update_preview(
    element: &Element,
    preview: Option<&crate::model::ToolPreviewView>,
) -> Result<(), JsValue> {
    if let Some(preview) = preview {
        element.remove_attribute("hidden")?;
        write_text(&part(element, "preview-text")?, &preview.text);
        let incomplete = part(element, "preview-incomplete")?;
        if preview.truncated {
            incomplete.remove_attribute("hidden")?;
        } else {
            incomplete.set_attribute("hidden", "")?;
        }
    } else {
        write_text(&part(element, "preview-text")?, "");
        element.set_attribute("hidden", "")?;
    }
    Ok(())
}

fn part(element: &Element, name: &str) -> Result<Element, JsValue> {
    element
        .query_selector(&format!("[data-part='{name}']"))?
        .ok_or_else(|| err("Tool detail field is missing."))
}

fn write_text(element: &Element, value: &str) {
    if element.text_content().as_deref() != Some(value) {
        element.set_text_content(Some(value));
    }
}
