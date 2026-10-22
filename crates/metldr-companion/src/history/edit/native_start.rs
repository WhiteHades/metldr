use super::{Edit, NativeTask, NativeWorkflow, Selection, storage};
use crate::{Result, history::schema::*, model::valid_request_id};
use redb::ReadableTable;

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

    pub(crate) fn native_request(
        &self,
        scope: &Selection,
        request: &str,
    ) -> Result<Option<NativeTask>> {
        let result = (|| {
            self.require_selection(scope)?;
            if !valid_request_id(request) {
                return Err("invalid request identity".into());
            }
            self.require_run_request(request)?;
            let saved = storage(|| {
                self.write
                    .open_table(STARTS)?
                    .get(request)?
                    .map(|bytes| decode::<Start>(bytes.value()))
                    .transpose()
            })?;
            let Some(saved) = saved else { return Ok(None) };
            storage(|| {
                if let ReceiptFormat::Scoped { epoch, digest } = &saved.format
                    && (*epoch == 0
                        || digest.len() != 64
                        || !digest.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err("invalid saved scoped receipt".into());
                }
                Ok(())
            })?;
            if saved.conversation != scope.conversation
                || matches!(saved.format, ReceiptFormat::Scoped { epoch, .. } if epoch != scope.epoch)
            {
                return Err("request identity already names a different scope".into());
            }
            Ok(Some(self.native_task(scope, &saved.run)?))
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
