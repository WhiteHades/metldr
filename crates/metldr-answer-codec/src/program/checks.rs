use metldr_conversation::{GeneratedAnswer, GeneratedUpdate};

use crate::{Binding, GuestProgram, HostMessage, decode_host, decode_initialize, encode_program};

fn binding() -> Binding {
    Binding::new(
        "0123456789abcdef0123456789abcdef".into(),
        "thread".into(),
        "run".into(),
        "answer".into(),
    )
    .unwrap()
}

fn answer() -> GeneratedAnswer {
    let mut answer = GeneratedAnswer::default();
    for update in [
        GeneratedUpdate::Css("body { color: red; }".into()),
        GeneratedUpdate::CssComplete,
        GeneratedUpdate::Html("<script>legacy()</script>".into()),
        GeneratedUpdate::HtmlComplete,
        GeneratedUpdate::Functions("function add(a, b) { return a + b; }".into()),
        GeneratedUpdate::FunctionsComplete,
        GeneratedUpdate::Expression("add(2, 3)".into()),
        GeneratedUpdate::Expression("6 * 7".into()),
        GeneratedUpdate::ExpressionsComplete,
    ] {
        answer = answer.updated(update).unwrap();
    }
    answer
}

#[test]
fn saved_program_replays_the_shared_host_protocol_in_order() {
    let binding = binding();
    let encoded = encode_program(&binding, &answer()).unwrap();
    let mut program = GuestProgram::new(encoded).unwrap();
    assert_eq!(
        decode_initialize(&program.initialization().unwrap()).unwrap(),
        binding
    );
    let messages = std::iter::from_fn(|| program.next().unwrap())
        .map(|encoded| decode_host(&encoded, &binding).unwrap())
        .collect::<Vec<_>>();
    assert!(
        matches!(messages[0], HostMessage::Document { request_id, ref css, ref html }
        if request_id.get() == 1 && css == "body { color: red; }" && html == "<script>legacy()</script>")
    );
    assert!(
        matches!(messages[1], HostMessage::Functions { request_id, ref code }
        if request_id.get() == 2 && code == "function add(a, b) { return a + b; }")
    );
    assert!(
        matches!(messages[2], HostMessage::Expression { request_id, ref code }
        if request_id.get() == 3 && code == "add(2, 3)")
    );
    assert!(
        matches!(messages[3], HostMessage::Expression { request_id, ref code }
        if request_id.get() == 4 && code == "6 * 7")
    );
    assert_eq!(program.next().unwrap(), None);
    assert_eq!(program.next().unwrap(), None);
    assert!(encode_program(&binding, &GeneratedAnswer::default()).is_err());
    let encoded = encode_program(&binding, &answer()).unwrap();
    let mut extra: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    extra["unexpected"] = serde_json::json!(true);
    assert!(GuestProgram::new(serde_json::to_string(&extra).unwrap()).is_err());
    let mut unfinished: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    unfinished["answer"]["css"]["state"] = serde_json::json!("open");
    assert!(GuestProgram::new(serde_json::to_string(&unfinished).unwrap()).is_err());
    assert!(GuestProgram::new(" ".repeat(super::MAX_PROGRAM_BYTES + 1)).is_err());

    let large_valid = GeneratedAnswer::default()
        .updated(GeneratedUpdate::Html("x".repeat(261_850)))
        .unwrap()
        .updated(GeneratedUpdate::HtmlComplete)
        .unwrap();
    let encoded = encode_program(&binding, &large_valid).unwrap();
    assert!(GuestProgram::new(encoded).is_ok());
    let escaped = GeneratedAnswer::default()
        .updated(GeneratedUpdate::Html("\0".repeat(40_000)))
        .unwrap()
        .updated(GeneratedUpdate::HtmlComplete)
        .unwrap();
    let mut escaped = GuestProgram::new(encode_program(&binding, &escaped).unwrap()).unwrap();
    assert!(
        matches!(decode_host(&escaped.next().unwrap().unwrap(), &binding).unwrap(), HostMessage::Document { html, .. } if html == "\0".repeat(40_000))
    );
}
