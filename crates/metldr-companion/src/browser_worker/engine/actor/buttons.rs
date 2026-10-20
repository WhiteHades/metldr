use super::*;
use chromiumoxide::cdp::browser_protocol::input::{GestureSourceType, SynthesizeTapGestureParams};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    x: f64,
    y: f64,
}

impl Actor {
    pub(super) fn prepare_button(&mut self, tool: String, handle: String, job: Job) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("observe the page before activating a button")?;
            self.contexts.validate(&snapshot.context)?;
            let button = snapshot.button(&handle)?;
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::check_button(button.slot, &button.check_label),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::ReadButton { tool, handle, job },
                Some(context),
                Some(root),
            ),
            Err(error) => {
                self.finish(job, Err(error));
                Ok(())
            }
        }
    }

    pub(super) fn arm_button(&mut self, action: Action, job: Job) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            self.validate_action(&action, &snapshot.context)?;
            let Intent::ActivateButton {
                button_id,
                button_label,
            } = &action.proposal.intent
            else {
                return Err("not a button action".into());
            };
            let button = snapshot.button(button_id)?;
            if &button.label != button_label {
                return Err("approved button label changed".into());
            }
            if self.seen.len() == 64 || !self.seen.insert((action.id.clone(), action.revision)) {
                return Err("browser action was reused or action capacity reached".into());
            }
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::arm_button(button.slot, &button.check_label),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::ArmButton { action, job },
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

    pub(super) fn button_read(
        &mut self,
        tool: String,
        handle: String,
        job: Job,
        value: Value,
    ) -> Result<()> {
        let proposal = (|| -> Result<Proposal> {
            let label: Option<String> = serde_json::from_value(results::evaluation_value(&value)?)?;
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            let button = snapshot.button(&handle)?;
            if label.as_deref() != Some(&button.check_label) {
                return Err("button changed before proposal".into());
            }
            Ok(Proposal {
                run: self.client.run.clone(),
                tool_use_id: tool,
                url: self.source.url.clone(),
                binding: self.binding(&snapshot.context),
                intent: Intent::ActivateButton {
                    button_id: handle,
                    button_label: button.label.clone(),
                },
            })
        })();
        match proposal {
            Ok(proposal) => self.value(job, proposal),
            Err(error) => {
                self.finish(job, Err(error));
                Ok(())
            }
        }
    }

    pub(super) fn button_armed(&mut self, action: Action, job: Job, value: Value) -> Result<()> {
        let point: Option<Point> = serde_json::from_value(results::evaluation_value(&value)?)?;
        let Some(point) = point.filter(|p| {
            p.x.is_finite()
                && p.y.is_finite()
                && (0.0..=100_000.0).contains(&p.x)
                && (0.0..=100_000.0).contains(&p.y)
        }) else {
            return self.value(
                job,
                Outcome::Prevented {
                    reason: "button changed or is covered before dispatch".into(),
                },
            );
        };
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or("button snapshot disappeared")?;
        let context = snapshot.context.clone();
        let root = snapshot.root.clone();
        let params = SynthesizeTapGestureParams::builder()
            .x(point.x)
            .y(point.y)
            .duration(50)
            .tap_count(1)
            .gesture_source_type(GestureSourceType::Mouse)
            .build()?;
        self.queue(
            SynthesizeTapGestureParams::IDENTIFIER,
            session(&context),
            serde_json::to_value(params)?,
            Work::Gesture { action, job },
            Some(context),
            Some(root),
        )
    }

    pub(super) fn button_gestured(&mut self, job: Job) -> Result<()> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or("button snapshot disappeared after input")?;
        let context = snapshot.context.clone();
        let root = snapshot.root.clone();
        self.queue(
            "Runtime.callFunctionOn",
            session(&context),
            call_params(&context, &root, scripts::button_receipt()),
            Work::ButtonReceipt { job },
            Some(context),
            Some(root),
        )
    }
}
