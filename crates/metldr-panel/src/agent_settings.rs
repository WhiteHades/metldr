use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, Event, HtmlInputElement};

use crate::{
    FlowClient, err,
    model::{AgentKind, AgentProfile, AgentSettingState, FlowError, PageView},
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
    drafts: RefCell<[Draft; 3]>,
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
    for agent in AgentKind::ALL {
        for suffix in if agent == AgentKind::Pi {
            vec![
                "executable",
                "workspace",
                "session_dir",
                "provider",
                "model",
                "offline",
            ]
        } else {
            vec!["executable", "workspace"]
        } {
            let id = control_id(agent, suffix);
            let changed = Rc::clone(&state);
            let event_name = if suffix == "offline" {
                "change"
            } else {
                "input"
            };
            let listener = Closure::<dyn FnMut(Event)>::new(move |_| {
                let mut drafts = changed.drafts.borrow_mut();
                let draft = &mut drafts[index(agent)];
                if !draft.dirty {
                    draft.expected_revision = row_revision(&changed.document, agent).unwrap_or(0);
                    draft.dirty = true;
                }
                draft.version = draft.version.wrapping_add(1);
            });
            state
                .document
                .get_element_by_id(&id)
                .ok_or_else(|| err("Agent setting control missing."))?
                .add_event_listener_with_callback(event_name, listener.as_ref().unchecked_ref())?;
            listener.forget();
        }
        for (action, kind) in [("save", 0), ("remove", 1), ("reset", 2)] {
            let id = control_id(agent, action);
            let action_state = Rc::clone(&state);
            let listener = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
                event.prevent_default();
                match kind {
                    0 => save(Rc::clone(&action_state), agent),
                    1 => remove(Rc::clone(&action_state), agent),
                    _ => use_saved(Rc::clone(&action_state), agent),
                }
            });
            state
                .document
                .get_element_by_id(&id)
                .ok_or_else(|| err("Agent setting action missing."))?
                .add_event_listener_with_callback("click", listener.as_ref().unchecked_ref())?;
            listener.forget();
        }
    }
    let action_state = Rc::clone(&state);
    let listener =
        Closure::<dyn FnMut(Event)>::new(move |_| refresh_agents(Rc::clone(&action_state)));
    state
        .document
        .get_element_by_id("agent-refresh")
        .ok_or_else(|| err("Agent refresh action missing."))?
        .add_event_listener_with_callback("click", listener.as_ref().unchecked_ref())?;
    listener.forget();
    PANEL.with(|slot| *slot.borrow_mut() = Some(state));
    Ok(())
}

