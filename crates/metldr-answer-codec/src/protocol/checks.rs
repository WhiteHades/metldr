use super::*;

fn binding() -> Binding {
    Binding::new(
        "0123456789abcdef0123456789abcdef".into(),
        "thread".into(),
        "run".into(),
        "answer".into(),
    )
    .unwrap()
}

fn escaped_binding() -> Binding {
    Binding::new(
        "0123456789abcdef0123456789abcdef".into(),
        "\0".repeat(MAX_THREAD_ID_BYTES),
        "\0".repeat(MAX_OWNED_ID_BYTES),
        "\0".repeat(MAX_OWNED_ID_BYTES),
    )
    .unwrap()
}

#[test]
fn binding_deserialization_is_validated_and_strict() {
    let raw = serde_json::json!({"v":1,"frameId":"0123456789abcdef0123456789abcdef","threadId":"thread","runId":"run","answerId":"answer"});
    assert!(serde_json::from_value::<Binding>(raw.clone()).is_ok());
    let mut extra = raw.clone();
    extra["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Binding>(extra).is_err());
    let mut version = raw.clone();
    version["v"] = serde_json::json!(2);
    assert!(serde_json::from_value::<Binding>(version).is_err());
    let mut frame = raw;
    frame["frameId"] = serde_json::json!("bad");
    assert!(serde_json::from_value::<Binding>(frame).is_err());
}

#[test]
fn child_rejects_zero_ids_unknown_fields_and_wrong_binding() {
    let b = binding();
    for message in [
        r#"{"binding":{"v":1,"frameId":"0123456789abcdef0123456789abcdef","threadId":"thread","runId":"run","answerId":"answer"},"message":{"kind":"ack","requestId":0}}"#,
        r#"{"binding":{"v":1,"frameId":"0123456789abcdef0123456789abcdef","threadId":"thread","runId":"run","answerId":"answer","extra":1},"message":{"kind":"ready"}}"#,
    ] {
        assert!(decode_child(message, &b).is_err());
    }
    let mut other = b.clone();
    other
        .set_frame_id("ffffffffffffffffffffffffffffffff".into())
        .unwrap();
    let child = encode_child(&b, ChildMessage::Ready {}).unwrap();
    assert!(matches!(
        decode_child(&child, &b),
        Ok(ChildMessage::Ready {})
    ));
    assert!(decode_child(&child, &other).is_err());
    let max_id = format!(
        r#"{{"binding":{},"message":{{"kind":"ack","requestId":4294967295}}}}"#,
        serde_json::to_string(&b).unwrap()
    );
    assert!(
        matches!(decode_child(&max_id, &b), Ok(ChildMessage::Ack { request_id }) if request_id.get() == u32::MAX)
    );
    let max_host_id = encode_host(
        &b,
        HostMessage::FollowupConsumed {
            request_id: NonZeroU32::new(u32::MAX).unwrap(),
        },
    )
    .unwrap();
    assert!(
        matches!(decode_host(&max_host_id, &b), Ok(HostMessage::FollowupConsumed { request_id }) if request_id.get() == u32::MAX)
    );
}

#[test]
fn source_and_followup_caps_allow_json_escape_expansion() {
    let b = escaped_binding();
    let text = "\0".repeat(MAX_FOLLOWUP_BYTES);
    let child = encode_child(&b, ChildMessage::Followup { text }).unwrap();
    assert!(child.len() <= MAX_CHILD_BYTES);
    assert!(matches!(
        decode_child(&child, &b),
        Ok(ChildMessage::Followup { .. })
    ));
    let src = "\0".repeat(MAX_GENERATED_BYTES);
    let host = encode_host(
        &b,
        HostMessage::Functions {
            request_id: NonZeroU32::new(1).unwrap(),
            code: src,
        },
    )
    .unwrap();
    assert!(host.len() <= MAX_HOST_BYTES);
    assert!(matches!(
        decode_host(&host, &b),
        Ok(HostMessage::Functions { code, .. }) if code.len() == MAX_GENERATED_BYTES
    ));
    assert!(
        encode_host(
            &b,
            HostMessage::Document {
                request_id: NonZeroU32::new(1).unwrap(),
                css: "x".repeat(MAX_GENERATED_BYTES / 2 + 1),
                html: "x".repeat(MAX_GENERATED_BYTES / 2),
            }
        )
        .is_err()
    );
    assert!(
        encode_host(
            &b,
            HostMessage::Functions {
                request_id: NonZeroU32::new(1).unwrap(),
                code: "x".repeat(MAX_GENERATED_BYTES + 1)
            }
        )
        .is_err()
    );
}

#[test]
fn multibyte_followup_uses_utf8_bytes() {
    let b = binding();
    let valid = "界".repeat(MAX_FOLLOWUP_BYTES / 3);
    let json = format!(
        r#"{{"binding":{},"message":{{"kind":"followup","text":{}}}}}"#,
        serde_json::to_string(&b).unwrap(),
        serde_json::to_string(&valid).unwrap()
    );
    assert!(decode_child(&json, &b).is_ok());
    let too_long = "界".repeat(MAX_FOLLOWUP_BYTES / 3 + 1);
    let json = format!(
        r#"{{"binding":{},"message":{{"kind":"followup","text":{}}}}}"#,
        serde_json::to_string(&b).unwrap(),
        serde_json::to_string(&too_long).unwrap()
    );
    assert!(decode_child(&json, &b).is_err());
}

#[test]
fn initialization_shape_is_shared_and_strict() {
    let b = binding();
    let encoded = encode_initialize(&b).unwrap();
    assert_eq!(decode_initialize(&encoded).unwrap(), b);
    assert!(decode_initialize(&encoded.replace("initialize", "other")).is_err());
    assert!(decode_initialize(&format!("{encoded} ")).is_ok());
    let extra = encoded.trim_end_matches('}').to_owned() + ",\"extra\":true}";
    assert!(decode_initialize(&extra).is_err());
}
