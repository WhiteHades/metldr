use rune::{
    Cx, El, Env, GrowthSpec, Key, KeyEvent, Mods, Runtime, ScrollSpec, TextEdit, TextStyle, col,
    el, text_input,
};

fn field(edit: &TextEdit, key: &str) -> El {
    text_input(edit, TextStyle::new(14.0, 400.0))
        .key(key)
        .size(200.0, 40.0)
}

fn press<S: 'static>(runtime: &mut Runtime<S>, key: Key) {
    runtime.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    runtime.frame(0.0);
}

fn focused<S: 'static>(runtime: &Runtime<S>) -> Option<String> {
    runtime
        .elements()
        .into_iter()
        .find(|element| element.focused)
        .and_then(|element| element.key)
}

#[test]
fn disabled_fields_cannot_take_or_keep_editing_focus() {
    struct Form {
        first: TextEdit,
        second: TextEdit,
        disabled: bool,
    }
    fn view(s: &Form, _: &mut Cx<Form>) -> El {
        col().children([
            field(&s.first, "first")
                .on_edit(|s: &mut Form| &mut s.first)
                .disabled(s.disabled),
            field(&s.second, "second").on_edit(|s: &mut Form| &mut s.second),
        ])
    }
    let mut rt = Runtime::new(
        Form {
            first: TextEdit::new("unchanged"),
            second: TextEdit::default(),
            disabled: true,
        },
        view,
        (300.0, 200.0),
        Env::default(),
    );
    rt.frame(0.0);
    press(&mut rt, Key::Tab);
    assert_eq!(focused(&rt).as_deref(), Some("second"));
    rt.update(|s, _| {
        s.disabled = false;
        s.first.focused = true;
    });
    rt.frame(0.0);
    assert_eq!(focused(&rt).as_deref(), Some("first"));
    rt.update(|s, _| s.disabled = true);
    rt.frame(0.0);
    press(&mut rt, Key::Text("blocked".into()));
    rt.update_if(|s, _| {
        assert_eq!(s.first.text, "unchanged");
        assert!(!s.first.focused);
        false
    });
    rt.update(|s, _| s.first.focused = true);
    rt.frame(0.0);
    rt.update_if(|s, _| {
        assert!(!s.first.focused);
        false
    });
}

#[test]
fn virtual_fields_keep_enabled_focus_and_drop_disabled_focus() {
    struct Form {
        fields: Vec<TextEdit>,
        disabled: bool,
    }
    fn view(s: &Form, _: &mut Cx<Form>) -> El {
        col()
            .key("root")
            .size(200.0, 240.0)
            .child(
                el().key("other")
                    .size(30.0, 30.0)
                    .focusable(true)
                    .role("button"),
            )
            .child(
                col()
                    .key("viewport")
                    .size(200.0, 200.0)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .clip_soft(0.0)
                    .child(
                        col()
                            .key("rows")
                            .w_full()
                            .growth(GrowthSpec::default())
                            .virtualize(40.0)
                            .children(s.fields.iter().enumerate().map(|(index, edit)| {
                                field(edit, &format!("row-{index}"))
                                    .on_edit(move |s: &mut Form| &mut s.fields[index])
                                    .disabled(index == 0 && s.disabled)
                            })),
                    ),
            )
    }
    let mut fields: Vec<_> = (0..100).map(|_| TextEdit::default()).collect();
    fields[0].focused = true;
    let mut rt = Runtime::new(
        Form {
            fields,
            disabled: false,
        },
        view,
        (200.0, 240.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    rt.wheel(100.0, 100.0, -2000.0);
    rt.frame(0.0);
    assert!(
        rt.find("row-0").is_none(),
        "the focused row must actually be unrealized"
    );
    press(&mut rt, Key::Text("offscreen".into()));
    rt.update_if(|s, _| {
        assert_eq!(s.fields[0].text, "offscreen");
        false
    });
    let (x, y) = rt.find("other").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(focused(&rt).as_deref(), Some("other"));
    rt.wheel(100.0, 100.0, 2000.0);
    rt.frame(0.0);
    assert!(rt.find("row-0").is_some());
    assert_eq!(focused(&rt).as_deref(), Some("other"));
    rt.update(|s, _| s.fields[0].focused = true);
    rt.frame(0.0);
    rt.wheel(100.0, 100.0, -2000.0);
    rt.frame(0.0);
    assert!(rt.find("row-0").is_none());
    rt.update(|s, _| s.disabled = true);
    rt.frame(0.0);
    press(&mut rt, Key::Text("blocked".into()));
    rt.update_if(|s, _| {
        assert_eq!(s.fields[0].text, "offscreen");
        assert!(!s.fields[0].focused);
        false
    });
    rt.update(|s, _| {
        s.disabled = false;
        s.fields[0].focused = true;
    });
    rt.frame(0.0);
    press(&mut rt, Key::Text("resumed".into()));
    rt.update_if(|s, _| {
        assert_eq!(s.fields[0].text, "offscreenresumed");
        false
    });
}

#[test]
fn modal_close_cannot_restore_editing_to_a_disabled_field() {
    struct Form {
        open: bool,
        disabled: bool,
        opener: TextEdit,
        field: TextEdit,
    }
    fn view(s: &Form, _: &mut Cx<Form>) -> El {
        col()
            .size(300.0, 300.0)
            .child(
                field(&s.opener, "opener")
                    .on_edit(|s: &mut Form| &mut s.opener)
                    .disabled(s.disabled),
            )
            .children(s.open.then(|| {
                col()
                    .key("sheet")
                    .absolute(100.0, 0.0)
                    .size(150.0, 100.0)
                    .modal(true, |s: &mut Form| s.open = false)
                    .child(field(&s.field, "field").on_edit(|s: &mut Form| &mut s.field))
            }))
    }
    for escape in [false, true] {
        let mut opener = TextEdit::new("unchanged");
        opener.focused = true;
        let mut rt = Runtime::new(
            Form {
                open: false,
                disabled: false,
                opener,
                field: TextEdit::default(),
            },
            view,
            (300.0, 300.0),
            Env::new(true, 1.0),
        );
        rt.frame(0.0);
        rt.update(|s, _| {
            s.open = true;
            s.field.focused = true;
        });
        rt.frame(0.0);
        assert_eq!(focused(&rt).as_deref(), Some("field"));
        rt.update(|s, _| s.disabled = true);
        rt.frame(0.0);
        if escape {
            press(&mut rt, Key::Escape);
        } else {
            rt.update(|s, _| s.open = false);
            rt.frame(0.0);
        }
        let restored_disabled = focused(&rt).as_deref() == Some("opener");
        press(&mut rt, Key::Text("blocked".into()));
        rt.update_if(|s, _| {
            assert_eq!(s.opener.text, "unchanged");
            assert!(!s.opener.focused);
            false
        });
        assert!(!restored_disabled);
    }
}
