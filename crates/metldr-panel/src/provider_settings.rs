use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, Event, HtmlInputElement};

use crate::{
    FlowClient, err,
    model::{FlowError, ProviderConnectionState, ProviderProtocol, ProviderSettingsView},
};

thread_local! { static PANEL: RefCell<Option<Rc<SettingsPanel>>> = const { RefCell::new(None) }; }

struct Draft {
    dirty: bool,
    expected_revision: u64,
    version: u64,
}

struct SettingsPanel {
    document: Document,
    panel: Weak<RefCell<super::Panel>>,
    generation: Cell<u64>,
    drafts: RefCell<[Draft; ProviderProtocol::ALL.len()]>,
    mutation_busy: Cell<bool>,
    mutation_epoch: Cell<u64>,
    server_busy: Cell<bool>,
    unknown: Cell<bool>,
    unknown_store: RefCell<Option<String>>,
}

pub(super) fn install(
    document: &Document,
    panel: &Rc<RefCell<super::Panel>>,
) -> Result<(), JsValue> {
    let state = Rc::new(SettingsPanel {
        document: document.clone(),
        panel: Rc::downgrade(panel),
        generation: Cell::new(panel.borrow().connection_generation),
        drafts: RefCell::new(std::array::from_fn(|_| Draft {
            dirty: false,
            expected_revision: 0,
            version: 0,
        })),
        mutation_busy: Cell::new(false),
        mutation_epoch: Cell::new(0),
        server_busy: Cell::new(false),
        unknown: Cell::new(false),
        unknown_store: RefCell::new(None),
    });
    for protocol in ProviderProtocol::ALL {
        for suffix in ["endpoint", "model", "key"] {
            let id = control_id(protocol, suffix);
            let changed_state = Rc::clone(&state);
            let change = Closure::<dyn FnMut(Event)>::new(move |_| {
                let mut drafts = changed_state.drafts.borrow_mut();
                let draft = &mut drafts[index(protocol)];
                if !draft.dirty {
                    draft.expected_revision =
                        row_revision(&changed_state.document, protocol).unwrap_or(0);
                    draft.dirty = true;
                }
                draft.version = draft.version.wrapping_add(1);
            });
            document
                .get_element_by_id(&id)
                .ok_or_else(|| err("Provider setting control missing."))?
                .add_event_listener_with_callback("input", change.as_ref().unchecked_ref())?;
            change.forget();
        }
        for (action, kind) in [("save", 0), ("remove", 1), ("reset", 2)] {
            let id = format!("provider-{}-{action}", protocol.name());
            let action_state = Rc::clone(&state);
            let click = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
                event.prevent_default();
                match kind {
                    0 => save_connection(Rc::clone(&action_state), protocol),
                    1 => remove_connection(Rc::clone(&action_state), protocol),
                    _ => use_saved_values(Rc::clone(&action_state), protocol),
                }
            });
            document
                .get_element_by_id(&id)
                .ok_or_else(|| err("Provider setting action missing."))?
                .add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
            click.forget();
        }
    }
    let refresh_state = Rc::clone(&state);
    let refresh = Closure::<dyn FnMut(Event)>::new(move |_| refresh(Rc::clone(&refresh_state)));
    document
        .get_element_by_id("provider-refresh")
        .ok_or_else(|| err("Provider refresh action missing."))?
        .add_event_listener_with_callback("click", refresh.as_ref().unchecked_ref())?;
    refresh.forget();
    PANEL.with(|slot| *slot.borrow_mut() = Some(state));
    Ok(())
}

