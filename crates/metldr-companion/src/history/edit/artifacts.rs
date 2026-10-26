use super::super::artifacts::{artifact_refs, eligible, valid_call, valid_id, valid_surface};
use super::super::schema::{ARTIFACTS, LAUNCHES, RUNS, Record, decode, encode};
use super::super::{Artifact, Observed};
use super::{Edit, Selection, record, storage};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Status},
};
use metldr_conversation::{ArtifactId, Change, Table};
use metldr_history_pages::Counter;
use redb::ReadableTable;

impl Edit<'_> {
    pub(crate) fn accept_table(
        &mut self,
        scope: &Selection,
        run_id: &str,
        call: &str,
        surface_id: &str,
        table: Table,
    ) -> Result<ArtifactId> {
        let result = self.accept_table_inner(scope, run_id, call, surface_id, table);
        self.checked(result)
    }

    fn accept_table_inner(
        &mut self,
        scope: &Selection,
        run_id: &str,
        call: &str,
        surface_id: &str,
        table: Table,
    ) -> Result<ArtifactId> {
        self.require_selection(scope)?;
        valid_call(call)?;
        valid_surface(surface_id)?;
        let before = self.saved_run(run_id)?.ok_or("unknown history run")?;
        if before.conversation != scope.conversation || before.value.id != run_id {
            return Err("run belongs to a different conversation".into());
        }
        self.validate_artifacts(scope, &before.value)?;
        let launch: Record<Launch> =
            record(self.write, LAUNCHES, run_id)?.ok_or("run has no launch")?;
        if launch.conversation != scope.conversation || !eligible(&launch.value) {
            return Err("table artifacts require a Responses text run".into());
        }
        let refs = artifact_refs(&before.value);
        if refs.len() > 4 {
            return Err("run exceeds artifact reference limit".into());
        }
        let mut calls = Vec::with_capacity(refs.len());
        for id in &refs {
            valid_id(&self.profile, id.as_str())?;
            let saved: Record<Artifact> = record(self.write, ARTIFACTS, id.as_str())?
                .ok_or("run artifact reference is absent")?;
            valid_call(&saved.value.call)?;
            valid_surface(&saved.value.surface_id)?;
            if saved.conversation != scope.conversation
                || saved.value.id != *id
                || saved.value.run != run_id
                || saved.revision != 1
                || calls.contains(&saved.value.call)
            {
                return Err("run artifact ownership differs".into());
            }
            calls.push(saved.value.call.clone());
            if saved.value.call == call {
                if saved.value.surface_id == surface_id && saved.value.table == table {
                    return Ok(id.clone());
                }
                return Err("artifact call identity already has different content".into());
            }
        }
        if before.value.status != Status::Running {
            return Err("new artifacts require a running response".into());
        }
        if refs.len() == 4 {
            return Err("run artifact reference limit reached".into());
        }

        let id = ArtifactId::new(self.id()?)?;
        let artifact = Artifact {
            id: id.clone(),
            run: run_id.to_owned(),
            call: call.to_owned(),
            surface_id: surface_id.to_owned(),
            table,
        };
        storage(|| {
            let value = Record {
                conversation: scope.conversation.clone(),
                revision: 1,
                value: artifact,
            };
            if self
                .write
                .open_table(ARTIFACTS)?
                .insert(id.as_str(), encode(&value)?.as_slice())?
                .is_some()
            {
                return Err("artifact identity already exists".into());
            }
            Ok(())
        })?;
        let mut next = before.value;
        next.output.change(Change::Artifact(id.clone()))?;
        self.put_run(scope, next)?;
        Ok(id)
    }

    pub(crate) fn artifact_detail(
        &self,
        scope: &Selection,
        id: &str,
    ) -> Result<Observed<Artifact>> {
        self.checked((|| {
            self.require_selection(scope)?;
            let saved: Record<Artifact> =
                record(self.write, ARTIFACTS, id)?.ok_or("unknown history artifact")?;
            valid_id(&self.profile, id)?;
            if saved.value.id.as_str() != id
                || saved.revision != 1
                || saved.conversation != scope.conversation
            {
                return Err("artifact belongs to a different conversation".into());
            }
            let run = self
                .saved_run(&saved.value.run)?
                .ok_or("artifact run is absent")?;
            self.validate_artifacts(scope, &run.value)?;
            let launch: Record<Launch> = record(self.write, LAUNCHES, &saved.value.run)?
                .ok_or("artifact run launch is absent")?;
            if run.conversation != scope.conversation
                || run.value.id != saved.value.run
                || launch.conversation != scope.conversation
                || !eligible(&launch.value)
                || artifact_refs(&run.value)
                    .iter()
                    .filter(|reference| reference.as_str() == id)
                    .count()
                    != 1
            {
                return Err("artifact is not referenced by its owning run".into());
            }
            valid_call(&saved.value.call)?;
            valid_surface(&saved.value.surface_id)?;
            Ok(Observed {
                revision: Counter(saved.revision),
                value: saved.value,
            })
        })())
    }

    pub(crate) fn validate_artifacts(&self, scope: &Selection, run: &Run) -> Result<()> {
        self.checked((|| {
            self.require_selection(scope)?;
            let refs = artifact_refs(run);
            if refs.is_empty() {
                return Ok(());
            }
            if refs.len() > 4 {
                return Err("run exceeds artifact reference limit".into());
            }
            let saved_run: Record<Run> =
                record(self.write, RUNS, &run.id)?.ok_or("unknown history run")?;
            let launch: Record<Launch> =
                record(self.write, LAUNCHES, &run.id)?.ok_or("run has no launch")?;
            if saved_run.conversation != scope.conversation
                || saved_run.value.id != run.id
                || launch.conversation != scope.conversation
                || !eligible(&launch.value)
            {
                return if refs.is_empty() {
                    Ok(())
                } else {
                    Err("run is not eligible for artifact references".into())
                };
            }
            let mut calls = Vec::with_capacity(refs.len());
            for id in refs {
                valid_id(&self.profile, id.as_str())?;
                let artifact: Record<Artifact> = record(self.write, ARTIFACTS, id.as_str())?
                    .ok_or("run artifact reference is absent")?;
                valid_call(&artifact.value.call)?;
                valid_surface(&artifact.value.surface_id)?;
                if artifact.value.id != id
                    || artifact.value.run != run.id
                    || artifact.conversation != scope.conversation
                    || artifact.revision != 1
                    || calls.contains(&artifact.value.call)
                {
                    return Err("run artifact ownership or call identity differs".into());
                }
                calls.push(artifact.value.call);
            }
            Ok(())
        })())
    }
}
