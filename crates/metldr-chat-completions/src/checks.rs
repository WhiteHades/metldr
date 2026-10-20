use super::*;
use std::sync::atomic::Ordering;

fn queued() -> Conversation {
    let client = Client::new(
        "https://example.test/chat/completions",
        "model",
        "dummy-key",
        None,
    )
    .unwrap();
    let mut conversation = client
        .start(Prompt {
            instruction: "Inspect the page.",
            title: "Page",
            url: "https://example.test/",
            text: "Data",
        })
        .unwrap();
    conversation.request["messages"].as_array_mut().unwrap().push(json!({
        "role":"assistant", "content":null, "tool_calls":[
            {"id":"first","type":"function","function":{"name":"observe_page","arguments":"{}"}},
            {"id":"second","type":"function","function":{"name":"observe_page","arguments":"{}"}}
        ]
    }));
    conversation.phase = Phase::Queued(
        [
            Invocation {
                id: "first".into(),
                call: Call::Observe,
            },
            Invocation {
                id: "second".into(),
                call: Call::Observe,
            },
        ]
        .into(),
    );
    conversation
}

#[tokio::test]
async fn queued_calls_require_one_matching_result_before_the_next_call() {
    let stop = AtomicBool::new(false);
    let mut conversation = queued();
    assert_eq!(
        conversation
            .next(&stop, 2, |_| panic!("queued call emitted text"))
            .await
            .unwrap()
            .unwrap()
            .id,
        "first"
    );
    conversation
        .submit_tool_result(&json!({"observed":"one"}))
        .unwrap();
    assert_eq!(
        conversation
            .next(&stop, 1, |_| panic!("queued call emitted text"))
            .await
            .unwrap()
            .unwrap()
            .id,
        "second"
    );
    conversation
        .submit_tool_result(&json!({"observed":"two"}))
        .unwrap();
    let messages = conversation.request["messages"].as_array().unwrap();
    assert_eq!(messages[messages.len() - 2]["tool_call_id"], "first");
    assert_eq!(messages.last().unwrap()["tool_call_id"], "second");
    assert_eq!(messages.last().unwrap()["content"], r#"{"observed":"two"}"#);
    assert!(matches!(conversation.phase, Phase::Ready));
    assert!(
        conversation
            .submit_tool_result(&json!({"extra":true}))
            .is_err()
    );
    assert!(matches!(conversation.phase, Phase::Closed));

    let mut missing_result = queued();
    missing_result.next(&stop, 2, |_| Ok(())).await.unwrap();
    assert!(missing_result.next(&stop, 1, |_| Ok(())).await.is_err());
    assert!(matches!(missing_result.phase, Phase::Closed));
}

#[tokio::test]
async fn queued_stop_and_insufficient_budget_close_without_another_call() {
    let stop = AtomicBool::new(false);
    let mut conversation = queued();
    assert!(conversation.next(&stop, 1, |_| Ok(())).await.is_err());
    assert!(matches!(conversation.phase, Phase::Closed));
    let mut conversation = queued();
    conversation.next(&stop, 2, |_| Ok(())).await.unwrap();
    conversation
        .submit_tool_result(&json!({"observed":"one"}))
        .unwrap();
    stop.store(true, Ordering::Release);
    assert!(conversation.next(&stop, 1, |_| Ok(())).await.is_err());
    assert!(matches!(conversation.phase, Phase::Closed));
    assert!(
        conversation
            .submit_tool_result(&json!({"unused":true}))
            .is_err()
    );
}
