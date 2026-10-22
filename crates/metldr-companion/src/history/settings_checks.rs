use super::{History, fixture, schema::*};
use crate::{
    Result,
    agent_settings::{Kind, Profile as AgentProfile},
    flow::{Event, Owner},
    model::Store,
    store::Disk,
    transport::Transport,
};
use redb::ReadableDatabase;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "settings-record-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile(history: &History) -> Result<Profile> {
    let read = history.db.begin_read()?;
    let table = read.open_table(META)?;
    decode(
        table
            .get("profile")?
            .ok_or("missing history profile")?
            .value(),
    )
}

fn owner(disk: Disk, state: Store, history: History) -> Owner {
    let (transport, _) = Transport::captured();
    let (tx, _) = mpsc::sync_channel::<Event>(8);
    Owner {
        transport,
        disk,
        state,
        browser_view: BTreeMap::new(),
        record_history: Some(history),
        history_pins: Default::default(),
        settings: Default::default(),
        agent_runtime: Default::default(),
        fixture: None,
        subscribers: BTreeMap::new(),
        operation: None,
        tx,
        pi_profile: None,
        codex_profile: None,
        opencode_profile: None,
        native_permissions: BTreeMap::new(),
        pi_ui: None,
        codex_input: None,
        browser_approval: None,
        browser_io: None,
        browser_route: None,
        responses_route: None,
        google_route: None,
        chat_route: None,
    }
}