pub(super) fn render(
    document: &Document,
    view: &ProviderSettingsView,
    generation: u64,
) -> Result<(), JsValue> {
    PANEL.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            sync_generation(state, generation);
            state.server_busy.set(view.busy || view.pending.is_some());
        }
    });
    let section = document
        .get_element_by_id("provider-settings")
        .ok_or_else(|| err("Provider settings section missing."))?;
    section.remove_attribute("hidden")?;
    for connection in &view.connections {
        let protocol = connection.protocol;
        let status = match connection.state {
            ProviderConnectionState::Unmanaged => "Not saved here",
            ProviderConnectionState::Removed => "Disabled",
            ProviderConnectionState::Loading => "Loading saved connection",
            ProviderConnectionState::Ready => "Ready",
            ProviderConnectionState::Missing => "Credential missing",
            ProviderConnectionState::Locked => "Credential store locked",
            ProviderConnectionState::Corrupt => "Saved credential is corrupt",
            ProviderConnectionState::Unavailable => "Connection unavailable",
        };
        set_text(
            document,
            &format!("provider-{}-status", protocol.name()),
            &format!("{status}. Revision {}.", connection.revision),
        )?;
        if let Some(element) =
            document.get_element_by_id(&format!("provider-{}-status", protocol.name()))
        {
            element.set_attribute("data-revision", &connection.revision.to_string())?;
        }
        set_text(
            document,
            &format!("provider-{}-metadata", protocol.name()),
            &format!(
                "{}. {}.",
                connection.endpoint.as_deref().unwrap_or("No endpoint"),
                connection.model.as_deref().unwrap_or("No model")
            ),
        )?;
        PANEL.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                let draft = &mut state.drafts.borrow_mut()[index(protocol)];
                if !draft.dirty {
                    draft.expected_revision = connection.revision;
                    set_value(
                        document,
                        protocol,
                        "endpoint",
                        connection.endpoint.as_deref().unwrap_or(""),
                    );
                    set_value(
                        document,
                        protocol,
                        "model",
                        connection.model.as_deref().unwrap_or(""),
                    );
                }
            }
        });
    }
    set_text(
        document,
        "provider-settings-notice",
        view.notice.as_deref().unwrap_or(""),
    )?;
    let busy = PANEL.with(|slot| {
        slot.borrow().as_ref().is_some_and(|state| {
            state.mutation_busy.get() || state.server_busy.get() || state.unknown.get()
        })
    });
    for protocol in ProviderProtocol::ALL {
        for action in ["save", "remove"] {
            if let Some(button) =
                document.get_element_by_id(&format!("provider-{}-{action}", protocol.name()))
            {
                if busy {
                    button.set_attribute("disabled", "")?;
                } else {
                    button.remove_attribute("disabled")?;
                }
            }
        }
    }
    let refresh_busy = PANEL.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|state| state.mutation_busy.get() || state.server_busy.get())
    });
    if let Some(button) = document.get_element_by_id("provider-refresh") {
        if refresh_busy {
            button.set_attribute("disabled", "")?;
        } else {
            button.remove_attribute("disabled")?;
        }
    }
    Ok(())
}

pub(super) fn mutation_epoch() -> u64 {
    PANEL.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(0, |state| state.mutation_epoch.get())
    })
}

pub(super) fn reconcile_current_page(
    panel: &Rc<RefCell<super::Panel>>,
    view: &crate::model::PageView,
    generation: u64,
    read_epoch: u64,
) {
    let current = panel.borrow();
    if current.connection_generation != generation
        || current
            .snapshot
            .as_ref()
            .is_some_and(|old| view.is_older_than(old))
    {
        return;
    }
    drop(current);
    PANEL.with(|slot| {
        let state = slot.borrow().as_ref().cloned();
        let Some(state) = state else {
            return;
        };
        sync_generation(&state, generation);
        let idle = !view.provider_settings.busy && view.provider_settings.pending.is_none();
        let same_store = state.unknown_store.borrow().as_deref() == Some(view.store.as_str());
        if state.unknown.get() && idle && same_store && state.mutation_epoch.get() == read_epoch {
            state.unknown.set(false);
            *state.unknown_store.borrow_mut() = None;
        }
    });
}

fn context(
    state: &SettingsPanel,
) -> Option<(Rc<RefCell<super::Panel>>, FlowClient, u64, Option<String>)> {
    let panel = state.panel.upgrade()?;
    let current = panel.borrow();
    Some((
        panel.clone(),
        current.flow.clone(),
        current.connection_generation,
        current.snapshot.as_ref().map(|view| view.store.clone()),
    ))
}

fn sync_generation(state: &SettingsPanel, generation: u64) {
    if state.generation.replace(generation) != generation {
        if state.mutation_busy.replace(false) {
            state.unknown.set(true);
        }
        for protocol in ProviderProtocol::ALL {
            clear_key(&state.document, protocol);
        }
    }
}

