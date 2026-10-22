use crate::{
    Result,
    agent_model::Launch,
    flow::Owner,
    history::NativeWorkflow,
    model::{Run, Status},
};

impl Owner {
    pub(super) fn native_change(
        &mut self,
        run: &str,
        change: impl FnOnce(&mut Run, &mut Launch, &mut NativeWorkflow) -> Result<bool>,
    ) -> Result<bool> {
        if self
            .operation
            .as_ref()
            .is_none_or(|operation| operation.run != run)
        {
            return Err("native callback came from a stale operation".into());
        }
        if self.record_history.is_some() {
            return self.record_change(|edit, scope| {
                let mut task = edit.native_task(scope, run)?;
                if task.run.status != Status::Running
                    || !change(&mut task.run, &mut task.launch, &mut task.workflow)?
                {
                    return Ok((false, None));
                }
                edit.put_native(scope, task)?;
                Ok((true, Some(edit.project(scope, Some(run))?)))
            });
        }
        // shortcut: remove the snapshot branch with the remaining snapshot callers at record cutover.
        let mut next = self.state.clone();
        let mut launch = next
            .launches
            .get(run)
            .ok_or("callback launch is absent")?
            .clone();
        let mut workflow = NativeWorkflow::from_store(&next, &launch)?;
        let task = next
            .view
            .runs
            .iter_mut()
            .find(|task| task.id == run)
            .ok_or("callback task is absent")?;
        if task.status != Status::Running || !change(task, &mut launch, &mut workflow)? {
            return Ok(false);
        }
        let name = match &launch {
            Launch::Pi { workflow, .. }
            | Launch::Codex { workflow, .. }
            | Launch::OpenCode { workflow, .. } => workflow.clone(),
            _ => return Err("callback has no native launch".into()),
        };
        workflow.install(&name, &mut next);
        next.launches.insert(run.into(), launch);
        self.commit(next)?;
        Ok(true)
    }
}
