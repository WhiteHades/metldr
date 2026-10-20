use super::*;
use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;

pub(super) struct Released {
    run: String,
    tab: i32,
    epoch: u64,
    last_call: u64,
    cleanup: Promise,
}

pub(super) fn remember(connection: u64, attachment: &Attachment, cleanup: Promise) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.connection == connection && state.native.is_some() && state.active.is_none() {
            state.released = Some(Released {
                run: attachment.run.clone(),
                tab: attachment.tab,
                epoch: attachment.epoch,
                last_call: attachment.last_call,
                cleanup,
            });
        }
    });
}

pub(super) async fn acknowledge(
    connection: u64,
    call: u64,
    run: &str,
    tab: i32,
) -> Result<Value, String> {
    ensure_connection(connection)?;
    let (epoch, cleanup) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.active.is_some() {
            return Err("Browser attachment changed before release confirmation.".to_owned());
        }
        let released = state
            .released
            .as_mut()
            .filter(|released| {
                released.run == run && released.tab == tab && call > released.last_call
            })
            .ok_or_else(|| "No matching browser release is available.".to_owned())?;
        released.last_call = call;
        Ok((released.epoch, released.cleanup.clone()))
    })?;
    let confirmed = JsFuture::from(cleanup).await.map_err(message)?;
    ensure_connection(connection)?;
    let current = STATE.with(|state| {
        let state = state.borrow();
        state.active.is_none()
            && state.released.as_ref().is_some_and(|released| {
                released.run == run
                    && released.tab == tab
                    && released.epoch == epoch
                    && released.last_call == call
            })
    });
    if !current || confirmed.as_bool() != Some(true) {
        return Err("Browser release was not confirmed.".into());
    }
    Ok(json!({"detached":true}))
}