fn save_connection(state: Rc<SettingsPanel>, protocol: ProviderProtocol) {
    if state.mutation_busy.get() || state.server_busy.get() || state.unknown.get() {
        return;
    }
    let Some((panel, flow, generation, store)) = context(&state) else {
        return;
    };
    state.mutation_busy.set(true);
    state
        .mutation_epoch
        .set(state.mutation_epoch.get().wrapping_add(1));
    *state.unknown_store.borrow_mut() = store.clone();
    let endpoint = value(&state.document, protocol, "endpoint");
    let model = value(&state.document, protocol, "model");
    let key_element = state
        .document
        .get_element_by_id(&control_id(protocol, "key"))
        .and_then(|element| element.dyn_into::<HtmlInputElement>().ok());
    let Some(key_element) = key_element else {
        state.mutation_busy.set(false);
        return;
    };
    let key = key_element.value();
    key_element.set_value("");
    let (revision, version) = {
        let drafts = state.drafts.borrow();
        let draft = &drafts[index(protocol)];
        (draft.expected_revision, draft.version)
    };
    status(&state.document, "Saving provider connection.");
    update_controls(&state);
    spawn_local(async move {
        let result = flow
            .provider_save(
                protocol,
                revision,
                endpoint,
                model,
                (!key.is_empty()).then_some(key),
            )
            .await;
        finish(
            state,
            panel,
            flow,
            generation,
            store,
            result,
            Some((protocol, version)),
            false,
        )
        .await;
    });
}

fn remove_connection(state: Rc<SettingsPanel>, protocol: ProviderProtocol) {
    if state.mutation_busy.get() || state.server_busy.get() || state.unknown.get() {
        return;
    }
    let Some((panel, flow, generation, store)) = context(&state) else {
        return;
    };
    state.mutation_busy.set(true);
    state
        .mutation_epoch
        .set(state.mutation_epoch.get().wrapping_add(1));
    *state.unknown_store.borrow_mut() = store.clone();
    clear_key(&state.document, protocol);
    let (revision, version) = {
        let drafts = state.drafts.borrow();
        let draft = &drafts[index(protocol)];
        (draft.expected_revision, draft.version)
    };
    status(&state.document, "Disabling provider connection.");
    update_controls(&state);
    spawn_local(async move {
        let result = flow.provider_remove(protocol, revision).await;
        finish(
            state,
            panel,
            flow,
            generation,
            store,
            result,
            Some((protocol, version)),
            false,
        )
        .await;
    });
}

fn use_saved_values(state: Rc<SettingsPanel>, protocol: ProviderProtocol) {
    let saved = state
        .panel
        .upgrade()
        .and_then(|panel| panel.borrow().snapshot.clone())
        .and_then(|view| {
            view.provider_settings
                .connections
                .into_iter()
                .find(|connection| connection.protocol == protocol)
        });
    let Some(connection) = saved else {
        status(
            &state.document,
            "No saved view is available. Refresh connections.",
        );
        return;
    };
    {
        let mut drafts = state.drafts.borrow_mut();
        let draft = &mut drafts[index(protocol)];
        draft.dirty = false;
        draft.expected_revision = connection.revision;
        draft.version = draft.version.wrapping_add(1);
    }
    set_value(
        &state.document,
        protocol,
        "endpoint",
        connection.endpoint.as_deref().unwrap_or(""),
    );
    set_value(
        &state.document,
        protocol,
        "model",
        connection.model.as_deref().unwrap_or(""),
    );
    clear_key(&state.document, protocol);
    status(
        &state.document,
        "Draft discarded. Saved values loaded from the current view.",
    );
}

fn refresh(state: Rc<SettingsPanel>) {
    if state.server_busy.get() || state.mutation_busy.replace(true) {
        return;
    }
    let Some((panel, flow, generation, store)) = context(&state) else {
        state.mutation_busy.set(false);
        return;
    };
    status(&state.document, "Refreshing provider connections.");
    update_controls(&state);
    spawn_local(async move {
        let result = flow.provider_refresh().await;
        finish(state, panel, flow, generation, store, result, None, true).await;
    });
}

