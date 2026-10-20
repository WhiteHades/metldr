use crate::{await_call, call, chrome_api, err, flow::validate_url, get};
use js_sys::Function;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::spawn_local;

mod navigation;
mod release;

const MAX_FRAME: usize = 65_536;
const MAX_SESSIONS: usize = 64;
const MAX_SAFE_CALL: u64 = 9_007_199_254_740_991;

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[derive(Default)]
struct State {
    native: Option<JsValue>,
    connection: u64,
    serial: u64,
    generation: u64,
    listeners: bool,
    active: Option<Attachment>,
    released: Option<release::Released>,
    busy: BTreeSet<u64>,
    blocked_tabs: BTreeMap<i32, u64>,
}

struct Attachment {
    run: String,
    tab: i32,
    document: String,
    url: String,
    epoch: u64,
    fence: u64,
    attaching: bool,
    last_call: u64,
    sessions: BTreeMap<String, Option<String>>,
    grant_revision: u64,
    pending_navigation: Option<navigation::PendingNavigation>,
    confirming_navigation: bool,
    command_count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: u8,
    kind: String,
    call: u64,
    run: String,
    operation: Operation,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Attach {
        tab: i32,
        document: String,
        url: String,
    },
    Command {
        tab: i32,
        session: Option<String>,
        method: String,
        params: Value,
    },
    Navigate {
        action: String,
        revision: u64,
        binding: navigation::Binding,
        url: String,
        destination: String,
        params: Value,
    },
    ConfirmNavigation {
        action: String,
        revision: u64,
        tab: i32,
        frame: String,
        loader: String,
    },
    Detach {
        tab: i32,
    },
}

pub(super) fn connect(native: JsValue) -> Result<(), JsValue> {
    if native.is_null() || native.is_undefined() {
        return Err(err("Native port is unavailable."));
    }
    install_listeners()?;
    disconnect();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.connection = state.connection.wrapping_add(1).max(1);
        state.native = Some(native);
    });
    Ok(())
}

pub(super) fn receive(message: &JsValue) -> bool {
    if get(message, "kind")
        .ok()
        .and_then(|v| v.as_string())
        .as_deref()
        != Some("browser_rpc")
    {
        return false;
    }
    let raw = js_sys::JSON::stringify(message)
        .ok()
        .and_then(|v| v.as_string());
    let Some(raw) = raw.filter(|raw| raw.len() <= MAX_FRAME) else {
        return true;
    };
    let Ok(envelope) = serde_json::from_str::<Envelope>(&raw) else {
        return true;
    };
    if envelope.v != 3 || envelope.kind != "browser_rpc" || !valid_run(&envelope.run) {
        return true;
    }
    let connection = STATE.with(|state| state.borrow().connection);
    spawn_local(async move {
        let (call_id, run) = (envelope.call, envelope.run.clone());
        let result = handle(envelope, connection).await;
        post_reply(connection, call_id, &run, result);
    });
    true
}

pub(super) fn disconnect() {
    let attachment = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.connection = state.connection.wrapping_add(1).max(1);
        state.native = None;
        state.released = None;
        state.active.take()
    });
    if let Some(attachment) = attachment.filter(|attachment| !attachment.attaching) {
        let _ = detach_later(attachment.tab);
    }
}

async fn handle(envelope: Envelope, connection: u64) -> Result<Value, String> {
    if envelope.call == 0 || envelope.call > MAX_SAFE_CALL {
        return Err("Invalid browser call number.".into());
    }
    ensure_connection(connection)?;
    match envelope.operation {
        Operation::Attach { tab, document, url } => {
            attach(connection, envelope.call, envelope.run, tab, document, url).await
        }
        Operation::Command {
            tab,
            session,
            method,
            params,
        } => {
            command(
                connection,
                envelope.call,
                &envelope.run,
                tab,
                session,
                &method,
                params,
            )
            .await
        }
        Operation::Navigate {
            action,
            revision,
            binding,
            url,
            destination,
            params,
        } => {
            navigation::navigate(
                connection,
                envelope.call,
                &envelope.run,
                action,
                revision,
                binding,
                url,
                destination,
                params,
            )
            .await
        }
        Operation::ConfirmNavigation {
            action,
            revision,
            tab,
            frame,
            loader,
        } => {
            navigation::confirm(
                connection,
                envelope.call,
                &envelope.run,
                action,
                revision,
                tab,
                frame,
                loader,
            )
            .await
        }
        Operation::Detach { tab } => detach(connection, envelope.call, &envelope.run, tab).await,
    }
}

