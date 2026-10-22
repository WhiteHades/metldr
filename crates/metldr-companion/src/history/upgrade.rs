use super::schema::*;
use crate::Result;
use redb::{Database, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata};

pub(super) fn conversations(db: &Database) -> Result<()> {
    let read = db.begin_read()?;
    let meta = read.open_table(META)?;
    let profile: Profile = decode(
        meta.get("profile")?
            .ok_or("history profile is absent")?
            .value(),
    )?;
    match profile.format {
        FORMAT => return Ok(()),
        10 => (),
        _ => return Err("unsupported history profile format".into()),
    }
    require_base_tables(&read)?;
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
            10 => (),
            _ => return Err("unsupported history profile format".into()),
        }
        validate_origin(
            &profile,
            meta.get("legacy_snapshot")?
                .ok_or("history origin is absent")?
                .value(),
        )?;

        let conversations = write.open_table(CONVERSATIONS)?;
        if conversations.len()? != 1 {
            return Err("history conversation count is invalid for upgrade".into());
        }
        let bytes = conversations
            .get(profile.selected.as_str())?
            .ok_or("selected history conversation is absent")?;
        let conversation: Conversation = decode(bytes.value())?;
        if conversation.id != profile.selected {
            return Err("selected history identity differs".into());
        }
        let number = conversation_number(&profile, &conversation.id)?;

        let mut order = write.open_table(CONVERSATION_ORDER)?;
        let selections = write.open_table(SELECTIONS)?;
        if order.len()? != 0 || selections.len()? != 0 {
            return Err("conversation upgrade tables are not empty".into());
        }

        profile.format = FORMAT;
        profile.validate()?;
        order.insert(number, conversation.id.as_str())?;
        meta.insert("profile", encode(&profile)?.as_slice())?;
    }
    write.commit()?;
    Ok(())
}
