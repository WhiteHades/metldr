use super::*;

fn table_reference() -> TableReference {
    let run_id: RunId =
        serde_json::from_value(serde_json::json!("0123456789abcdef0123456789abcdef:1")).unwrap();
    let artifact_id =
        metldr_conversation::ArtifactId::new("0123456789abcdef0123456789abcdef:2".into()).unwrap();
    TableReference {
        run_id,
        artifact_id,
    }
}

fn text(id: &str, content: &str) -> EntryState {
    EntryState::Text(MessageState {
        id: id.into(),
        content: content.into(),
        completion: Completion::Streaming,
    })
}

fn closed(id: &str, content: &str) -> EntryState {
    EntryState::Text(MessageState {
        id: id.into(),
        content: content.into(),
        completion: Completion::FinalSnapshot,
    })
}

fn table(reference: &TableReference) -> EntryState {
    EntryState::Artifact {
        id: format!(
            "{}:artifact:{}",
            reference.run_id,
            reference.artifact_id.as_str()
        ),
        activity: Activity::Table(reference.clone()),
    }
}

fn chart(reference: &TableReference) -> EntryState {
    let AgUiMessage::Activity { id, activity } =
        AgUiMessage::chart(&reference.run_id, &reference.artifact_id)
    else {
        unreachable!()
    };
    EntryState::Artifact { id, activity }
}

#[test]
fn charts_keep_the_artifact_position_and_identity() {
    let reference = table_reference();
    let mut stream = RunStream::new("thread".into(), "ui-run".into(), vec![]).unwrap();
    let events = stream
        .push_messages(
            vec![text("before", "A"), chart(&reference), text("after", "B")],
            RunStatus::Running,
            None,
        )
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ActivitySnapshot {
            message_id,
            activity: Activity::Chart(chart),
            replace: false,
        } if message_id == &format!("{}:artifact:{}", reference.run_id, reference.artifact_id.as_str())
            && chart.run_id == reference.run_id
            && chart.artifact_id == reference.artifact_id
    )));
}

fn generated(
    reference: &TableReference,
    revision: u64,
    completion: metldr_conversation::ArtifactCompletion,
) -> EntryState {
    let AgUiMessage::Activity { id, activity } = AgUiMessage::generated(
        &reference.run_id,
        &reference.artifact_id,
        revision,
        completion,
    ) else {
        unreachable!()
    };
    EntryState::Artifact { id, activity }
}

