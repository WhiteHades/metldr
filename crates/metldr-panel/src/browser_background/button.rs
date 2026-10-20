use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TapParams {
    x: f64,
    y: f64,
    duration: u32,
    #[serde(rename = "tapCount")]
    tap_count: u8,
    #[serde(rename = "gestureSourceType")]
    gesture_source_type: String,
}

pub(super) async fn activate(
    connection: u64,
    call_id: u64,
    run: &str,
    action: String,
    revision: u64,
    binding: navigation::Binding,
    url: String,
    params: Value,
) -> Result<Value, String> {
    let tap: TapParams = serde_json::from_value(params.clone())
        .map_err(|_| "Invalid button gesture parameters.".to_owned())?;
    if !bounded_id(&action, 128)
        || revision == 0
        || !navigation::valid_binding(&binding)
        || binding.tab < 0
        || validate_url(&url).is_err()
        || !tap.x.is_finite()
        || !tap.y.is_finite()
        || !(0.0..=100_000.0).contains(&tap.x)
        || !(0.0..=100_000.0).contains(&tap.y)
        || tap.duration != 50
        || tap.tap_count != 1
        || tap.gesture_source_type != "mouse"
        || !bounded_json(&params, MAX_FRAME)
    {
        return Err("Invalid browser button request.".into());
    }
    let session = navigation::root_session(&binding.session);
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
            || !active.buttons.insert(action.clone())
        {
            return false;
        }
        active.command_count = 1;
        true
    });
    if !reserved {
        return Err("Button action does not match the active browser grant.".into());
    }

    let result = async {
        verify(connection, epoch).await?;
        let target = send(
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
            .and_then(|value| value.get("targetId"))
            .and_then(Value::as_str)
            != Some(binding.target.as_str())
        {
            return Err("Browser target changed before button activation.".into());
        }
        let tree = send(
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
            .and_then(|value| value.get("frame"))
            .ok_or_else(|| "Missing root frame before button activation.".to_owned())?;
        if root.get("id").and_then(Value::as_str) != Some(binding.frame.as_str())
            || root.get("url").and_then(Value::as_str) != Some(url.as_str())
        {
            return Err("Browser root frame changed before button activation.".into());
        }
        verify(connection, epoch).await?;
        let debugger = chrome_api("debugger").map_err(message)?;
        let target = match session {
            Some(session) => json!({"tabId":binding.tab,"sessionId":session}),
            None => json!({"tabId":binding.tab}),
        };
        let response = await_call(
            &debugger,
            "sendCommand",
            &[
                parse(&target).map_err(message)?,
                JsValue::from_str("Input.synthesizeTapGesture"),
                parse(&params).map_err(message)?,
            ],
        )
        .await;
        ensure_epoch(connection, epoch)?;
        response.map_err(message)?;
        verify(connection, epoch).await?;
        Ok(json!({"activated":true}))
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

async fn send(
    connection: u64,
    epoch: u64,
    tab: i32,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let debugger = chrome_api("debugger").map_err(message)?;
    let target = match session {
        Some(session) => json!({"tabId":tab,"sessionId":session}),
        None => json!({"tabId":tab}),
    };
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
