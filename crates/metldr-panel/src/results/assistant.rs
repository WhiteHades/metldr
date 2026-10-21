use metldr_conversation::{Completion, Display, EntryRef, MessageKey};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, Text};

use super::{part, tools, write_text};
use crate::{
    err,
    model::{RunRoute, RunView},
};

enum Part<'a> {
    Leading(Vec<&'a str>),
    Message {
        key: &'a MessageKey,
        text: &'a str,
        completion: &'a Completion,
        tools: Vec<&'a str>,
    },
    Flat(&'a str),
}

fn parts(display: &Display) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    for entry in display.entries() {
        match entry {
            EntryRef::Message {
                key,
                text,
                completion,
            } => {
                parts.push(Part::Message {
                    key,
                    text,
                    completion,
                    tools: Vec::new(),
                });
            }
            EntryRef::Tool { id } => match parts.last_mut() {
                Some(Part::Message { tools, .. } | Part::Leading(tools)) => tools.push(id.as_str()),
                _ => parts.push(Part::Leading(vec![id.as_str()])),
            },
        }
    }
    if parts.is_empty() {
        parts.push(Part::Flat(display.text()));
    }
    parts
}

pub(super) fn render(doc: &Document, card: &Element, run: &RunView) -> Result<bool, JsValue> {
    let conversation = part(card, "conversation")?;
    let old_children = conversation.children();
    let existing: Vec<_> = (0..old_children.length())
        .filter_map(|i| old_children.item(i))
        .collect();
    let calls = card.query_selector_all("[data-tool-call]")?;
    let calls: Vec<_> = (0..calls.length())
        .filter_map(|i| calls.item(i)?.dyn_into::<Element>().ok())
        .collect();
    let mut retained = Vec::new();
    let mut retained_calls = Vec::new();
    let mut changed = false;
    for (position, item) in parts(&run.output).into_iter().enumerate() {
        let slot = match &item {
            Part::Message { key, .. } => key.dom_key(),
            Part::Leading(_) => "leading-tools".into(),
            Part::Flat(_) => "flat".into(),
        };
        let element = match existing
            .iter()
            .find(|node| node.get_attribute("data-slot").as_deref() == Some(&slot))
        {
            Some(node) => node.clone(),
            None => {
                let node = if matches!(&item, Part::Leading(_)) {
                    let template = doc.create_element("div")?;
                    template.set_inner_html(include_str!("assistant.html"));
                    part(&template, "message-tools")?
                } else {
                    let node = doc.create_element("article")?;
                    node.set_class_name("assistant-message");
                    node.set_inner_html(include_str!("assistant.html"));
                    node
                };
                node.set_attribute("data-slot", &slot)?;
                node
            }
        };
        let ids = match item {
            Part::Leading(ids) => {
                tools::reconcile(doc, &element, &ids, &run.activity, &calls)?;
                ids
            }
            Part::Message {
                key,
                text,
                completion,
                tools: ids,
            } => {
                element.set_attribute("data-message-key", &key.dom_key())?;
                let completion = match completion {
                    Completion::Streaming => "streaming",
                    Completion::FinalSnapshot => "final_snapshot",
                    Completion::StreamEnded => "stream_ended",
                    Completion::Interrupted => "interrupted",
                };
                element.set_attribute("data-completion", completion)?;
                let output = part(&element, "output")?;
                output.set_attribute("data-message-key", &key.dom_key())?;
                output.set_attribute("data-completion", completion)?;
                changed |= update_text(doc, &output, text)?;
                write_text(&part(&element, "message-label")?, name(&run.route));
                tools::reconcile(
                    doc,
                    &part(&element, "message-tools")?,
                    &ids,
                    &run.activity,
                    &calls,
                )?;
                ids
            }
            Part::Flat(text) => {
                changed |= update_text(doc, &part(&element, "output")?, text)?;
                write_text(&part(&element, "message-label")?, name(&run.route));
                let mut ids = Vec::new();
                for report in &run.activity {
                    if let Some(call) = &report.call {
                        if !ids.contains(&call.id.as_str()) {
                            ids.push(call.id.as_str());
                        }
                    }
                }
                tools::reconcile(
                    doc,
                    &part(&element, "message-tools")?,
                    &ids,
                    &run.activity,
                    &calls,
                )?;
                ids
            }
        };
        retained_calls.extend(ids);
        let current = conversation.children().item(position as u32);
        if !current
            .as_ref()
            .is_some_and(|current| current.is_same_node(Some(&element)))
        {
            conversation.insert_before(&element, current.as_ref().map(|node| node.as_ref()))?;
        }
        retained.push(element);
    }
    for node in existing {
        if !retained.iter().any(|kept| kept.is_same_node(Some(&node))) {
            conversation.remove_child(&node)?;
            changed = true;
        }
    }
    for call in calls {
        if !call
            .get_attribute("data-tool-call")
            .is_some_and(|id| retained_calls.contains(&id.as_str()))
        {
            call.remove();
        }
    }
    Ok(changed)
}

