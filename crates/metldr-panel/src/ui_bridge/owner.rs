use crate::{
    flow::ConversationRequest,
    history::Context,
    model::{FlowError, RunId},
};

#[derive(Default)]
pub(super) enum Owner {
    #[default]
    Idle,
    Capturing,
    Starting {
        ui: String,
        stop: bool,
    },
    Running {
        ui: String,
        native: RunId,
    },
    Observing(RunId),
    Navigating(Navigation),
    Retired,
    Uncertain,
}

pub(super) enum Navigation {
    Pending(ConversationRequest),
    Uncertain(ConversationRequest),
    Sending(ConversationRequest),
    Confirmed(Context),
    Refreshing(Context),
}

pub(super) enum NavigationWork {
    Send(ConversationRequest),
    Retry(ConversationRequest),
    Refresh(Context),
}

pub(super) enum Stop {
    Pending,
    Native(RunId),
}

impl Owner {
    pub(super) fn settings_ready(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub(super) fn observe(&mut self, native: RunId) -> Result<(), FlowError> {
        self.idle()?;
        *self = Self::Observing(native);
        Ok(())
    }

    pub(super) fn observation_finished(&mut self, run: &RunId) {
        if matches!(self, Self::Observing(native) if native == run) {
            *self = Self::Idle;
        }
    }

    pub(super) fn stop_pending(&self) -> bool {
        matches!(self, Self::Starting { stop: true, .. })
    }

    pub(super) fn begin(&mut self, ui: &str) -> Result<(), FlowError> {
        self.idle()?;
        *self = Self::Starting {
            ui: ui.into(),
            stop: false,
        };
        Ok(())
    }

    pub(super) fn capture(&mut self) -> Result<(), FlowError> {
        self.idle()?;
        *self = Self::Capturing;
        Ok(())
    }

    fn idle(&self) -> Result<(), FlowError> {
        match self {
            Self::Idle => Ok(()),
            Self::Retired => Err(FlowError::HistoryStale),
            Self::Uncertain => Err(FlowError::Host(
                "Reconnect and check saved work before starting again.".into(),
            )),
            _ => Err(FlowError::HistoryBusy),
        }
    }

    pub(super) fn readable(&self) -> Result<(), FlowError> {
        match self {
            Self::Navigating(_) => Err(FlowError::HistoryBusy),
            Self::Retired => Err(FlowError::HistoryStale),
            _ => Ok(()),
        }
    }

    pub(super) fn navigate(&mut self, request: ConversationRequest) -> Result<(), FlowError> {
        self.idle()?;
        *self = Self::Navigating(Navigation::Pending(request));
        Ok(())
    }

    pub(super) fn navigation_step(&mut self) -> Result<NavigationWork, FlowError> {
        match self {
            Self::Navigating(Navigation::Pending(request)) => {
                let request = request.clone();
                *self = Self::Navigating(Navigation::Sending(request.clone()));
                Ok(NavigationWork::Send(request))
            }
            Self::Navigating(Navigation::Confirmed(context)) => {
                let context = context.clone();
                *self = Self::Navigating(Navigation::Refreshing(context.clone()));
                Ok(NavigationWork::Refresh(context))
            }
            Self::Navigating(Navigation::Uncertain(request)) => {
                let request = request.clone();
                *self = Self::Navigating(Navigation::Sending(request.clone()));
                Ok(NavigationWork::Retry(request))
            }
            Self::Retired => Err(FlowError::HistoryStale),
            _ => Err(FlowError::HistoryBusy),
        }
    }

    pub(super) fn navigation_confirmed(&mut self, context: Context) {
        *self = Self::Navigating(Navigation::Refreshing(context));
    }

    pub(super) fn navigation_failed(&mut self, error: &FlowError) {
        if matches!(self, Self::Navigating(Navigation::Sending(_)))
            && matches!(error, FlowError::HistoryBusy | FlowError::HistoryStale)
        {
            *self = Self::Idle;
        } else {
            self.navigation_interrupted();
        }
    }

    pub(super) fn navigation_interrupted(&mut self) {
        match self {
            Self::Navigating(Navigation::Sending(request)) => {
                *self = Self::Navigating(Navigation::Uncertain(request.clone()));
            }
            Self::Navigating(Navigation::Refreshing(context)) => {
                *self = Self::Navigating(Navigation::Confirmed(context.clone()));
            }
            _ => {}
        }
    }

    pub(super) fn capture_finished(&mut self) {
        // Capture saves a source only. The next run reads and checks it again.
        *self = Self::Idle;
    }

    pub(super) fn not_sent(&mut self) {
        *self = Self::Idle;
    }

    pub(super) fn accepted(&mut self, native: RunId) -> bool {
        let old = std::mem::replace(self, Self::Uncertain);
        if let Self::Starting { ui, stop } = old {
            *self = Self::Running { ui, native };
            stop
        } else {
            true
        }
    }

    pub(super) fn stop(&mut self, id: &str) -> Result<Stop, FlowError> {
        match self {
            Self::Starting { ui, stop } if ui == id => {
                *stop = true;
                Ok(Stop::Pending)
            }
            Self::Running { ui, native } if ui == id => Ok(Stop::Native(native.clone())),
            _ => Err(FlowError::InvalidInput(
                "The Stop request does not match an active UI run.".into(),
            )),
        }
    }

    pub(super) fn start_failed(&mut self, error: &FlowError) {
        *self = if error.may_have_committed() {
            Self::Uncertain
        } else {
            Self::Idle
        };
    }

    pub(super) fn run_uncertain(&mut self, ui_id: &str, run: &RunId) {
        if matches!(self, Self::Running { ui, native } if ui == ui_id && native == run) {
            *self = Self::Uncertain;
        }
    }

    pub(super) fn finished(&mut self, run: &RunId) {
        if matches!(self, Self::Running { native, .. } if native == run) {
            *self = Self::Idle;
        }
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn stop_during_start_is_retained_and_unknown_receipts_block_replay() {
        let native: RunId =
            serde_json::from_value(serde_json::json!("0123456789abcdef0123456789abcdef:1"))
                .unwrap();
        let mut owner = Owner::Idle;
        owner.observe(native.clone()).unwrap();
        assert!(owner.begin("while-observing").is_err());
        assert!(owner.capture().is_err());
        assert!(owner.readable().is_ok());
        assert!(owner.stop("unrelated").is_err());
        owner.observation_finished(&native);
        owner.begin("first").unwrap();
        assert!(!owner.stop_pending());
        assert!(owner.begin("second").is_err());
        assert!(owner.stop("wrong").is_err());
        assert!(matches!(owner.stop("first").unwrap(), Stop::Pending));
        assert!(owner.stop_pending());
        assert!(owner.accepted(native.clone()));
        assert!(!owner.stop_pending());
        assert!(matches!(owner.stop("first").unwrap(), Stop::Native(id) if id == native));
        owner.run_uncertain("different-ui-run", &native);
        assert!(matches!(&owner, Owner::Running { .. }));
        owner.finished(&native);
        owner.begin("second").unwrap();
        assert!(!owner.accepted(native.clone()));
        owner.run_uncertain("first", &native);
        assert!(matches!(&owner, Owner::Running { .. }));
        owner.run_uncertain("second", &native);
        assert!(matches!(&owner, Owner::Uncertain));
        assert!(owner.begin("must-not-replay").is_err());
        owner = Owner::Idle;
        owner.begin("second").unwrap();
        owner.start_failed(&FlowError::Timeout);
        assert!(owner.begin("second").is_err());
        assert!(owner.capture().is_err());
        let mut rejected = Owner::Idle;
        rejected.begin("rejected").unwrap();
        rejected.start_failed(&FlowError::Remote("unavailable".into()));
        assert!(rejected.begin("another").is_err());
        assert!(rejected.capture().is_err());
        let mut invalid = Owner::Idle;
        invalid.begin("invalid").unwrap();
        invalid.start_failed(&FlowError::InvalidInput("invalid mode".into()));
        invalid.begin("corrected").unwrap();
        let mut capture = Owner::Idle;
        capture.capture().unwrap();
        capture.capture_finished();
        capture.begin("after-permission-denied").unwrap();
        let mut retired = Owner::Retired;
        assert!(matches!(retired.readable(), Err(FlowError::HistoryStale)));
        assert!(matches!(retired.capture(), Err(FlowError::HistoryStale)));
        assert!(matches!(
            retired.begin("retired"),
            Err(FlowError::HistoryStale)
        ));
    }
}
