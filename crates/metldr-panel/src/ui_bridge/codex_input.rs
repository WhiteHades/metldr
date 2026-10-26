use super::*;
use crate::model::{
    CodexFormField, CodexInputPrompt, CodexInputView, CodexQuestion, CodexQuestionOption, RunRoute,
    RunStatus,
};
use serde_json::{Map, Value, json};

mod answer;
mod display;
mod observe;
pub use answer::{UiCodexAnswer, UiCodexFieldAnswer, UiCodexQuestionAnswer};
pub use display::{
    UiCodexField, UiCodexOption, UiCodexPromptKind, UiCodexQuestion, UiCodexRequest,
    UiCodexSnapshot,
};
pub(super) use observe::Observer;
pub use observe::UiCodexSubscription;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Key {
    run: RunId,
    request: RunId,
}
impl Key {
    fn offered(&self, ui: Option<&CodexInputView>) -> bool {
        ui.is_some_and(|ui| {
            ui.run == self.run && ui.pending.iter().any(|p| p.request == self.request)
        })
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Operation {
    #[default]
    Idle,
    Sending(Key),
    Waiting(Key),
    Uncertain(Key),
}
impl Operation {
    fn observe(&mut self, ui: Option<&CodexInputView>) {
        if let Self::Waiting(key) = self
            && !key.offered(ui)
        {
            *self = Self::Idle;
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct State {
    connected: bool,
    stale: bool,
    ready: bool,
    recovery_required: bool,
    running: bool,
    ui: Option<CodexInputView>,
    operation: Operation,
}
impl State {
    fn read(core: &Core) -> Self {
        let view = core.page.view.borrow();
        let ui = view.codex_input.clone();
        let running = ui
            .as_ref()
            .and_then(|ui| view.runs.iter().find(|r| r.id == ui.run))
            .is_some_and(|run| {
                matches!(&run.route, RunRoute::Codex { .. }) && run.status == RunStatus::Running
            });
        Self {
            connected: core.flow().is_connected(),
            stale: core.page.invalidated.get()
                || view.selection != core.scope
                || !core
                    .page
                    .current
                    .borrow()
                    .upgrade()
                    .is_some_and(|p| std::ptr::eq(p.as_ref(), core)),
            ready: core.owner.borrow().readable().is_ok()
                && !core.page.recovery.borrow().active()
                && !matches!(*core.owner.borrow(), Owner::Uncertain),
            recovery_required: matches!(*core.owner.borrow(), Owner::Uncertain),
            running,
            ui,
            operation: core.codex_operation.borrow().clone(),
        }
    }
    fn enabled(&self) -> bool {
        self.connected
            && !self.stale
            && self.ready
            && self.running
            && matches!(self.operation, Operation::Idle)
    }
}

#[wasm_bindgen]
pub struct UiCodexInput {
    core: Rc<Core>,
    activation: super::activation::Activation,
}
#[wasm_bindgen]
impl UiBridge {
    pub fn codex_input(&self) -> Result<UiCodexInput, JsValue> {
        Ok(UiCodexInput {
            core: Rc::clone(&self.core),
            activation: super::activation::Activation::new().map_err(js_error)?,
        })
    }
}
#[wasm_bindgen]
impl UiCodexInput {
    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiCodexSnapshot) => void")]
        callback: Function,
    ) -> Result<UiCodexSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.core), callback)
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn send(
        &self,
        run: String,
        request: String,
        answer: UiCodexAnswer,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.activation.require(&event).map_err(js_error)?;
        let parse = |id| {
            serde_json::from_value(Value::String(id))
                .map_err(|_| crate::err("Invalid Codex request identity."))
        };
        let key = Key {
            run: parse(run)?,
            request: parse(request)?,
        };
        let state = State::read(&self.core);
        let prompt = state
            .ui
            .as_ref()
            .filter(|ui| ui.run == key.run)
            .and_then(|ui| ui.pending.iter().find(|p| p.request == key.request))
            .map(|p| &p.prompt)
            .ok_or_else(|| crate::err("This Codex request is no longer available."))?;
        if !state.enabled() {
            return Err(crate::err("This Codex reply is no longer available."));
        }
        let reply = answer
            .validate(prompt)
            .and_then(ValidatedReply::bounded)
            .map_err(|message| crate::err(&message))?;
        *self.core.codex_operation.borrow_mut() = Operation::Sending(key.clone());
        self.core.notify_codex_input();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let result = core
                .flow()
                .codex_input_reply(
                    core.scope.clone(),
                    key.run.clone(),
                    key.request.clone(),
                    reply,
                )
                .await;
            let result = match result {
                Ok(view) => match core.accept(view) {
                    Ok(_) => {
                        *core.codex_operation.borrow_mut() = Operation::Waiting(key);
                        Ok(())
                    }
                    Err(error) => {
                        *core.codex_operation.borrow_mut() = Operation::Uncertain(key);
                        Err(error)
                    }
                },
                Err(error) => {
                    *core.codex_operation.borrow_mut() = Operation::Uncertain(key);
                    Err(error)
                }
            };
            core.notify_codex_input();
            result.map(|()| JsValue::UNDEFINED).map_err(js_error)
        }))
    }
}