async fn attach(
    connection: u64,
    call_id: u64,
    run: String,
    tab: i32,
    document: String,
    url: String,
) -> Result<Value, String> {
    if tab < 0 || !bounded_id(&document, 128) || validate_url(&url).is_err() {
        return Err("Invalid browser attachment target.".into());
    }
    let (epoch, fence) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection != connection
            || state.native.is_none()
            || state.active.is_some()
            || !state.busy.is_empty()
        {
            return Err(
                "A browser attachment is already active or the host disconnected.".to_owned(),
            );
        }
        state.generation = state.generation.wrapping_add(1).max(1);
        let fence = state.generation;
        let epoch = fence;
        state.busy.insert(fence);
        state.blocked_tabs.insert(tab, fence);
        state.released = None;
        state.active = Some(Attachment {
            run: run.clone(),
            tab,
            document: document.clone(),
            url: url.clone(),
            epoch,
            fence,
            attaching: true,
            last_call: call_id,
            sessions: BTreeMap::new(),
            grant_revision: 1,
            pending_navigation: None,
            confirming_navigation: false,
            command_count: 0,
        });
        Ok((epoch, fence))
    })?;
    let mut attach_started = false;
    let mut attached = false;
    let operation = async {
        verify(connection, epoch).await?;
        let debugger = chrome_api("debugger").map_err(message)?;
        attach_started = true;
        let attach_result = await_call(
            &debugger,
            "attach",
            &[
                parse(&json!({"tabId":tab})).map_err(message)?,
                JsValue::from_str("1.3"),
            ],
        )
        .await;
        attached = attach_result.is_ok();
        ensure_epoch(connection, epoch).map_err(|error| error.to_string())?;
        attach_result.map_err(message)?;
        verify(connection, epoch).await?;
        let info = await_call(
            &debugger,
            "sendCommand",
            &[
                parse(&json!({"tabId":tab})).map_err(message)?,
                JsValue::from_str("Target.getTargetInfo"),
                parse(&json!({})).map_err(message)?,
            ],
        )
        .await;
        ensure_epoch(connection, epoch).map_err(|error| error.to_string())?;
        let info = info.map_err(message)?;
        verify(connection, epoch).await?;
        let value = to_value(&info)?;
        if !value
            .get("targetInfo")
            .and_then(|target| target.get("targetId"))
            .and_then(Value::as_str)
            .is_some_and(|id| bounded_id(id, 128))
        {
            return Err("Debugger returned an invalid target identity.".into());
        }
        Ok(value)
    }
    .await;
    match operation {
        Ok(info) => {
            if !set_attached(connection, epoch) {
                let _ = cleanup_pending(tab, fence, attached).await;
                return Err("Browser attachment became stale.".into());
            }
            Ok(
                json!({"targetInfo":info.get("targetInfo").cloned().unwrap_or(info),"grant_revision":1}),
            )
        }
        Err(error) => {
            clear_attachment(connection, epoch);
            if attach_started {
                if let Err(cleanup_error) = cleanup_pending(tab, fence, attached).await {
                    return Err(format!("{error} Cleanup failed: {cleanup_error}"));
                }
            } else {
                release_fence(tab, fence);
            }
            Err(error)
        }
    }
}

