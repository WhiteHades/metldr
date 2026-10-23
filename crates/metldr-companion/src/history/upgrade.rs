use super::{edit, schema::*};
use crate::Result;
use redb::{Database, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata};

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
        10 | 11 | 12 => (),
        _ => return Err("unsupported history profile format".into()),
    }
    require_base_tables(&read)?;
    if profile.format == 12 {
        read.open_table(NATIVE_CHOICES)?;
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
            10 | 11 | 12 => (),
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
        profile.format = FORMAT;
        profile.validate()?;
        meta.insert("profile", encode(&profile)?.as_slice())?;
    }
    write.commit()?;
    Ok(())
}
