use std::num::NonZeroU32;

use metldr_conversation::{CompleteSource, GeneratedAnswer};
use serde::{Deserialize, Serialize};

use crate::protocol::{Binding, HostMessage, encode_host, encode_initialize};

const MAX_PROGRAM_BYTES: usize = 2 * crate::protocol::MAX_HOST_BYTES;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedProgram<B, A> {
    binding: B,
    answer: A,
}

pub fn encode_program(binding: &Binding, answer: &GeneratedAnswer) -> Result<String, String> {
    let answer = answer.clone().finish()?;
    let encoded = serde_json::to_string(&SavedProgram { binding, answer })
        .map_err(|error| error.to_string())?;
    bounded(encoded)
}

fn bounded(encoded: String) -> Result<String, String> {
    if encoded.len() > MAX_PROGRAM_BYTES {
        Err("saved answer program exceeds its byte limit".into())
    } else {
        Ok(encoded)
    }
}

#[cfg_attr(feature = "guest", wasm_bindgen::prelude::wasm_bindgen)]
pub struct GuestProgram {
    binding: Binding,
    source: CompleteSource,
    cursor: usize,
}

impl GuestProgram {
    fn parse(encoded: &str) -> Result<Self, String> {
        if encoded.len() > MAX_PROGRAM_BYTES {
            return Err("saved answer program exceeds its byte limit".into());
        }
        let saved: SavedProgram<Binding, CompleteSource> =
            serde_json::from_str(encoded).map_err(|_| "invalid saved answer program".to_owned())?;
        Ok(Self {
            binding: saved.binding,
            source: saved.answer,
            cursor: 0,
        })
    }

    fn initialization_message(&self) -> Result<String, String> {
        encode_initialize(&self.binding)
    }

    fn next_message(&mut self) -> Result<Option<String>, String> {
        let request_id =
            |value| NonZeroU32::new(value).ok_or_else(|| "invalid request id".to_owned());
        let view = self.source.view();
        let message = match self.cursor {
            0 => Some(HostMessage::Document {
                request_id: request_id(1)?,
                css: view.css.unwrap_or_default().to_owned(),
                html: view.html.unwrap_or_default().concat(),
            }),
            1 => Some(HostMessage::Functions {
                request_id: request_id(2)?,
                code: view.js_functions.unwrap_or_default().to_owned(),
            }),
            cursor => view
                .js_expressions
                .unwrap_or_default()
                .get(cursor - 2)
                .map(|code| HostMessage::Expression {
                    request_id: request_id(cursor as u32 + 1).expect("bounded expression count"),
                    code: code.clone(),
                }),
        };
        let Some(message) = message else {
            return Ok(None);
        };
        let encoded = encode_host(&self.binding, message)?;
        self.cursor += 1;
        Ok(Some(encoded))
    }
}

#[cfg_attr(feature = "guest", wasm_bindgen::prelude::wasm_bindgen)]
impl GuestProgram {
    #[cfg_attr(feature = "guest", wasm_bindgen::prelude::wasm_bindgen(constructor))]
    pub fn new(encoded: String) -> Result<Self, String> {
        Self::parse(&encoded)
    }

    pub fn initialization(&self) -> Result<String, String> {
        self.initialization_message()
    }

    pub fn next(&mut self) -> Result<Option<String>, String> {
        self.next_message()
    }
}

#[cfg(test)]
#[path = "program/checks.rs"]
mod checks;