async fn command(
    connection: u64,
    call_id: u64,
    run: &str,
    tab: i32,
    session: Option<String>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    if !allowed_method(method)
        || !params.is_object()
        || !bounded_json(&params, MAX_FRAME)
        || method == "Page.createIsolatedWorld" && !valid_isolated_world(&params)
    {
        return Err("Unsupported or oversized debugger command.".into());
    }
    let epoch = reserve_call(connection, call_id, run, tab, session.as_deref())?;
    let pending = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch) else {
            return Err("Browser attachment is stale.".to_owned());
        };
        let pending = active.pending_navigation.is_some();
        if pending && !navigation::allowed_while_pending(method) {
            return Err("Debugger command is blocked until navigation is confirmed.".into());
        }
        active.command_count = active
            .command_count
            .checked_add(1)
            .ok_or_else(|| "Too many concurrent browser commands.".to_owned())?;
        Ok(pending)
    })?;
    let result = async {
        if pending {
            ensure_epoch(connection, epoch)?;
        } else {
            verify(connection, epoch).await?;
        }
        let target = match session {
            Some(session) => json!({"tabId":tab,"sessionId":session}),
            None => json!({"tabId":tab}),
        };
        let debugger = chrome_api("debugger").map_err(message)?;
        let result = await_call(
            &debugger,
            "sendCommand",
            &[
                parse(&target).map_err(message)?,
                JsValue::from_str(method),
                parse(&params).map_err(message)?,
            ],
        )
        .await;
        ensure_epoch(connection, epoch)?;
        let result = result.map_err(message)?;
        if pending {
            ensure_epoch(connection, epoch)?;
        } else {
            verify(connection, epoch).await?;
        }
        to_value(&result)
    }
    .await;
    STATE.with(|state| {
        if let Some(active) = state
            .borrow_mut()
            .active
            .as_mut()
            .filter(|active| active.epoch == epoch)
        {
            active.command_count = active.command_count.saturating_sub(1);
        }
    });
    result
}

async fn detach(connection: u64, call_id: u64, run: &str, tab: i32) -> Result<Value, String> {
    if STATE.with(|state| state.borrow().active.is_none()) {
        return release::acknowledge(connection, call_id, run, tab).await;
    }
    let epoch = reserve_call(connection, call_id, run, tab, None)?;
    let current = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection != connection
            || state
                .active
                .as_ref()
                .is_none_or(|active| active.epoch != epoch)
        {
            return false;
        }
        state.active = None;
        true
    });
    if current {
        let fence = STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.generation = state.generation.wrapping_add(1).max(1);
            let fence = state.generation;
            state.blocked_tabs.insert(tab, fence);
            state.busy.insert(fence);
            fence
        });
        cleanup_pending(tab, fence, true).await?;
    }
    Ok(json!({"detached":current}))
}

fn reserve_call(
    connection: u64,
    call_id: u64,
    run: &str,
    tab: i32,
    session: Option<&str>,
) -> Result<u64, String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection != connection || state.native.is_none() {
            return Err("Native browser connection changed.".into());
        }
        let active = state
            .active
            .as_mut()
            .ok_or_else(|| "No browser attachment is active.".to_owned())?;
        if active.run != run || active.tab != tab || active.attaching || call_id <= active.last_call
        {
            return Err("Browser call does not match the active attachment.".into());
        }
        if let Some(session) = session {
            if active.sessions.get(session).is_none() {
                return Err("Unknown browser target session.".into());
            }
        }
        active.last_call = call_id;
        Ok(active.epoch)
    })
}

fn ensure_connection(connection: u64) -> Result<(), String> {
    STATE.with(|state| {
        let state = state.borrow();
        if current_connection(state.connection, connection) && state.native.is_some() {
            Ok(())
        } else {
            Err("Native browser connection changed.".into())
        }
    })
}

async fn verify(connection: u64, epoch: u64) -> Result<(), String> {
    let (tab_id, document, url) = attachment(connection, epoch)?;
    let site = crate::site::permission(&url).map_err(|_| "Browser URL is invalid.".to_owned())?;
    let tabs = chrome_api("tabs").map_err(message)?;
    let tab_result = await_call(&tabs, "get", &[JsValue::from_f64(tab_id as f64)]).await;
    ensure_epoch(connection, epoch)?;
    let tab = tab_result.map_err(message)?;
    if get(&tab, "url").ok().and_then(|v| v.as_string()).as_deref() != Some(url.as_str()) {
        return invalidate(connection, epoch, "Browser tab URL changed.");
    }
    ensure_epoch(connection, epoch)?;
    let navigation = chrome_api("webNavigation").map_err(message)?;
    let frame_result = await_call(
        &navigation,
        "getFrame",
        &[parse(&json!({"tabId":tab_id,"frameId":0})).map_err(message)?],
    )
    .await;
    ensure_epoch(connection, epoch)?;
    let frame = frame_result.map_err(message)?;
    if get(&frame, "documentId")
        .ok()
        .and_then(|v| v.as_string())
        .as_deref()
        != Some(document.as_str())
    {
        return invalidate(connection, epoch, "Browser document changed.");
    }
    ensure_epoch(connection, epoch)?;
    let permission = chrome_api("permissions").map_err(message)?;
    let grant_result = await_call(
        &permission,
        "contains",
        &[parse(&json!({"origins":[site]})).map_err(message)?],
    )
    .await;
    ensure_epoch(connection, epoch)?;
    let granted = grant_result.map_err(message)?;
    if !granted.as_bool().unwrap_or(false) {
        return invalidate(connection, epoch, "Site access was revoked.");
    }
    ensure_epoch(connection, epoch)
}

