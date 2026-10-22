use super::{
    Edit, NativeTask, NativeWorkflow, Selection, native_task::workflow_id, scoped_digest, storage,
};
use crate::{
    Result,
    agent_model::Launch,
    browser_action::Target,
    history::schema::*,
    model::{Run, Status, valid_request_id},
};
use redb::{ReadableTable, ReadableTableMetadata};

impl Edit<'_> {
    pub(crate) fn admit_native(
        &mut self,
        scope: &Selection,
        request: &str,
        run: Run,
        launch: Launch,
        workflow: NativeWorkflow,
    ) -> Result<()> {
        let result = self.admit_native_inner(scope, request, run, launch, workflow);
        self.checked(result)
    }

    fn admit_native_inner(
        &mut self,
        scope: &Selection,
        request: &str,
        run: Run,
        launch: Launch,
        workflow: NativeWorkflow,
    ) -> Result<()> {
        self.require_selection(scope)?;
        let digest = launch
            .input_digest()
            .ok_or("native task has no input identity")?;
        if !valid_request_id(request) || self.duplicate(scope, request, digest)?.is_some() {
            return Err("native request already admitted or invalid".into());
        }
        let number = self.new_record_id(&run.id)?;
        let name = workflow_id(&launch)?;
        if run.id == name
            || !workflow.matches(&launch)
            || workflow.last_run() != run.id
            || run.kind != "agent"
            || run.status != Status::Running
            || run.error.is_some()
            || run
                .question
                .as_ref()
                .is_none_or(|q| q.trim().is_empty() || q.len() > 1024)
            || serde_json::to_vec(&run.output)?
                != serde_json::to_vec(&metldr_conversation::Content::native())?
        {
            return Err("invalid native task admission".into());
        }
        if storage(|| Ok(self.write.open_table(ACTIVE_RUNS)?.len()? != 0))? {
            return Err("another run is active".into());
        }
        let exists = storage(|| {
            Ok(self.write.open_table(PI)?.get(name)?.is_some()
                || self.write.open_table(CODEX)?.get(name)?.is_some()
                || self.write.open_table(OPENCODE)?.get(name)?.is_some())
        })?;
        let (workflow_revision, initial) = if exists {
            let (latest, target) = self.native_latest(scope, name)?;
            let before = self.native_task(scope, &latest)?;
            if !before.run.status.terminal() {
                return Err("native workflow still has active work".into());
            }
            before.workflow.validate_continuation(&workflow, &run.id)?;
            (
                before.versions[2]
                    .checked_add(1)
                    .ok_or("native workflow record revision exhausted")?,
                target,
            )
        } else {
            self.new_record_id(name)?;
            if !workflow.pending()
                || workflow.revision() != 2
                || self.selected_source(scope)?.as_ref().is_none_or(|s| {
                    s.id != workflow.source().id || s.page != workflow.source().page
                })
            {
                return Err("new native workflow has changed source binding or revision".into());
            }
            (1, Target::from(&workflow.source().page))
        };
        match &launch {
            Launch::Pi { activity, .. } if activity.is_empty() => (),
            Launch::Codex {
                activity,
                selection,
                ..
            } if activity.is_empty() && selection.is_none() => (),
            Launch::OpenCode {
                activity,
                selection,
                ..
            } if activity.is_empty() && selection.is_none() => (),
            _ => return Err("new native launch already has activity or selection".into()),
        }
        let task = NativeTask {
            identity: run.id.clone(),
            scope: scope.clone(),
            run,
            launch,
            workflow,
            versions: [0; 3],
        };
        self.native_state(&task)?;
        let name = workflow_id(&task.launch)?;
        let receipt = Start {
            conversation: scope.conversation.clone(),
            run: task.run.id.clone(),
            format: ReceiptFormat::Scoped {
                epoch: scope.epoch,
                digest: scoped_digest(scope, task.launch.input_digest().unwrap())?,
            },
        };
        storage(|| {
            self.save_native_workflow(scope, name, &task.workflow, workflow_revision)?;
            self.write.open_table(RUNS)?.insert(
                task.run.id.as_str(),
                encode(&Record {
                    conversation: scope.conversation.clone(),
                    revision: 1,
                    value: &task.run,
                })?
                .as_slice(),
            )?;
            if self
                .write
                .open_table(LAUNCHES)?
                .insert(
                    task.run.id.as_str(),
                    encode(&Record {
                        conversation: scope.conversation.clone(),
                        revision: 1,
                        value: &task.launch,
                    })?
                    .as_slice(),
                )?
                .is_some()
                || self
                    .write
                    .open_table(STARTS)?
                    .insert(request, encode(&receipt)?.as_slice())?
                    .is_some()
                || self
                    .write
                    .open_table(RUN_ORDER)?
                    .insert((scope.conversation.as_str(), number), task.run.id.as_str())?
                    .is_some()
                || self
                    .write
                    .open_table(WORKFLOW_RUNS)?
                    .insert((name, number), task.run.id.as_str())?
                    .is_some()
                || self
                    .write
                    .open_table(NATIVE_TARGETS)?
                    .insert(
                        task.run.id.as_str(),
                        encode(&Record {
                            conversation: scope.conversation.clone(),
                            revision: 1,
                            value: NativeTarget {
                                workflow: name.into(),
                                current: initial.clone(),
                                initial,
                                navigation: None,
                            },
                        })?
                        .as_slice(),
                    )?
                    .is_some()
            {
                return Err("native admission collides with saved history".into());
            }
            self.write
                .open_table(ACTIVE_RUNS)?
                .insert(task.run.id.as_str(), ())?;
            Ok(())
        })?;
        self.changed = true;
        self.native_task(scope, &task.run.id)?;
        Ok(())
    }

    pub(super) fn new_record_id(&self, id: &str) -> Result<u64> {
        let number = id
            .strip_prefix(&format!("{}:", self.profile.store))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("invalid new native identity")?;
        if number < self.first_id
            || number >= self.profile.next
            || id != format!("{}:{number}", self.profile.store)
        {
            return Err("native identity was not allocated by this transaction".into());
        }
        let exists = storage(|| {
            Ok(self.write.open_table(RUNS)?.get(id)?.is_some()
                || self.write.open_table(SOURCES)?.get(id)?.is_some()
                || self.write.open_table(PI)?.get(id)?.is_some()
                || self.write.open_table(CODEX)?.get(id)?.is_some()
                || self.write.open_table(OPENCODE)?.get(id)?.is_some()
                || self.write.open_table(ACTIONS)?.get(id)?.is_some()
                || self.write.open_table(CONVERSATIONS)?.get(id)?.is_some())
        })?;
        if exists {
            return Err("allocated native identity already belongs to a record".into());
        }
        Ok(number)
    }
}
