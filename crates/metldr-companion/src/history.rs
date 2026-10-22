use crate::Result;
use redb::{Builder, Database, ReadableDatabase};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

mod context;
mod edit;
pub(crate) use edit::{
    ConversationBusy, ConversationChange, ConversationChanged, ConversationPage, Cursor, Edit,
    NativeWorkflow, Observed, RunPage, RunSummary, Selection,
};
mod import;
#[cfg(target_os = "linux")]
mod migrate;
mod schema;
mod upgrade;

#[cfg(test)]
mod browser_owner_checks;
#[cfg(test)]
mod checks;
#[cfg(test)]
mod conversation_checks;
#[cfg(all(test, target_os = "linux"))]
mod cutover_checks;
#[cfg(test)]
mod edit_checks;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod native_callback_checks;
#[cfg(test)]
mod native_edit_checks;
#[cfg(test)]
mod native_input_checks;
#[cfg(test)]
mod native_start_checks;
#[cfg(test)]
mod native_target_checks;
#[cfg(test)]
mod owner_checks;
#[cfg(test)]
mod read_checks;
#[cfg(test)]
mod recovery_checks;
#[cfg(test)]
mod settings_checks;
#[cfg(all(test, target_os = "linux"))]
mod startup_checks;

pub(crate) fn is_failure(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<edit::Failure>().is_some()
}

pub(super) struct History {
    db: Database,
    _lease: Arc<File>,
}

impl History {
    pub(super) fn load(directory: PathBuf) -> Result<(crate::store::Disk, Self)> {
        #[cfg(target_os = "linux")]
        {
            let disk = crate::store::Disk::lock(directory)?;
            let history = migrate::load(&disk)?;
            Ok((disk, history))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = directory;
            Err(
                "history migration directory durability has not been verified on this platform"
                    .into(),
            )
        }
    }

    fn import_new(path: &Path, original: &[u8], lease: Arc<File>) -> Result<Self> {
        let prepared = import::Prepared::new(original)?;
        let history = Self::file(path, true, lease)?;
        prepared.write(&history.db, original)?;
        Ok(history)
    }

    fn open(path: &Path, lease: Arc<File>) -> Result<Self> {
        let history = Self::file(path, false, lease)?;
        upgrade::conversations(&history.db)?;
        let read = history.db.begin_read()?;
        let meta = read.open_table(schema::META)?;
        let bytes = meta.get("profile")?.ok_or("history profile is absent")?;
        let profile: schema::Profile = schema::decode(bytes.value())?;
        profile.validate()?;
        schema::require_base_tables(&read)?;
        read.open_table(schema::SELECTIONS)?;
        let order = read.open_table(schema::CONVERSATION_ORDER)?;
        let selected_number = schema::conversation_number(&profile, &profile.selected)?;
        if order
            .get(selected_number)?
            .map(|id| id.value() == profile.selected)
            != Some(true)
        {
            return Err("selected conversation order differs".into());
        }
        let conversations = read.open_table(schema::CONVERSATIONS)?;
        let selected = conversations
            .get(profile.selected.as_str())?
            .ok_or("selected history conversation is absent")?;
        let selected: schema::Conversation = schema::decode(selected.value())?;
        if selected.id != profile.selected {
            return Err("selected history identity differs".into());
        }
        Ok(history)
    }

    pub(super) fn migrate(directory: PathBuf) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            migrate::run(directory, |_| Ok(()))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = directory;
            Err(
                "history migration directory durability has not been verified on this platform"
                    .into(),
            )
        }
    }

    fn file(path: &Path, create: bool, lease: Arc<File>) -> Result<Self> {
        let mut builder = Builder::new();
        builder.set_cache_size(8 * 1024 * 1024);
        if !create {
            return Ok(Self {
                db: builder.open(path)?,
                _lease: lease,
            });
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(create);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        Ok(Self {
            db: builder.create_file(file)?,
            _lease: lease,
        })
    }
}
