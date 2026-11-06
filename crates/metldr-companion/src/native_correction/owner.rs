use super::{Command, LIMIT, Phase, Receipt, Target};
use crate::{Result, flow::Owner, history::Selection, model::Status};
use serde_json::json;
use std::sync::{atomic::Ordering, mpsc::SyncSender};

impl Owner {
    pub(crate) fn correct_native(
        &mut self,
        request: &str,
        scope: &Selection,
        run: &str,
        text: &str,
    ) -> Result<()> {
        let proposed = Receipt::pending(request, scope.epoch, text)?;
        let channel = self
            .operations
            .get(run)
            .filter(|op| !op.stop.load(Ordering::Acquire))
            .and_then(|op| op.correction.as_ref())
            .and_then(|slot| {
                slot.ready
                    .as_ref()
                    .map(|target| (target.clone(), slot.send.clone()))
            });
        let ((mut receipt, ordinal, admitted), changed) =
            self.persist_records(|edit, current| {
                if current != scope {
                    return Err("correction selection changed".into());
                }
                let execution = edit.current_execution(scope, run)?;
                let mut task = edit.native_task(&execution, run)?;
                if let Some((index, saved)) = task
                    .run
                    .corrections
                    .iter()
                    .enumerate()
                    .find(|(_, saved)| saved.request == request)
                {
                    if saved.epoch != proposed.epoch || saved.text != proposed.text {
                        return Err(
                            "correction identity already names a different input or selection"
                                .into(),
                        );
                    }
                    return Ok(((saved.clone(), index + 1, false), None));
                }
                let (target, _) = channel
                    .as_ref()
                    .ok_or("this native run cannot accept a correction")?;
                if task.run.status != Status::Running
                    || !target.matches(&task.launch)
                    || task.run.corrections.len() >= LIMIT
                    || task
                        .run
                        .corrections
                        .last()
                        .is_some_and(|r| r.state.unresolved())
                {
                    return Err(
                        "native correction is stopped, pending, full or has a different route"
                            .into(),
                    );
                }
                task.run.corrections.push(proposed.clone());
                let ordinal = task.run.corrections.len();
                edit.put_native(&execution, task)?;
                Ok((
                    (proposed.clone(), ordinal, true),
                    Some(edit.refresh_view(scope, Some(run))?),
                ))
            })?;
        if changed {
            self.publish()?;
        }
        if admitted {
            let (target, send) = channel.ok_or("admitted correction channel disappeared")?;
            if send
                .try_send(Command {
                    request: request.into(),
                    text: text.into(),
                    target,
                })
                .is_err()
            {
                if let Some(slot) = self
                    .operations
                    .get_mut(run)
                    .and_then(|op| op.correction.as_mut())
                {
                    slot.ready = None;
                }
                receipt.state = Phase::NotSent {
                    message: "The native worker could not receive the correction.".into(),
                };
                self.correction_finished(run, request, receipt.state.clone())?;
            }
        }
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"correction",
            "scope":crate::history_io::Scope::from(scope),"run":run,"receipt":receipt.public(ordinal,false)}))
    }

    pub(crate) fn correction_ready(&mut self, run: &str, target: Target) -> Result<()> {
        if !self.native_input_active(run, target.kind())? {
            return Ok(());
        }
        let slot = self
            .operations
            .get_mut(run)
            .and_then(|op| op.correction.as_mut())
            .ok_or("native correction slot is absent")?;
        if slot.ready.as_ref() == Some(&target) {
            return Ok(());
        }
        slot.ready = Some(target);
        self.record_execution(run, |edit, execution| edit.refresh_execution(execution))
    }

    pub(crate) fn correction_dispatch(
        &mut self,
        run: &str,
        request: &str,
        target: Target,
        ack: SyncSender<bool>,
    ) -> Result<()> {
        let ready = self
            .operations
            .get(run)
            .filter(|op| !op.stop.load(Ordering::Acquire))
            .and_then(|op| op.correction.as_ref())
            .is_some_and(|slot| slot.ready.as_ref() == Some(&target));
        let result = if ready {
            self.record_execution(run, |edit, execution| {
                let mut task = edit.native_task(execution, run)?;
                if task.run.status != Status::Running || !target.matches(&task.launch) {
                    return Ok(false);
                }
                let receipt = task
                    .run
                    .corrections
                    .iter_mut()
                    .find(|r| r.request == request)
                    .ok_or("correction dispatch has no admission")?;
                if receipt.state != Phase::Pending {
                    return Ok(false);
                }
                receipt.resolve(Phase::Dispatching)?;
                edit.put_native(execution, task)?;
                Ok(true)
            })
        } else {
            Ok(false)
        };
        let _ = ack.send(result.as_ref().is_ok_and(|allowed| *allowed));
        result.map(|_| ())
    }

    pub(crate) fn correction_finished(
        &mut self,
        run: &str,
        request: &str,
        outcome: Phase,
    ) -> Result<()> {
        if !self.operations.contains_key(run) || outcome.unresolved() {
            return Err("correction outcome has no matching owned run".into());
        }
        self.record_execution(run, |edit, execution| {
            let mut task = edit.native_task(execution, run)?;
            let receipt = task
                .run
                .corrections
                .iter_mut()
                .find(|r| r.request == request)
                .ok_or("correction outcome has no saved admission")?;
            if receipt.state == outcome {
                return Ok(());
            }
            receipt.resolve(outcome)?;
            edit.put_native(execution, task)?;
            Ok(())
        })
    }

    pub(crate) fn correction_closed(&mut self, run: &str) -> Result<()> {
        let slot = self
            .operations
            .get_mut(run)
            .and_then(|op| op.correction.as_mut());
        if slot.is_none_or(|slot| slot.ready.take().is_none()) {
            return Ok(());
        }
        self.record_execution(run, |edit, execution| edit.refresh_execution(execution))
    }
}
