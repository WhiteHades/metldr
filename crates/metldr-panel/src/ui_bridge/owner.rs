use crate::model::{FlowError, RunId};

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
    Uncertain,
}

pub(super) enum Stop {
    Pending,
    Native(RunId),
}

impl Owner {
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
            Self::Uncertain => Err(FlowError::Host(
                "Reconnect and check saved work before starting again.".into(),
            )),
            _ => Err(FlowError::HistoryBusy),
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
        *self = match error {
            FlowError::HostRejected(_)
            | FlowError::InvalidInput(_)
            | FlowError::HistoryBusy
            | FlowError::HistoryStale
            | FlowError::NoSource => Self::Idle,
            FlowError::Disconnected
            | FlowError::Timeout
            | FlowError::Protocol(_)
            | FlowError::Host(_) => Self::Uncertain,
        };
    }

    pub(super) fn uncertain(&mut self) {
        *self = Self::Uncertain;
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
        owner.begin("first").unwrap();
        assert!(owner.begin("second").is_err());
        assert!(owner.stop("wrong").is_err());
        assert!(matches!(owner.stop("first").unwrap(), Stop::Pending));
        assert!(owner.accepted(native.clone()));
        assert!(matches!(owner.stop("first").unwrap(), Stop::Native(id) if id == native));
        owner.finished(&native);
        owner.begin("second").unwrap();
        owner.start_failed(&FlowError::Timeout);
        assert!(owner.begin("second").is_err());
        assert!(owner.capture().is_err());
        let mut rejected = Owner::Idle;
        rejected.begin("rejected").unwrap();
        rejected.start_failed(&FlowError::HostRejected("unavailable".into()));
        rejected.begin("another").unwrap();
        let mut capture = Owner::Idle;
        capture.capture().unwrap();
        capture.capture_finished();
        capture.begin("after-permission-denied").unwrap();
    }
}
