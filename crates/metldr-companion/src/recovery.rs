use crate::{
    Result,
    agent_model::Launch,
    model::{Status, Store},
};

pub(super) fn interrupt(state: &mut Store) -> Result<()> {
    let mut changed = state.interrupt_browser()?;
    for run in &mut state.view.runs {
        if let Some(Launch::OpenCode { workflow, .. }) = state.launches.get(&run.id) {
            if run.status.terminal() && run.status != Status::Interrupted {
                continue;
            }
            let binding = &mut state
                .opencode_workflows
                .get_mut(workflow)
                .ok_or("missing interrupted OpenCode workflow")?
                .binding;
            if run.status == Status::Interrupted
                && matches!(
                    binding,
                    crate::opencode_model::Binding::ReleaseUnknown { .. }
                )
            {
                continue;
            }
            run.status = Status::Interrupted;
            run.error = Some("native host restarted; native session ownership is unknown".into());
            binding.block_recovery();
            changed = true;
            continue;
        }
        if let Some(Launch::Codex { workflow, .. }) = state.launches.get(&run.id) {
            if run.status.terminal() && run.status != Status::Interrupted {
                continue;
            }
            let binding = &mut state
                .codex_workflows
                .get_mut(workflow)
                .ok_or("missing interrupted Codex workflow")?
                .binding;
            if run.status == Status::Interrupted
                && matches!(binding, crate::codex_model::Binding::ReleaseUnknown { .. })
            {
                continue;
            }
            run.status = Status::Interrupted;
            run.error = Some("native host restarted; native session ownership is unknown".into());
            binding.block_recovery();
            changed = true;
            continue;
        }
        let workflow = match state.launches.get(&run.id) {
            Some(Launch::Pi { workflow, .. }) => Some(workflow),
            _ => None,
        };
        if run.status.terminal() && (run.status != Status::Interrupted || workflow.is_none()) {
            continue;
        }
        if run.status == Status::Interrupted
            && workflow.is_some_and(|id| {
                state.workflows.get(id).is_some_and(|w| {
                    matches!(
                        w.binding,
                        crate::agent_model::Binding::ReleaseUnknown { .. }
                    )
                })
            })
        {
            continue;
        }
        run.status = Status::Interrupted;
        run.error = Some("native host restarted".into());
        if let Some(workflow) = workflow {
            state
                .workflows
                .get_mut(workflow)
                .ok_or("missing interrupted Pi workflow")?
                .binding
                .block_recovery();
            run.error = Some("native host restarted; native session ownership is unknown".into());
        }
        changed = true;
    }
    changed |= state.seal_outputs()?;
    if changed {
        state.view.sequence = state
            .view
            .sequence
            .checked_add(1)
            .ok_or("view sequence capacity reached")?;
    }
    Ok(())
}