fn attachment(connection: u64, epoch: u64) -> Result<(i32, String, String), String> {
    STATE.with(|state| {
        let state = state.borrow();
        let active = state
            .active
            .as_ref()
            .filter(|active| state.connection == connection && active.epoch == epoch)
            .ok_or_else(|| "Browser attachment is stale.".to_owned())?;
        Ok((active.tab, active.document.clone(), active.url.clone()))
    })
}

fn ensure_epoch(connection: u64, epoch: u64) -> Result<(), String> {
    STATE.with(|state| {
        let state = state.borrow();
        if state.native.is_some()
            && current_lease(
                state.connection,
                connection,
                state.active.as_ref().map(|active| active.epoch),
                epoch,
            )
        {
            Ok(())
        } else {
            Err("Browser attachment is stale.".into())
        }
    })
}

fn invalidate<T>(connection: u64, epoch: u64, reason: &str) -> Result<T, String> {
    let details = STATE.with(|state| {
        let state = state.borrow();
        state
            .active
            .as_ref()
            .filter(|active| state.connection == connection && active.epoch == epoch)
            .map(|active| (active.run.clone(), active.tab))
    });
    clear_attachment(connection, epoch);
    if let Some((run, tab)) = details {
        post_event(run, tab, None, "MetLDR.detached", json!({"reason":reason}));
    }
    Err(reason.into())
}

fn set_attached(connection: u64, epoch: u64) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection != connection || state.native.is_none() {
            return false;
        }
        let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch) else {
            return false;
        };
        active.attaching = false;
        let (tab, fence) = (active.tab, active.fence);
        state.busy.remove(&fence);
        if state.blocked_tabs.get(&tab) == Some(&fence) {
            state.blocked_tabs.remove(&tab);
        }
        true
    })
}

fn clear_attachment(connection: u64, epoch: u64) {
    let attachment = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection == connection
            && state
                .active
                .as_ref()
                .is_some_and(|active| active.epoch == epoch)
        {
            state.active.take()
        } else {
            None
        }
    });
    if let Some(attachment) = attachment {
        let attaching =
            STATE.with(|state| state.borrow().blocked_tabs.contains_key(&attachment.tab));
        if !attaching {
            if let Some(cleanup) = detach_later(attachment.tab) {
                release::remember(connection, &attachment, cleanup);
            }
        }
    }
}

fn release_fence(tab: i32, fence: u64) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.blocked_tabs.get(&tab) == Some(&fence) {
            state.blocked_tabs.remove(&tab);
        }
        state.busy.remove(&fence);
    });
}

fn detach_later(tab: i32) -> Option<js_sys::Promise> {
    let fence = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.tab == tab)
            || state.blocked_tabs.contains_key(&tab)
        {
            None
        } else {
            state.generation = state.generation.wrapping_add(1).max(1);
            let fence = state.generation;
            state.blocked_tabs.insert(tab, fence);
            state.busy.insert(fence);
            Some(fence)
        }
    });
    fence.map(|fence| {
        wasm_bindgen_futures::future_to_promise(async move {
            Ok(JsValue::from_bool(
                cleanup_pending(tab, fence, true).await.is_ok(),
            ))
        })
    })
}

async fn cleanup_pending(tab: i32, fence: u64, attached: bool) -> Result<(), String> {
    if !attached {
        release_fence(tab, fence);
        return Ok(());
    }
    let debugger = chrome_api("debugger").map_err(message)?;
    let result = await_call(
        &debugger,
        "detach",
        &[parse(&json!({"tabId":tab})).map_err(message)?],
    )
    .await;
    result.map_err(message)?;
    release_fence(tab, fence);
    Ok(())
}