pub(super) fn render(
    document: &Document,
    view: &PageView,
    generation: u64,
    start_pending: bool,
    conversation_busy: bool,
) -> Result<(), JsValue> {
    let busy = view.agent_settings.busy
        || view.provider_settings.busy
        || view.provider_settings.pending.is_some()
        || start_pending
        || view.active_run().is_some();
    PANEL.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            sync_generation(state, generation);
            state.server_busy.set(busy);
        }
    });
    document
        .get_element_by_id("agent-settings")
        .ok_or_else(|| err("Agent settings section missing."))?
        .remove_attribute("hidden")?;
    for row in &view.agent_settings.agents {
        let agent = row.agent;
        let state_label = match row.state {
            AgentSettingState::Unmanaged => "Not configured",
            AgentSettingState::Ready => "Ready",
            AgentSettingState::Removed => "Disabled",
            AgentSettingState::Unavailable => "Unavailable; saved paths are editable",
        };
        set_text(
            document,
            &control_id(agent, "status"),
            &format!("{state_label}. Revision {}.", row.revision),
        )?;
        if let Some(status) = document.get_element_by_id(&control_id(agent, "status")) {
            status.set_attribute("data-revision", &row.revision.to_string())?;
        }
        PANEL.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                let draft = &mut state.drafts.borrow_mut()[index(agent)];
                if !draft.dirty {
                    draft.expected_revision = row.revision;
                    if let Some(profile) = &row.profile {
                        write_profile(document, profile);
                    } else {
                        clear_profile(document, agent);
                    }
                }
            }
        });
    }
    let disabled = PANEL.with(|slot| {
        slot.borrow().as_ref().is_some_and(|state| {
            state.mutation_busy.get()
                || state.server_busy.get()
                || state.unknown.get()
                || conversation_busy
        })
    });
    let refresh_disabled = PANEL.with(|slot| {
        slot.borrow().as_ref().is_some_and(|state| {
            state.mutation_busy.get() || state.server_busy.get() || conversation_busy
        })
    });
    for agent in AgentKind::ALL {
        for action in ["save", "remove"] {
            if let Some(button) = document.get_element_by_id(&control_id(agent, action)) {
                if disabled {
                    button.set_attribute("disabled", "")?;
                } else {
                    button.remove_attribute("disabled")?;
                }
            }
        }
    }
    if let Some(button) = document.get_element_by_id("agent-refresh") {
        if refresh_disabled {
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
    view: &PageView,
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
        if let Some(state) = slot.borrow().as_ref() {
            sync_generation(state, generation);
            let same_store = state.unknown_store.borrow().as_deref() == Some(view.store.as_str());
            if state.unknown.get()
                && !view.agent_settings.busy
                && same_store
                && state.mutation_epoch.get() == read_epoch
            {
                state.unknown.set(false);
                *state.unknown_store.borrow_mut() = None;
            }
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
    if state.generation.replace(generation) != generation && state.mutation_busy.replace(false) {
        state.unknown.set(true);
    }
}
fn can_mutate(state: &SettingsPanel) -> bool {
    !state.mutation_busy.get()
        && !state.server_busy.get()
        && !state.unknown.get()
        && state
            .panel
            .upgrade()
            .is_some_and(|panel| !crate::conversations::settings_blocked(&panel.borrow()))
}

pub(super) fn interlocked() -> bool {
    PANEL.with(|slot| {
        slot.borrow().as_ref().is_some_and(|state| {
            state.mutation_busy.get() || state.server_busy.get() || state.unknown.get()
        })
    })
}
fn start_mutation(state: &SettingsPanel, store: Option<String>) {
    state.mutation_busy.set(true);
    state
        .mutation_epoch
        .set(state.mutation_epoch.get().wrapping_add(1));
    *state.unknown_store.borrow_mut() = store;
    update_controls(state);
}
fn save(state: Rc<SettingsPanel>, agent: AgentKind) {
    if !can_mutate(&state) {
        return;
    }
    let Some((panel, flow, generation, store)) = context(&state) else {
        return;
    };
    if crate::conversations::settings_blocked(&panel.borrow()) {
        return;
    }
    let profile = read_profile(&state.document, agent);
    let (revision, version) = {
        let drafts = state.drafts.borrow();
        let draft = &drafts[index(agent)];
        (draft.expected_revision, draft.version)
    };
    start_mutation(&state, store.clone());
    set_status(&state.document, "Saving agent settings.");
    spawn_local(async move {
        let result = flow.agent_save(revision, profile).await;
        finish(
            state,
            panel,
            flow,
            generation,
            store,
            result,
            Some((agent, version)),
        )
        .await;
    });
}
fn remove(state: Rc<SettingsPanel>, agent: AgentKind) {
    if !can_mutate(&state) {
        return;
    }
    let Some((panel, flow, generation, store)) = context(&state) else {
        return;
    };
    let (revision, version) = {
        let drafts = state.drafts.borrow();
        let draft = &drafts[index(agent)];
        (draft.expected_revision, draft.version)
    };
    start_mutation(&state, store.clone());
    set_status(&state.document, "Disabling agent settings.");
    spawn_local(async move {
        let result = flow.agent_remove(agent, revision).await;
        finish(
            state,
            panel,
            flow,
            generation,
            store,
            result,
            Some((agent, version)),
        )
        .await;
    });
}
fn refresh_agents(state: Rc<SettingsPanel>) {
    if state.mutation_busy.get() || state.server_busy.get() {
        return;
    }
    let Some((panel, flow, generation, store)) = context(&state) else {
        return;
    };
    if crate::conversations::settings_blocked(&panel.borrow()) {
        return;
    }
    state.mutation_busy.set(true);
    let read_epoch = state.mutation_epoch.get();
    update_controls(&state);
    set_status(&state.document, "Refreshing agent availability.");
    spawn_local(async move {
        let result = flow.agent_refresh().await;
        if panel.borrow().connection_generation != generation {
            return;
        }
        match result {
            Ok(view) => {
                reconcile_current_page(&panel, &view, generation, read_epoch);
                state.mutation_busy.set(false);
                super::apply_view(&panel, view, generation);
                set_status(
                    &state.document,
                    if state.unknown.get() {
                        "Agent outcome remains unknown. Reconnect and review the saved view before retrying."
                    } else {
                        "Agent settings refreshed."
                    },
                );
            }
            Err(error) => {
                state.mutation_busy.set(false);
                set_status(&state.document, &format!("Agent refresh failed: {error}"));
            }
        }
        let _ = store;
        update_controls(&state);
    });
}
async fn finish(
    state: Rc<SettingsPanel>,
    panel: Rc<RefCell<super::Panel>>,
    flow: FlowClient,
    generation: u64,
    store: Option<String>,
    result: Result<PageView, FlowError>,
    mutation: Option<(AgentKind, u64)>,
) {
    if panel.borrow().connection_generation != generation {
        return;
    }
    match result {
        Ok(view) => {
            state.mutation_busy.set(false);
            state.unknown.set(false);
            *state.unknown_store.borrow_mut() = None;
            if let Some((agent, version)) = mutation {
                let draft = &mut state.drafts.borrow_mut()[index(agent)];
                if let Some(row) = view
                    .agent_settings
                    .agents
                    .iter()
                    .find(|row| row.agent == agent)
                {
                    draft.expected_revision = row.revision;
                }
                if draft.version == version {
                    draft.dirty = false;
                }
            }
            super::apply_view(&panel, view, generation);
            set_status(&state.document, "Agent settings updated.");
        }
        Err(error)
            if matches!(
                error,
                FlowError::Timeout | FlowError::Disconnected | FlowError::Protocol(_)
            ) && mutation.is_some() =>
        {
            state.unknown.set(true);
            *state.unknown_store.borrow_mut() = store.clone();
            let read_epoch = state.mutation_epoch.get();
            set_status(
                &state.document,
                "No receipt arrived. Checking saved agent settings before another change.",
            );
            let refreshed = flow.current_page().await;
            if panel.borrow().connection_generation != generation {
                return;
            }
            match refreshed {
                Ok(view) => {
                    reconcile_current_page(&panel, &view, generation, read_epoch);
                    super::apply_view(&panel, view, generation);
                    set_status(
                        &state.document,
                        if state.unknown.get() {
                            "Agent outcome remains unknown. Refresh agent settings to reconcile it."
                        } else {
                            "Agent view reconciled. Review saved values before retrying."
                        },
                    );
                }
                _ => set_status(
                    &state.document,
                    "Agent outcome is unknown. Reconnect and refresh before another change.",
                ),
            }
            state.mutation_busy.set(false);
        }
        Err(error) => {
            state.mutation_busy.set(false);
            set_status(&state.document, &format!("Agent settings failed: {error}"));
        }
    }
    update_controls(&state);
}
fn use_saved(state: Rc<SettingsPanel>, agent: AgentKind) {
    let row = state
        .panel
        .upgrade()
        .and_then(|panel| panel.borrow().snapshot.clone())
        .and_then(|view| {
            view.agent_settings
                .agents
                .into_iter()
                .find(|row| row.agent == agent)
        });
    let Some(row) = row else {
        set_status(
            &state.document,
            "No saved agent view. Refresh agent settings.",
        );
        return;
    };
    {
        let mut drafts = state.drafts.borrow_mut();
        let draft = &mut drafts[index(agent)];
        draft.dirty = false;
        draft.expected_revision = row.revision;
        draft.version = draft.version.wrapping_add(1);
    }
    if let Some(profile) = row.profile {
        write_profile(&state.document, &profile);
    } else {
        clear_profile(&state.document, agent);
    }
    set_status(
        &state.document,
        "Draft discarded. Saved values loaded from the current view.",
    );
}
fn update_controls(state: &SettingsPanel) {
    let conversation_busy = state
        .panel
        .upgrade()
        .is_some_and(|panel| crate::conversations::settings_blocked(&panel.borrow()));
    let disabled = state.mutation_busy.get()
        || state.server_busy.get()
        || state.unknown.get()
        || conversation_busy;
    for agent in AgentKind::ALL {
        for action in ["save", "remove"] {
            if let Some(button) = state.document.get_element_by_id(&control_id(agent, action)) {
                if disabled {
                    let _ = button.set_attribute("disabled", "");
                } else {
                    let _ = button.remove_attribute("disabled");
                }
            }
        }
    }
    if let Some(button) = state.document.get_element_by_id("agent-refresh") {
        if state.mutation_busy.get() || state.server_busy.get() || conversation_busy {
            let _ = button.set_attribute("disabled", "");
        } else {
            let _ = button.remove_attribute("disabled");
        }
    }
}
fn read_profile(document: &Document, agent: AgentKind) -> AgentProfile {
    let get = |suffix| value(document, agent, suffix);
    match agent {
        AgentKind::Codex => AgentProfile::Codex {
            executable: get("executable"),
            workspace: get("workspace"),
        },
        AgentKind::Opencode => AgentProfile::Opencode {
            executable: get("executable"),
            workspace: get("workspace"),
        },
        AgentKind::Pi => AgentProfile::Pi {
            executable: get("executable"),
            workspace: get("workspace"),
            session_dir: get("session_dir"),
            provider: get("provider"),
            model: get("model"),
            offline: document
                .get_element_by_id(&control_id(agent, "offline"))
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .is_some_and(|e| e.checked()),
        },
    }
}
fn write_profile(document: &Document, profile: &AgentProfile) {
    match profile {
        AgentProfile::Pi {
            executable,
            workspace,
            session_dir,
            provider,
            model,
            offline,
        } => {
            set_value(document, AgentKind::Pi, "executable", executable);
            set_value(document, AgentKind::Pi, "workspace", workspace);
            set_value(document, AgentKind::Pi, "session_dir", session_dir);
            set_value(document, AgentKind::Pi, "provider", provider);
            set_value(document, AgentKind::Pi, "model", model);
            if let Some(e) = document
                .get_element_by_id("agent-pi-offline")
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
            {
                e.set_checked(*offline);
            }
        }
        AgentProfile::Codex {
            executable,
            workspace,
        } => {
            set_value(document, AgentKind::Codex, "executable", executable);
            set_value(document, AgentKind::Codex, "workspace", workspace);
        }
        AgentProfile::Opencode {
            executable,
            workspace,
        } => {
            set_value(document, AgentKind::Opencode, "executable", executable);
            set_value(document, AgentKind::Opencode, "workspace", workspace);
        }
    }
}
fn clear_profile(document: &Document, agent: AgentKind) {
    for suffix in if agent == AgentKind::Pi {
        vec![
            "executable",
            "workspace",
            "session_dir",
            "provider",
            "model",
        ]
    } else {
        vec!["executable", "workspace"]
    } {
        set_value(document, agent, suffix, "");
    }
    if agent == AgentKind::Pi {
        if let Some(e) = document
            .get_element_by_id("agent-pi-offline")
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        {
            e.set_checked(false);
        }
    }
}
fn control_id(agent: AgentKind, suffix: &str) -> String {
    format!("agent-{}-{suffix}", agent.name())
}
fn index(agent: AgentKind) -> usize {
    AgentKind::ALL
        .iter()
        .position(|item| *item == agent)
        .unwrap_or(0)
}
fn row_revision(document: &Document, agent: AgentKind) -> Option<u64> {
    document
        .get_element_by_id(&control_id(agent, "status"))?
        .get_attribute("data-revision")?
        .parse()
        .ok()
}
fn value(document: &Document, agent: AgentKind, suffix: &str) -> String {
    document
        .get_element_by_id(&control_id(agent, suffix))
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        .map_or_else(String::new, |e| e.value())
}
fn set_value(document: &Document, agent: AgentKind, suffix: &str, value: &str) {
    if let Some(e) = document
        .get_element_by_id(&control_id(agent, suffix))
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        e.set_value(value);
    }
}
fn set_text(document: &Document, id: &str, text: &str) -> Result<(), JsValue> {
    let element = document
        .get_element_by_id(id)
        .ok_or_else(|| err("Agent setting output missing."))?;
    element.set_text_content(Some(text));
    Ok(())
}
fn set_status(document: &Document, text: &str) {
    if let Some(element) = document.get_element_by_id("agent-settings-status") {
        element.set_text_content(Some(text));
    }
}
