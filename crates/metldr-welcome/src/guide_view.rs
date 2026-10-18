use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{Document, Event, HtmlInputElement, HtmlSelectElement};

use crate::{dom, guide};

pub fn mount(document: &Document) -> Result<(), JsValue> {
    let select = dom::element(document, "faq-category")?;
    option(
        document,
        &select,
        "all",
        &format!("All topics ({})", guide::FAQS.len()),
    )?;
    for (category, id, label) in guide::CATEGORIES {
        let count = guide::filter_faqs(Some(category), "").count();
        option(document, &select, id, &format!("{label} ({count})"))?;
    }
    for name in ["input", "change", "click"] {
        let document = document.clone();
        let events = document.clone();
        let callback = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            let id = event
                .target()
                .and_then(|v| v.dyn_into::<web_sys::Element>().ok())
                .map(|v| v.id());
            let result = match id.as_deref() {
                Some("faq-search" | "faq-category") => refresh(&events),
                Some("faq-clear") if event.type_() == "click" => clear(&events),
                _ => return,
            };
            if result.is_err() {
                let _ = dom::text(
                    &events,
                    "faq-count",
                    "Search could not update. Reload to try again.",
                );
            }
        });
        document.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
        callback.forget();
    }
    refresh(document)?;
    for id in ["faq-search", "faq-category", "faq-clear"] {
        dom::element(document, id)?.remove_attribute("disabled")?;
    }
    dom::element(document, "faq-filters")?.remove_attribute("hidden")?;
    Ok(())
}

fn option(
    document: &Document,
    select: &web_sys::Element,
    value: &str,
    label: &str,
) -> Result<(), JsValue> {
    let option = document.create_element("option")?;
    option.set_attribute("value", value)?;
    option.set_text_content(Some(label));
    select.append_child(&option)?;
    Ok(())
}

fn refresh(document: &Document) -> Result<(), JsValue> {
    let query = dom::element(document, "faq-search")?
        .dyn_into::<HtmlInputElement>()?
        .value();
    let value = dom::element(document, "faq-category")?
        .dyn_into::<HtmlSelectElement>()?
        .value();
    let category = guide::CATEGORIES
        .iter()
        .find(|(_, id, _)| *id == value)
        .map(|(category, _, _)| *category);
    let matches: Vec<_> = guide::filter_faqs(category, &query).collect();
    for (index, faq) in guide::FAQS.iter().enumerate() {
        let detail = dom::element(document, &format!("faq-{index}"))?;
        if matches.contains(&faq) {
            detail.remove_attribute("hidden")?;
        } else {
            detail.set_attribute("hidden", "")?;
            detail.remove_attribute("open")?;
        }
    }
    let count = matches.len();
    dom::text(
        document,
        "faq-count",
        &format!("{count} {}", if count == 1 { "answer" } else { "answers" }),
    )?;
    let empty = dom::element(document, "faq-empty")?;
    if count == 0 {
        empty.remove_attribute("hidden")?;
    } else {
        empty.set_attribute("hidden", "")?;
    }
    Ok(())
}

fn clear(document: &Document) -> Result<(), JsValue> {
    let search = dom::element(document, "faq-search")?.dyn_into::<HtmlInputElement>()?;
    search.set_value("");
    dom::element(document, "faq-category")?
        .dyn_into::<HtmlSelectElement>()?
        .set_value("all");
    refresh(document)?;
    search.focus()
}
