#![doc = include_str!("../README.md")]

mod content;
mod key;
mod projection;
mod table;

pub use content::{Change, Completion, Content, Ending};
pub use key::{ArtifactId, MessageKey, ToolId};
pub use projection::{Display, EntryRef, ProjectedEntry, Projection};
pub use table::{MAX_TABLE_BYTES, Table};

/// Ordered assistant text. A final snapshot closes the current message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextEvent {
    Delta(String),
    Final(String),
}

pub const MAX_MESSAGES: usize = 64;
pub const MAX_TOOLS: usize = 64;
const MAX_ENTRIES: usize = 132;
pub const MAX_ARTIFACTS: usize = 4;
const MAX_ID_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 8192;

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn ordered_artifact_references_roundtrip_without_owning_payloads() {
        let first = MessageKey::observed(1).unwrap();
        let second = MessageKey::observed(2).unwrap();
        let artifact = ArtifactId::new("owned:1".into()).unwrap();
        let mut content = Content::native();
        content
            .change(Change::Append {
                key: first.clone(),
                text: "Before".into(),
            })
            .unwrap();
        assert!(content.change(Change::Artifact(artifact.clone())).is_err());
        content
            .change(Change::Final {
                key: first,
                text: "Before".into(),
            })
            .unwrap();
        assert!(content.change(Change::Artifact(artifact.clone())).unwrap());
        assert!(!content.change(Change::Artifact(artifact.clone())).unwrap());
        content
            .change(Change::Final {
                key: second,
                text: "After".into(),
            })
            .unwrap();
        let encoded = serde_json::to_string(&content).unwrap();
        let restored: Content = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored, content);
        let display = Display::new(restored.projection()).unwrap();
        assert!(
            matches!(&display.entries().collect::<Vec<_>>()[..], [EntryRef::Message {text:"Before",..}, EntryRef::Artifact { id }, EntryRef::Message {text:"After",..}] if *id == &artifact)
        );
        assert_eq!(display.text(), "BeforeAfter");
        assert!(
            Content::flat("old history".into())
                .unwrap()
                .change(Change::Artifact(artifact))
                .is_err()
        );
        for invalid in ["", "bad\nkey", &"x".repeat(129)] {
            assert!(serde_json::from_value::<ArtifactId>(serde_json::json!(invalid)).is_err());
        }
        for number in 2..=MAX_ARTIFACTS {
            content
                .change(Change::Artifact(
                    ArtifactId::new(format!("owned:{number}")).unwrap(),
                ))
                .unwrap();
        }
        let unchanged = content.clone();
        assert!(
            content
                .change(Change::Artifact(
                    ArtifactId::new("owned:extra".into()).unwrap()
                ))
                .is_err()
        );
        assert_eq!(content, unchanged);
        let mut projection = restored.projection();
        if let ProjectedEntry::Message { completion, .. } = &mut projection.conversation[0] {
            *completion = Completion::Streaming;
        }
        assert!(Display::new(projection).is_err());
    }

    #[test]
    fn transitions_projection_bounds_and_serde_are_validated() {
        let mut content = Content::native();
        let first = MessageKey::native("first".into()).unwrap();
        let second = MessageKey::native("second".into()).unwrap();
        let tool = ToolId::new("tool-1".into()).unwrap();
        content
            .change(Change::Append {
                key: first.clone(),
                text: "provisional".into(),
            })
            .unwrap();
        content
            .change(Change::Final {
                key: first.clone(),
                text: "same".into(),
            })
            .unwrap();
        content.observe_tool(tool.clone()).unwrap();
        content
            .change(Change::Final {
                key: second.clone(),
                text: "same".into(),
            })
            .unwrap();

        let projection = content.projection();
        assert_eq!(projection.output, "samesame");
        assert!(matches!(
            &projection.conversation[..],
            [
                ProjectedEntry::Message { key: a, start: 0, end: 4, .. },
                ProjectedEntry::Tool { id },
                ProjectedEntry::Message { key: b, start: 4, end: 8, .. }
            ] if a == &first && b == &second && id == &tool
        ));
        let roundtrip: Content =
            serde_json::from_str(&serde_json::to_string(&content).unwrap()).unwrap();
        assert_eq!(roundtrip, content);
        assert!(
            serde_json::from_value::<Content>(serde_json::json!({
                "entries": [],
                "extra": true
            }))
            .is_err()
        );

        let mut bounded = Content::native();
        let key = MessageKey::native("bounded".into()).unwrap();
        bounded.change(Change::Begin(key.clone())).unwrap();
        bounded
            .change(Change::Append {
                key: key.clone(),
                text: "x".repeat(MAX_TEXT_BYTES),
            })
            .unwrap();
        assert!(
            bounded
                .change(Change::Append {
                    key: key.clone(),
                    text: "y".into(),
                })
                .is_err()
        );
        assert_eq!(bounded.text().len(), MAX_TEXT_BYTES);
        assert!(matches!(
            Display::new(bounded.projection()).unwrap().entries().next(),
            Some(EntryRef::Message {
                completion: Completion::Streaming,
                ..
            })
        ));

        let final_key = MessageKey::native("final".into()).unwrap();
        let open_key = MessageKey::native("open".into()).unwrap();
        let mut complete = Content::native();
        complete
            .change(Change::Final {
                key: final_key.clone(),
                text: "fixed".into(),
            })
            .unwrap();
        complete.change(Change::Begin(open_key.clone())).unwrap();
        complete
            .change(Change::Append {
                key: open_key,
                text: "live".into(),
            })
            .unwrap();
        complete.end(Ending::Complete).unwrap();
        let complete_display = Display::new(complete.projection()).unwrap();
        let entries: Vec<_> = complete_display.entries().collect();
        assert!(matches!(
            entries.as_slice(),
            [
                EntryRef::Message {
                    text: "fixed",
                    completion: Completion::FinalSnapshot,
                    ..
                },
                EntryRef::Message {
                    text: "live",
                    completion: Completion::StreamEnded,
                    ..
                }
            ]
        ));

        let mut interrupted = Content::native();
        interrupted
            .change(Change::Final {
                key: final_key,
                text: "fixed".into(),
            })
            .unwrap();
        interrupted
            .change(Change::Begin(MessageKey::observed(1).unwrap()))
            .unwrap();
        interrupted.end(Ending::Interrupted).unwrap();
        let interrupted_display = Display::new(interrupted.projection()).unwrap();
        let entries: Vec<_> = interrupted_display.entries().collect();
        assert!(matches!(
            entries.as_slice(),
            [
                EntryRef::Message {
                    completion: Completion::FinalSnapshot,
                    ..
                },
                EntryRef::Message {
                    completion: Completion::Interrupted,
                    ..
                }
            ]
        ));
    }
}
