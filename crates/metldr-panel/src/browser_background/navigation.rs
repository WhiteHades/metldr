use super::*;
use serde::Deserialize;
use web_sys::Url;

const ROOT_SESSION: &str = "metldr-owned-root";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    tab: i32,
    document: String,
    target: String,
    frame: String,
    session: String,
    unique_context: String,
    grant_revision: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NavigateParams {
    url: String,
    #[serde(rename = "frameId")]
    frame_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NavigateResponse {
    #[serde(rename = "frameId")]
    frame_id: String,
    #[serde(rename = "loaderId")]
    loader_id: Option<String>,
    #[serde(rename = "errorText")]
    error_text: Option<String>,
    #[serde(rename = "isDownload")]
    is_download: Option<bool>,
}

pub(super) struct PendingNavigation {
    action: String,
    revision: u64,
    binding: Binding,
    destination: String,
    response_frame: Option<String>,
    response_loader: Option<String>,
    response: Option<Value>,
}

pub(super) fn allowed_while_pending(method: &str) -> bool {
    matches!(
        method,
        "Page.enable"
            | "Page.getFrameTree"
            | "Target.setAutoAttach"
            | "Runtime.runIfWaitingForDebugger"
            | "Runtime.enable"
            | "Runtime.releaseObject"
    )
}

pub(super) async fn navigate(
    connection: u64,
    call_id: u64,
    run: &str,
    action: String,
    revision: u64,
    binding: Binding,
    url: String,
    destination: String,
    params: Value,
) -> Result<Value, String> {
    let typed: NavigateParams = serde_json::from_value(params.clone())
        .map_err(|_| "Invalid Page.navigate parameters.".to_owned())?;
    if !bounded_id(&action, 128)
        || revision == 0
        || !valid_binding(&binding)
        || binding.tab < 0
        || typed.url != destination
        || params
            .get("frameId")
            .is_some_and(|frame| !frame.is_string())
        || params
            .get("frameId")
            .and_then(Value::as_str)
            .is_some_and(|frame| frame != binding.frame.as_str())
        || typed
            .frame_id
            .as_deref()
            .is_some_and(|frame| frame != binding.frame)
        || !valid_navigation_url(&url, &destination)
        || !bounded_json(&params, MAX_FRAME)
    {
        return Err("Invalid browser navigation request.".into());
    }
    let session = root_session(&binding.session);
    let epoch = reserve_call(connection, call_id, run, binding.tab, session)?;
    let reserved = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch) else {
            return false;
        };
        if active.document != binding.document
            || active.url != url
            || active.grant_revision != binding.grant_revision
            || active.command_count != 0
            || active.pending_navigation.is_some()
            || active.confirming_navigation
        {
            return false;
        }
        active.pending_navigation = Some(PendingNavigation {
            action: action.clone(),
            revision,
            binding: binding.clone(),
            destination: destination.clone(),
            response_frame: None,
            response_loader: None,
            response: None,
        });
        true
    });
    if !reserved {
        return Err("Navigation does not match the active browser grant.".into());
    }

    let preflight = async {
        verify(connection, epoch).await?;
        let target = send_command(
            connection,
            epoch,
            binding.tab,
            session,
            "Target.getTargetInfo",
            json!({}),
        )
        .await?;
        if target
            .get("targetInfo")
            .and_then(|v| v.get("targetId"))
            .and_then(Value::as_str)
            != Some(binding.target.as_str())
        {
            return Err("Browser target changed before navigation.".into());
        }
        let tree = send_command(
            connection,
            epoch,
            binding.tab,
            session,
            "Page.getFrameTree",
            json!({}),
        )
        .await?;
        let root = tree
            .get("frameTree")
            .and_then(|v| v.get("frame"))
            .ok_or_else(|| "Missing root frame before navigation.".to_owned())?;
        if root.get("id").and_then(Value::as_str) != Some(binding.frame.as_str())
            || root.get("url").and_then(Value::as_str) != Some(url.as_str())
        {
            return Err("Browser root frame changed before navigation.".into());
        }
        ensure_epoch(connection, epoch)
    }
    .await;
    if let Err(error) = preflight {
        clear_unstarted(connection, epoch, &action, revision);
        return Err(error);
    }

    let response = send_command(
        connection,
        epoch,
        binding.tab,
        session,
        "Page.navigate",
        params,
    )
    .await?;
    let stored = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch) else {
            return false;
        };
        let Some(pending) = active
            .pending_navigation
            .as_mut()
            .filter(|pending| pending.action == action && pending.revision == revision)
        else {
            return false;
        };
        pending.response_frame = response
            .get("frameId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        pending.response_loader = response
            .get("loaderId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        pending.response = Some(response.clone());
        true
    });
    if !stored {
        return Err("Navigation grant became stale after dispatch.".into());
    }
    Ok(response)
}

