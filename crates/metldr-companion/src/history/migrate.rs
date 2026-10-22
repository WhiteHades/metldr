use super::{History, import::Prepared, schema};
use crate::{Result, store::Disk};
use redb::ReadableDatabase;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MARKER_SCHEMA: u8 = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Backup,
    Imported,
    DatabaseSynced,
    DatabaseRenamed,
    DatabaseInstalled,
    MarkerSynced,
    MarkerRenamed,
    Complete,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema: u8,
    store: String,
    legacy_sha256: String,
}

#[derive(Deserialize)]
struct Header {
    schema: u8,
}

pub(super) fn run(
    directory: PathBuf,
    observed: impl FnMut(Phase) -> Result<()>,
) -> Result<History> {
    let disk = Disk::lock(directory)?;
    run_locked(&disk, observed)
}

pub(super) fn load(disk: &Disk) -> Result<History> {
    if !regular(&disk.directory.join("history.redb"))? {
        match read(&disk.directory.join("snapshot.json"))? {
            None => disk.save(&disk.read_snapshot()?)?,
            Some(bytes)
                if serde_json::from_slice::<Header>(&bytes)
                    .is_ok_and(|header| matches!(header.schema, 8 | MARKER_SCHEMA)) => {}
            Some(_) => {
                disk.read_snapshot()?;
            }
        }
    }
    run_locked(disk, |_| Ok(()))
}

fn run_locked(disk: &Disk, mut observed: impl FnMut(Phase) -> Result<()>) -> Result<History> {
    let directory = &disk.directory;
    let snapshot = read(&directory.join("snapshot.json"))?.ok_or("legacy snapshot is absent")?;
    let header: Header = serde_json::from_slice(&snapshot)?;
    let installed = directory.join("history.redb");
    let history = if regular(&installed)? {
        let history = History::open(&installed, disk.lease())?;
        let (original, marker) = origin(&history)?;
        require_backup(directory, &original)?;
        match header.schema {
            8 if snapshot == original => (),
            MARKER_SCHEMA if serde_json::from_slice::<Marker>(&snapshot)? == marker => (),
            _ => return Err("snapshot conflicts with installed history".into()),
        }
        history
    } else {
        if header.schema != 8 {
            return Err("history marker has no installed database".into());
        }
        let prepared = Prepared::new(&snapshot)?;
        backup(directory, &snapshot)?;
        observed(Phase::Backup)?;
        let temporary = directory.join("history.import.redb");
        if regular(&temporary)? {
            fs::remove_file(&temporary)?;
        }
        let history = History::file(&temporary, true, disk.lease())?;
        prepared.write(&history.db, &snapshot)?;
        observed(Phase::Imported)?;
        drop(history);
        File::open(&temporary)?.sync_all()?;
        observed(Phase::DatabaseSynced)?;
        fs::rename(&temporary, &installed)?;
        observed(Phase::DatabaseRenamed)?;
        sync_directory(directory)?;
        observed(Phase::DatabaseInstalled)?;
        History::open(&installed, disk.lease())?
    };
    let (original, marker) = origin(&history)?;
    require_backup(directory, &original)?;
    let current = read(&directory.join("snapshot.json"))?.ok_or("history snapshot is absent")?;
    if header.schema == MARKER_SCHEMA {
        if serde_json::from_slice::<Marker>(&current)? != marker {
            return Err("history marker changed".into());
        }
    } else {
        if current != original {
            return Err("legacy snapshot changed before marker publication".into());
        }
        let temporary = directory.join("snapshot.format.next");
        write_reserved(&temporary, &serde_json::to_vec(&marker)?)?;
        observed(Phase::MarkerSynced)?;
        fs::rename(&temporary, directory.join("snapshot.json"))?;
        observed(Phase::MarkerRenamed)?;
    }
    File::open(directory.join("snapshot.json"))?.sync_all()?;
    sync_directory(directory)?;
    observed(Phase::Complete)?;
    Ok(history)
}

fn origin(history: &History) -> Result<(Vec<u8>, Marker)> {
    let read = history.db.begin_read()?;
    let meta = read.open_table(schema::META)?;
    let profile: schema::Profile = schema::decode(
        meta.get("profile")?
            .ok_or("history profile is absent")?
            .value(),
    )?;
    let original = meta
        .get("legacy_snapshot")?
        .ok_or("history origin is absent")?;
    if original.value().len() > crate::STORE_MAX {
        return Err("history origin exceeds bound".into());
    }
    let original = original.value().to_vec();
    schema::validate_origin(&profile, &original)?;
    Ok((
        original,
        Marker {
            schema: MARKER_SCHEMA,
            store: profile.store,
            legacy_sha256: profile.legacy_sha256,
        },
    ))
}

fn backup(directory: &Path, original: &[u8]) -> Result<()> {
    let destination = directory.join("snapshot.v8.json");
    if let Some(bytes) = read(&destination)? {
        if bytes != original {
            return Err("legacy backup does not match snapshot".into());
        }
        File::open(&destination)?.sync_all()?;
        return sync_directory(directory);
    }
    let temporary = directory.join("snapshot.v8.next");
    write_reserved(&temporary, original)?;
    fs::rename(&temporary, &destination)?;
    sync_directory(directory)
}

fn require_backup(directory: &Path, original: &[u8]) -> Result<()> {
    if read(&directory.join("snapshot.v8.json"))?.as_deref() != Some(original) {
        return Err("installed history backup is absent or changed".into());
    }
    Ok(())
}

fn regular(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err("history path is not a regular file".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    if !regular(path)? {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(crate::STORE_MAX as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > crate::STORE_MAX {
        return Err("migration input exceeds bound".into());
    }
    Ok(Some(bytes))
}

fn write_reserved(path: &Path, bytes: &[u8]) -> Result<()> {
    let existed = regular(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(!existed);
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn sync_directory(directory: &Path) -> Result<()> {
    File::open(directory)?.sync_all()?;
    Ok(())
}
