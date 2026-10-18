pub mod guide;
pub mod setup_guide;
mod state;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
mod controller;
#[cfg(target_arch = "wasm32")]
mod dom;
#[cfg(target_arch = "wasm32")]
mod download;
#[cfg(target_arch = "wasm32")]
mod guide_view;
#[cfg(target_arch = "wasm32")]
mod scene;
#[cfg(target_arch = "wasm32")]
mod setup_view;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    controller::mount()
}
