use super::schema::{
    ARTIFACTS, CONVERSATIONS, LAUNCHES, Profile, RUNS, Record, conversation_number, decode,
};
use crate::{Result, agent_model::Launch, model::Run};
use metldr_conversation::{ArtifactId, ProjectedEntry, Table};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub id: ArtifactId,
    pub run: String,
    pub call: String,
    pub surface_id: String,
    pub table: Table,
}

pub(super) fn audit(read: &redb::ReadTransaction, profile: &Profile) -> Result<()> {
    let artifacts = read.open_table(ARTIFACTS)?;
    let runs = read.open_table(RUNS)?;
    let conversations = read.open_table(CONVERSATIONS)?;
    let launches = read.open_table(LAUNCHES)?;

    for row in artifacts.iter()? {
        let (key, bytes) = row?;
        let id = key.value();
        let record: Record<Artifact> = decode(bytes.value())?;
        let artifact = &record.value;
        valid_id(profile, id)?;
        valid_call(&artifact.call)?;
        valid_surface(&artifact.surface_id)?;
        if artifact.id.as_str() != id || artifact.run.is_empty() || record.revision != 1 {
            return Err("invalid saved artifact identity or revision".into());
        }
        valid_id(profile, &artifact.run)?;
        let run_bytes = runs
            .get(artifact.run.as_str())?
            .ok_or("artifact run is absent")?;
        let run_record: Record<Run> = decode(run_bytes.value())?;
        if run_record.value.id != artifact.run
            || run_record.revision == 0
            || run_record.conversation != record.conversation
            || conversation_number(profile, &run_record.conversation).is_err()
            || conversations
                .get(run_record.conversation.as_str())?
                .map(|bytes| decode::<super::schema::Conversation>(bytes.value()))
                .transpose()?
                .is_none_or(|conversation| conversation.id != run_record.conversation)
        {
            return Err("artifact and run ownership differs".into());
        }
        let launch_bytes = launches
            .get(artifact.run.as_str())?
            .ok_or("artifact run launch is absent")?;
        let launch: Record<Launch> = decode(launch_bytes.value())?;
        if launch.conversation != run_record.conversation || !eligible(&launch.value) {
            return Err("artifact run is not an eligible Responses run".into());
        }
        let refs = artifact_refs(&run_record.value);
        if refs
            .iter()
            .filter(|reference| reference.as_str() == id)
            .count()
            != 1
            || refs.len() > 4
        {
            return Err("artifact reference index differs".into());
        }
    }

    for row in runs.iter()? {
        let (key, bytes) = row?;
        let run_id = key.value();
        let run: Record<Run> = decode(bytes.value())?;
        if run.value.id != run_id || run.revision == 0 {
            return Err("saved run identity differs".into());
        }
        let refs = artifact_refs(&run.value);
        if refs.len() > 4 {
            return Err("run exceeds artifact reference limit".into());
        }
        let mut calls = Vec::with_capacity(refs.len());
        for id in &refs {
            valid_id(profile, id.as_str())?;
            let bytes = artifacts
                .get(id.as_str())?
                .ok_or("run artifact reference is absent")?;
            let artifact: Record<Artifact> = decode(bytes.value())?;
            valid_call(&artifact.value.call)?;
            valid_surface(&artifact.value.surface_id)?;
            if artifact.value.id.as_str() != id.as_str()
                || artifact.value.run != run_id
                || artifact.conversation != run.conversation
                || artifact.revision != 1
                || !eligible_launch(&launches, run_id, &run.conversation)?
                || calls.contains(&artifact.value.call)
            {
                return Err("run artifact ownership or call identity differs".into());
            }
            calls.push(artifact.value.call);
        }
    }
    Ok(())
}

fn eligible_launch(
    launches: &redb::ReadOnlyTable<&str, &[u8]>,
    run: &str,
    conversation: &str,
) -> Result<bool> {
    let bytes = launches.get(run)?.ok_or("artifact run launch is absent")?;
    let launch: Record<Launch> = decode(bytes.value())?;
    Ok(launch.conversation == conversation && eligible(&launch.value))
}

pub(super) fn eligible(launch: &Launch) -> bool {
    matches!(
        launch,
        Launch::Text {
            protocol: crate::provider_settings::Protocol::Responses,
            ..
        }
    )
}

pub(super) fn artifact_refs(run: &Run) -> Vec<ArtifactId> {
    run.output
        .projection()
        .conversation
        .into_iter()
        .filter_map(|entry| match entry {
            ProjectedEntry::Artifact { id } => Some(id),
            _ => None,
        })
        .collect()
}

pub(super) fn valid_id(profile: &Profile, id: &str) -> Result<u64> {
    let number = id
        .strip_prefix(&format!("{}:", profile.store))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|number| *number > 0 && *number < profile.next)
        .ok_or("invalid artifact identity")?;
    if id != format!("{}:{number}", profile.store) {
        return Err("noncanonical artifact identity".into());
    }
    Ok(number)
}

pub(super) fn valid_call(call: &str) -> Result<()> {
    if call.is_empty() || call.len() > 256 || call.chars().any(char::is_control) {
        return Err("invalid artifact call identity".into());
    }
    Ok(())
}

pub(super) fn valid_surface(surface: &str) -> Result<()> {
    if surface.is_empty()
        || surface.encode_utf16().count() > 256
        || surface.chars().any(char::is_control)
    {
        return Err("invalid artifact surface identity".into());
    }
    Ok(())
}
