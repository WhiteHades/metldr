use rune::{App, AppSpec, Color, Cx, El, TextEdit, TextStyle, Tx, col, oklch, text, text_input};
use std::sync::LazyLock;

static COLORS: LazyLock<[Color; 4]> = LazyLock::new(|| {
    [
        oklch(0.26, 0.006, 271.191),
        oklch(0.964, 0.002, 247.839),
        oklch(0.541, 0.01, 264.484),
        oklch(0.308, 0.006, 258.354),
    ]
});

struct State {
    draft: TextEdit,
    recipient: TextEdit,
    secret: TextEdit,
    saved: TextEdit,
    disabled: TextEdit,
    submitted: Option<String>,
    recipient_renamed: bool,
    recipient_hidden: bool,
}

impl Default for State {
    fn default() -> Self {
        let mut secret = TextEdit::new("fixture-only-secret");
        secret.set_secure(true);
        Self {
            draft: TextEdit::default(),
            recipient: TextEdit::new("recipient-start"),
            secret,
            saved: TextEdit::new("saved-start"),
            disabled: TextEdit::new("disabled-start"),
            submitted: None,
            recipient_renamed: false,
            recipient_hidden: false,
        }
    }
}

fn submit(state: &mut State, _: &mut Tx) {
    let prompt = state.draft.text.trim();
    if !prompt.is_empty() {
        state.submitted = Some(prompt.to_owned());
        state.draft.clear();
    }
}

fn view(state: &State, _: &mut Cx<State>) -> El {
    let [surface, ink, secondary, line] = *COLORS;
    let field = text_input(&state.draft, TextStyle::new(14.0, 400.0))
        .key("prompt")
        .label("Prompt")
        .placeholder("Write a message…", secondary)
        .color(ink)
        .w_full()
        .min_h(68.0)
        .max_h(100.0)
        .padding(8.0)
        .on_edit(|state: &mut State| &mut state.draft)
        .on_submit(submit);
    let input = |edit: &TextEdit, key: &str, label: &str| {
        text_input(edit, TextStyle::new(14.0, 400.0))
            .key(key)
            .label(label)
            .placeholder(label, secondary)
            .color(ink)
            .w_full()
            .min_h(38.0)
            .padding(8.0)
            .fill(surface)
            .border(1.0, line)
    };
    col()
        .w_full()
        .gap(12.0)
        .padding(16.0)
        .child(
            col()
                .w_full()
                .padding(14.0)
                .radius(22.0)
                .fill(surface)
                .border(1.0, line)
                .child(field),
        )
        .children((!state.recipient_hidden).then(|| {
            input(
                &state.recipient,
                "recipient",
                if state.recipient_renamed {
                    "Reply recipient"
                } else {
                    "Recipient"
                },
            )
            .on_edit(|state: &mut State| &mut state.recipient)
        }))
        .child(
            input(&state.secret, "secret", "API token")
                .on_edit(|state: &mut State| &mut state.secret),
        )
        .child(input(&state.saved, "saved", "Saved value"))
        .child(
            input(&state.disabled, "disabled", "Unavailable")
                .on_edit(|state: &mut State| &mut state.disabled)
                .disabled(true),
        )
        .child(
            text(state.submitted.as_deref().unwrap_or(""))
                .key("submitted")
                .w_full()
                .font(TextStyle::new(14.0, 400.0))
                .color(ink),
        )
}

fn app() -> App<State> {
    App::new(AppSpec::new(
        "MetLDR Rune runtime probe",
        (640.0, 220.0),
        State::default,
        view,
    ))
}

pub fn run() -> Result<(), String> {
    app().try_run()
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static OWNER: RefCell<Option<rune::AppHandle<State>>> = const { RefCell::new(None) };
    }

    #[wasm_bindgen]
    pub fn mount() -> Result<(), JsValue> {
        OWNER.with(|owner| {
            let mut owner = owner.borrow_mut();
            if owner.is_some() {
                return Err(JsValue::from_str("Rune probe is already mounted"));
            }
            let app = app().canvas_id("metldr-rune-probe").fit_content_height();
            let handle = app.handle();
            app.try_run().map_err(|error| JsValue::from_str(&error))?;
            *owner = Some(handle);
            Ok(())
        })
    }

    #[wasm_bindgen]
    pub fn unmount() {
        OWNER.with(|owner| {
            if let Some(handle) = owner.borrow_mut().take() {
                handle.quit();
            }
        });
    }

    #[wasm_bindgen]
    pub fn rename_recipient() -> Result<(), JsValue> {
        OWNER.with(|owner| {
            owner
                .borrow()
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Rune probe is not mounted"))?
                .update(|state, _| state.recipient_renamed = true);
            Ok(())
        })
    }

    #[wasm_bindgen]
    pub fn handoff_recipient() -> Result<(), JsValue> {
        OWNER.with(|owner| {
            owner
                .borrow()
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Rune probe is not mounted"))?
                .update(|state, _| {
                    state.recipient_hidden = true;
                    state.recipient.focused = false;
                    state.draft.focused = true;
                });
            Ok(())
        })
    }
}

#[cfg(test)]
mod checks;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_nonempty_submission_clears_the_draft() {
        let mut state = State::default();
        let mut tx = Tx::default();
        submit(&mut state, &mut tx);
        assert!(state.submitted.is_none());
        state.draft.replace_text("  owned prompt  ");
        submit(&mut state, &mut tx);
        assert_eq!(state.submitted.as_deref(), Some("owned prompt"));
        assert!(state.draft.text.is_empty());
    }
}
