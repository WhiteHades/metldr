use crate::model::{Store, View};
use crate::{Result, STORE_MAX};
use serde::{Deserialize, Deserializer};
use std::{
    collections::{BTreeMap, hash_map::RandomState},
    fs::{self, File, OpenOptions},
    hash::BuildHasher,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyStore {
    view: View,
    next: u64,
    starts: BTreeMap<String, String>,
}

fn present_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u8>, D::Error> {
    u8::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
struct SchemaHeader {
    #[serde(default, deserialize_with = "present_schema")]
    schema: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV2 {
    schema: u8,
    view: View,
    next: u64,
    starts: BTreeMap<String, String>,
    launches: BTreeMap<String, crate::agent_model::Launch>,
    workflows: BTreeMap<String, crate::agent_model::Workflow>,
    #[serde(default)]
    codex_workflows: BTreeMap<String, crate::codex_model::Workflow>,
    #[serde(default)]
    opencode_workflows: BTreeMap<String, crate::opencode_model::Workflow>,
    #[serde(default)]
    browser_actions: BTreeMap<String, crate::browser_action::Action>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV3 {
    schema: u8,
    view: View,
    next: u64,
    starts: BTreeMap<String, String>,
    launches: BTreeMap<String, crate::agent_model::Launch>,
    workflows: BTreeMap<String, crate::agent_model::Workflow>,
    #[serde(default)]
    codex_workflows: BTreeMap<String, crate::codex_model::Workflow>,
    #[serde(default)]
    opencode_workflows: BTreeMap<String, crate::opencode_model::Workflow>,
    #[serde(default)]
    browser_actions: BTreeMap<String, crate::browser_action::Action>,
    providers: crate::provider_settings::Settings,
}

fn require_flat_outputs(view: &View) -> Result<()> {
    if view.runs.iter().any(|run| run.output.is_native()) {
        return Err("legacy store contains native output".into());
    }
    Ok(())
}

fn reject_text_launches(launches: &BTreeMap<String, crate::agent_model::Launch>) -> Result<()> {
    if launches
        .values()
        .any(|launch| matches!(launch, crate::agent_model::Launch::Text { .. }))
    {
        return Err("legacy store contains text launch data".into());
    }
    Ok(())
}

pub(super) struct Disk {
    pub(super) directory: PathBuf,
    _lock: std::sync::Arc<File>,
}
impl Disk {
    pub(super) fn lease(&self) -> std::sync::Arc<File> {
        std::sync::Arc::clone(&self._lock)
    }

    pub(super) fn lock(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let lock = private_file(&directory.join("writer.lock"))?;
        lock.try_lock()?;
        let disk = Self {
            directory,
            _lock: std::sync::Arc::new(lock),
        };
        Ok(disk)
    }

    pub(super) fn open(directory: PathBuf) -> Result<(Self, Store)> {
        let disk = Self::lock(directory)?;
        let state = disk.read_snapshot()?;
        Ok((disk, state))
    }

    pub(super) fn read_snapshot(&self) -> Result<Store> {
        let path = self.directory.join("snapshot.json");
        let state = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(STORE_MAX as u64 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > STORE_MAX {
                    return Err("saved store exceeds bound".into());
                }
                let header: SchemaHeader = serde_json::from_slice(&bytes)?;
                match header.schema {
                    Some(crate::STORE_VERSION) => {
                        let state: Store = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&state.browser_actions)?;
                        state.validate()?;
                        state
                    }
                    Some(7) => {
                        let mut state: Store = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&state.browser_actions)?;
                        reject_text_launches(&state.launches)?;
                        state.schema = crate::STORE_VERSION;
                        state.validate()?;
                        self.backup_versioned(&bytes, 7)?;
                        self.save(&state)?;
                        state
                    }
                    Some(6) => {
                        let mut state: Store = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&state.browser_actions)?;
                        reject_text_launches(&state.launches)?;
                        crate::browser_action::reject_selection_actions(&state.browser_actions)?;
                        state.schema = crate::STORE_VERSION;
                        state.validate()?;
                        self.backup_versioned(&bytes, 6)?;
                        self.save(&state)?;
                        state
                    }
                    Some(5) => {
                        let mut state: Store = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&state.browser_actions)?;
                        reject_text_launches(&state.launches)?;
                        crate::browser_action::reject_button_actions(&state.browser_actions)?;
                        crate::browser_action::reject_selection_actions(&state.browser_actions)?;
                        state.schema = crate::STORE_VERSION;
                        state.validate()?;
                        self.backup_versioned(&bytes, 5)?;
                        self.save(&state)?;
                        state
                    }
                    Some(4) => {
                        let mut state: Store = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&state.browser_actions)?;
                        reject_text_launches(&state.launches)?;
                        crate::browser_action::reject_button_actions(&state.browser_actions)?;
                        crate::browser_action::reject_selection_actions(&state.browser_actions)?;
                        if state.view.runs.iter().any(|run| run.output.is_native()) {
                            return Err("schema 4 store contains native output".into());
                        }
                        state.schema = crate::STORE_VERSION;
                        state.validate()?;
                        self.backup_versioned(&bytes, 4)?;
                        self.save(&state)?;
                        state
                    }
                    Some(3) => {
                        let legacy: LegacyV3 = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&legacy.browser_actions)?;
                        if legacy.schema != 3 {
                            return Err("invalid schema 3 store".into());
                        }
                        reject_text_launches(&legacy.launches)?;
                        require_flat_outputs(&legacy.view)?;
                        crate::browser_action::reject_button_actions(&legacy.browser_actions)?;
                        crate::browser_action::reject_selection_actions(&legacy.browser_actions)?;
                        let state = Store {
                            schema: crate::STORE_VERSION,
                            view: legacy.view,
                            next: legacy.next,
                            starts: legacy.starts,
                            launches: legacy.launches,
                            workflows: legacy.workflows,
                            codex_workflows: legacy.codex_workflows,
                            opencode_workflows: legacy.opencode_workflows,
                            browser_actions: legacy.browser_actions,
                            providers: legacy.providers,
                            agent_settings: crate::agent_settings::Settings::default(),
                        };
                        state.validate()?;
                        self.backup_versioned(&bytes, 3)?;
                        self.save(&state)?;
                        state
                    }
                    Some(2) => {
                        let legacy: LegacyV2 = serde_json::from_slice(&bytes)?;
                        crate::browser_action::reject_scroll_actions(&legacy.browser_actions)?;
                        if legacy.schema != 2 {
                            return Err("invalid schema 2 store".into());
                        }
                        reject_text_launches(&legacy.launches)?;
                        require_flat_outputs(&legacy.view)?;
                        crate::browser_action::reject_button_actions(&legacy.browser_actions)?;
                        crate::browser_action::reject_selection_actions(&legacy.browser_actions)?;
                        let state = Store {
                            schema: crate::STORE_VERSION,
                            view: legacy.view,
                            next: legacy.next,
                            starts: legacy.starts,
                            launches: legacy.launches,
                            workflows: legacy.workflows,
                            codex_workflows: legacy.codex_workflows,
                            opencode_workflows: legacy.opencode_workflows,
                            browser_actions: legacy.browser_actions,
                            providers: crate::provider_settings::Settings::default(),
                            agent_settings: crate::agent_settings::Settings::default(),
                        };
                        state.validate()?;
                        self.backup_versioned(&bytes, 2)?;
                        self.save(&state)?;
                        state
                    }
                    Some(schema) => {
                        return Err(format!("unsupported saved store schema {schema}").into());
                    }
                    None => {
                        let legacy: LegacyStore = serde_json::from_slice(&bytes)?;
                        require_flat_outputs(&legacy.view)?;
                        let mut state = Store {
                            schema: crate::STORE_VERSION,
                            view: legacy.view,
                            next: legacy.next,
                            starts: legacy.starts,
                            launches: BTreeMap::new(),
                            workflows: BTreeMap::new(),
                            codex_workflows: BTreeMap::new(),
                            opencode_workflows: BTreeMap::new(),
                            browser_actions: BTreeMap::new(),
                            providers: crate::provider_settings::Settings::default(),
                            agent_settings: crate::agent_settings::Settings::default(),
                        };
                        state.launches = crate::agent_model::legacy_launches(&state);
                        state.validate()?;
                        self.backup_versioned(&bytes, 1)?;
                        self.save(&state)?;
                        state
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let rng = RandomState::new();
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
                // Correlation identity only; never an authentication token.
                let identity = format!(
                    "{:016x}{:016x}",
                    rng.hash_one((now, std::process::id(), 0)),
                    rng.hash_one((now, std::process::id(), 1))
                );
                Store {
                    schema: crate::STORE_VERSION,
                    view: View {
                        sequence: 0,
                        store: identity,
                        source: None,
                        runs: vec![],
                    },
                    next: 1,
                    starts: BTreeMap::new(),
                    launches: BTreeMap::new(),
                    workflows: BTreeMap::new(),
                    codex_workflows: BTreeMap::new(),
                    opencode_workflows: BTreeMap::new(),
                    browser_actions: BTreeMap::new(),
                    providers: crate::provider_settings::Settings::default(),
                    agent_settings: crate::agent_settings::Settings::default(),
                }
            }
            Err(e) => return Err(e.into()),
        };
        Ok(state)
    }

    fn backup_versioned(&self, bytes: &[u8], version: u8) -> Result<()> {
        let path = self.directory.join(format!("snapshot.v{version}.json"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                let result = file.write_all(bytes).and_then(|()| file.sync_all());
                drop(file);
                if let Err(error) = result {
                    let _ = fs::remove_file(&path);
                    return Err(error.into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.file_type().is_file() {
                    return Err("store backup is not a regular file".into());
                }
                let mut existing = Vec::new();
                File::open(&path)?
                    .take(STORE_MAX as u64 + 1)
                    .read_to_end(&mut existing)?;
                if existing != bytes {
                    return Err("store backup does not match snapshot".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                }
            }
            Err(error) => return Err(error.into()),
        }
        File::open(&path)?.sync_all()?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    pub(super) fn save(&self, state: &Store) -> Result<()> {
        let bytes = serde_json::to_vec(state)?;
        if bytes.len() > STORE_MAX {
            return Err("saved store capacity reached".into());
        }
        let next = self.directory.join("snapshot.next");
        let mut file = private_file(&next)?;
        file.set_len(0)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(next, self.directory.join("snapshot.json"))?;
        #[cfg(unix)]
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn migration_preserves_bytes_and_rejects_malformed_versions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "settings-migration-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();
        let (disk, state) = Disk::open(root.join("legacy")).unwrap();
        let mut legacy = serde_json::to_value(&state).unwrap();
        legacy["schema"] = serde_json::json!(2);
        legacy.as_object_mut().unwrap().remove("providers");
        legacy.as_object_mut().unwrap().remove("agent_settings");
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(disk.directory.join("snapshot.json"), &bytes).unwrap();
        drop(disk);
        let (disk, migrated) = Disk::open(root.join("legacy")).unwrap();
        assert_eq!(migrated.schema, crate::STORE_VERSION);
        assert_eq!(
            serde_json::to_value(&migrated.view).unwrap(),
            legacy["view"]
        );
        assert_eq!(
            fs::read(disk.directory.join("snapshot.v2.json")).unwrap(),
            bytes
        );
        let lease = disk.lease();
        drop(disk);
        assert!(Disk::open(root.join("legacy")).is_err());
        drop(lease);
        assert!(Disk::open(root.join("legacy")).is_ok());
        for (index, invalid) in [
            b"{\"schema\":null}".as_slice(),
            b"{\"schema\":3}",
            b"{\"schema\":3,\"schema\":2}",
            b"{\"schema\":99}",
        ]
        .into_iter()
        .enumerate()
        {
            let path = root.join(index.to_string());
            fs::create_dir(&path).unwrap();
            fs::write(path.join("snapshot.json"), invalid).unwrap();
            assert!(Disk::open(path.clone()).is_err());
            assert_eq!(fs::read(path.join("snapshot.json")).unwrap(), invalid);
            assert!(!path.join("snapshot.v1.json").exists());
            assert!(!path.join("snapshot.v2.json").exists());
        }
    }

    #[test]
    fn schema3_migration_keeps_exact_backup_and_defaults_agent_slots() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "schema3-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();
        let (disk, mut initial) = Disk::open(root.join("state")).unwrap();
        initial.schema = 3;
        let mut legacy = serde_json::to_value(&initial).unwrap();
        legacy.as_object_mut().unwrap().remove("agent_settings");
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(disk.directory.join("snapshot.json"), &bytes).unwrap();
        drop(disk);
        let (disk, migrated) = Disk::open(root.join("state")).unwrap();
        assert_eq!(migrated.schema, crate::STORE_VERSION);
        assert_eq!(
            migrated.agent_settings,
            crate::agent_settings::Settings::default()
        );
        assert_eq!(
            fs::read(disk.directory.join("snapshot.v3.json")).unwrap(),
            bytes
        );
        let migrated_value: serde_json::Value =
            serde_json::from_slice(&fs::read(disk.directory.join("snapshot.json")).unwrap())
                .unwrap();
        for key in [
            "view",
            "next",
            "starts",
            "launches",
            "workflows",
            "codex_workflows",
            "opencode_workflows",
            "browser_actions",
            "providers",
        ] {
            assert_eq!(
                migrated_value[key], legacy[key],
                "schema 3 migration changed {key}"
            );
        }
        drop(disk);

        for (name, mut invalid) in [
            (
                "schema3-with-agent-settings",
                serde_json::to_value(&initial).unwrap(),
            ),
            ("schema4-without-agent-settings", {
                let mut value = serde_json::to_value(&initial).unwrap();
                value["schema"] = serde_json::json!(4);
                value.as_object_mut().unwrap().remove("agent_settings");
                value
            }),
        ] {
            if name == "schema3-with-agent-settings" {
                invalid["schema"] = serde_json::json!(3);
            }
            let directory = root.join(name);
            fs::create_dir_all(&directory).unwrap();
            let raw = serde_json::to_vec(&invalid).unwrap();
            fs::write(directory.join("snapshot.json"), &raw).unwrap();
            assert!(Disk::open(directory.clone()).is_err());
            assert_eq!(fs::read(directory.join("snapshot.json")).unwrap(), raw);
            assert!(!directory.join("snapshot.v3.json").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema5_migration_keeps_exact_backup_and_rejects_button_variants() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "schema5-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();
        let (disk, mut initial) = Disk::open(root.join("state")).unwrap();
        initial.schema = 5;
        let bytes = serde_json::to_vec(&initial).unwrap();
        fs::write(disk.directory.join("snapshot.json"), &bytes).unwrap();
        drop(disk);
        let (disk, migrated) = Disk::open(root.join("state")).unwrap();
        assert_eq!(migrated.schema, crate::STORE_VERSION);
        assert_eq!(
            fs::read(disk.directory.join("snapshot.v5.json")).unwrap(),
            bytes
        );
        drop(disk);

        let invalid = root.join("button-in-v5");
        fs::create_dir_all(&invalid).unwrap();
        let mut forged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        forged["browser_actions"]["a:1"] = serde_json::json!({
            "id":"a:1", "revision":1,
            "proposal":{"run":"a:1","tool_use_id":"tool","binding":{
                "tab":1,"document":"doc","target":"target","frame":"frame",
                "session":"session","unique_context":"context","grant_revision":1
            },"url":"http://127.0.0.1:8000/article","intent":{
                "kind":"activate_button","button_id":"button_1","button_label":"Submit"
            }},
            "state":{"phase":"pending"}
        });
        let invalid_bytes = serde_json::to_vec(&forged).unwrap();
        fs::write(invalid.join("snapshot.json"), &invalid_bytes).unwrap();
        assert!(Disk::open(invalid.clone()).is_err());
        assert_eq!(
            fs::read(invalid.join("snapshot.json")).unwrap(),
            invalid_bytes
        );
        assert!(!invalid.join("snapshot.v5.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema6_migration_backs_up_exact_bytes_and_older_schemas_reject_selection_data() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "schema6-selection-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();

        let current = root.join("valid-schema6");
        let (disk, mut state) = Disk::open(current.clone()).unwrap();
        state.schema = 6;
        disk.save(&state).unwrap();
        let bytes = fs::read(disk.directory.join("snapshot.json")).unwrap();
        drop(disk);
        let (disk, migrated) = Disk::open(current).unwrap();
        assert_eq!(migrated.schema, crate::STORE_VERSION);
        assert_eq!(
            fs::read(disk.directory.join("snapshot.v6.json")).unwrap(),
            bytes
        );
        drop(disk);

        for version in 2..=6 {
            for (label, second_action) in [
                (
                    "intent",
                    serde_json::json!({
                        "id":format!("0123456789abcdef0123456789abcdef:4"),
                        "revision":1,
                        "proposal":{"run":"0123456789abcdef0123456789abcdef:2","tool_use_id":"select-call",
                            "binding":{"tab":1,"document":"doc","target":"target","frame":"frame",
                                "session":"session","unique_context":"context","grant_revision":1},
                            "url":"http://127.0.0.1:8000/article","intent":{"kind":"select_option","selection":{
                                "control_id":"select_1","control_label":"Region","previous":null,
                                "selected":{"id":"option_2","label":"North","index":1}}}},
                        "state":{"phase":"pending"}
                    }),
                ),
                (
                    "outcome",
                    serde_json::json!({
                        "id":"0123456789abcdef0123456789abcdef:5", "revision":4,
                        "proposal":{"run":"0123456789abcdef0123456789abcdef:2","tool_use_id":"field-call",
                            "binding":{"tab":1,"document":"doc","target":"target","frame":"frame",
                                "session":"session","unique_context":"context","grant_revision":1},
                            "url":"http://127.0.0.1:8000/article","intent":{"kind":"field","field_id":"field_1",
                                "field_label":"Name","previous_value":"old","value":"new"}},
                        "state":{"phase":"settled","outcome":{"kind":"selection_observed",
                            "selected":{"id":"option_2","label":"North","index":1}}}
                    }),
                ),
            ] {
                let directory = root.join(format!("v{version}-{label}"));
                fs::create_dir_all(&directory).unwrap();
                let (_, state) = Disk::open(directory.clone()).unwrap();
                let mut invalid = serde_json::to_value(state).unwrap();
                invalid["schema"] = serde_json::json!(version);
                invalid["browser_actions"] = serde_json::json!({
                    (second_action["id"].as_str().unwrap()): second_action
                });
                if version == 2 {
                    invalid.as_object_mut().unwrap().remove("providers");
                    invalid.as_object_mut().unwrap().remove("agent_settings");
                } else if version == 3 {
                    invalid.as_object_mut().unwrap().remove("agent_settings");
                }
                let bytes = serde_json::to_vec(&invalid).unwrap();
                fs::write(directory.join("snapshot.json"), &bytes).unwrap();
                let error = Disk::open(directory.clone()).err().unwrap().to_string();
                assert_eq!(
                    error, "legacy browser store contains selection action data",
                    "schema {version} {label}"
                );
                assert_eq!(fs::read(directory.join("snapshot.json")).unwrap(), bytes);
                assert!(!directory.join(format!("snapshot.v{version}.json")).exists());
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema4_migration_keeps_exact_backup_and_reopens_as_schema8() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "schema4-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();
        let (disk, mut initial) = Disk::open(root.join("state")).unwrap();
        initial.schema = 4;
        let mut legacy = serde_json::to_value(&initial).unwrap();
        let run_id = format!("{}:1", initial.view.store);
        legacy["view"]["runs"] = serde_json::json!([{
            "id": run_id,
            "source": format!("{}:1", initial.view.store),
            "kind": "answer",
            "question": null,
            "status": "complete",
            "output": "héllo, 🌱",
            "error": null
        }]);
        legacy["next"] = serde_json::json!(2);
        legacy["launches"][run_id] = serde_json::json!({ "kind": "legacy_unspecified" });
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(disk.directory.join("snapshot.json"), &bytes).unwrap();
        drop(disk);

        let (disk, migrated) = Disk::open(root.join("state")).unwrap();
        assert_eq!(migrated.schema, crate::STORE_VERSION);
        assert_eq!(migrated.view.runs[0].output.text(), "héllo, 🌱");
        assert_eq!(
            fs::read(disk.directory.join("snapshot.v4.json")).unwrap(),
            bytes
        );
        drop(disk);
        let (_, reopened) = Disk::open(root.join("state")).unwrap();
        assert_eq!(reopened.schema, crate::STORE_VERSION);
        assert_eq!(reopened.view.runs[0].output.text(), "héllo, 🌱");

        let invalid_directory = root.join("native-v4");
        fs::create_dir_all(&invalid_directory).unwrap();
        let mut native = legacy;
        native["view"]["runs"][0]["output"] = serde_json::json!({ "entries": [] });
        let native_bytes = serde_json::to_vec(&native).unwrap();
        fs::write(invalid_directory.join("snapshot.json"), &native_bytes).unwrap();
        assert!(Disk::open(invalid_directory.clone()).is_err());
        assert_eq!(
            fs::read(invalid_directory.join("snapshot.json")).unwrap(),
            native_bytes
        );
        assert!(!invalid_directory.join("snapshot.v4.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema7_migrates_with_backup_and_old_versions_reject_text_launches() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "schema7-text-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&root).unwrap();

        let current = root.join("valid-v7");
        let (disk, mut state) = Disk::open(current.clone()).unwrap();
        state.schema = 7;
        disk.save(&state).unwrap();
        let bytes = fs::read(disk.directory.join("snapshot.json")).unwrap();
        drop(disk);
        let (disk, migrated) = Disk::open(current).unwrap();
        assert_eq!(migrated.schema, crate::STORE_VERSION);
        assert_eq!(
            fs::read(disk.directory.join("snapshot.v7.json")).unwrap(),
            bytes
        );
        drop(disk);

        for version in [2, 3, 7] {
            let directory = root.join(format!("smuggled-v{version}"));
            let (disk, mut state) = Disk::open(directory.clone()).unwrap();
            let run_id = format!("{}:1", state.view.store);
            state.view.runs.push(crate::model::Run {
                id: run_id.clone(),
                source: run_id.clone(),
                kind: "summary".into(),
                question: None,
                status: crate::model::Status::Complete,
                output: Default::default(),
                error: None,
                corrections: Vec::new(),
            });
            state.next = 2;
            state.launches.insert(
                run_id.clone(),
                crate::agent_model::Launch::Text {
                    input_digest: "a".repeat(64),
                    protocol: crate::provider_settings::Protocol::Messages,
                    binding: crate::https_profile::Binding {
                        endpoint: "https://example.com".into(),
                        model: "model".into(),
                        revision: 1,
                        fingerprint: "b".repeat(64),
                    },
                },
            );
            state.schema = version;
            let mut forged = serde_json::to_value(&state).unwrap();
            if version == 2 {
                forged.as_object_mut().unwrap().remove("providers");
                forged.as_object_mut().unwrap().remove("agent_settings");
            } else if version == 3 {
                forged.as_object_mut().unwrap().remove("agent_settings");
            }
            let bytes = serde_json::to_vec(&forged).unwrap();
            fs::write(disk.directory.join("snapshot.json"), &bytes).unwrap();
            drop(disk);
            let error = Disk::open(directory.clone()).err().unwrap().to_string();
            assert_eq!(error, "legacy store contains text launch data");
            assert_eq!(fs::read(directory.join("snapshot.json")).unwrap(), bytes);
            assert!(!directory.join(format!("snapshot.v{version}.json")).exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
