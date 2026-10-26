use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct UiCodexQuestionAnswer {
    pub(super) id: String,
    pub(super) values: Vec<String>,
}
#[wasm_bindgen]
impl UiCodexQuestionAnswer {
    #[wasm_bindgen(constructor)]
    pub fn new(id: String, values: Vec<String>) -> Self {
        Self { id, values }
    }
}

#[wasm_bindgen]
pub struct UiCodexFieldAnswer {
    pub(super) name: String,
    pub(super) value: String,
}
#[wasm_bindgen]
impl UiCodexFieldAnswer {
    #[wasm_bindgen(constructor)]
    pub fn new(name: String, value: String) -> Self {
        Self { name, value }
    }
}

pub(super) struct Question {
    pub id: String,
    pub values: Vec<String>,
}
pub(super) struct Field {
    pub name: String,
    pub value: String,
}
pub(super) enum Kind {
    Questions(Vec<Question>),
    Form(Vec<Field>),
    Skip,
    Allow,
    Deny,
    Cancel,
}

#[wasm_bindgen]
pub struct UiCodexAnswer {
    pub(super) kind: Kind,
}
#[wasm_bindgen]
impl UiCodexAnswer {
    pub fn questions(values: Vec<UiCodexQuestionAnswer>) -> Self {
        Self {
            kind: Kind::Questions(
                values
                    .into_iter()
                    .map(|v| Question {
                        id: v.id,
                        values: v.values,
                    })
                    .collect(),
            ),
        }
    }
    pub fn form(values: Vec<UiCodexFieldAnswer>) -> Self {
        Self {
            kind: Kind::Form(
                values
                    .into_iter()
                    .map(|v| Field {
                        name: v.name,
                        value: v.value,
                    })
                    .collect(),
            ),
        }
    }
    pub fn skip() -> Self {
        Self { kind: Kind::Skip }
    }
    pub fn allow() -> Self {
        Self { kind: Kind::Allow }
    }
    pub fn deny() -> Self {
        Self { kind: Kind::Deny }
    }
    pub fn cancel() -> Self {
        Self { kind: Kind::Cancel }
    }
}