pub(super) async fn confirm(
    connection: u64,
    call_id: u64,
    run: &str,
    action: String,
    revision: u64,
    tab: i32,
    frame: String,
    loader: String,
) -> Result<Value, String> {
    if tab < 0 || !bounded_id(&action, 128) || !bounded_id(&frame, 128) || !bounded_id(&loader, 128)
    {
        return Err("Invalid navigation confirmation.".into());
    }
    let epoch = reserve_call(connection, call_id, run, tab, None)?;
    let pending = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch) else {
            return None;
        };
        let pending = active.pending_navigation.as_ref()?;
        let raw_response = pending.response.as_ref()?;
        let Ok(raw_response) = serde_json::from_value::<NavigateResponse>(raw_response.clone())
        else {
            return None;
        };
        if active.confirming_navigation
            || active.command_count != 0
            || pending.action != action
            || pending.revision != revision
            || pending.binding.tab != tab
            || pending.response_frame.as_deref() != Some(frame.as_str())
            || pending.response_loader.as_deref() != Some(loader.as_str())
            || raw_response.frame_id != frame
            || raw_response.loader_id.as_deref() != Some(loader.as_str())
            || raw_response.error_text.is_some()
            || raw_response.is_download == Some(true)
        {
            return None;
        }
        active.confirming_navigation = true;
        Some((pending.binding.clone(), pending.destination.clone()))
    });
    let Some((binding, destination)) = pending else {
        return Err("Navigation confirmation does not match the pending response.".into());
    };

    let checked =
        check_confirmed_document(connection, epoch, &binding, &destination, &frame, &loader).await;
    let document = match checked {
        Ok(value) => value,
        Err(error) => return invalidate(connection, epoch, &error),
    };
    let Some(next_revision) = binding.grant_revision.checked_add(1) else {
        return invalidate(connection, epoch, "Browser grant revision exhausted.");
    };
    let committed = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch) else {
            return false;
        };
        let Some(pending) = active.pending_navigation.as_ref().filter(|pending| {
            pending.action == action
                && pending.revision == revision
                && pending.response_frame.as_deref() == Some(frame.as_str())
                && pending.response_loader.as_deref() == Some(loader.as_str())
        }) else {
            return false;
        };
        let _ = pending;
        active.document = document.clone();
        active.url = destination.clone();
        active.grant_revision = next_revision;
        active.pending_navigation = None;
        active.confirming_navigation = false;
        true
    });
    if !committed {
        return invalidate(
            connection,
            epoch,
            "Browser navigation confirmation became stale.",
        );
    }
    Ok(
        json!({"document":document,"url":destination,"grant_revision":next_revision,"frame":frame,"loader":loader}),
    )
}

