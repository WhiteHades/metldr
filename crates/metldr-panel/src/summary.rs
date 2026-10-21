use crate::{
    Panel, active_run, begin_start,
    composer::{self, Destination},
    flow::PageTextPrompt,
    follow_start,
    model::{FlowError, ProviderProtocol},
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlSelectElement;

enum Target {
    Fixture,
    Provider {
        protocol: ProviderProtocol,
        fingerprint: String,
    },
}

fn target(panel: &Panel) -> Option<Target> {
    match composer::selected(panel) {
        Ok(Destination::PageText {
            protocol,
            fingerprint,
        }) => Some(Target::Provider {
            protocol,
            fingerprint,
        }),
        Ok(Destination::Model) => Some(Target::Fixture),
        _ if panel
            .snapshot
            .as_ref()
            .is_some_and(|view| view.fixture_available)
            && panel
                .document
                .get_element_by_id("execution")
                .and_then(|element| element.dyn_into::<HtmlSelectElement>().ok())
                .is_some_and(|select| select.value().is_empty()) =>
        {
            Some(Target::Fixture)
        }
        _ => None,
    }
}

pub(super) fn render(panel: &Panel) -> Result<(), JsValue> {
    let Some(button) = panel.document.query_selector("[data-action='summarize']")? else {
        return Ok(());
    };
    let target = target(panel);
    button.set_text_content(Some(if matches!(target, Some(Target::Fixture)) {
        "Capture and summarize with test model"
    } else {
        "Summarize current page"
    }));
    if target.is_some() {
        button.remove_attribute("hidden")?;
    } else {
        button.set_attribute("hidden", "")?;
    }
    if target.is_none()
        || !panel.connected
        || panel.start_pending
        || active_run(&panel.snapshot).is_some()
    {
        button.set_attribute("disabled", "")?;
    } else {
        button.remove_attribute("disabled")?;
    }
    Ok(())
}

pub(super) fn start(panel: &Rc<RefCell<Panel>>) -> Result<(), FlowError> {
    let (flow, generation, target) = {
        let state = panel.borrow();
        if !state.connected || state.start_pending || active_run(&state.snapshot).is_some() {
            return Ok(());
        }
        let target = target(&state).ok_or_else(|| {
            FlowError::InvalidInput("Choose a page questions provider before summarizing.".into())
        })?;
        (state.flow.clone(), state.connection_generation, target)
    };
    begin_start(panel);
    match target {
        Target::Fixture => {
            if let Some(select) = panel
                .borrow()
                .document
                .get_element_by_id("execution")
                .and_then(|element| element.dyn_into::<HtmlSelectElement>().ok())
            {
                select.set_value("model");
            }
            let start = flow.summarize_selected();
            let panel = Rc::clone(panel);
            spawn_local(async move {
                follow_start(panel, flow, generation, start.await, "Capture failed").await;
            });
        }
        Target::Provider {
            protocol,
            fingerprint,
        } => {
            let capture = flow.capture_selected();
            let panel = Rc::clone(panel);
            spawn_local(async move {
                let result = async {
                    let receipt = capture.await?;
                    let source = receipt.source().ok_or(FlowError::NoSource)?.clone();
                    flow.page_text(source, protocol, fingerprint, PageTextPrompt::Summary)
                        .await
                }
                .await;
                follow_start(panel, flow, generation, result, "Summary failed").await;
            });
        }
    }
    Ok(())
}
