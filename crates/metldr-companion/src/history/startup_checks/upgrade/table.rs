use super::*;

pub(super) struct Fixture {
    conversation: String,
    artifacts: Vec<(String, u64, Value)>,
}

impl Fixture {
    pub(super) fn len(&self) -> usize {
        self.artifacts.len()
    }

    pub(super) fn insert(directory: &Path, format: u64) -> Result<Self> {
        let db = redb::Database::open(directory.join("history.redb"))?;
        let write = db.begin_write()?;
        let fixture = {
            let mut meta = write.open_table(META)?;
            let mut profile: Profile = decode(meta.get("profile")?.unwrap().value())?;
            assert_eq!(u64::from(profile.format), format);
            let run = format!("{}:8", profile.store);
            let mut runs = write.open_table(RUNS)?;
            let mut record: Record<crate::model::Run> =
                decode(runs.get(run.as_str())?.unwrap().value())?;
            record.value.output = Content::native();
            record.value.output.change(Change::Final {
                key: MessageKey::observed(1)?,
                text: "Figures before the table.".into(),
            })?;
            let first_id = format!("{}:{}", profile.store, profile.next);
            record
                .value
                .output
                .change(Change::Artifact(ArtifactId::new(first_id.clone())?))?;
            let table = json!({"title":"Saved figures","columns":["Item","Amount"],
                "rows":[["North 🦀","7.50"],["South","9.25"]],"source":"Independent fixture"});
            let first_value = if format == 16 {
                json!({"id":first_id,"run":run,"call":"upgrade-table-call","surface_id":"upgrade-table","table":table})
            } else {
                json!({"id":first_id,"run":run,"call":"upgrade-table-call","surface_id":"upgrade-table","content":{"kind":"table","table":table}})
            };
            let mut artifacts = vec![(first_id, 1, first_value)];
            if format == 17 {
                use metldr_conversation::{ArtifactCompletion, GeneratedAnswer, GeneratedUpdate};
                let id = format!("{}:{}", profile.store, profile.next + 1);
                let source = GeneratedAnswer::default()
                    .updated(GeneratedUpdate::Html("<p>Generated answer</p>".into()))?
                    .updated(GeneratedUpdate::HtmlComplete)?
                    .finish()?;
                record.value.output.change(Change::Generated {
                    id: ArtifactId::new(id.clone())?,
                    revision: 1,
                    completion: ArtifactCompletion::Streaming,
                })?;
                record.value.output.change(Change::Generated {
                    id: ArtifactId::new(id.clone())?,
                    revision: 2,
                    completion: ArtifactCompletion::Completed,
                })?;
                artifacts.push((
                    id.clone(),
                    2,
                    json!({"id":id,"run":run,
                    "call":"upgrade-generated-call","surface_id":"upgrade-generated",
                    "content":{"kind":"generated","source":source,"completion":"completed"}}),
                ));
            }
            record.value.output.change(Change::Final {
                key: MessageKey::observed(2)?,
                text: "Figures after the artifacts.".into(),
            })?;
            record.revision += 1;
            let mut encoded = serde_json::to_value(&record)?;
            if format == 16 {
                let entries = encoded["value"]["output"]["entries"]
                    .as_array_mut()
                    .unwrap();
                assert_eq!(entries[1]["kind"], "artifact");
                assert!(
                    entries[1]
                        .as_object_mut()
                        .unwrap()
                        .remove("presentation")
                        .is_some()
                );
            }
            runs.insert(run.as_str(), encode(&encoded)?.as_slice())?;
            let mut table = write.open_table(ARTIFACTS)?;
            for (id, revision, value) in &artifacts {
                let row =
                    json!({"conversation":record.conversation,"revision":revision,"value":value});
                table.insert(id.as_str(), encode(&row)?.as_slice())?;
            }
            profile.next += artifacts.len() as u64;
            profile.sequence += artifacts.len() as u64;
            meta.insert("profile", encode(&profile)?.as_slice())?;
            Self {
                conversation: record.conversation,
                artifacts,
            }
        };
        write.commit()?;
        Ok(fixture)
    }

    fn value(value: &Value, converted: bool) -> Value {
        let mut value = value.clone();
        if converted && value.get("table").is_some() {
            value["content"] = json!({"kind":"table","table":value["table"]});
            value.as_object_mut().unwrap().remove("table");
        }
        value
    }

    pub(super) fn rows(&self, converted: bool) -> Value {
        Value::Array(self.artifacts.iter().map(|(id, revision, value)| json!([id, {
            "conversation":self.conversation,"revision":revision,"value":Self::value(value, converted)
        }])).collect())
    }

    pub(super) fn read(&self, host: &mut Running, version: u8, converted: bool) -> Result<()> {
        for (index, (id, revision, value)) in self.artifacts.iter().enumerate() {
            let current_tag = format!("artifact-current-{index}");
            host.send_version(version, &current_tag, json!({"op":"history_current"}))?;
            let current = host.reply(&current_tag)?;
            assert_eq!(current["v"], version);
            assert_eq!(current["kind"], "history_current");
            assert_eq!(current["scope"]["conversation"], self.conversation);
            let pin_tag = format!("artifact-pin-{index}");
            host.send_version(version, &pin_tag, json!({"op":"history_pin","scope":current["scope"],
                "generation":current["generation"],"kind":"artifact","document":id,"revision":revision.to_string()}))?;
            let pinned = host.reply(&pin_tag)?;
            assert_eq!(pinned["kind"], "history_pinned");
            let identity = &pinned["identity"];
            for field in ["store", "conversation", "epoch"] {
                assert_eq!(identity[field], current["scope"][field]);
            }
            assert_eq!(identity["generation"], current["generation"]);
            assert_eq!(identity["document"], id.as_str());
            assert_eq!(identity["kind"], "artifact");
            assert_eq!(identity["revision"], revision.to_string());
            let page_tag = format!("artifact-page-{index}");
            host.send_version(
                version,
                &page_tag,
                json!({"op":"history_page","identity":identity,"offset":0}),
            )?;
            let page = host.reply(&page_tag)?;
            assert_eq!(page["kind"], "detail");
            assert_eq!(page["identity"], *identity);
            assert_eq!(page["offset"], 0);
            assert_eq!(page["next"], Value::Null);
            let text = page["text"].as_str().ok_or("artifact page has no text")?;
            assert_eq!(page["total"], text.len());
            assert!(serde_json::to_vec(&page)?.len() <= crate::FRAME_MAX);
            assert_eq!(
                serde_json::from_str::<Value>(text)?,
                Self::value(value, converted)
            );
            let release_tag = format!("artifact-release-{index}");
            host.send_version(
                version,
                &release_tag,
                json!({"op":"history_release","identity":identity}),
            )?;
            let released = host.reply(&release_tag)?;
            assert_eq!(released["kind"], "history_released");
            assert_eq!(released["released"], true);
        }
        Ok(())
    }
}