async fn check_confirmed_document(
    connection: u64,
    epoch: u64,
    binding: &Binding,
    destination: &str,
    frame_id: &str,
    loader_id: &str,
) -> Result<String, String> {
    let pending = STATE.with(|state| {
        state
            .borrow()
            .active
            .as_ref()
            .and_then(|active| active.pending_navigation.as_ref())
            .is_some_and(|pending| {
                pending.response_frame.as_deref() == Some(frame_id)
                    && pending.response_loader.as_deref() == Some(loader_id)
                    && pending.destination == destination
            })
    });
    if !pending {
        return Err("Navigation response changed before confirmation.".into());
    }
    ensure_epoch(connection, epoch)?;
    let session = root_session(&binding.session);
    let tree = send_command(
        connection,
        epoch,
        binding.tab,
        session,
        "Page.getFrameTree",
        json!({}),
    )
    .await?;
    let root = tree
        .get("frameTree")
        .and_then(|v| v.get("frame"))
        .ok_or_else(|| "Missing root frame after navigation.".to_owned())?;
    if root.get("id").and_then(Value::as_str) != Some(frame_id)
        || root.get("loaderId").and_then(Value::as_str) != Some(loader_id)
        || root.get("url").and_then(Value::as_str) != Some(destination)
    {
        return Err("Root frame does not match the approved navigation.".into());
    }
    if Url::new(destination)
        .ok()
        .is_none_or(|url| !matches!(url.pathname().as_str(), "/article" | "/decoy"))
    {
        return Err("Confirmed document is outside the owned capture routes.".into());
    }
    ensure_epoch(connection, epoch)?;
    let tabs = chrome_api("tabs").map_err(message)?;
    let tab = await_call(&tabs, "get", &[JsValue::from_f64(binding.tab as f64)]).await;
    ensure_epoch(connection, epoch)?;
    let tab = tab.map_err(message)?;
    if get(&tab, "url").ok().and_then(|v| v.as_string()).as_deref() != Some(destination) {
        return Err("Active tab URL does not match the approved destination.".into());
    }
    ensure_epoch(connection, epoch)?;
    let navigation = chrome_api("webNavigation").map_err(message)?;
    let frame = await_call(
        &navigation,
        "getFrame",
        &[parse(&json!({"tabId":binding.tab,"frameId":0})).map_err(message)?],
    )
    .await;
    ensure_epoch(connection, epoch)?;
    let frame = frame.map_err(message)?;
    let document = get(&frame, "documentId")
        .ok()
        .and_then(|v| v.as_string())
        .filter(|document| bounded_id(document, 128) && document != &binding.document)
        .ok_or_else(|| "Chrome did not report a new main document.".to_owned())?;
    if get(&frame, "documentLifecycle")
        .ok()
        .and_then(|value| value.as_string())
        .as_deref()
        != Some("active")
    {
        return Err("Navigation document is not active.".into());
    }
    if get(&frame, "url")
        .ok()
        .and_then(|v| v.as_string())
        .as_deref()
        != Some(destination)
    {
        return Err("Chrome main frame URL does not match the approved destination.".into());
    }
    ensure_epoch(connection, epoch)?;
    let permission = chrome_api("permissions").map_err(message)?;
    let site = crate::site::permission(destination).map_err(|error| error.to_string())?;
    let grant = await_call(
        &permission,
        "contains",
        &[parse(&json!({"origins":[site]})).map_err(message)?],
    )
    .await;
    ensure_epoch(connection, epoch)?;
    if !grant.map_err(message)?.as_bool().unwrap_or(false) {
        return Err("Site access was revoked.".into());
    }
    let confirmed = send_command(
        connection,
        epoch,
        binding.tab,
        session,
        "Page.getFrameTree",
        json!({}),
    )
    .await?;
    let root = &confirmed["frameTree"]["frame"];
    if root["id"] != frame_id || root["loaderId"] != loader_id || root["url"] != destination {
        return Err("Destination changed during navigation confirmation.".into());
    }
    Ok(document)
}

async fn send_command(
    connection: u64,
    epoch: u64,
    tab: i32,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    ensure_epoch(connection, epoch)?;
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
    to_value(&result.map_err(message)?)
}

fn clear_unstarted(connection: u64, epoch: u64, action: &str, revision: u64) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection != connection {
            return;
        }
        if let Some(active) = state.active.as_mut().filter(|active| active.epoch == epoch)
            && active.pending_navigation.as_ref().is_some_and(|pending| {
                pending.action == action
                    && pending.revision == revision
                    && pending.response_frame.is_none()
            })
        {
            active.pending_navigation = None;
        }
    });
}

fn root_session(session: &str) -> Option<&str> {
    (session != ROOT_SESSION).then_some(session)
}

fn valid_binding(binding: &Binding) -> bool {
    binding.grant_revision > 0
        && [
            &binding.document,
            &binding.target,
            &binding.frame,
            &binding.session,
        ]
        .into_iter()
        .all(|value| bounded_id(value, 128))
        && !binding.unique_context.is_empty()
        && binding.unique_context.len() <= 128
        && binding
            .unique_context
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_' | b'.'))
}

fn valid_navigation_url(current: &str, destination_url: &str) -> bool {
    let (Ok(current), Ok(destination)) = (Url::new(current), Url::new(destination_url)) else {
        return false;
    };
    let path = destination.pathname();
    destination.protocol() == "http:"
        && destination.hostname() == "127.0.0.1"
        && destination.origin() == current.origin()
        && matches!(path.as_str(), "/article" | "/decoy" | "/redirect")
        && destination.username().is_empty()
        && destination.password().is_empty()
        && destination.search().is_empty()
        && destination.hash().is_empty()
        && !destination_url
            .bytes()
            .any(|byte| matches!(byte, b'?' | b'#'))
        && destination.href() == destination_url
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn pending_navigation_allows_only_setup_and_cleanup() {
        assert!(allowed_while_pending("Page.getFrameTree"));
        assert!(allowed_while_pending("Runtime.runIfWaitingForDebugger"));
        assert!(!allowed_while_pending("Runtime.evaluate"));
        assert!(!allowed_while_pending("Runtime.callFunctionOn"));
        let mut binding = Binding {
            tab: 1,
            document: "doc".into(),
            target: "target".into(),
            frame: "frame".into(),
            session: ROOT_SESSION.into(),
            unique_context: "-1197265248696450148.-6130030496362069712".into(),
            grant_revision: 1,
        };
        assert!(valid_binding(&binding));
        binding.unique_context.push('\n');
        assert!(!valid_binding(&binding));
    }
}