async fn finish(
    state: Rc<SettingsPanel>,
    panel: Rc<RefCell<super::Panel>>,
    flow: FlowClient,
    generation: u64,
    store: Option<String>,
    result: Result<crate::model::PageView, FlowError>,
    mutation: Option<(ProviderProtocol, u64)>,
    explicit_refresh: bool,
) {
    if panel.borrow().connection_generation != generation {
        return;
    }
    let read_epoch = state.mutation_epoch.get();
    if let Some((protocol, _)) = mutation {
        clear_key(&state.document, protocol);
    }
    match result {
        Ok(view) => {
            state.mutation_busy.set(false);
            if let Some((protocol, version)) = mutation {
                state.unknown.set(false);
                *state.unknown_store.borrow_mut() = None;
                let draft = &mut state.drafts.borrow_mut()[index(protocol)];
                if draft.version == version {
                    draft.dirty = false;
                }
            } else if explicit_refresh
                && panel
                    .borrow()
                    .snapshot
                    .as_ref()
                    .is_none_or(|old| !view.is_older_than(old))
                && state
                    .unknown_store
                    .borrow()
                    .as_ref()
                    .is_none_or(|store| store == &view.store)
            {
                state
                    .unknown
                    .set(view.provider_settings.busy || view.provider_settings.pending.is_some());
                if !state.unknown.get() {
                    *state.unknown_store.borrow_mut() = None;
                }
            }
            super::apply_view(&panel, view, generation);
            status(
                &state.document,
                if explicit_refresh && state.unknown.get() {
                    "Provider outcome remains unknown. Refresh connections to reconcile it."
                } else {
                    "Provider settings updated."
                },
            );
        }
        Err(error)
            if matches!(
                error,
                FlowError::Timeout | FlowError::Disconnected | FlowError::Protocol(_)
            ) && mutation.is_some() =>
        {
            state.unknown.set(true);
            *state.unknown_store.borrow_mut() = store.clone();
            status(
                &state.document,
                "No receipt arrived. Checking the saved provider view before another change.",
            );
            let refreshed = flow.current_page().await;
            if panel.borrow().connection_generation != generation {
                return;
            }
            match refreshed {
                Ok(view) => {
                    let settings = &view.provider_settings;
                    let idle = !settings.busy && settings.pending.is_none();
                    clear_key(&state.document, mutation.unwrap().0);
                    reconcile_current_page(&panel, &view, generation, read_epoch);
                    super::apply_view(&panel, view, generation);
                    if !idle {
                        state.unknown.set(true);
                    }
                    status(
                        &state.document,
                        if state.unknown.get() {
                            "Provider outcome remains unknown. Refresh connections to reconcile it."
                        } else {
                            "Provider view reconciled. Review it before another change."
                        },
                    );
                }
                Err(_) => status(
                    &state.document,
                    "Provider outcome is unknown. Reconnect and refresh before another change.",
                ),
            }
            clear_key(&state.document, mutation.unwrap().0);
            state.mutation_busy.set(false);
        }
        Err(error) => {
            state.mutation_busy.set(false);
            status(
                &state.document,
                &format!("Provider settings failed: {error}"),
            );
        }
    }
    update_controls(&state);
}

fn update_controls(state: &SettingsPanel) {
    let disabled = state.mutation_busy.get() || state.server_busy.get() || state.unknown.get();
    for protocol in ProviderProtocol::ALL {
        for action in ["save", "remove"] {
            if let Some(button) = state
                .document
                .get_element_by_id(&format!("provider-{}-{action}", protocol.name()))
            {
                if disabled {
                    let _ = button.set_attribute("disabled", "");
                } else {
                    let _ = button.remove_attribute("disabled");
                }
            }
        }
    }
    if let Some(button) = state.document.get_element_by_id("provider-refresh") {
        if state.mutation_busy.get() || state.server_busy.get() {
            let _ = button.set_attribute("disabled", "");
        } else {
            let _ = button.remove_attribute("disabled");
        }
    }
}

fn control_id(protocol: ProviderProtocol, suffix: &str) -> String {
    format!("provider-{}-{suffix}", protocol.name())
}
fn index(protocol: ProviderProtocol) -> usize {
    ProviderProtocol::ALL
        .iter()
        .position(|item| *item == protocol)
        .unwrap_or(0)
}
fn value(document: &Document, protocol: ProviderProtocol, suffix: &str) -> String {
    document
        .get_element_by_id(&control_id(protocol, suffix))
        .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
        .map(|element| element.value())
        .unwrap_or_default()
}
fn set_value(document: &Document, protocol: ProviderProtocol, suffix: &str, value: &str) {
    if let Some(element) = document
        .get_element_by_id(&control_id(protocol, suffix))
        .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
    {
        element.set_value(value);
    }
}
fn clear_key(document: &Document, protocol: ProviderProtocol) {
    set_value(document, protocol, "key", "");
}
fn row_revision(document: &Document, protocol: ProviderProtocol) -> Option<u64> {
    document
        .get_element_by_id(&format!("provider-{}-status", protocol.name()))
        .and_then(|element| element.get_attribute("data-revision"))?
        .parse()
        .ok()
}
fn set_text(document: &Document, id: &str, value: &str) -> Result<(), JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| err("Provider setting display missing."))?
        .set_text_content(Some(value));
    Ok(())
}
fn status(document: &Document, message: &str) {
    if let Some(status) = document.get_element_by_id("provider-settings-status") {
        status.set_text_content(Some(message));
    }
}
