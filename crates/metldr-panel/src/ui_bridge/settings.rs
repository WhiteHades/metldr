use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Family {
    Providers,
    Agents,
}

impl Family {
    pub(super) fn busy(self, view: &PageView) -> bool {
        match self {
            Self::Providers => {
                view.provider_settings.busy || view.provider_settings.pending.is_some()
            }
            Self::Agents => view.agent_settings.busy,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Operation {
    #[default]
    Idle,
    Mutation(Family),
    Refresh {
        family: Family,
        uncertain: bool,
    },
    Unknown(Family),
}

impl Operation {
    pub(super) fn blocked(self) -> bool {
        !matches!(self, Self::Idle)
    }

    pub(super) fn can_refresh(self, family: Family) -> bool {
        matches!(self, Self::Idle) || matches!(self, Self::Unknown(owner) if owner == family)
    }

    pub(super) fn uncertain(self, family: Family) -> bool {
        matches!(self, Self::Unknown(owner) | Self::Refresh { family: owner, uncertain: true } if owner == family)
    }

    fn begin(&mut self, family: Family, refresh: bool) -> Result<(), FlowError> {
        *self = match (*self, refresh) {
            (Self::Idle, false) => Self::Mutation(family),
            (Self::Idle, true) => Self::Refresh {
                family,
                uncertain: false,
            },
            (Self::Unknown(owner), true) if owner == family => Self::Refresh {
                family,
                uncertain: true,
            },
            _ => return Err(FlowError::HistoryBusy),
        };
        Ok(())
    }

    fn failed(&mut self, error: &FlowError) {
        *self = match *self {
            Self::Mutation(family)
                if !matches!(
                    error,
                    FlowError::InvalidInput(_) | FlowError::HistoryBusy | FlowError::HistoryStale
                ) =>
            {
                Self::Unknown(family)
            }
            Self::Refresh {
                family,
                uncertain: true,
            }
            | Self::Unknown(family) => Self::Unknown(family),
            _ => Self::Idle,
        };
    }

    fn confirmed(&mut self, busy: bool) {
        *self = match *self {
            Self::Refresh {
                family,
                uncertain: true,
            } if busy => Self::Unknown(family),
            _ => Self::Idle,
        };
    }
}

pub(super) struct Work {
    page: Rc<PageSession>,
    family: Family,
    finished: bool,
}

impl Work {
    pub(super) async fn current_flow(&self) -> Result<FlowClient, FlowError> {
        let flow = self.page.flow.borrow().clone();
        self.page.accept(flow.current_page().await?)?;
        Ok(flow)
    }

    pub(super) fn begin(
        page: Rc<PageSession>,
        family: Family,
        refresh: bool,
    ) -> Result<Self, FlowError> {
        if !page.controls_ready() || (!refresh && server_busy(&page.view.borrow())) {
            return Err(FlowError::HistoryBusy);
        }
        page.settings.borrow_mut().begin(family, refresh)?;
        page.notify_settings();
        Ok(Self {
            page,
            family,
            finished: false,
        })
    }

    pub(super) async fn refresh(self) -> Result<js_sys::Undefined, FlowError> {
        let result = async {
            let flow = self.page.flow.borrow().clone();
            let (flow, current) = if !flow.is_connected() {
                let flow = flow.reconnect().await?;
                let current = self.page.accept(flow.current_page().await?)?;
                self.page.replace_flow(flow.clone());
                (flow, current)
            } else {
                let current = self.page.accept(flow.current_page().await?)?;
                (flow, current)
            };
            if server_busy(&current) {
                return Ok(current);
            }
            match self.family {
                Family::Providers => flow.provider_refresh().await,
                Family::Agents => flow.agent_refresh().await,
            }
        }
        .await;
        self.finish(result)
    }

    pub(super) fn finish(
        mut self,
        result: Result<PageView, FlowError>,
    ) -> Result<js_sys::Undefined, FlowError> {
        let result = match result {
            Ok(view) => match self.page.accept(view) {
                Ok(view) => Ok(view),
                Err(error) => {
                    *self.page.settings.borrow_mut() = Operation::Unknown(self.family);
                    self.finished = true;
                    self.page.notify_settings();
                    return Err(error);
                }
            },
            Err(error) => Err(error),
        };
        match &result {
            Ok(view) => self.page.settings.borrow_mut().confirmed(server_busy(view)),
            Err(error) => self.page.settings.borrow_mut().failed(error),
        }
        self.finished = true;
        self.page.notify_settings();
        result.map(|_| js_sys::Undefined::UNDEFINED)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        if !self.finished {
            self.page
                .settings
                .borrow_mut()
                .failed(&FlowError::Disconnected);
            self.page.notify_settings();
        }
    }
}

fn server_busy(view: &PageView) -> bool {
    Family::Providers.busy(view) || Family::Agents.busy(view)
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn unknown_receipts_require_their_own_explicit_refresh() {
        for family in [Family::Providers, Family::Agents] {
            let other = match family {
                Family::Providers => Family::Agents,
                Family::Agents => Family::Providers,
            };
            let mut state = Operation::default();
            state.begin(family, false).unwrap();
            assert!(state.blocked());
            assert!(state.begin(other, false).is_err());
            assert!(state.begin(other, true).is_err());
            state.failed(&FlowError::Timeout);
            assert_eq!(state, Operation::Unknown(family));
            assert!(state.uncertain(family));
            assert!(!state.can_refresh(other));
            assert!(state.begin(other, true).is_err());
            assert!(state.begin(family, false).is_err());
            state.begin(family, true).unwrap();
            state.failed(&FlowError::Disconnected);
            assert_eq!(state, Operation::Unknown(family));
            state.begin(family, true).unwrap();
            state.confirmed(true);
            assert_eq!(state, Operation::Unknown(family));
            state.begin(family, true).unwrap();
            state.confirmed(false);
            assert!(!state.blocked());
            state.begin(family, false).unwrap();
            state.confirmed(true);
            assert_eq!(state, Operation::Idle);
            state.begin(family, false).unwrap();
            state.failed(&FlowError::Remote("revision changed".into()));
            assert!(state.uncertain(family));
            assert!(state.begin(family, false).is_err());
            state.begin(family, true).unwrap();
            state.confirmed(false);
            state.begin(family, false).unwrap();
            state.failed(&FlowError::InvalidInput("invalid model".into()));
            assert!(!state.blocked());
        }
        let mut owner = Owner::Idle;
        assert!(owner.settings_ready());
        owner.begin("ui-run").unwrap();
        assert!(!owner.settings_ready());
        owner.not_sent();
        owner.capture().unwrap();
        assert!(!owner.settings_ready());
        owner.capture_finished();
        assert!(owner.settings_ready());
    }
}
