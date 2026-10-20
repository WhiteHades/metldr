use js_sys::Object;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::{BACKGROUND, MAX_FRAME, browser_background, call, flow, forward_panel, get, profile};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Connecting,
    SelectingProfile,
    AwaitingReady,
    Ready,
}

#[derive(Default)]
pub(super) struct Gate {
    phase: Option<Phase>,
    pending: Vec<JsValue>,
}

impl Gate {
    pub(super) fn connect(&mut self) {
        self.phase = Some(Phase::Connecting);
        self.pending.clear();
    }

    pub(super) fn disconnect(&mut self) {
        self.phase = None;
        self.pending.clear();
    }

    pub(super) fn queue(&mut self, frame: JsValue) -> Result<bool, ()> {
        if self.phase == Some(Phase::Ready) {
            return Ok(true);
        }
        if self.phase.is_none() || self.pending.len() >= 16 {
            return Err(());
        }
        let raw = js_sys::JSON::stringify(&frame)
            .map_err(|_| ())?
            .as_string()
            .ok_or(())?;
        if raw.len() > MAX_FRAME {
            return Err(());
        }
        self.pending.push(frame);
        Ok(false)
    }

    pub(super) fn select_profile(&mut self) -> bool {
        if self.phase != Some(Phase::Connecting) {
            return false;
        }
        self.phase = Some(Phase::SelectingProfile);
        true
    }

    pub(super) fn awaiting_ready(&mut self) -> bool {
        if self.phase != Some(Phase::SelectingProfile) {
            return false;
        }
        self.phase = Some(Phase::AwaitingReady);
        true
    }

    pub(super) fn ready(&mut self) -> Option<Vec<JsValue>> {
        if !matches!(self.phase, Some(Phase::Connecting | Phase::AwaitingReady)) {
            return None;
        }
        self.phase = Some(Phase::Ready);
        Some(std::mem::take(&mut self.pending))
    }

    pub(super) fn is_ready(&self) -> bool {
        self.phase == Some(Phase::Ready)
    }

    pub(super) fn can_discard_changed(&self) -> bool {
        matches!(self.phase, Some(Phase::Connecting | Phase::AwaitingReady))
    }
}

pub(super) fn native_message(message: JsValue) {
    let raw = js_sys::JSON::stringify(&message)
        .ok()
        .and_then(|value| value.as_string());
    let Some(_raw) = raw.filter(|raw| raw.len() <= MAX_FRAME) else {
        disconnect_current();
        return;
    };
    match profile::is_profile_required(&message) {
        Ok(true) => {
            let native = BACKGROUND.with(|b| {
                let mut background = b.borrow_mut();
                if !background.readiness.select_profile() {
                    return None;
                }
                background.native.clone()
            });
            if let Some(native) = native {
                request_profile(native);
            } else {
                disconnect_current();
            }
            return;
        }
        Ok(false) => {}
        Err(_) => {
            disconnect_current();
            return;
        }
    }
    let kind = get(&message, "kind")
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_default();
    if kind == "ready" {
        if !flow::is_snapshot_frame(message.clone(), "ready") {
            disconnect_current();
            return;
        }
        let (native, pending) = BACKGROUND.with(|b| {
            let mut background = b.borrow_mut();
            let pending = background.readiness.ready();
            (background.native.clone(), pending)
        });
        let Some(pending) = pending else {
            disconnect_current();
            return;
        };
        if let Some(native) = native {
            for frame in pending {
                if !is_current(&native) {
                    break;
                }
                if call(&native, "postMessage", &[frame]).is_err() {
                    disconnect_current();
                    return;
                }
            }
        }
    } else if kind == "changed" && BACKGROUND.with(|b| b.borrow().readiness.can_discard_changed()) {
        if !flow::is_snapshot_frame(message, "changed") {
            disconnect_current();
        }
        return;
    } else if !BACKGROUND.with(|b| b.borrow().readiness.is_ready()) {
        disconnect_current();
        return;
    }
    if browser_background::receive(&message) {
        return;
    }
    let Ok(raw) = js_sys::JSON::stringify(&message) else {
        return;
    };
    let Some(raw) = raw.as_string().filter(|raw| raw.len() <= MAX_FRAME) else {
        return;
    };
    let Ok(value) = js_sys::JSON::parse(&raw) else {
        return;
    };
    let panels = BACKGROUND.with(|b| b.borrow().panels.clone());
    for port in panels {
        let _ = call(&port, "postMessage", &[value.clone()]);
    }
}

fn request_profile(native: JsValue) {
    let lookup = profile::key();
    spawn_local(async move {
        let key = JsFuture::from(lookup)
            .await
            .ok()
            .and_then(|value| value.as_string());
        if !is_current(&native) {
            return;
        }
        let Some(key) = key else {
            forward_panel(&json!({"v":4,"request":null,"kind":"disconnected"}));
            disconnect_current();
            return;
        };
        let frame = json!({"v":4,"kind":"profile","key":key});
        let Ok(raw) = serde_json::to_string(&frame) else {
            disconnect_current();
            return;
        };
        if raw.len() > MAX_FRAME {
            disconnect_current();
            return;
        }
        let Ok(value) = js_sys::JSON::parse(&raw) else {
            disconnect_current();
            return;
        };
        let awaiting_ready = BACKGROUND.with(|b| {
            let mut background = b.borrow_mut();
            background
                .native
                .as_ref()
                .is_some_and(|current| Object::is(current, &native))
                && background.readiness.awaiting_ready()
        });
        if !awaiting_ready {
            disconnect_current();
            return;
        }
        if is_current(&native) && call(&native, "postMessage", &[value]).is_err() {
            disconnect_current();
        }
    });
}

fn is_current(native: &JsValue) -> bool {
    BACKGROUND.with(|b| {
        b.borrow()
            .native
            .as_ref()
            .is_some_and(|current| Object::is(current, native))
    })
}

pub(super) fn disconnect_current() {
    if let Some(native) = BACKGROUND.with(|b| b.borrow().native.clone()) {
        BACKGROUND.with(|b| b.borrow_mut().readiness.disconnect());
        let _ = call(&native, "disconnect", &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::Gate;

    #[test]
    fn ready_requires_profile_reply_but_accepts_legacy_startup() {
        let mut installed = Gate::default();
        installed.connect();
        assert!(installed.select_profile());
        assert!(installed.ready().is_none());
        assert!(installed.awaiting_ready());
        assert!(installed.ready().is_some());

        let mut legacy = Gate::default();
        legacy.connect();
        assert!(legacy.ready().is_some());
    }
}
