use super::*;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::Event;

const KEY: &str = "bui-sounds";

pub(super) struct Listener {
    window: Window,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .window
            .remove_event_listener_with_callback("storage", self.callback.as_ref().unchecked_ref());
    }
}

pub(super) fn install(core: &Rc<Core>) -> Result<(), JsValue> {
    let weak = Rc::downgrade(core);
    let callback = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        let Some(core) = weak.upgrade().filter(|core| !core.closed.get()) else {
            return;
        };
        let Ok(key) = get(event.as_ref(), "key") else {
            return;
        };
        if !key.is_null() && key.as_string().as_deref() != Some(KEY) {
            return;
        }
        let Some(storage) = core.window.local_storage().ok().flatten() else {
            return;
        };
        if get(event.as_ref(), "storageArea").ok().as_ref() != Some(storage.as_ref()) {
            return;
        }
        core.read_sound();
    });
    core.window
        .add_event_listener_with_callback("storage", callback.as_ref().unchecked_ref())?;
    core.sound_listener.replace(Some(Listener {
        window: core.window.clone(),
        callback,
    }));
    Ok(())
}

impl Core {
    pub(super) fn read_sound(&self) {
        if self.closed.get() {
            return;
        }
        let stored = self.window.local_storage().and_then(|storage| {
            storage
                .ok_or_else(|| err("Sound storage is unavailable."))?
                .get_item(KEY)
        });
        let sound = match stored {
            Ok(value) => Sound {
                enabled: model::sounds_enabled(value.as_deref()),
                status: UiPreferenceStatus::Saved,
            },
            Err(_) => Sound {
                enabled: true,
                status: UiPreferenceStatus::LoadFailed,
            },
        };
        self.sound.set(sound);
        self.notify();
    }

    pub(super) fn save_sound(&self, enabled: bool) -> Result<(), JsValue> {
        let result = self.window.local_storage().and_then(|storage| {
            let storage = storage.ok_or_else(|| err("Sound storage is unavailable."))?;
            if enabled {
                storage.remove_item(KEY)
            } else {
                storage.set_item(KEY, "off")
            }
        });
        self.sound.set(Sound {
            enabled,
            status: if result.is_ok() {
                UiPreferenceStatus::Saved
            } else {
                UiPreferenceStatus::SaveFailed
            },
        });
        self.notify();
        result.map_err(|_| err("Could not save the sound setting. Retry to save it."))
    }
}
