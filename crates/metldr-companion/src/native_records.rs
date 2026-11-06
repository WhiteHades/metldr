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
        if !self.operations.contains_key(run) {
            return Err("native callback came from a stale operation".into());
        }
        self.record_execution(run, |edit, execution| {
            let mut task = edit.native_task(execution, run)?;
            if task.run.status != Status::Running
                || !change(&mut task.run, &mut task.launch, &mut task.workflow)?
            {
                return Ok(false);
            }
            edit.put_native(execution, task)?;
            Ok(true)
        })
    }
}
