use crate::{
    Result,
    agent_model::Launch,
    agent_settings::Kind,
    flow::Owner,
    model::{Status, Store},
};
use std::sync::atomic::Ordering;

pub(super) fn fatal(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<std::io::Error>().is_some() || crate::history::is_failure(error)
}

fn matches_kind(launch: &Launch, kind: Kind) -> bool {
    matches!(
        (launch, kind),
        (Launch::Pi { .. }, Kind::Pi)
            | (Launch::Codex { .. }, Kind::Codex)
            | (Launch::OpenCode { .. }, Kind::Opencode)
    )
}

impl Owner {
    pub(super) fn native_input_state(&mut self, run: &str) -> Result<Store> {
        self.record_history.edit(|edit| {
            let execution = edit.execution(run)?;
            edit.project_execution(&execution)
        })
    }

    pub(super) fn native_input_active(&mut self, run: &str, kind: Kind) -> Result<bool> {
        if self
            .operations
            .get(run)
            .is_none_or(|op| op.stop.load(Ordering::Acquire))
        {
            return Ok(false);
        }
        Ok(self.native_input_status(run, kind)? == Some(Status::Running))
    }

    pub(super) fn native_input_status(&mut self, run: &str, kind: Kind) -> Result<Option<Status>> {
        let result: Result<Option<Status>> = (|| {
            if !self.operations.contains_key(run) {
                return Ok(None);
            }
            self.record_history.edit(|edit| {
                let execution = edit.execution(run)?;
                let task = edit.native_task(&execution, run)?;
                Ok(
                    (!task.run.status.terminal() && matches_kind(&task.launch, kind))
                        .then_some(task.run.status),
                )
            })
        })();
        if result.as_ref().is_err_and(|error| fatal(error.as_ref())) {
            self.abort_native_inputs(run);
        }
        result
    }

    pub(super) fn commit_native_input(&mut self, run: &str, kind: Kind, next: Store) -> Result<()> {
        let result = (|| {
            let status = next
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .map(|run| run.status)
                .ok_or("native input has no matching run")?;
            self.record_execution(run, |edit, scope| {
                let counter = edit.next_counter();
                let reservation = if next.next == counter {
                    None
                } else if counter.checked_add(1) == Some(next.next) {
                    Some(format!("{}:{counter}", scope.store()))
                } else {
                    return Err("native input changed the identity counter unexpectedly".into());
                };
                if next.view.store != scope.store() {
                    return Err("native input changed store identity".into());
                }
                let task = edit.native_task(scope, run)?;
                if task.run.status.terminal()
                    || task.run.status != status
                    || !matches_kind(&task.launch, kind)
                    || (reservation.is_some() && task.run.status != Status::Running)
                {
                    return Err("native input task is stale or has a different route".into());
                }
                if let Some(expected) = reservation
                    && edit.id()? != expected
                {
                    return Err("native input identity reservation is stale".into());
                }
                edit.refresh_execution(scope)
            })
        })();
        if result.as_ref().is_err_and(|error| fatal(error.as_ref())) {
            self.abort_native_inputs(run);
        }
        result
    }

    pub(super) fn stop_native_record_inputs(
        &mut self,
        run: &str,
        reason: &str,
        error: Option<&str>,
        publish: bool,
    ) -> Result<()> {
        let Some(op) = self.operations.get(run) else {
            return Ok(());
        };
        op.stop.store(true, Ordering::Release);
        let pending = self.detach_inputs(run);
        let saved = self.persist_execution(run, |edit, scope| {
            edit.prevent_browser(scope, run, reason)?;
            let mut task = edit.native_task(scope, run)?;
            if !task.run.status.terminal() {
                task.run.status = Status::StopRequested;
                if let Some(error) = error {
                    task.run.error = Some(error.chars().take(200).collect());
                }
                edit.put_native(scope, task)?;
            }
            edit.refresh_execution(scope)
        });
        let (_, changed) = match saved {
            Ok(saved) => saved,
            Err(error) => {
                self.restore_inputs(pending);
                if fatal(error.as_ref()) {
                    self.abort_native_inputs(run);
                }
                return Err(error);
            }
        };
        let result = if publish && changed {
            self.publish()
        } else {
            Ok(())
        };
        self.cancel_browser_approval(run);
        self.cancel_inputs(run, pending);
        result
    }

    pub(super) fn abort_native_inputs(&mut self, run: &str) {
        if let Some(op) = self.operations.get(run) {
            op.stop.store(true, Ordering::Release);
        }
        let pending = self.detach_inputs(run);
        self.cancel_browser_approval(run);
        self.cancel_inputs(run, pending);
    }
}
