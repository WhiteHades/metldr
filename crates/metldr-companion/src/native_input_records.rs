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
    pub(super) fn native_input_active(&mut self, run: &str, kind: Kind) -> Result<bool> {
        if self
            .operation
            .as_ref()
            .is_none_or(|op| op.stop.load(Ordering::Acquire))
        {
            return Ok(false);
        }
        Ok(self.native_input_status(run, kind)? == Some(Status::Running))
    }

    pub(super) fn native_input_status(&mut self, run: &str, kind: Kind) -> Result<Option<Status>> {
        let result: Result<Option<Status>> = (|| {
            if self.operation.as_ref().is_none_or(|op| op.run != run) {
                return Ok(None);
            }
            let missing = !self.state.view.runs.iter().any(|saved| saved.id == run);
            if let Some(history) = &mut self.record_history {
                let scope = history.selection()?;
                let (status, projection) = history.edit(|edit| {
                    let task = edit.native_task(&scope, run)?;
                    let status = (!task.run.status.terminal() && matches_kind(&task.launch, kind))
                        .then_some(task.run.status);
                    let projection = if missing && status.is_some() {
                        let state = edit.project(&scope, Some(run))?;
                        let runs: Vec<_> =
                            state.view.runs.iter().map(|run| run.id.as_str()).collect();
                        let actions = edit.browser_view(&scope, &runs)?;
                        Some((state, actions))
                    } else {
                        None
                    };
                    Ok((status, projection))
                })?;
                if let Some((state, actions)) = projection {
                    self.state = state;
                    self.browser_view = actions;
                }
                return Ok(status);
            }
            Ok(self
                .state
                .view
                .runs
                .iter()
                .find(|r| r.id == run && !r.status.terminal())
                .filter(|_| {
                    self.state
                        .launches
                        .get(run)
                        .is_some_and(|launch| matches_kind(launch, kind))
                })
                .map(|r| r.status))
        })();
        if result.as_ref().is_err_and(|error| fatal(error.as_ref())) {
            self.abort_native_inputs(run);
        }
        result
    }

    pub(super) fn commit_native_input(&mut self, run: &str, kind: Kind, next: Store) -> Result<()> {
        let result = (|| {
            if self.record_history.is_none() {
                return self.commit(next);
            }
            let reservation = if next.next == self.state.next {
                None
            } else if self.state.next.checked_add(1) == Some(next.next) {
                Some(format!("{}:{}", self.state.view.store, self.state.next))
            } else {
                return Err("native input changed the identity counter unexpectedly".into());
            };
            if next.view.store != self.state.view.store {
                return Err("native input changed store identity".into());
            }
            let status = next
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .map(|run| run.status);
            self.record_change(|edit, scope| {
                let task = edit.native_task(scope, run)?;
                if task.run.status.terminal()
                    || status.is_some_and(|status| task.run.status != status)
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
                Ok(((), Some(edit.refresh_view(scope, Some(run))?)))
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
        let Some(op) = self.operation.as_ref().filter(|op| op.run == run) else {
            return Ok(());
        };
        op.stop.store(true, Ordering::Release);
        let pending = self.detach_inputs(run);
        let saved = self.persist_records(|edit, scope| {
            edit.prevent_browser(scope, run, reason)?;
            let mut task = edit.native_task(scope, run)?;
            if !task.run.status.terminal() {
                task.run.status = Status::StopRequested;
                if let Some(error) = error {
                    task.run.error = Some(error.chars().take(200).collect());
                }
                edit.put_native(scope, task)?;
            }
            Ok(((), Some(edit.refresh_view(scope, Some(run))?)))
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
        if let Some(op) = self.operation.as_ref().filter(|op| op.run == run) {
            op.stop.store(true, Ordering::Release);
        }
        let pending = self.detach_inputs(run);
        self.cancel_browser_approval(run);
        self.cancel_inputs(run, pending);
    }
}
