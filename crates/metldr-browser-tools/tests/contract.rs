use metldr_browser_tools::{Call, decode_call, definitions};
use serde_json::json;

#[test]
fn decodes_the_shared_browser_calls_and_bounds_untrusted_arguments() {
    assert_eq!(
        decode_call("observe_page", json!({})).unwrap(),
        Call::Observe
    );
    assert_eq!(
        decode_call("navigate_page", json!({"url":"https://example.com/report"})).unwrap(),
        Call::Navigate {
            destination: "https://example.com/report".into()
        }
    );
    let value = "é".repeat(512);
    assert_eq!(
        decode_call("set_field_value", json!({"handle":"h_1-A","value":value})).unwrap(),
        Call::Change {
            handle: "h_1-A".into(),
            value
        }
    );
    assert!(
        decode_call(
            "set_field_value",
            json!({"handle":"a".repeat(64),"value":""})
        )
        .is_ok()
    );
    assert!(decode_call("navigate_page", json!({"url":"a".repeat(2048)})).is_ok());
    for (name, arguments) in [
        ("unknown", json!({})),
        ("observe_page", json!([])),
        ("observe_page", json!({"selector":"body"})),
        ("navigate_page", json!({})),
        ("navigate_page", json!({"url":""})),
        ("navigate_page", json!({"url":12})),
        ("navigate_page", json!({"url":"https://example.com/\n"})),
        ("navigate_page", json!({"url":"é".repeat(1025)})),
        (
            "navigate_page",
            json!({"url":"https://example.com","approve":true}),
        ),
        ("set_field_value", json!({"handle":"field_1"})),
        ("set_field_value", json!({"handle":"field_1","value":null})),
        ("set_field_value", json!({"handle":"","value":"ready"})),
        (
            "set_field_value",
            json!({"handle":"#subject","value":"ready"}),
        ),
        (
            "set_field_value",
            json!({"handle":"[name=subject]","value":"ready"}),
        ),
        ("set_field_value", json!({"handle":"é","value":"ready"})),
        (
            "set_field_value",
            json!({"handle":"a".repeat(65),"value":"ready"}),
        ),
        (
            "set_field_value",
            json!({"handle":"field_1","value":"é".repeat(513)}),
        ),
        (
            "set_field_value",
            json!({"handle":"field_1","value":"ready","approve":true}),
        ),
    ] {
        assert!(
            decode_call(name, arguments.clone()).is_err(),
            "accepted {name}: {arguments}"
        );
    }
    let tools = definitions();
    assert_eq!(
        tools.map(|tool| tool.name),
        ["observe_page", "set_field_value", "navigate_page"]
    );
    for tool in definitions() {
        assert_eq!(tool.input_schema["additionalProperties"], false);
    }
}