#[test]
fn generated_revision_replaces_activity_in_place_without_message_snapshot() {
    let reference = table_reference();
    let mut stream = RunStream::new("thread".into(), "ui-run".into(), vec![]).unwrap();
    stream
        .push_messages(
            vec![
                text("before", "A"),
                generated(
                    &reference,
                    1,
                    metldr_conversation::ArtifactCompletion::Streaming,
                ),
                text("after", "B"),
            ],
            RunStatus::Running,
            None,
        )
        .unwrap();
    let update = stream
        .push_messages(
            vec![
                text("before", "A"),
                generated(
                    &reference,
                    2,
                    metldr_conversation::ArtifactCompletion::Streaming,
                ),
                text("after", "B"),
            ],
            RunStatus::Running,
            None,
        )
        .unwrap();
    assert_eq!(update.len(), 1);
    assert!(
        matches!(&update[0], Event::ActivitySnapshot { replace: true, activity: Activity::Generated(reference), .. } if reference.revision.0 == 2)
    );
    assert!(
        stream
            .push_messages(
                vec![
                    text("before", "A"),
                    generated(
                        &reference,
                        2,
                        metldr_conversation::ArtifactCompletion::Streaming
                    ),
                    text("after", "B"),
                ],
                RunStatus::Running,
                None,
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        stream
            .push_messages(
                vec![
                    text("before", "A"),
                    generated(
                        &reference,
                        3,
                        metldr_conversation::ArtifactCompletion::Completed
                    ),
                    text("after", "B")
                ],
                RunStatus::Running,
                None
            )
            .is_ok()
    );
    assert!(
        stream
            .push_messages(
                vec![
                    text("before", "A"),
                    generated(
                        &reference,
                        4,
                        metldr_conversation::ArtifactCompletion::Completed
                    ),
                    text("after", "B")
                ],
                RunStatus::Running,
                None
            )
            .is_err()
    );
    assert!(
        stream
            .push_messages(
                vec![
                    text("before", "A"),
                    generated(
                        &reference,
                        3,
                        metldr_conversation::ArtifactCompletion::Streaming
                    ),
                    text("after", "B")
                ],
                RunStatus::Running,
                None
            )
            .is_err()
    );
}

#[test]
fn streams_text_around_tables_and_preserves_saved_table_positions() {
    let reference = table_reference();
    let mut stream = RunStream::new("thread".into(), "ui-run".into(), vec![]).unwrap();

    let first = stream
        .push_messages(vec![text("before", "A")], RunStatus::Running, None)
        .unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::json!([
            {"type":"RUN_STARTED","threadId":"thread","runId":"ui-run"},
            {"type":"TEXT_MESSAGE_START","messageId":"before","role":"assistant"},
            {"type":"TEXT_MESSAGE_CONTENT","messageId":"before","delta":"A"}
        ])
    );

    let reference_id = format!(
        "{}:artifact:{}",
        reference.run_id,
        reference.artifact_id.as_str()
    );
    let around_table = stream
        .push_messages(
            vec![
                closed("before", "AB"),
                table(&reference),
                text("after", "C"),
            ],
            RunStatus::Running,
            None,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(around_table).unwrap(),
        serde_json::json!([
            {"type":"TEXT_MESSAGE_CONTENT","messageId":"before","delta":"B"},
            {"type":"TEXT_MESSAGE_END","messageId":"before"},
            {
                "type":"ACTIVITY_SNAPSHOT",
                "messageId":reference_id,
                "activityType":"metldr.table",
                "content":{"runId":reference.run_id,"artifactId":"0123456789abcdef0123456789abcdef:2"},
                "replace":false
            },
            {"type":"TEXT_MESSAGE_START","messageId":"after","role":"assistant"},
            {"type":"TEXT_MESSAGE_CONTENT","messageId":"after","delta":"C"}
        ])
    );

    let appended = stream
        .push_messages(
            vec![
                closed("before", "AB"),
                table(&reference),
                text("after", "CD"),
            ],
            RunStatus::Running,
            None,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(appended).unwrap(),
        serde_json::json!([
            {"type":"TEXT_MESSAGE_CONTENT","messageId":"after","delta":"D"}
        ])
    );

    let rewritten = stream
        .push_messages(
            vec![
                closed("before", "AB"),
                table(&reference),
                text("after", "revised"),
            ],
            RunStatus::Running,
            None,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(rewritten).unwrap(),
        serde_json::json!([
            {"type":"TEXT_MESSAGE_END","messageId":"after"},
            {
                "type":"MESSAGES_SNAPSHOT",
                "messages":[
                    {"id":"before","role":"assistant","content":"AB"},
                    {
                        "id":reference_id,
                        "role":"activity",
                        "activityType":"metldr.table",
                        "content":{"runId":reference.run_id,"artifactId":"0123456789abcdef0123456789abcdef:2"}
                    },
                    {"id":"after","role":"assistant","content":"revised"}
                ]
            },
            {"type":"TEXT_MESSAGE_START","messageId":"after","role":"assistant"}
        ])
    );

    for invalid in [
        vec![text("before", "revised")],
        vec![
            text("after", "CD"),
            table(&reference),
            text("before", "revised"),
        ],
    ] {
        let previous_messages = stream.messages.clone();
        let previous_started = stream.started;
        let previous_finished = stream.finished;
        assert!(
            stream
                .push_messages(invalid, RunStatus::Running, None)
                .is_err()
        );
        assert_eq!(stream.messages, previous_messages);
        assert_eq!(stream.started, previous_started);
        assert_eq!(stream.finished, previous_finished);
    }
}
