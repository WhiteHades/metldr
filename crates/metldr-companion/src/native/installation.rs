use crate::{CONFIG_MAX, Result, WIRE_VERSION, wire};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Installation {
    schema: u8,
    extension_origin: String,
    state_root: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Greeting {
    v: u8,
    kind: String,
    key: String,
}

struct ProfileKey([u8; 32]);

impl ProfileKey {
    fn decode(bytes: &[u8]) -> Result<Self> {
        let greeting: Greeting =
            serde_json::from_slice(bytes).map_err(|_| "invalid native profile greeting")?;
        if greeting.v != WIRE_VERSION
            || greeting.kind != "profile"
            || greeting.key.len() != 64
            || !greeting
                .key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid native profile greeting".into());
        }
        let mut key = [0; 32];
        for (byte, pair) in key.iter_mut().zip(greeting.key.as_bytes().chunks_exact(2)) {
            let pair = std::str::from_utf8(pair)?;
            *byte = u8::from_str_radix(pair, 16)?;
        }
        Ok(Self(key))
    }

    fn state_path(&self, root: &Path, origin: &str) -> Result<PathBuf> {
        let mut hash = Sha256::new();
        hash.update(b"metldr-native-profile-v1\0");
        hash.update(origin.as_bytes());
        hash.update([0]);
        hash.update(self.0);
        let path = root.join(format!("{:x}", hash.finalize()));
        match fs::symlink_metadata(&path) {
            Ok(_) => private_directory(&path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        Ok(path)
    }
}

fn private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !path.is_absolute() || !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("native state root must be a private absolute directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err("native state directory is not owned and private".into());
        }
    }
    #[cfg(not(unix))]
    return Err("installed state permissions require native platform verification".into());
    Ok(())
}

impl Installation {
    fn load(executable: &Path, actual: &str) -> Result<Self> {
        let path = executable.with_file_name("metldr-companion.json");
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("invalid installed companion configuration".into());
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take((CONFIG_MAX + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > CONFIG_MAX {
            return Err("installed companion configuration exceeds bound".into());
        }
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "invalid installed companion configuration")?;
        if config.schema != 1
            || config.extension_origin != actual
            || !super::config::valid_extension_origin(actual)
        {
            return Err("extension origin not authorized by installed configuration".into());
        }
        private_directory(&config.state_root)?;
        Ok(Self {
            state_root: config.state_root.canonicalize()?,
            ..config
        })
    }
}

pub(super) fn select_state(actual: &str) -> Result<PathBuf> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let config = Installation::load(&executable, actual)?;
    wire::write_frame(
        &mut std::io::stdout().lock(),
        &json!({"v":WIRE_VERSION,"kind":"profile_required"}),
    )?;
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let greeting = wire::read_raw(&mut std::io::stdin().lock(), 256);
        let _ = tx.send(greeting);
    });
    let frame = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "native profile selection timed out")??
        .ok_or("native profile selection disconnected")?;
    ProfileKey::decode(&frame)?.state_path(&config.state_root, &config.extension_origin)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn greeting(key: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"v":4,"kind":"profile","key":key})).unwrap()
    }

    #[test]
    fn profile_stores_stay_separate_and_reopen_under_one_writer() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "metldr-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        private_directory(&root)?;
        let origin = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
        let a = ProfileKey::decode(&greeting(&"01".repeat(32)))?;
        let b = ProfileKey::decode(&greeting(&"02".repeat(32)))?;
        let path_a = a.state_path(&root, origin)?;
        let path_b = b.state_path(&root, origin)?;
        assert_ne!(path_a, path_b);
        let (disk_a, state_a) = crate::store::Disk::open(path_a.clone())?;
        disk_a.save(&state_a)?;
        assert!(crate::store::Disk::open(path_a.clone()).is_err());
        let (disk_b, state_b) = crate::store::Disk::open(path_b)?;
        assert_ne!(state_a.view.store, state_b.view.store);
        drop(disk_a);
        let (reopened, restored) = crate::store::Disk::open(a.state_path(&root, origin)?)?;
        assert_eq!(restored.view.store, state_a.view.store);
        drop((reopened, disk_b));
        fs::remove_dir_all(&path_a)?;
        symlink(&root, &path_a)?;
        assert!(a.state_path(&root, origin).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn profile_greeting_accepts_only_the_bounded_canonical_shape() {
        assert!(ProfileKey::decode(&greeting(&"ab".repeat(32))).is_ok());
        for key in ["../state".to_owned(), "A0".repeat(32), "ab".repeat(31)] {
            assert!(ProfileKey::decode(&greeting(&key)).is_err());
        }
        for bytes in [
            br#"{"v":4,"command":{"op":"view"},"request":"early"}"#.as_slice(),
            br#"{"v":4,"kind":"profile","key":"bad"}"#.as_slice(),
            br#"{"v":4,"kind":"profile","key":"bad","path":"/elsewhere"}"#.as_slice(),
        ] {
            assert!(ProfileKey::decode(bytes).is_err());
        }
    }
}
