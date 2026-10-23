use super::super::{ControlPreparation, observation::NativeState};
use super::*;
use crate::browser_action::ControlReceipt;

impl Actor {
    pub(super) fn prepare_control(
        &mut self,
        tool: String,
        handle: String,
        checked: bool,
        job: Job,
    ) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("observe the page before changing a native control")?;
            self.contexts.validate(&snapshot.context)?;
            let control = snapshot.control(&handle)?;
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::controls::read(control.slot),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::ReadControl {
                    tool,
                    handle,
                    checked,
                    job,
                },
                Some(context),
                Some(root),
            ),
            Err(error) => {
                self.finish(job, Err(error));
                Ok(())
            }
        }
    }

    pub(super) fn control_read(
        &mut self,
        tool: String,
        handle: String,
        checked: bool,
        job: Job,
        value: Value,
    ) -> Result<()> {
        let prepared = (|| -> Result<ControlPreparation> {
            let state: Option<NativeState> =
                serde_json::from_value(results::evaluation_value(&value)?)?;
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            let observed = snapshot.control(&handle)?;
            let state = state
                .filter(|state| state == &observed.state)
                .ok_or("native control changed before proposal")?;
            let Some(control) = observed.proposal(handle, checked)? else {
                return Ok(ControlPreparation::NoOp { state });
            };
            let proposal = Proposal {
                run: self.client.run.clone(),
                tool_use_id: tool,
                url: self.source.url.clone(),
                binding: self.binding(&snapshot.context),
                intent: Intent::SetNativeControl { control },
            };
            proposal.validate()?;
            Ok(ControlPreparation::Propose { proposal })
        })();
        match prepared {
            Ok(prepared) => self.value(job, prepared),
            Err(error) => {
                self.finish(job, Err(error));
                Ok(())
            }
        }
    }

    pub(super) fn arm_control(&mut self, action: Action, job: Job) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            self.validate_action(&action, &snapshot.context)?;
            let Intent::SetNativeControl { control } = &action.proposal.intent else {
                return Err("not a native control action".into());
            };
            let (handle, desired) = match control {
                crate::browser_action::Control::Checkbox {
                    control_id,
                    checked,
                    ..
                } => (control_id, *checked),
                crate::browser_action::Control::Radio { control_id, .. } => (control_id, true),
            };
            let observed = snapshot.control(handle)?;
            if observed.proposal(handle.clone(), desired)?.as_ref() != Some(control) {
                return Err("approved native control differs from the observed control".into());
            }
            if self.seen.len() == 64 || !self.seen.insert((action.id.clone(), action.revision)) {
                return Err("browser action was reused or action capacity reached".into());
            }
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::controls::arm(observed.slot, control),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::ArmControl { action, job },
                Some(context),
                Some(root),
            ),
            Err(error) => self.value(
                job,
                Outcome::Prevented {
                    reason: error.to_string(),
                },
            ),
        }
    }

    pub(super) fn control_gestured(&mut self, action: Action, job: Job) -> Result<()> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or("native control snapshot disappeared after input")?;
        let context = snapshot.context.clone();
        let root = snapshot.root.clone();
        self.queue(
            "Runtime.callFunctionOn",
            session(&context),
            call_params(&context, &root, scripts::controls::receipt()),
            Work::ControlReceipt { action, job },
            Some(context),
            Some(root),
        )
    }

    pub(super) fn control_receipt(&mut self, action: Action, job: Job, value: Value) -> Result<()> {
        let receipt = results::evaluation_value(&value)
            .ok()
            .and_then(|value| serde_json::from_value::<Option<ControlReceipt>>(value).ok())
            .flatten();
        let valid = match (&action.proposal.intent, receipt) {
            (Intent::SetNativeControl { control }, Some(receipt)) if control.matches(&receipt) => {
                Some(receipt)
            }
            _ => None,
        };
        self.value(
            job,
            match valid {
                Some(receipt) => Outcome::ControlObserved { receipt },
                None => Outcome::Uncertain {
                    reason: "approved native control state could not be verified".into(),
                },
            },
        )
    }
}
