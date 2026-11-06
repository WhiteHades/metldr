use super::*;

struct PendingRun {
    core: Rc<Core>,
    ui: String,
}

impl Drop for PendingRun {
    fn drop(&mut self) {
        let changed = self.core.owner.borrow_mut().abandon(&self.ui);
        if changed {
            self.core.page.notify_settings();
        }
    }
}

impl Core {
    pub(super) fn run(
        self: &Rc<Self>,
        request: UiRunRequest,
        mut emit: impl FnMut(Event) -> Result<(), FlowError> + 'static,
    ) -> Result<impl Future<Output = Result<(), FlowError>> + 'static, FlowError> {
        self.ensure_current()?;
        if request.thread != self.scope.conversation {
            return Err(FlowError::InvalidInput(
                "The UI request belongs to another conversation.".into(),
            ));
        }
        if self.page.settings_blocked() {
            return Err(FlowError::HistoryBusy);
        }
        self.owner.borrow_mut().begin(&request.id)?;
        let pending = PendingRun {
            core: Rc::clone(self),
            ui: request.id.clone(),
        };
        self.page.notify_settings();
        Ok(async move {
            let pending = pending;
            let core = &pending.core;
            let prepared = async {
                let Some(view) = core.prepare_current().await? else {
                    return Ok(None);
                };
                if view.runs.iter().any(|run| !run.status.is_terminal()) {
                    return Err(FlowError::HistoryBusy);
                }
                let source = view.source().cloned().ok_or(FlowError::NoSource)?;
                let destination = composer::resolve(&view, &request.destination)
                    .map_err(FlowError::InvalidInput)?;
                let messages = vec![AgUiMessage::User {
                    id: request.message.clone(),
                    content: request.text.as_str().into(),
                }];
                let stream = RunStream::new(request.thread, request.id.clone(), messages)
                    .map_err(FlowError::InvalidInput)?;
                Ok::<_, FlowError>(Some((source, destination, stream)))
            }
            .await;
            let (source, destination, mut stream) = match prepared {
                Ok(Some(value)) => value,
                Ok(None) => {
                    core.owner.borrow_mut().not_sent();
                    core.page.notify_settings();
                    return Ok(());
                }
                Err(error) => {
                    core.owner.borrow_mut().not_sent();
                    core.page.notify_settings();
                    return Err(error);
                }
            };
            core.ensure_current()?;
            let flow = core.flow();
            core.owner.borrow_mut().dispatch(&request.id)?;
            let started = composer::dispatch(
                &flow,
                core.scope.clone(),
                source,
                destination,
                request.text,
                &request.mode,
            )
            .await;
            let native = match started {
                Ok(native) => native,
                Err(error) => {
                    core.owner.borrow_mut().start_failed(&error);
                    core.page.notify_settings();
                    return Err(error);
                }
            };
            core.ensure_current()?;
            if !core.flow().same_connection(&flow) {
                return Err(FlowError::HistoryStale);
            }
            let stop = core.owner.borrow_mut().accepted(native.clone());
            core.page.notify_settings();
            let result = async {
                let events = stream
                    .accept_question(&native)
                    .map_err(FlowError::Protocol)?;
                for event in events {
                    emit(event)?;
                }
                if stop {
                    flow.request_stop(core.scope.clone(), native.clone())
                        .await?;
                }
                core.stream(&native, &mut stream, &mut emit).await
            }
            .await;
            match result {
                Ok(()) => core.owner.borrow_mut().finished(&native),
                Err(error) => {
                    if core.ensure_current().is_ok()
                        && core.flow().same_connection(&flow)
                        && !matches!(error, FlowError::Disconnected)
                    {
                        let _ = flow.request_stop(core.scope.clone(), native.clone()).await;
                    }
                    if core.ensure_current().is_ok() {
                        core.owner.borrow_mut().run_uncertain(&request.id, &native);
                    }
                    core.page.notify_settings();
                    return Err(error);
                }
            }
            core.page.notify_settings();
            Ok(())
        })
    }
}
