mod model;
mod observe;
mod sound;
mod storage;

use crate::{await_call, call, chrome_api, err, get};
use js_sys::{Function, Promise};
use model::{Appearance, Change, Phase, TextSize, Theme};
pub use observe::UiPreferenceSubscription;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use web_sys::Window;

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiPreferenceStatus {
    Loading,
    Saving,
    Saved,
    LoadFailed,
    SaveFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sound {
    enabled: bool,
    status: UiPreferenceStatus,
}

struct Core {
    window: Window,
    chrome: Option<JsValue>,
    appearance: RefCell<Appearance>,
    sound: Cell<Sound>,
    closed: Cell<bool>,
    chrome_listener: RefCell<Option<storage::Listener>>,
    sound_listener: RefCell<Option<sound::Listener>>,
    observers: RefCell<Vec<std::rc::Weak<observe::Observer>>>,
}

impl Core {
    fn ensure_open(&self) -> Result<(), JsValue> {
        if self.closed.get() {
            Err(err("These preferences were closed."))
        } else {
            Ok(())
        }
    }

    fn snapshot(&self) -> UiPreferenceSnapshot {
        let appearance = self.appearance.borrow();
        let status = match appearance.phase {
            Phase::Loading(_) => UiPreferenceStatus::Loading,
            Phase::Saving(_) => UiPreferenceStatus::Saving,
            Phase::Ready => UiPreferenceStatus::Saved,
            Phase::LoadFailed => UiPreferenceStatus::LoadFailed,
            Phase::SaveFailed(_) => UiPreferenceStatus::SaveFailed,
        };
        UiPreferenceSnapshot {
            theme: appearance.values.theme,
            size: appearance.values.size,
            appearance: status,
            sound: self.sound.get(),
        }
    }

    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.chrome_listener.borrow_mut().take();
        self.sound_listener.borrow_mut().take();
        self.observers.borrow_mut().clear();
    }
}

#[wasm_bindgen]
pub struct UiPreferences {
    core: Rc<Core>,
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiPreferenceSnapshot {
    theme: Theme,
    size: TextSize,
    appearance: UiPreferenceStatus,
    sound: Sound,
}

#[wasm_bindgen]
impl UiPreferenceSnapshot {
    #[wasm_bindgen(getter)]
    pub fn theme(&self) -> String {
        self.theme.key().into()
    }
    #[wasm_bindgen(getter)]
    pub fn font_size(&self) -> String {
        self.size.key().into()
    }
    #[wasm_bindgen(getter)]
    pub fn appearance_status(&self) -> UiPreferenceStatus {
        self.appearance
    }
    #[wasm_bindgen(getter)]
    pub fn sounds_enabled(&self) -> bool {
        self.sound.enabled
    }
    #[wasm_bindgen(getter)]
    pub fn sound_status(&self) -> UiPreferenceStatus {
        self.sound.status
    }
}

#[wasm_bindgen]
impl UiPreferences {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<UiPreferences, JsValue> {
        let window = web_sys::window().ok_or_else(|| err("Preferences need a browser window."))?;
        let chrome = chrome_api("storage")
            .ok()
            .and_then(|storage| get(&storage, "local").ok());
        let core = Rc::new(Core {
            window,
            chrome,
            appearance: RefCell::new(Appearance::default()),
            sound: Cell::new(Sound {
                enabled: true,
                status: UiPreferenceStatus::Saved,
            }),
            closed: Cell::new(false),
            chrome_listener: RefCell::new(None),
            sound_listener: RefCell::new(None),
            observers: RefCell::new(Vec::new()),
        });
        storage::install(&core)?;
        sound::install(&core)?;
        core.read_sound();
        let load = Rc::clone(&core);
        wasm_bindgen_futures::spawn_local(async move {
            load.load_appearance().await;
        });
        Ok(Self { core })
    }

    pub fn snapshot(&self) -> Result<UiPreferenceSnapshot, JsValue> {
        self.core.ensure_open()?;
        Ok(self.core.snapshot())
    }

    pub fn themes(&self) -> Vec<String> {
        Theme::ALL
            .into_iter()
            .map(|theme| theme.key().into())
            .collect()
    }
    pub fn font_sizes(&self) -> Vec<String> {
        TextSize::ALL
            .into_iter()
            .map(|size| size.key().into())
            .collect()
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn set_theme(&self, value: String) -> Result<Promise, JsValue> {
        let value = Theme::parse(&value).ok_or_else(|| err("Choose a supported theme."))?;
        self.change(Change::Theme(value))
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn set_font_size(&self, value: String) -> Result<Promise, JsValue> {
        let value = TextSize::parse(&value).ok_or_else(|| err("Choose a supported text size."))?;
        self.change(Change::Size(value))
    }

    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn retry_appearance(&self) -> Result<Promise, JsValue> {
        self.core.ensure_open()?;
        let pending = self.core.appearance.borrow_mut().retry().map_err(err)?;
        self.core.notify();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            match pending {
                Some(patch) => core.save_appearance(patch).await?,
                None => core.load_appearance().await,
            }
            Ok(JsValue::UNDEFINED)
        }))
    }

    pub fn set_sounds_enabled(&self, enabled: bool) -> Result<(), JsValue> {
        self.core.ensure_open()?;
        self.core.save_sound(enabled)
    }

    pub fn retry_sound(&self) -> Result<(), JsValue> {
        self.core.ensure_open()?;
        let sound = self.core.sound.get();
        if sound.status == UiPreferenceStatus::SaveFailed {
            self.core.save_sound(sound.enabled)
        } else {
            self.core.read_sound();
            Ok(())
        }
    }

    pub fn close(&self) {
        self.core.close();
    }
}

impl UiPreferences {
    fn change(&self, value: Change) -> Result<Promise, JsValue> {
        self.core.ensure_open()?;
        let patch = self
            .core
            .appearance
            .borrow_mut()
            .change(value)
            .map_err(err)?;
        self.core.notify();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            core.save_appearance(patch).await?;
            Ok(JsValue::UNDEFINED)
        }))
    }
}

impl Drop for UiPreferences {
    fn drop(&mut self) {
        self.core.close();
    }
}
