use super::*;
use crate::browser_action::Selection;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    #[serde(deserialize_with = "Option::deserialize")]
    selected: Option<usize>,
}

impl Actor {
    pub(super) fn prepare_selection(
        &mut self,
        tool: String,
        handle: String,
        option: String,
        job: Job,
    ) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("observe the page before choosing an option")?;
            self.contexts.validate(&snapshot.context)?;
            let control = snapshot.select(&handle)?;
            let choice = control.option(&option)?;
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::selects::read(control.slot, choice.index),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::ReadSelection {
                    tool,
                    handle,
                    option,
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

    pub(super) fn selection_read(
        &mut self,
        tool: String,
        handle: String,
        option: String,
        job: Job,
        value: Value,
    ) -> Result<()> {
        let proposal = (|| -> Result<_> {
            let current: Current = serde_json::from_value(results::evaluation_value(&value)?)?;
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            let control = snapshot.select(&handle)?;
            let selected = control.option(&option)?.clone();
            let previous = control.selected(current.selected)?;
            Ok(Proposal {
                run: self.client.run.clone(),
                tool_use_id: tool,
                binding: self.binding(&snapshot.context),
                url: self.source.url.clone(),
                intent: Intent::SelectOption {
                    selection: Selection {
                        control_id: handle,
                        control_label: control.label.clone(),
                        previous,
                        selected,
                    },
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

    pub(super) fn select_option(&mut self, action: Action, job: Job) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            self.validate_action(&action, &snapshot.context)?;
            let Intent::SelectOption { selection } = &action.proposal.intent else {
                return Err("not a selection action".into());
            };
            let control = snapshot.select(&selection.control_id)?;
            if control.label != selection.control_label
                || control.option(&selection.selected.id)? != &selection.selected
                || control.selected(selection.previous.as_ref().map(|p| p.index))?
                    != selection.previous
            {
                return Err("approved selection does not match its observation".into());
            }
            let script = scripts::selects::write(
                control.slot,
                selection.previous.as_ref().map(|p| p.index),
                selection.selected.index,
            );
            let setup = (snapshot.context.clone(), snapshot.root.clone(), script);
            if self.seen.len() == 64 || !self.seen.insert((action.id.clone(), action.revision)) {
                return Err("browser action was reused or action capacity reached".into());
            }
            Ok(setup)
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::WriteSelection { action, job },
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

    pub(super) fn selection_written(
        &mut self,
        action: Action,
        job: Job,
        value: Value,
    ) -> Result<()> {
        match results::evaluation_value(&value) {
            Ok(Value::Bool(false)) => self.value(
                job,
                Outcome::Prevented {
                    reason: "selection changed before dispatch".into(),
                },
            ),
            Ok(Value::Bool(true)) => {
                let snapshot = self
                    .snapshot
                    .as_ref()
                    .ok_or("browser snapshot disappeared after selection")?;
                let Intent::SelectOption { selection } = &action.proposal.intent else {
                    return Err("invalid selection verification intent".into());
                };
                let control = snapshot.select(&selection.control_id)?;
                let context = snapshot.context.clone();
                let root = snapshot.root.clone();
                let script = scripts::selects::verify(control.slot, selection.selected.index);
                self.queue(
                    "Runtime.callFunctionOn",
                    session(&context),
                    call_params(&context, &root, script),
                    Work::VerifySelection { action, job },
                    Some(context),
                    Some(root),
                )
            }
            _ => self.value(
                job,
                Outcome::Uncertain {
                    reason: "selection dispatch result was lost".into(),
                },
            ),
        }
    }

    pub(super) fn selection_verified(
        &mut self,
        action: Action,
        job: Job,
        value: Value,
    ) -> Result<()> {
        let Intent::SelectOption { selection } = action.proposal.intent else {
            return Err("invalid selection verification intent".into());
        };
        self.value(
            job,
            if results::evaluation_value(&value).is_ok_and(|v| v == Value::Bool(true)) {
                Outcome::SelectionObserved {
                    selected: selection.selected,
                }
            } else {
                Outcome::Uncertain {
                    reason: "selection could not be observed after dispatch".into(),
                }
            },
        )
    }
}
