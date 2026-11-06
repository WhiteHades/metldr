use super::*;

impl Core {
    pub(super) async fn run(
        &self,
        request: UiRunRequest,
        mut emit: impl FnMut(Event) -> Result<(), FlowError>,
    ) -> Result<(), FlowError> {
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
        self.page.notify_settings();
        let prepared = async {
            let Some(view) = self.prepare_current().await? else {
                return Ok(None);
            };
            if view.runs.iter().any(|run| !run.status.is_terminal()) {
                return Err(FlowError::HistoryBusy);
            }
            let source = view.source().cloned().ok_or(FlowError::NoSource)?;
            let destination =
                composer::resolve(&view, &request.destination).map_err(FlowError::InvalidInput)?;
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
                self.owner.borrow_mut().not_sent();
                self.page.notify_settings();
                return Ok(());
            }
            Err(error) => {
                self.owner.borrow_mut().not_sent();
                self.page.notify_settings();
                return Err(error);
            }
        };
        self.ensure_current()?;
        let flow = self.flow();
        let started = composer::dispatch(
            &flow,
            self.scope.clone(),
            source,
            destination,
            request.text,
            &request.mode,
        )
        .await;
        let native = match started {
            Ok(native) => native,
            Err(error) => {
                self.owner.borrow_mut().start_failed(&error);
                self.page.notify_settings();
                return Err(error);
            }
        };
        self.ensure_current()?;
        if !self.flow().same_connection(&flow) {
            return Err(FlowError::HistoryStale);
        }
        let stop = self.owner.borrow_mut().accepted(native.clone());
        self.page.notify_settings();
        let result = async {
            let events = stream
                .accept_question(&native)
                .map_err(FlowError::Protocol)?;
            for event in events {
                emit(event)?;
            }
            if stop {
                flow.request_stop(self.scope.clone(), native.clone())
                    .await?;
            }
            self.stream(&native, &mut stream, &mut emit).await
        }
        .await;
        match result {
            Ok(()) => self.owner.borrow_mut().finished(&native),
            Err(error) => {
                if self.ensure_current().is_ok()
                    && self.flow().same_connection(&flow)
                    && !matches!(error, FlowError::Disconnected)
                {
                    let _ = flow.request_stop(self.scope.clone(), native.clone()).await;
                }
                if self.ensure_current().is_ok() {
                    self.owner.borrow_mut().run_uncertain(&request.id, &native);
                }
                self.page.notify_settings();
                return Err(error);
            }
        }
        self.page.notify_settings();
        Ok(())
    }
}
