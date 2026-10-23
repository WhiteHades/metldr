use metldr_browser_tools::{
    ACTIVATE_TOOL, CONTROL_TOOL, Call, SCROLL_TOOL, SELECT_TOOL, ScrollDirection, decode_call,
    definitions,
};
use serde_json::json;

#[test]
fn decodes_the_shared_browser_calls_and_bounds_untrusted_arguments() {
    assert_eq!(
        decode_call("observe_page", json!({})).unwrap(),
        Call::Observe
    );
    assert_eq!(
        decode_call(
            SCROLL_TOOL,
            json!({"handle":"r1-2-0","direction":"left","pixels":2000})
        )
        .unwrap(),
        Call::Scroll {
            handle: "r1-2-0".into(),
            direction: ScrollDirection::Left,
            pixels: 2000
        }
    );
    assert_eq!(
        decode_call(ACTIVATE_TOOL, json!({"handle":"button_1-A"})).unwrap(),
        Call::ActivateButton {
            handle: "button_1-A".into()
        }
    );
    assert_eq!(
        decode_call(
            SELECT_TOOL,
            json!({"handle":"select_1-A","option":"option_2-B"})
        )
        .unwrap(),
        Call::SelectOption {
            handle: "select_1-A".into(),
            option: "option_2-B".into()
        }
    );
    assert_eq!(
        decode_call(
            CONTROL_TOOL,
            json!({"handle":"checkbox_1-A","checked":true})
        )
        .unwrap(),
        Call::SetNativeControl {
            handle: "checkbox_1-A".into(),
            checked: true
        }
    );
    assert_eq!(
        decode_call(CONTROL_TOOL, json!({"handle":"radio_2","checked":false})).unwrap(),
        Call::SetNativeControl {
            handle: "radio_2".into(),
            checked: false
        }
    );
    assert!(
        decode_call(
            CONTROL_TOOL,
            json!({"handle":"a".repeat(64),"checked":true})
        )
        .is_ok()
    );
    assert!(
        decode_call(
            SELECT_TOOL,
            json!({"handle":"a".repeat(64),"option":"b".repeat(64)})
        )
        .is_ok()
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
        (CONTROL_TOOL, json!({})),
        (CONTROL_TOOL, json!({"handle":"checkbox_1"})),
        (CONTROL_TOOL, json!({"checked":true})),
        (
            CONTROL_TOOL,
            json!({"handle":"checkbox_1","checked":"true"}),
        ),
        (CONTROL_TOOL, json!({"handle":"checkbox_1","checked":1})),
        (CONTROL_TOOL, json!({"handle":"checkbox_1","checked":null})),
        (
            CONTROL_TOOL,
            json!({"handle":"checkbox_1","checked":true,"approve":true}),
        ),
        (CONTROL_TOOL, json!({"handle":"","checked":true})),
        (CONTROL_TOOL, json!({"handle":"#checkbox","checked":true})),
        (CONTROL_TOOL, json!({"handle":"é","checked":true})),
        (
            CONTROL_TOOL,
            json!({"handle":"a".repeat(65),"checked":true}),
        ),
        ("unknown", json!({})),
        (SCROLL_TOOL, json!({"handle":"page","direction":"down"})),
        (
            SCROLL_TOOL,
            json!({"handle":"#region","direction":"down","pixels":100}),
        ),
        (
            SCROLL_TOOL,
            json!({"handle":"r1","direction":"diagonal","pixels":100}),
        ),
        (
            SCROLL_TOOL,
            json!({"handle":"r1","direction":"down","pixels":0}),
        ),
        (
            SCROLL_TOOL,
            json!({"handle":"r1","direction":"down","pixels":2001}),
        ),
        (
            SCROLL_TOOL,
            json!({"handle":"r1","direction":"down","pixels":1.5}),
        ),
        (
            SCROLL_TOOL,
            json!({"handle":"r1","direction":"down","pixels":"100"}),
        ),
        (
            SCROLL_TOOL,
            json!({"handle":"r1","direction":"down","pixels":100,"approve":true}),
        ),
        ("observe_page", json!([])),
        ("observe_page", json!({"selector":"body"})),
        (ACTIVATE_TOOL, json!({})),
        (
            ACTIVATE_TOOL,
            json!({"handle":"button_1","selector":"button"}),
        ),
        (ACTIVATE_TOOL, json!({"handle":"#submit"})),
        (ACTIVATE_TOOL, json!({"handle":"é"})),
        (SELECT_TOOL, json!({})),
        (SELECT_TOOL, json!({"handle":"select_1"})),
        (SELECT_TOOL, json!({"option":"option_1"})),
        (SELECT_TOOL, json!({"handle":"select_1","option":null})),
        (SELECT_TOOL, json!({"handle":"#select","option":"option_1"})),
        (
            SELECT_TOOL,
            json!({"handle":"select_1","option":"[value=x]"}),
        ),
        (SELECT_TOOL, json!({"handle":"é","option":"option_1"})),
        (SELECT_TOOL, json!({"handle":"select_1","option":"é"})),
        (
            SELECT_TOOL,
            json!({"handle":"a".repeat(65),"option":"option_1"}),
        ),
        (
            SELECT_TOOL,
            json!({"handle":"select_1","option":"a".repeat(65)}),
        ),
        (
            SELECT_TOOL,
            json!({"handle":"select_1","option":"option_1","approve":true}),
        ),
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
        tools.each_ref().map(|tool| tool.name),
        [
            "observe_page",
            "set_field_value",
            "navigate_page",
            ACTIVATE_TOOL,
            SELECT_TOOL,
            SCROLL_TOOL,
            CONTROL_TOOL
        ]
    );
    assert_eq!(tools[3].input_schema["required"], json!(["handle"]));
    assert_eq!(
        tools[3].input_schema["properties"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        tools[4].input_schema,
        json!({
            "type":"object",
            "properties":{
                "handle":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$"},
                "option":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$"}
            },
            "required":["handle","option"],
            "additionalProperties":false
        })
    );
    assert_eq!(
        tools[6].input_schema,
        json!({
            "type":"object",
            "properties":{
                "handle":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$"},
                "checked":{"type":"boolean"}
            },
            "required":["handle","checked"],
            "additionalProperties":false
        })
    );
    for tool in definitions() {
        assert_eq!(tool.input_schema["additionalProperties"], false);
    }
}