#[test]
fn record_settings_reload_admit_replay_and_reject_stale_aggregate() -> Result<()> {
    let directory = Directory::new()?;
    let disk = Disk::lock(directory.0.clone())?;
    let original_snapshot = serde_json::to_vec(&fixture::legacy())?;
    let mut state: Store = serde_json::from_slice(&original_snapshot)?;
    crate::recovery::interrupt(&mut state)?;
    let older = "0123456789abcdef0123456789abcdef:09";
    let latest = state.id()?;
    let mut latest_run = state
        .view
        .runs
        .iter()
        .find(|run| run.id == older)
        .unwrap()
        .clone();
    latest_run.id = latest.clone();
    state.view.runs.push(latest_run);
    state
        .launches
        .insert(latest.clone(), state.launches[older].clone());
    state
        .starts
        .insert("settings-fixture-latest".into(), latest.clone());
    let workflow = state
        .workflows
        .get_mut("0123456789abcdef0123456789abcdef:030")
        .unwrap();
    workflow.last_run = latest;
    workflow.binding = crate::agent_model::Binding::Pending;
    state.validate()?;
    disk.save(&state)?;
    let snapshot = fs::read(directory.0.join("snapshot.json"))?;
    let history_path = directory.0.join("history.redb");
    let mut history =
        History::import_new(&history_path, &serde_json::to_vec(&state)?, disk.lease())?;
    let scope = history.selection()?;
    let viewed = history.edit(|edit| edit.project(&scope, Some(older)))?;
    let viewed_runs: Vec<_> = viewed.view.runs.iter().map(|run| run.id.clone()).collect();
    assert_eq!(viewed_runs.len(), 2);
    let mut owner = owner(disk, viewed, history);
    owner.refresh_saved_settings()?;
    assert_eq!(
        owner
            .state
            .view
            .runs
            .iter()
            .map(|run| run.id.clone())
            .collect::<Vec<_>>(),
        viewed_runs,
        "settings refresh dropped the viewed older native task"
    );

    let stale_profile = profile(owner.record_history.as_ref().unwrap())?;
    let (stale_providers, stale_agents) = owner
        .record_history
        .as_mut()
        .unwrap()
        .edit(|edit| edit.settings(&scope))?;
    let start_sequence = profile(owner.record_history.as_ref().unwrap())?.sequence;
    let executable = directory.0.join("owned-agent");
    fs::write(&executable, b"#! /bin/sh\nexit 0\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    }
    let agent_profile = AgentProfile::Opencode {
        executable: executable.clone(),
        workspace: directory.0.clone(),
    };
    owner.agent_save("settings-save:1", 0, agent_profile.clone())?;
    let after_save = profile(owner.record_history.as_ref().unwrap())?;
    assert_eq!(after_save.sequence, start_sequence + 1);
    assert_eq!(after_save.agents.slot(Kind::Opencode).revision, 1);
    let saved_slot = after_save.agents.slot(Kind::Opencode);
    let save_receipt = saved_slot.last_change.as_ref().unwrap();
    assert_eq!(save_receipt.request, "settings-save:1");
    assert_eq!(save_receipt.expected_revision, 0);
    assert_eq!(save_receipt.digest.len(), 64);

    // Replay must use the saved receipt after a stale cache and a removed executable.
    owner.state.agent_settings = stale_agents.clone();
    fs::remove_file(&executable)?;
    owner.agent_save("settings-save:1", 0, agent_profile.clone())?;
    assert_eq!(
        profile(owner.record_history.as_ref().unwrap())?.sequence,
        after_save.sequence
    );
    let changed_profile = AgentProfile::Opencode {
        executable: executable.clone(),
        workspace: directory.0.parent().unwrap().to_path_buf(),
    };
    assert!(
        owner
            .agent_save("settings-save:1", 0, changed_profile)
            .is_err()
    );
    assert!(
        owner
            .agent_save("settings-stale:1", 0, agent_profile)
            .is_err()
    );
    owner.refresh_saved_settings()?;
    assert_eq!(
        profile(owner.record_history.as_ref().unwrap())?.sequence,
        after_save.sequence
    );

    let expected = owner.state.agent_settings.slot(Kind::Opencode).revision;
    owner.agent_remove("settings-remove:1", Kind::Opencode, expected)?;
    let after_remove = profile(owner.record_history.as_ref().unwrap())?;
    assert_eq!(after_remove.sequence, after_save.sequence + 1);
    assert_eq!(
        after_remove.agents.slot(Kind::Opencode).revision,
        expected + 1
    );
    let removed_slot = after_remove.agents.slot(Kind::Opencode);
    let receipt = removed_slot.last_change.as_ref().unwrap();
    assert_eq!(receipt.request, "settings-remove:1");
    assert_eq!(receipt.expected_revision, expected);
    assert_eq!(receipt.digest.len(), 64);
    owner.agent_remove("settings-remove:1", Kind::Opencode, expected)?;
    assert_eq!(
        profile(owner.record_history.as_ref().unwrap())?.sequence,
        after_remove.sequence
    );
    assert_eq!(
        owner
            .state
            .view
            .runs
            .iter()
            .map(|run| run.id.clone())
            .collect::<Vec<_>>(),
        viewed_runs
    );

    let current = profile(owner.record_history.as_ref().unwrap())?;
    assert_ne!(
        serde_json::to_value(&current.agents)?,
        serde_json::to_value(&stale_profile.agents)?
    );
    assert_eq!(
        current
            .providers
            .pending
            .as_ref()
            .map(|pending| pending.request.as_str()),
        Some("provider-save-next")
    );
    assert_eq!(current.providers.retired, stale_providers.retired);
    let stale_replacement = owner.record_history.as_mut().unwrap().edit(|edit| {
        let result = edit.replace_settings(
            &scope,
            &stale_providers,
            &stale_agents,
            stale_providers.clone(),
            stale_agents.clone(),
        );
        assert!(result.is_err());
        Ok(())
    });
    assert!(
        stale_replacement.is_err(),
        "caught settings rejection must poison the edit"
    );
    let after_rejection = profile(owner.record_history.as_ref().unwrap())?;
    assert_eq!(after_rejection.sequence, current.sequence);
    assert_eq!(
        serde_json::to_value(&after_rejection.providers)?,
        serde_json::to_value(&current.providers)?
    );
    assert_eq!(
        serde_json::to_value(&after_rejection.agents)?,
        serde_json::to_value(&current.agents)?
    );
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, snapshot);

    // These are owned completion messages, without a credential worker or credential store.
    owner.settings.job = Some(std::thread::spawn(|| {}));
    owner.provider_completed(crate::provider_settings::Completion::Cleaned {
        removed: current.providers.retired.clone(),
        failed: false,
    })?;
    let cleaned = profile(owner.record_history.as_ref().unwrap())?;
    assert_eq!(cleaned.sequence, current.sequence + 1);
    assert!(cleaned.providers.retired.is_empty());
    assert!(cleaned.providers.pending.is_some());
    owner.settings.job = Some(std::thread::spawn(|| {}));
    owner.provider_completed(crate::provider_settings::Completion::Loaded {
        request: Some("settings-load:1".into()),
        profiles: Vec::new(),
    })?;
    let loaded = profile(owner.record_history.as_ref().unwrap())?;
    assert_eq!(loaded.sequence, cleaned.sequence + 1);
    assert!(owner.settings.job.is_none());
    let pending = loaded.providers.pending.clone().unwrap();
    let provider = pending.connection.profile(
        pending.protocol,
        loaded.providers.scope.as_deref().unwrap(),
        pending.revision,
    )?;
    owner.state.providers = Default::default();
    owner.settings.job = Some(std::thread::spawn(|| {}));
    owner.provider_completed(crate::provider_settings::Completion::Saved {
        pending,
        result: Ok(provider),
    })?;
    let completed = profile(owner.record_history.as_ref().unwrap())?;
    assert_eq!(completed.sequence, loaded.sequence + 1);
    assert!(completed.providers.pending.is_none());
    assert!(completed.providers.retired.is_empty());
    let slot = completed
        .providers
        .slot(crate::provider_settings::Protocol::Messages);
    assert_eq!(slot.revision, 2);
    assert_eq!(slot.last_request.as_deref(), Some("provider-save-next"));
    assert!(
        owner
            .provider_route(crate::provider_settings::Protocol::Messages)
            .is_some()
    );
    assert!(owner.settings.job.is_none());
    assert_eq!(completed.next, stale_profile.next);
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, snapshot);
    let stale_provider = owner.record_history.as_mut().unwrap().edit(|edit| {
        edit.replace_settings(
            &scope,
            &stale_providers,
            &completed.agents,
            completed.providers.clone(),
            completed.agents.clone(),
        )
    });
    assert!(stale_provider.is_err());
    assert_eq!(
        profile(owner.record_history.as_ref().unwrap())?.sequence,
        completed.sequence
    );

    drop(owner);
    let reopened = History::open(&history_path, Disk::lock(directory.0.clone())?.lease())?;
    let reopened_profile = profile(&reopened)?;
    assert_eq!(reopened_profile.sequence, completed.sequence);
    assert_eq!(
        serde_json::to_value(&reopened_profile.agents)?,
        serde_json::to_value(&completed.agents)?
    );
    assert_eq!(
        serde_json::to_value(&reopened_profile.providers)?,
        serde_json::to_value(&completed.providers)?
    );
    Ok(())
}
