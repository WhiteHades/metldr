use super::{artifacts::ArtifactContent, edit, schema::*};
use crate::Result;
use metldr_conversation::{ArtifactId, ProjectedEntry, Table};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyArtifact {
    id: ArtifactId,
    run: String,
    call: String,
    surface_id: String,
    table: Table,
}

pub(super) fn run(db: &Database) -> Result<()> {
    let read = db.begin_read()?;
    let meta = read.open_table(META)?;
    let profile: Profile = decode(
        meta.get("profile")?
            .ok_or("history profile is absent")?
            .value(),
    )?;
    match profile.format {
        FORMAT => return Ok(()),
        10..=17 => (),
        _ => return Err("unsupported history profile format".into()),
    }
    require_base_tables(&read)?;
    if profile.format >= 12 {
        read.open_table(NATIVE_CHOICES)?;
    }
    for row in read.open_table(ACTIONS)?.iter()? {
        let (_, bytes) = row?;
        let record: Record<crate::browser_action::Action> = decode(bytes.value())?;
        crate::browser_action::reject_control_action(&record.value)?;
        if profile.format < 14 {
            crate::browser_action::reject_scroll_action(&record.value)?;
        }
    }
    if profile.format == 16 {
        for row in read.open_table(ARTIFACTS)?.iter()? {
            let (_, bytes) = row?;
            let artifact: Record<LegacyArtifact> = decode(bytes.value())?;
            if artifact.revision != 1 {
                return Err("legacy table artifact revision is invalid".into());
            }
        }
        for row in read.open_table(RUNS)?.iter()? {
            let (_, bytes) = row?;
            let run: Record<crate::model::Run> = decode(bytes.value())?;
            if run
                .value
                .output
                .projection()
                .conversation
                .iter()
                .any(|entry| {
                    matches!(entry, ProjectedEntry::Artifact { presentation, .. }
                    if !matches!(presentation, metldr_conversation::ArtifactPresentation::Table))
                })
            {
                return Err("history contains generated artifact references".into());
            }
        }
    }
    if profile.format == 17 {
        reject_charts(&read.open_table(ARTIFACTS)?, &read.open_table(RUNS)?)?;
    }
    drop(meta);
    drop(read);

    let mut write = db.begin_write()?;
    write.set_durability(Durability::Immediate)?;
    write.set_two_phase_commit(true);
    {
        let mut meta = write.open_table(META)?;
        let mut profile: Profile = decode(
            meta.get("profile")?
                .ok_or("history profile is absent")?
                .value(),
        )?;
        match profile.format {
            FORMAT => return Ok(()),
            10..=17 => (),
            _ => return Err("unsupported history profile format".into()),
        }
        let format = profile.format;
        validate_origin(
            &profile,
            meta.get("legacy_snapshot")?
                .ok_or("history origin is absent")?
                .value(),
        )?;

        let conversations = write.open_table(CONVERSATIONS)?;
        if format == 10 && conversations.len()? != 1 {
            return Err("history conversation count is invalid for upgrade".into());
        }
        let bytes = conversations
            .get(profile.selected.as_str())?
            .ok_or("selected history conversation is absent")?;
        let conversation: Conversation = decode(bytes.value())?;
        drop(bytes);
        if conversation.id != profile.selected {
            return Err("selected history identity differs".into());
        }
        let number = conversation_number(&profile, &conversation.id)?;
        drop(conversations);

        let mut order = write.open_table(CONVERSATION_ORDER)?;
        let selections = write.open_table(SELECTIONS)?;
        if format == 10 {
            if order.len()? != 0 || selections.len()? != 0 {
                return Err("conversation upgrade tables are not empty".into());
            }
            order.insert(number, conversation.id.as_str())?;
        } else if order.get(number)?.map(|id| id.value() == profile.selected) != Some(true) {
            return Err("selected conversation order differs".into());
        }
        drop(order);
        drop(selections);

        if format < 12 {
            edit::native_choices::backfill(&write, &profile)?;
        } else {
            edit::native_choices::validate_index(&write, &profile)?;
        }
        if format == 16 {
            let mut artifacts = write.open_table(ARTIFACTS)?;
            let old_records = artifacts
                .iter()?
                .map(|row| {
                    let (key, bytes) = row?;
                    Ok((key.value().to_owned(), bytes.value().to_vec()))
                })
                .collect::<Result<Vec<_>>>()?;
            for (key, bytes) in old_records {
                let old: Record<LegacyArtifact> = decode(&bytes)?;
                if old.revision != 1 {
                    return Err("legacy table artifact revision is invalid".into());
                }
                let artifact = super::Artifact {
                    id: old.value.id,
                    run: old.value.run,
                    call: old.value.call,
                    surface_id: old.value.surface_id,
                    content: ArtifactContent::Table {
                        table: old.value.table,
                    },
                };
                let converted = Record {
                    conversation: old.conversation,
                    revision: old.revision,
                    value: artifact,
                };
                artifacts.insert(key.as_str(), encode(&converted)?.as_slice())?;
            }
            drop(artifacts);
        } else if format < 16 && write.open_table(ARTIFACTS)?.len()? != 0 {
            return Err("artifact upgrade table is not empty".into());
        }
        if format == 17 {
            reject_charts(&write.open_table(ARTIFACTS)?, &write.open_table(RUNS)?)?;
        }
        for row in write.open_table(RUNS)?.iter()? {
            let (_, bytes) = row?;
            let run: Record<crate::model::Run> = decode(bytes.value())?;
            if run
                .value
                .output
                .projection()
                .conversation
                .iter()
                .any(|entry| match format {
                    10..=15 => matches!(entry, ProjectedEntry::Artifact { .. }),
                    16 => matches!(entry, ProjectedEntry::Artifact { presentation, .. }
                        if !matches!(presentation, metldr_conversation::ArtifactPresentation::Table)),
                    _ => false,
                })
            {
                return Err(if format < 16 {
                    "old history contains artifact references".into()
                } else {
                    "history contains generated artifact references".into()
                });
            }
        }
        profile.format = FORMAT;
        profile.validate()?;
        super::artifacts::audit_tables(
            &profile,
            &write.open_table(ARTIFACTS)?,
            &write.open_table(RUNS)?,
            &write.open_table(CONVERSATIONS)?,
            &write.open_table(LAUNCHES)?,
        )?;
        meta.insert("profile", encode(&profile)?.as_slice())?;
    }
    write.commit()?;
    Ok(())
}

fn reject_charts(
    artifacts: &impl ReadableTable<&'static str, &'static [u8]>,
    runs: &impl ReadableTable<&'static str, &'static [u8]>,
) -> Result<()> {
    for row in artifacts.iter()? {
        let (_, bytes) = row?;
        let record: Record<super::Artifact> = decode(bytes.value())?;
        if matches!(record.value.content, ArtifactContent::Chart { .. }) {
            return Err("old history contains chart content".into());
        }
    }
    for row in runs.iter()? {
        let (_, bytes) = row?;
        let run: Record<crate::model::Run> = decode(bytes.value())?;
        if run
            .value
            .output
            .projection()
            .conversation
            .iter()
            .any(|entry| {
                matches!(
                    entry,
                    ProjectedEntry::Artifact {
                        presentation: metldr_conversation::ArtifactPresentation::Chart,
                        ..
                    }
                )
            })
        {
            return Err("old history contains chart references".into());
        }
    }
    Ok(())
}
