use super::*;
use crate::{ag_ui::RunOrigin, model::RunView};

impl Core {
    pub(super) async fn referenced_run(
        &self,
        id: &RunId,
        origin: &RunOrigin,
    ) -> Result<RunView, FlowError> {
        self.ensure_current()?;
        let _read = self.begin_read()?;
        let run = match origin {
            RunOrigin::Live => {
                if !self.flow().is_connected() {
                    return Err(FlowError::Disconnected);
                }
                self.page
                    .view
                    .borrow()
                    .runs
                    .iter()
                    .find(|run| &run.id == id)
                    .cloned()
                    .ok_or(FlowError::HistoryStale)?
            }
            RunOrigin::Saved { reference } => {
                if &reference.id != id {
                    return Err(FlowError::HistoryStale);
                }
                let flow = self.flow();
                let context = flow.history_current().await?;
                if context.scope != self.scope {
                    return Err(FlowError::HistoryStale);
                }
                flow.history_run(&context, reference).await?
            }
        };
        self.ensure_current()?;
        Ok(run)
    }
}