fn install_listeners() -> Result<(), JsValue> {
    let install = STATE.with(|state| !state.borrow().listeners);
    if !install {
        return Ok(());
    }
    let debugger = chrome_api("debugger")?;
    let event = get(&debugger, "onEvent")?;
    let add = get(&event, "addListener")?.dyn_into::<Function>()?;
    let listener = Closure::<dyn FnMut(JsValue, String, JsValue)>::new(on_debugger_event);
    add.call1(&event, listener.as_ref())?;
    listener.forget();
    let detached = get(&debugger, "onDetach")?;
    let add = get(&detached, "addListener")?.dyn_into::<Function>()?;
    let listener = Closure::<dyn FnMut(JsValue, String)>::new(|source, _reason| on_detach(source));
    add.call1(&detached, listener.as_ref())?;
    listener.forget();
    let permissions = chrome_api("permissions")?;
    let removed = get(&permissions, "onRemoved")?;
    let add = get(&removed, "addListener")?.dyn_into::<Function>()?;
    let listener = Closure::<dyn FnMut(JsValue)>::new(on_permission_removed);
    add.call1(&removed, listener.as_ref())?;
    listener.forget();
    STATE.with(|state| state.borrow_mut().listeners = true);
    Ok(())
}

fn on_detach(source: JsValue) {
    let tab = routing_tab(&source);
    let attachment = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if tab.is_some_and(|tab| state.blocked_tabs.contains_key(&tab))
            && !state
                .active
                .as_ref()
                .is_some_and(|active| Some(active.tab) == tab && active.attaching)
        {
            return None;
        }
        let attachment = if state
            .active
            .as_ref()
            .is_some_and(|active| Some(active.tab) == tab)
        {
            state.active.take()
        } else {
            None
        };
        attachment
    });
    if let Some(active) = attachment {
        post_event(
            active.run,
            active.tab,
            None,
            "MetLDR.detached",
            json!({"reason":"debugger detached"}),
        );
    }
}

fn on_permission_removed(permissions: JsValue) {
    let origins = get(&permissions, "origins")
        .ok()
        .and_then(|value| js_sys::JSON::stringify(&value).ok()?.as_string())
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    let details = STATE.with(|state| {
        let state = state.borrow();
        let active = state.active.as_ref()?;
        let site = crate::site::permission(&active.url).ok()?;
        origins
            .iter()
            .any(|removed| crate::site::removed_site(removed, &site))
            .then(|| {
                (
                    state.connection,
                    active.epoch,
                    active.run.clone(),
                    active.tab,
                )
            })
    });
    if let Some((connection, epoch, run, tab)) = details {
        clear_attachment(connection, epoch);
        post_event(
            run,
            tab,
            None,
            "MetLDR.detached",
            json!({"reason":"site access was revoked"}),
        );
    }
}

fn on_debugger_event(source: JsValue, method: String, params: JsValue) {
    let Some(tab) = routing_tab(&source) else {
        return;
    };
    if method == "Target.attachedToTarget" {
        track_attached(tab, &source, &params);
    } else if method == "Target.detachedFromTarget" {
        track_detached(tab, &params);
    }
    let session = match optional_session(&source, "sessionId") {
        Ok(session) => session,
        Err(()) => {
            invalidate_event(tab);
            return;
        }
    };
    if session.as_ref().is_some_and(|id| !bounded_id(id, 128)) {
        invalidate_event(tab);
        return;
    }
    let (run, allowed) = STATE.with(|state| {
        let state = state.borrow();
        let Some(active) = state
            .active
            .as_ref()
            .filter(|active| active.tab == tab && !active.attaching)
        else {
            return (String::new(), false);
        };
        let known = session
            .as_ref()
            .is_none_or(|id| active.sessions.contains_key(id));
        (active.run.clone(), known)
    });
    if !allowed {
        return;
    }
    let Ok(value) = to_value(&params) else {
        invalidate_event(tab);
        return;
    };
    if !valid_event(&method, &value) || !bounded_json(&value, MAX_FRAME) {
        invalidate_event(tab);
        return;
    }
    post_event(run, tab, session, &method, value);
}

