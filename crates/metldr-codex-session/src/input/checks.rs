use super::*;
use serde_json::json;

#[test]
fn input_codecs_validate_and_retain_native_identity() {
    let mut inputs = Inputs::default();
    let form_params = json!({"mode":"form","serverName":"owned","message":"Form","requestedSchema":{"type":"object","properties":{"label":{"type":"string","minLength":2,"maxLength":64}},"required":["label"]}});
    let input = inputs
        .insert(
            7,
            None,
            json!("native-form"),
            "mcpServer/elicitation/request",
            &form_params,
        )
        .unwrap();
    let mut values = BTreeMap::new();
    values.insert("label".into(), "مرحبا 🌱".into());
    let answer = InputAnswer::Accept(values);
    assert!(input.prompt().accepts(&answer));
    assert!(
        inputs
            .response(&input, InputAnswer::Cancel, Some(&TurnId("turn-1".into())))
            .is_ok()
    );
    let (idx, id, result) = inputs.response(&input, answer, None).unwrap();
    assert_eq!(id, json!("native-form"));
    assert_eq!(
        result,
        json!({"action":"accept","content":{"label":"مرحبا 🌱"}})
    );
    assert_ne!(
        form::encode(input.prompt(), &InputAnswer::Decline).unwrap(),
        form::encode(input.prompt(), &InputAnswer::Cancel).unwrap()
    );
    inputs.answered(idx);
    assert!(inputs.response(&input, InputAnswer::Decline, None).is_err());
    assert!(inputs.contains(&json!("native-form")));
    let old = inputs.resolved(&json!("native-form")).unwrap();
    assert_eq!(&old, input.id());
    assert!(!inputs.contains(&json!("native-form")));

    let questions = json!({"itemId":"item-1","isBlocking":true,"questions":[{"id":"choice","header":"Choice","question":"Pick","isOther":true,"isSecret":false,"options":[{"label":"A","description":"Option A"}]}]});
    let q = inputs
        .insert(
            7,
            Some(TurnId("turn-1".into())),
            json!(9),
            "item/tool/requestUserInput",
            &questions,
        )
        .unwrap();
    assert_eq!(
        form::encode(q.prompt(), &InputAnswer::Questions(BTreeMap::new())).unwrap(),
        json!({"answers":{}})
    );
    assert!(
        inputs
            .response(
                &q,
                InputAnswer::Questions(BTreeMap::new()),
                Some(&TurnId("turn-old".into()))
            )
            .is_err()
    );
    let mut wrong_connection = Input {
        id: q.id().clone(),
        prompt: q.prompt().clone(),
        turn: q.turn.clone(),
    };
    wrong_connection.id.connection = 8;
    assert!(
        inputs
            .response(
                &wrong_connection,
                InputAnswer::Questions(BTreeMap::new()),
                Some(&TurnId("turn-1".into()))
            )
            .is_err()
    );
    assert!(inputs.insert(7, Some(TurnId("turn-1".into())), json!(10), "item/tool/requestUserInput", &json!({"itemId":"i","isBlocking":true,"questions":[{"id":"x","header":"h","question":"q","isSecret":true,"options":[{"label":"a","description":"b"}]}]})).is_err());

    let approval_params = json!({"mode":"form","serverName":"owned","message":"Review","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"mcp_tool_call","persist":["session","always"],"tool_description":"Run tool","tool_params":{},"tool_params_display":[]}});
    let approval = inputs
        .insert(
            7,
            None,
            json!("approval"),
            "mcpServer/elicitation/request",
            &approval_params,
        )
        .unwrap();
    assert!(matches!(
        approval.prompt(),
        InputPrompt::McpToolApproval { .. }
    ));
    assert_eq!(
        form::encode(approval.prompt(), &InputAnswer::Accept(BTreeMap::new())).unwrap(),
        json!({"action":"accept","content":{}})
    );
    let fake = json!({"mode":"form","serverName":"owned","message":"x","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"other"}});
    assert!(
        inputs
            .insert(
                7,
                None,
                json!("fake"),
                "mcpServer/elicitation/request",
                &fake
            )
            .is_err()
    );
    let unsupported = json!({"mode":"form","serverName":"owned","message":"x","requestedSchema":{"type":"object","properties":{"url":{"type":"string","format":"uri"}},"required":["url"]}});
    assert!(
        inputs
            .insert(
                7,
                None,
                json!("unsupported"),
                "mcpServer/elicitation/request",
                &unsupported
            )
            .is_err()
    );
}
