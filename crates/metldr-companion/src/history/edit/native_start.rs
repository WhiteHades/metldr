use super::{Edit, NativeWorkflow, Selection, storage};
use crate::Result;

impl Edit<'_> {
    pub(crate) fn native_request_workflow(
        &self,
        scope: &Selection,
        request: &str,
    ) -> Result<Option<NativeWorkflow>> {
        let result = (|| {
            let Some((saved, launch)) = self.request_launch_inner(scope, request)? else {
                return Ok(None);
            };
            let name = super::native_task::workflow_id(&launch)
                .map_err(|_| "request identity already names a different task kind")?;
            storage(|| {
                let run = self
                    .saved_run(&saved.run)?
                    .ok_or("saved native start has no run")?;
                let workflow = self.load_native_workflow(scope, name, &launch)?;
                if run.value.source != workflow.value.source().id {
                    return Err("saved native start source differs".into());
                }
                Ok(Some(workflow.value))
            })
        })();
        self.checked(result)
    }

    pub(crate) fn native_workflow(
        &self,
        scope: &Selection,
        workflow: &str,
    ) -> Result<NativeWorkflow> {
        let result = (|| {
            let (run, _) = self.native_latest(scope, workflow)?;
            Ok(self.native_task(scope, &run)?.workflow)
        })();
        self.checked(result)
    }
}