fn track_attached(tab: i32, source: &JsValue, params: &JsValue) {
    let Ok(value) = to_value(params) else {
        invalidate_event(tab);
        return;
    };
    let Some(id) = value
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| bounded_id(id, 128))
    else {
        invalidate_event(tab);
        return;
    };
    let parent = match optional_session(source, "sessionId") {
        Ok(parent) => parent,
        Err(()) => {
            invalidate_event(tab);
            return;
        }
    };
    let valid = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.tab == tab && !active.attaching)
        else {
            return false;
        };
        if active.sessions.len() >= MAX_SESSIONS
            || active.sessions.contains_key(id)
            || !valid_parent_route(id, parent.as_deref(), &active.sessions)
        {
            return false;
        }
        active.sessions.insert(id.to_owned(), parent);
        true
    });
    if !valid {
        invalidate_event(tab);
    }
}

fn track_detached(tab: i32, params: &JsValue) {
    let id = to_value(params).ok().and_then(|v| {
        v.get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let Some(id) = id.filter(|id| bounded_id(id, 128)) else {
        invalidate_event(tab);
        return;
    };
    let known = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active.as_mut().filter(|active| active.tab == tab) else {
            return false;
        };
        remove_session_tree(&mut active.sessions, &id)
    });
    if !known {
        invalidate_event(tab);
    }
}

fn invalidate_event(tab: i32) {
    let details = STATE.with(|state| {
        let state = state.borrow();
        state
            .active
            .as_ref()
            .filter(|active| active.tab == tab)
            .map(|active| (state.connection, active.epoch, active.run.clone()))
    });
    if let Some((connection, epoch, run)) = details {
        clear_attachment(connection, epoch);
        post_event(
            run,
            tab,
            None,
            "MetLDR.detached",
            json!({"reason":"invalid or oversized debugger event"}),
        );
    }
}

fn post_reply(connection: u64, call_id: u64, run: &str, result: Result<Value, String>) {
    let command = match result {
        Ok(value) => {
            json!({"op":"browser_reply","call":call_id,"run":run,"result":{"status":"ok","value":value}})
        }
        Err(message) => {
            json!({"op":"browser_reply","call":call_id,"run":run,"result":{"status":"error","message":message.chars().take(512).collect::<String>()}})
        }
    };
    post_native(connection, format!("browser-{call_id}"), command);
}

fn post_event(run: String, tab: i32, session: Option<String>, method: &str, params: Value) {
    let (connection, request) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.serial = state.serial.wrapping_add(1).max(1);
        (state.connection, format!("browser-event-{}", state.serial))
    });
    post_native(
        connection,
        request,
        json!({"op":"browser_event","run":run,"tab":tab,"session":session,"method":method,"params":params}),
    );
}

fn post_native(connection: u64, request: String, command: Value) {
    let native = STATE.with(|state| {
        let state = state.borrow();
        (state.connection == connection)
            .then(|| state.native.clone())
            .flatten()
    });
    let Some(native) = native else {
        return;
    };
    let message = json!({"v":3,"request":request,"command":command});
    if !bounded_json(&message, MAX_FRAME) {
        invalidate_all();
        return;
    }
    match parse(&message).and_then(|message| call(&native, "postMessage", &[message])) {
        Ok(_) => (),
        Err(_) => invalidate_all(),
    }
}

fn invalidate_all() {
    let tab = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.connection = state.connection.wrapping_add(1).max(1);
        state.native = None;
        state.released = None;
        state.active.take().map(|active| active.tab)
    });
    if let Some(tab) = tab {
        let _ = detach_later(tab);
    }
}

fn valid_isolated_world(params: &Value) -> bool {
    params.as_object().is_some_and(|params| {
        params.len() == 3
            && ["frameId", "worldName"].into_iter().all(|key| {
                params
                    .get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|value| bounded_id(value, 128))
            })
            && params.get("grantUniveralAccess").and_then(Value::as_bool) == Some(false)
    })
}

fn allowed_method(method: &str) -> bool {
    matches!(
        method,
        "Target.getTargetInfo"
            | "Page.enable"
            | "Page.getFrameTree"
            | "Page.setLifecycleEventsEnabled"
            | "Runtime.enable"
            | "Page.addScriptToEvaluateOnNewDocument"
            | "Page.createIsolatedWorld"
            | "Network.enable"
            | "Target.setAutoAttach"
            | "Performance.enable"
            | "Log.enable"
            | "Network.setCacheDisabled"
            | "Runtime.evaluate"
            | "Runtime.callFunctionOn"
            | "Runtime.releaseObject"
            | "Runtime.runIfWaitingForDebugger"
    )
}