impl Core {
    pub(super) fn notify_codex_input(&self) {
        self.page.notify_connection();
        {
            let view = self.page.view.borrow();
            if view.selection == self.scope {
                self.codex_operation
                    .borrow_mut()
                    .observe(view.codex_input.as_ref());
            }
        }
        let observers: Vec<_> = {
            let mut list = self.codex_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|o| !o.closed()));
            list.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for observer in observers {
            if observer.emit().is_err() {
                observer.close();
            }
            observer.wake();
        }
    }
}

struct ValidatedReply {
    companion: Value,
    native: Value,
}

impl ValidatedReply {
    fn bounded(self) -> Result<Value, String> {
        if serde_json::to_vec(&self.native).map_or(true, |bytes| bytes.len() > 16 * 1024) {
            return Err("Shorten the answer. Codex accepts up to 16 KiB.".into());
        }
        Ok(self.companion)
    }
}

impl UiCodexAnswer {
    fn validate(self, prompt: &CodexInputPrompt) -> Result<ValidatedReply, String> {
        use answer::Kind;
        match (self.kind, prompt) {
            (Kind::Skip, CodexInputPrompt::Questions { .. }) => Ok(ValidatedReply {
                companion: json!({"kind":"questions","value":{}}),
                native: json!({"answers":{}}),
            }),
            (Kind::Questions(rows), CodexInputPrompt::Questions { questions, .. }) => {
                let mut values = Map::new();
                let mut native = Map::new();
                for row in rows {
                    let question = questions
                        .iter()
                        .find(|q| q.id == row.id)
                        .ok_or("Unknown question.")?;
                    if values.contains_key(&row.id) {
                        return Err("Question IDs must be unique.".into());
                    }
                    if row.values.is_empty() || row.values.len() > 16 {
                        return Err("Choose between 1 and 16 answers for every question.".into());
                    }
                    for value in &row.values {
                        let other = question.other
                            && value != ""
                            && !question.options.iter().any(|o| o.label == *value);
                        if !question.options.iter().any(|o| o.label == *value)
                            && !(other && value.chars().count() <= 4096)
                        {
                            return Err("Question answer was not offered.".into());
                        }
                    }
                    let answers = Value::Array(row.values.into_iter().map(Value::String).collect());
                    values.insert(row.id.clone(), answers.clone());
                    native.insert(row.id, json!({"answers":answers}));
                }
                if values.len() != questions.len() {
                    return Err("Choose an answer for every question, or use Skip.".into());
                }
                Ok(ValidatedReply {
                    companion: json!({"kind":"questions","value":values}),
                    native: json!({"answers":native}),
                })
            }
            (Kind::Form(rows), CodexInputPrompt::McpForm { fields, .. }) => {
                let values = validate_fields(rows, fields)?;
                Ok(ValidatedReply {
                    companion: json!({"kind":"accept","value":values}),
                    native: json!({"action":"accept","content":values}),
                })
            }
            (Kind::Allow, CodexInputPrompt::McpToolApproval { .. }) => Ok(ValidatedReply {
                companion: json!({"kind":"accept","value":{}}),
                native: json!({"action":"accept","content":{}}),
            }),
            (
                Kind::Deny,
                CodexInputPrompt::McpForm { .. } | CodexInputPrompt::McpToolApproval { .. },
            ) => Ok(ValidatedReply {
                companion: json!({"kind":"decline"}),
                native: json!({"action":"decline","content":null}),
            }),
            (
                Kind::Cancel,
                CodexInputPrompt::McpForm { .. } | CodexInputPrompt::McpToolApproval { .. },
            ) => Ok(ValidatedReply {
                companion: json!({"kind":"cancel"}),
                native: json!({"action":"cancel","content":null}),
            }),
            _ => Err("This answer does not match the current Codex request.".into()),
        }
    }
}
fn validate_fields(
    rows: Vec<answer::Field>,
    fields: &[CodexFormField],
) -> Result<Map<String, Value>, String> {
    let mut values = Map::new();
    for row in rows {
        let field = fields
            .iter()
            .find(|field| field.name == row.name)
            .ok_or("Unknown form field.")?;
        if values.contains_key(&row.name) {
            return Err("Form field names must be unique.".into());
        }
        let count = row.value.chars().count();
        if count < field.min_chars as usize
            || count > field.max_chars as usize
            || field
                .choices
                .as_ref()
                .is_some_and(|choices| !choices.contains(&row.value))
        {
            return Err("Form value is outside the allowed range.".into());
        }
        values.insert(row.name, Value::String(row.value));
    }
    if values.len() != fields.len() {
        return Err("Complete every form field.".into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use answer::{Field, Kind, Question};
    #[test]
    fn validates_complete_unicode_answers_and_distinct_mcp_outcomes() {
        let q = CodexInputPrompt::Questions {
            item: "setup".into(),
            is_blocking: true,
            questions: vec![CodexQuestion {
                id: "x".into(),
                header: String::new(),
                text: String::new(),
                other: true,
                options: vec![CodexQuestionOption {
                    label: "<b>raw🌱</b>".into(),
                    description: String::new(),
                }],
            }],
        };
        let submitted = UiCodexAnswer {
            kind: Kind::Questions(vec![Question {
                id: "x".into(),
                values: vec!["<b>raw🌱</b>".into()],
            }]),
        }
        .validate(&q)
        .unwrap();
        assert_eq!(submitted.companion["value"]["x"][0], "<b>raw🌱</b>");
        assert_eq!(
            submitted.native["answers"]["x"]["answers"][0],
            "<b>raw🌱</b>"
        );
        let skipped = UiCodexAnswer { kind: Kind::Skip }.validate(&q).unwrap();
        assert_eq!(skipped.companion["value"], json!({}));
        assert_eq!(skipped.native, json!({"answers":{}}));
        let field = CodexFormField {
            name: "name".into(),
            title: String::new(),
            description: None,
            min_chars: 1,
            max_chars: 1,
            choices: None,
            default: None,
        };
        let form = CodexInputPrompt::McpForm {
            server: "s".into(),
            message: String::new(),
            fields: vec![field],
        };
        assert!(
            UiCodexAnswer {
                kind: Kind::Form(vec![Field {
                    name: "name".into(),
                    value: "🌱".into()
                }])
            }
            .validate(&form)
            .is_ok()
        );
        assert!(
            UiCodexAnswer {
                kind: Kind::Questions(vec![])
            }
            .validate(&q)
            .is_err()
        );
        assert!(
            UiCodexAnswer {
                kind: Kind::Form(vec![
                    Field {
                        name: "name".into(),
                        value: "🌱".into()
                    },
                    Field {
                        name: "name".into(),
                        value: "🌱".into()
                    },
                ])
            }
            .validate(&form)
            .is_err()
        );
        let approval = CodexInputPrompt::McpToolApproval {
            server: "s".into(),
            message: String::new(),
            review: "{}".into(),
        };
        assert_eq!(
            UiCodexAnswer { kind: Kind::Deny }
                .validate(&approval)
                .unwrap()
                .companion["kind"],
            "decline"
        );
        assert_eq!(
            UiCodexAnswer { kind: Kind::Cancel }
                .validate(&approval)
                .unwrap()
                .companion["kind"],
            "cancel"
        );
        let id = |n| {
            serde_json::from_value(json!(format!("0123456789abcdef0123456789abcdef:{n}"))).unwrap()
        };
        let key = Key {
            run: id(1),
            request: id(2),
        };
        let mut waiting = Operation::Waiting(key.clone());
        waiting.observe(None);
        assert!(matches!(waiting, Operation::Idle));
        let mut sending = Operation::Sending(key.clone());
        sending.observe(None);
        assert!(matches!(sending, Operation::Sending(_)));
        let mut uncertain = Operation::Uncertain(key);
        uncertain.observe(None);
        assert!(matches!(uncertain, Operation::Uncertain(_)));
        let fields = ["a", "b", "c", "d"].map(|name| CodexFormField {
            name: name.into(),
            title: String::new(),
            description: None,
            min_chars: 0,
            max_chars: 4096,
            choices: None,
            default: None,
        });
        let values = ["a", "b", "c", "d"].map(|name| Field {
            name: name.into(),
            value: "x".repeat(if name == "d" { 4038 } else { 4096 }),
        });
        let boundary = UiCodexAnswer {
            kind: Kind::Form(values.into()),
        }
        .validate(&CodexInputPrompt::McpForm {
            server: "s".into(),
            message: String::new(),
            fields: fields.into(),
        })
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&boundary.companion).unwrap().len(),
            16381
        );
        assert_eq!(serde_json::to_vec(&boundary.native).unwrap().len(), 16385);
        assert!(boundary.bounded().is_err());
    }
}
