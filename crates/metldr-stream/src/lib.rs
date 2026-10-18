mod decoder;
mod thinking;

pub use decoder::Decoder;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::Decoder;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct OllamaStream(Decoder);

    #[wasm_bindgen]
    impl OllamaStream {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Self {
            Self(Decoder::new())
        }

        pub fn push(&mut self, bytes: &[u8]) -> Result<String, JsError> {
            self.0.push(bytes).map_err(|error| JsError::new(&error))
        }

        pub fn finish(&mut self) -> Result<String, JsError> {
            self.0.finish().map_err(|error| JsError::new(&error))
        }

        pub fn is_done(&self) -> bool {
            self.0.is_done()
        }
    }
}