fn valid_event(method: &str, value: &Value) -> bool {
    !method.is_empty()
        && method.len() <= 128
        && method
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        && value.is_object()
}

fn routing_tab(source: &JsValue) -> Option<i32> {
    let value = get(source, "tabId").ok()?.as_f64()?;
    (value.is_finite() && value.fract() == 0.0 && (0.0..=i32::MAX as f64).contains(&value))
        .then_some(value as i32)
}

fn optional_session(source: &JsValue, key: &str) -> Result<Option<String>, ()> {
    let value = get(source, key).map_err(|_| ())?;
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        value.as_string().map(Some).ok_or(())
    }
}

fn valid_parent_route(
    id: &str,
    parent: Option<&str>,
    sessions: &BTreeMap<String, Option<String>>,
) -> bool {
    bounded_id(id, 128)
        && parent.is_none_or(|parent| bounded_id(parent, 128) && sessions.contains_key(parent))
}

fn remove_session_tree(sessions: &mut BTreeMap<String, Option<String>>, id: &str) -> bool {
    if !sessions.contains_key(id) {
        return false;
    }
    let mut removed = BTreeSet::from([id.to_owned()]);
    loop {
        let children: Vec<_> = sessions
            .iter()
            .filter(|(id, parent)| {
                !removed.contains(*id) && parent.as_ref().is_some_and(|p| removed.contains(p))
            })
            .map(|(id, _)| id.clone())
            .collect();
        if children.is_empty() {
            break;
        }
        removed.extend(children);
    }
    sessions.retain(|id, _| !removed.contains(id));
    true
}

fn current_connection(current: u64, captured: u64) -> bool {
    current == captured
}

fn current_lease(
    connection_now: u64,
    connection_then: u64,
    epoch: Option<u64>,
    captured_epoch: u64,
) -> bool {
    current_connection(connection_now, connection_then) && epoch == Some(captured_epoch)
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn nested_session_detach_removes_descendants_and_preserves_other_trees() {
        let mut sessions = BTreeMap::from([
            ("child".into(), None),
            ("nested".into(), Some("child".into())),
            ("deep".into(), Some("nested".into())),
            ("sibling".into(), None),
            ("sibling-child".into(), Some("sibling".into())),
        ]);
        assert!(remove_session_tree(&mut sessions, "child"));
        assert_eq!(
            sessions,
            BTreeMap::from([
                ("sibling".into(), None),
                ("sibling-child".into(), Some("sibling".into())),
            ])
        );
        assert!(!remove_session_tree(&mut sessions, "unknown"));
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn validates_browser_event_routing_and_lifecycle_helpers() {
        assert!(allowed_method("Runtime.callFunctionOn"));
        assert!(allowed_method("Runtime.releaseObject"));
        assert!(!allowed_method("Browser.close"));
        let console = json!({"type":"log","executionContextId":17});
        assert!(valid_event("Runtime.consoleAPICalled", &console));
        let sessions = BTreeMap::from([("child".to_owned(), None)]);
        assert!(!valid_parent_route("new-child", Some("unknown"), &sessions));
        assert!(current_connection(8, 8));
        assert!(!current_connection(8, 7));
        assert!(current_lease(8, 8, Some(12), 12));
        assert!(!current_lease(8, 8, Some(13), 12));
    }
}

fn valid_run(value: &str) -> bool {
    bounded_id(value, 96)
}
fn bounded_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))
}

fn bounded_json(value: &Value, max: usize) -> bool {
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= max)
}
fn parse(value: &Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&serde_json::to_string(value).map_err(|_| err("Invalid browser message."))?)
}
fn to_value(value: &JsValue) -> Result<Value, String> {
    let raw = js_sys::JSON::stringify(value)
        .map_err(message)?
        .as_string()
        .ok_or_else(|| "Debugger returned non JSON data.".to_owned())?;
    if raw.len() > MAX_FRAME {
        return Err("Debugger message exceeds the frame limit.".into());
    }
    serde_json::from_str(&raw).map_err(|_| "Debugger returned invalid JSON.".to_owned())
}
fn message(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}
