use super::{Edit, Selection, record, storage};
use crate::{
    Result, agent_model::Launch, browser_action::Target, history::schema::*, model::Source,
};
use redb::ReadableTable;

impl Edit<'_> {
    pub(crate) fn native_initial_target(&self, expected: &Selection, run: &str) -> Result<Target> {
        let result = (|| {
            self.require_selection(expected)?;
            if self.run(expected, run)?.kind != "agent" {
                return Err("browser target requires a native agent run".into());
            }
            storage(|| {
                let (saved, source) = self.native_target_record(expected, run)?;
                let number = run
                    .rsplit(':')
                    .next()
                    .ok_or("invalid native run identity")?
                    .parse::<u64>()?;
                let prior = {
                    let table = self.write.open_table(WORKFLOW_RUNS)?;
                    table
                        .range(
                            (saved.value.workflow.as_str(), 0)
                                ..(saved.value.workflow.as_str(), number),
                        )?
                        .next_back()
                        .transpose()?
                        .map(|(key, id)| (key.value().1, id.value().to_owned()))
                };
                let initial = match prior {
                    Some((number, prior)) => {
                        let (previous, _) = self.native_target_record(expected, &prior)?;
                        if previous.value.workflow != saved.value.workflow
                            || prior.rsplit(':').next().unwrap().parse::<u64>()? != number
                        {
                            return Err("preceding native workflow index differs".into());
                        }
                        previous.value.current
                    }
                    None => Target::from(&source.page),
                };
                if saved.value.initial != initial {
                    return Err("native initial target differs from preceding destination".into());
                }
                Ok(initial)
            })
        })();
        self.checked(result)
    }

    pub(crate) fn native_latest(
        &self,
        expected: &Selection,
        workflow: &str,
    ) -> Result<(String, Target)> {
        let result = (|| {
            self.require_selection(expected)?;
            let entry = storage(|| {
                let table = self.write.open_table(WORKFLOW_RUNS)?;
                Ok(table
                    .range((workflow, 0)..=(workflow, u64::MAX))?
                    .next_back()
                    .transpose()?
                    .map(|(key, id)| (key.value().1, id.value().to_owned())))
            })?;
            let Some((number, id)) = entry else {
                let exists = storage(|| {
                    Ok(self.write.open_table(PI)?.get(workflow)?.is_some()
                        || self.write.open_table(CODEX)?.get(workflow)?.is_some()
                        || self.write.open_table(OPENCODE)?.get(workflow)?.is_some())
                })?;
                return if exists {
                    storage(|| Err("native workflow order is absent".into()))
                } else {
                    Err("unknown native workflow".into())
                };
            };
            let run = storage(|| {
                self.saved_run(&id)?
                    .ok_or_else(|| "indexed native run is absent".into())
            })?;
            if run.conversation != expected.conversation {
                return Err("workflow belongs to a different conversation".into());
            }
            storage(|| {
                if id.rsplit(':').next().unwrap().parse::<u64>()? != number {
                    return Err("latest native run order differs".into());
                }
                self.native_initial_target(expected, &id)?;
                let (saved, _) = self.native_target_record(expected, &id)?;
                if saved.value.workflow != workflow {
                    return Err("native workflow index differs".into());
                }
                Ok((id, saved.value.current))
            })
        })();
        self.checked(result)
    }

    fn native_target_record(
        &self,
        expected: &Selection,
        id: &str,
    ) -> Result<(Record<NativeTarget>, Source)> {
        let run = self.saved_run(id)?.ok_or("native target run is absent")?;
        let launch: Record<Launch> =
            record(self.write, LAUNCHES, id)?.ok_or("native target launch is absent")?;
        let target: Record<NativeTarget> =
            record(self.write, NATIVE_TARGETS, id)?.ok_or("native target is absent")?;
        if run.conversation != expected.conversation
            || launch.conversation != run.conversation
            || target.conversation != run.conversation
        {
            return Err("native target ownership differs".into());
        }
        target.value.initial.validate()?;
        target.value.current.validate()?;
        let head = self
            .write
            .open_table(NAVIGATION_HEADS)?
            .get(id)?
            .map(|head| head.value().to_owned());
        if target.value.navigation != head {
            return Err("native target navigation head differs".into());
        }
        if let Some(navigation) = &target.value.navigation {
            if !self.valid_id(navigation) {
                return Err("invalid native target navigation identity".into());
            }
            let action: Record<crate::browser_action::Action> =
                record(self.write, ACTIONS, navigation)?
                    .ok_or("native target navigation is absent")?;
            action.value.validate()?;
            if action.conversation != expected.conversation
                || action.value.id != *navigation
                || action.value.proposal.run != id
            {
                return Err("native target navigation ownership differs".into());
            }
            match action.value.state {
                crate::browser_action::Phase::Settled {
                    outcome: crate::browser_action::Outcome::Navigated { binding, url, .. },
                } if target.value.current.tab == binding.tab
                    && target.value.current.document == binding.document
                    && target.value.current.url == url => {}
                _ => return Err("native target differs from verified navigation".into()),
            }
        } else if target.value.current != target.value.initial {
            return Err("native destination changed without navigation".into());
        }
        let task = self.native_task_data(expected, id)?;
        if super::native_task::workflow_id(&task.launch)? != target.value.workflow {
            return Err("native target workflow differs".into());
        }
        let source = task.workflow.source().clone();
        let number = id
            .rsplit(':')
            .next()
            .ok_or("invalid native target identity")?
            .parse::<u64>()?;
        if self
            .write
            .open_table(WORKFLOW_RUNS)?
            .get((target.value.workflow.as_str(), number))?
            .is_none_or(|saved| saved.value() != id)
        {
            return Err("native target order differs".into());
        }
        Ok((target, source))
    }
}