fn name(route: &RunRoute) -> &'static str {
    match route {
        RunRoute::Text { protocol, .. } => match protocol {
            crate::model::ProviderProtocol::Messages => "Messages page questions",
            crate::model::ProviderProtocol::Responses => "OpenAI Responses page questions",
            crate::model::ProviderProtocol::GoogleInteractions => {
                "Google Interactions page questions"
            }
            crate::model::ProviderProtocol::ChatCompletions => "Chat Completions page questions",
        },
        RunRoute::Codex { .. } => "Codex",
        RunRoute::OpenCode { .. } => "OpenCode",
        RunRoute::Pi { .. } => "Pi",
        _ => "metldr",
    }
}

fn update_text(doc: &Document, output: &Element, value: &str) -> Result<bool, JsValue> {
    let text = match output.first_child() {
        Some(node) => node
            .dyn_into::<Text>()
            .map_err(|_| err("Result message is not text."))?,
        None => {
            let text = doc.create_text_node("");
            output.append_child(&text)?;
            text
        }
    };
    let previous = text.data();
    if previous == value {
        return Ok(false);
    }
    if let Some(suffix) = value.strip_prefix(&previous) {
        text.append_data(suffix)?;
    } else {
        text.set_data(value);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use metldr_conversation::Projection;

    #[test]
    fn groups_only_observed_tools_under_the_preceding_message() {
        let projection: Projection = serde_json::from_value(serde_json::json!({
            "output":"beforeafter",
            "conversation":[
                {"kind":"tool","id":"lead"},
                {"kind":"message","key":{"kind":"observed","ordinal":1},"start":0,"end":6,"completion":"streaming"},
                {"kind":"tool","id":"actual"},
                {"kind":"message","key":{"kind":"observed","ordinal":2},"start":6,"end":11,"completion":"final_snapshot"}
            ]
        })).unwrap();
        let display = Display::new(projection).unwrap();
        let grouped = parts(&display);
        assert!(matches!(&grouped[0], Part::Leading(ids) if ids == &["lead"]));
        assert!(
            matches!(&grouped[1], Part::Message { text, tools, .. } if *text == "before" && tools == &["actual"])
        );
        assert!(
            matches!(&grouped[2], Part::Message { text, tools, .. } if *text == "after" && tools.is_empty())
        );
        let display = Display::new(Projection {
            output: "flat".into(),
            conversation: Vec::new(),
        })
        .unwrap();
        assert!(matches!(parts(&display).as_slice(), [Part::Flat("flat")]));
        let display = Display::new(
            serde_json::from_value(
                serde_json::json!({"output":"","conversation":[{"kind":"tool","id":"only"}]}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(parts(&display).as_slice(), [Part::Leading(ids)] if ids == &["only"]));
    }
}
