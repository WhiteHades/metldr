use super::{Edit, NativeWorkflow, Selection, record, storage};
use crate::{
    Result,
    agent_model::Launch,
    history::schema::*,
    model::{Run, Store, View},
};
use std::collections::BTreeMap;

pub(crate) struct NativeTask {
    pub run: Run,
    pub launch: Launch,
    pub workflow: NativeWorkflow,
    pub(super) versions: [u64; 3],
    pub(super) identity: String,
    pub(super) scope: Selection,
}

pub(super) fn workflow_id(launch: &Launch) -> Result<&str> {
    match launch {
        Launch::Pi { workflow, .. }
        | Launch::Codex { workflow, .. }
        | Launch::OpenCode { workflow, .. } => Ok(workflow),
        _ => Err("task has no native workflow".into()),
    }
}

impl Edit<'_> {
    pub(crate) fn native_task(&self, scope: &Selection, id: &str) -> Result<NativeTask> {
        let result = (|| {
            self.native_initial_target(scope, id)?;
            self.native_task_data(scope, id)
        })();
        self.checked(result)
    }

    pub(super) fn native_task_data(&self, scope: &Selection, id: &str) -> Result<NativeTask> {
        self.require_selection(scope)?;
        storage(|| {
            let run = self.saved_run(id)?.ok_or("native run is absent")?;
            let launch: Record<Launch> =
                record(self.write, LAUNCHES, id)?.ok_or("native launch is absent")?;
            if run.conversation != scope.conversation || launch.conversation != scope.conversation {
                return Err("native task ownership differs".into());
            }
            let name = workflow_id(&launch.value)?;
            let workflow = self.load_native_workflow(scope, name, &launch.value)?;
            let task = NativeTask {
                run: run.value,
                launch: launch.value,
                workflow: workflow.value,
                versions: [run.revision, launch.revision, workflow.revision],
                identity: id.into(),
                scope: scope.clone(),
            };
            self.native_state(&task)?;
            Ok(task)
        })
    }

    pub(super) fn load_native_workflow(
        &self,
        scope: &Selection,
        id: &str,
        launch: &Launch,
    ) -> Result<Record<NativeWorkflow>> {
        storage(|| {
            macro_rules! load {
                ($table:ident, $ty:ty, $variant:ident) => {{
                    let saved: Record<$ty> =
                        record(self.write, $table, id)?.ok_or("native workflow is absent")?;
                    Record {
                        conversation: saved.conversation,
                        revision: saved.revision,
                        value: NativeWorkflow::$variant(saved.value),
                    }
                }};
            }
            let saved = match launch {
                Launch::Pi { .. } => load!(PI, crate::agent_model::Workflow, Pi),
                Launch::Codex { .. } => load!(CODEX, crate::codex_model::Workflow, Codex),
                Launch::OpenCode { .. } => {
                    load!(OPENCODE, crate::opencode_model::Workflow, OpenCode)
                }
                _ => return Err("task has no native workflow".into()),
            };
            let source = saved.value.source();
            if saved.conversation != scope.conversation
                || !self.valid_id(id)
                || self
                    .source_in(&scope.conversation, &source.id)?
                    .is_none_or(|known| known.page != source.page)
            {
                return Err("native workflow ownership or source differs".into());
            }
            Ok(saved)
        })
    }

    pub(super) fn native_state(&self, task: &NativeTask) -> Result<Store> {
        let name = workflow_id(&task.launch)?;
        if !task.workflow.matches(&task.launch) {
            return Err("native workflow kind differs".into());
        }
        let mut state = Store {
            schema: crate::STORE_VERSION,
            view: View {
                sequence: self.profile.sequence,
                store: self.profile.store.clone(),
                source: Some(task.workflow.source().clone()),
                runs: vec![task.run.clone()],
            },
            next: self.profile.next,
            starts: BTreeMap::new(),
            launches: BTreeMap::from([(task.run.id.clone(), task.launch.clone())]),
            workflows: BTreeMap::new(),
            codex_workflows: BTreeMap::new(),
            opencode_workflows: BTreeMap::new(),
            browser_actions: BTreeMap::new(),
            providers: self.profile.providers.clone(),
            agent_settings: self.profile.agents.clone(),
        };
        if task.workflow.last_run() != task.run.id {
            let id = task.workflow.last_run();
            let run = self.run(&super::selection(&self.profile), id)?;
            let launch: Record<Launch> =
                record(self.write, LAUNCHES, id)?.ok_or("native workflow last launch is absent")?;
            if launch.conversation != self.profile.selected {
                return Err("native last launch ownership differs".into());
            }
            state.view.runs.push(run);
            state.launches.insert(id.into(), launch.value);
        }
        state.view.runs.sort_by_key(|run| {
            run.id
                .rsplit(':')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        });
        task.workflow.install(name, &mut state);
        state.validate()?;
        Ok(state)
    }

    pub(super) fn project_native(&self, scope: &Selection, id: &str) -> Result<Store> {
        let task = self.native_task(scope, id)?;
        let mut state = self.native_state(&task)?;
        state.view.sequence = self
            .profile
            .sequence
            .checked_add(u64::from(self.changed))
            .ok_or("native view sequence exhausted")?;
        state.view.source = self.selected_source(scope)?;
        state.validate()?;
        Ok(state)
    }

    pub(super) fn save_native_workflow(
        &self,
        scope: &Selection,
        name: &str,
        workflow: &NativeWorkflow,
        revision: u64,
    ) -> Result<()> {
        super::native_choices::update_index(self.write, &self.profile, scope, name, workflow)?;
        macro_rules! save {
            ($table:ident, $value:expr) => {{
                self.write.open_table($table)?.insert(
                    name,
                    encode(&Record {
                        conversation: scope.conversation.clone(),
                        revision,
                        value: $value,
                    })?
                    .as_slice(),
                )?;
            }};
        }
        match workflow {
            NativeWorkflow::Pi(value) => save!(PI, value),
            NativeWorkflow::Codex(value) => save!(CODEX, value),
            NativeWorkflow::OpenCode(value) => save!(OPENCODE, value),
        }
        Ok(())
    }
}
