use super::{Edit, NativeWorkflow, Selection, native_task::workflow_id, record, storage};
use crate::{Result, agent_model::Launch, agent_settings::Kind, history::schema::*, model::Status};
use redb::{ReadableTable, ReadableTableMetadata};
use std::ops::Bound;

impl Edit<'_> {
    pub(crate) fn recover(&mut self, scope: &Selection) -> Result<()> {
        let result = (|| {
            self.require_selection(scope)?;
            loop {
                let next = storage(|| {
                    let table = self.write.open_table(ACTIVE_RUNS)?;
                    Ok(table
                        .iter()?
                        .next()
                        .transpose()?
                        .map(|(id, _)| id.value().to_owned()))
                })?;
                let Some(id) = next else { break };
                let mut run = self.run(scope, &id)?;
                if let Some(action) = self.browser_open(scope, &id)?
                    && let Some(next) = action.finish(
                        "native host restarted before dispatch",
                        "native host restarted after dispatch",
                    )?
                {
                    self.put_browser(scope, next)?;
                }
                let launch: Record<Launch> = storage(|| {
                    record(self.write, LAUNCHES, &id)?.ok_or("recovery launch is absent".into())
                })?;
                if matches!(
                    launch.value,
                    Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
                ) {
                    let mut task = self.native_task(scope, &id)?;
                    task.run.status = Status::Interrupted;
                    task.run.error =
                        Some("native host restarted; native session ownership is unknown".into());
                    block(&mut task.workflow);
                    self.put_native(scope, task)?;
                } else {
                    run.status = Status::Interrupted;
                    run.error = Some("native host restarted".into());
                    self.put_run(scope, run)?;
                }
            }
            storage(|| {
                if self.write.open_table(OPEN_ACTIONS)?.len()? != 0 {
                    return Err("recovery left an open browser action".into());
                }
                Ok(())
            })?;

            for (table, kind) in [
                (PI, Kind::Pi),
                (CODEX, Kind::Codex),
                (OPENCODE, Kind::Opencode),
            ] {
                let mut cursor: Option<String> = None;
                loop {
                    let next = storage(|| {
                        let table = self.write.open_table(table)?;
                        let entry = match cursor.as_deref() {
                            Some(after) => table
                                .range::<&str>((Bound::Excluded(after), Bound::Unbounded))?
                                .next()
                                .transpose()?,
                            None => table.iter()?.next().transpose()?,
                        };
                        Ok(entry.map(|(id, _)| id.value().to_owned()))
                    })?;
                    let Some(id) = next else { break };
                    let owner = storage(|| {
                        let saved = record::<serde::de::IgnoredAny>(self.write, table, &id)?
                            .ok_or("recovery workflow is absent")?;
                        self.saved_conversation(&saved.conversation)?;
                        Ok(saved.conversation)
                    })?;
                    if owner != scope.conversation {
                        cursor = Some(id);
                        continue;
                    }
                    let (latest, _) = self.native_latest(scope, &id)?;
                    let mut task = self.native_task(scope, &latest)?;
                    storage(|| {
                        if workflow_id(&task.launch)? != id
                            || task.workflow.last_run() != latest
                            || !matches!(
                                (kind, &task.workflow),
                                (Kind::Pi, NativeWorkflow::Pi(_))
                                    | (Kind::Codex, NativeWorkflow::Codex(_))
                                    | (Kind::Opencode, NativeWorkflow::OpenCode(_))
                            )
                        {
                            return Err("recovery workflow head or kind differs".into());
                        }
                        Ok(())
                    })?;
                    if task.run.status == Status::Interrupted
                        && (task.workflow.pending() || task.workflow.bound())
                    {
                        block(&mut task.workflow);
                        self.put_native(scope, task)?;
                    }
                    cursor = Some(id);
                }
            }

            let (providers, agents) = self.settings(scope)?;
            if providers.pending.is_some() {
                let mut next = providers.clone();
                let pending = next
                    .pending
                    .take()
                    .ok_or("pending provider save disappeared")?;
                next.retired.push(pending.connection.credential);
                self.replace_settings(scope, &providers, &agents, next, agents.clone())?;
            }
            Ok(())
        })();
        self.checked(result)
    }
}

fn block(workflow: &mut NativeWorkflow) {
    match workflow {
        NativeWorkflow::Pi(value) => value.binding.block_recovery(),
        NativeWorkflow::Codex(value) => value.binding.block_recovery(),
        NativeWorkflow::OpenCode(value) => value.binding.block_recovery(),
    }
}
