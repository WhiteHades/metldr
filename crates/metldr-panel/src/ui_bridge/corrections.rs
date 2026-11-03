use super::*;
use crate::model::{CorrectionHistory, CorrectionReceipt, CorrectionState, RunRoute, RunStatus};

mod observe;
pub(super) use observe::Observer;
pub use observe::UiNativeCorrectionSubscription;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Pending {
    run: RunId,
    request: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Operation {
    #[default]
    Idle,
    Sending(Pending),
    Waiting(Pending),
    Uncertain(Pending),
    Recorded(Pending),
}
impl Operation {
    pub(super) fn blocked(&self) -> bool {
        !matches!(self, Self::Idle | Self::Recorded(_))
    }
    pub(super) fn sending(&self) -> bool {
        matches!(self, Self::Sending(_))
    }
    pub(super) fn uncertain(&self) -> bool {
        matches!(self, Self::Uncertain(_))
    }
    fn pending(&self) -> Option<&Pending> {
        match self {
            Self::Idle | Self::Recorded(_) => None,
            Self::Sending(p) | Self::Waiting(p) | Self::Uncertain(p) => Some(p),
        }
    }
    fn owns(&self, pending: &Pending) -> bool {
        self.pending() == Some(pending)
    }
    fn recorded(&self, pending: &Pending) -> bool {
        matches!(self, Self::Recorded(saved) if saved == pending)
    }
    pub(super) fn observe(&mut self, view: &PageView, scope: &Scope) {
        if &view.selection == scope
            && self.pending().is_some_and(|p| {
                view.runs.iter().any(|run| {
                    run.id == p.run
                        && run
                            .corrections
                            .entries
                            .iter()
                            .any(|r| r.request == p.request)
                })
            })
        {
            if let Some(pending) = self.pending().cloned() {
                *self = Self::Recorded(pending);
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct State {
    connected: bool,
    stale: bool,
    available: bool,
    operation: Operation,
    run: Option<RunId>,
    receipts: Vec<UiCorrectionReceipt>,
}
impl State {
    fn read(core: &Core) -> Self {
        let view = core.page.view.borrow();
        let operation = core.correction_operation.borrow().clone();
        let stale = core.ensure_current().is_err();
        let connected = core.flow().is_connected();
        let target = view.native_correction.as_ref().filter(|target| {
            view.runs.iter().any(|run| {
                run.id == target.run
                    && run.status == RunStatus::Running
                    && matches!(&run.route, RunRoute::Codex { .. } | RunRoute::Pi { .. })
            })
        });
        let available = connected
            && !stale
            && target.is_some_and(|target| target.available)
            && !operation.blocked()
            && core.owner.borrow().readable().is_ok()
            && !core.page.recovery.borrow().active()
            && !core.page.settings.borrow().blocked()
            && !core.page.navigating.get()
            && !matches!(*core.owner.borrow(), Owner::Uncertain);
        Self {
            connected,
            stale,
            available,
            operation,
            run: target.map(|target| target.run.clone()),
            receipts: view
                .runs
                .iter()
                .flat_map(|run| receipts(&run.id, &run.corrections))
                .collect(),
        }
    }
}

#[wasm_bindgen]
pub struct UiNativeCorrectionSnapshot {
    state: State,
}
#[wasm_bindgen]
impl UiNativeCorrectionSnapshot {
    #[wasm_bindgen(getter)]
    pub fn connected(&self) -> bool {
        self.state.connected
    }
    #[wasm_bindgen(getter)]
    pub fn stale(&self) -> bool {
        self.state.stale
    }
    #[wasm_bindgen(getter)]
    pub fn available(&self) -> bool {
        self.state.available
    }
    #[wasm_bindgen(getter)]
    pub fn pending(&self) -> bool {
        matches!(
            self.state.operation,
            Operation::Sending(_) | Operation::Waiting(_)
        )
    }
    #[wasm_bindgen(getter)]
    pub fn uncertain(&self) -> bool {
        self.state.operation.uncertain()
    }
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> Option<String> {
        self.state.run.as_ref().map(|run| run.as_str().to_owned())
    }
    #[wasm_bindgen(getter)]
    pub fn admitted_request_id(&self) -> Option<String> {
        match &self.state.operation {
            Operation::Recorded(pending) => Some(pending.request.clone()),
            _ => None,
        }
    }
    pub fn receipts(&self) -> Vec<UiCorrectionReceipt> {
        self.state.receipts.clone()
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiCorrectionStatus {
    Pending,
    Dispatching,
    CodexAccepted,
    PiQueued,
    PiHandled,
    Rejected,
    NotSent,
    Uncertain,
}
#[wasm_bindgen]
#[derive(Clone, PartialEq, Eq)]
pub struct UiCorrectionReceipt {
    run: RunId,
    receipt: CorrectionReceipt,
}
pub(super) fn receipts(run: &RunId, history: &CorrectionHistory) -> Vec<UiCorrectionReceipt> {
    history
        .entries
        .iter()
        .map(|receipt| UiCorrectionReceipt {
            run: run.clone(),
            receipt: receipt.clone(),
        })
        .collect()
}
#[wasm_bindgen]
impl UiCorrectionReceipt {
    #[wasm_bindgen(getter)]
    pub fn run_id(&self) -> String {
        self.run.as_str().into()
    }
    #[wasm_bindgen(getter)]
    pub fn request_id(&self) -> String {
        self.receipt.request.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn text(&self) -> String {
        self.receipt.text.text.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn truncated(&self) -> bool {
        self.receipt.text.truncated
    }
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.receipt.state.label()
    }
    #[wasm_bindgen(getter)]
    pub fn detail(&self) -> Option<String> {
        match &self.receipt.state {
            CorrectionState::Rejected { message }
            | CorrectionState::NotSent { message }
            | CorrectionState::Uncertain { message } => Some(message.clone()),
            _ => None,
        }
    }
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> UiCorrectionStatus {
        match &self.receipt.state {
            CorrectionState::Pending => UiCorrectionStatus::Pending,
            CorrectionState::Dispatching => UiCorrectionStatus::Dispatching,
            CorrectionState::CodexAccepted => UiCorrectionStatus::CodexAccepted,
            CorrectionState::PiQueued => UiCorrectionStatus::PiQueued,
            CorrectionState::PiHandled => UiCorrectionStatus::PiHandled,
            CorrectionState::Rejected { .. } => UiCorrectionStatus::Rejected,
            CorrectionState::NotSent { .. } => UiCorrectionStatus::NotSent,
            CorrectionState::Uncertain { .. } => UiCorrectionStatus::Uncertain,
        }
    }
}

#[wasm_bindgen]
pub struct UiNativeCorrections {
    core: Rc<Core>,
    activation: activation::SubmissionActivation,
    last_request: RefCell<Option<String>>,
}
#[wasm_bindgen]
impl UiBridge {
    pub fn native_corrections(&self) -> Result<UiNativeCorrections, JsValue> {
        Ok(UiNativeCorrections {
            core: Rc::clone(&self.core),
            activation: activation::SubmissionActivation::new().map_err(js_error)?,
            last_request: RefCell::new(None),
        })
    }
}
#[wasm_bindgen]
impl UiNativeCorrections {
    #[wasm_bindgen(getter)]
    pub fn request_id(&self) -> Result<String, JsValue> {
        self.last_request
            .borrow()
            .clone()
            .ok_or_else(|| crate::err("No correction has been submitted by this handle."))
    }

    pub fn subscribe(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(snapshot: UiNativeCorrectionSnapshot) => void")]
        callback: Function,
    ) -> Result<UiNativeCorrectionSubscription, JsValue> {
        Observer::subscribe(Rc::clone(&self.core), callback)
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn send(
        &self,
        text: String,
        #[wasm_bindgen(unchecked_param_type = "Event")] event: JsValue,
    ) -> Result<Promise, JsValue> {
        self.activation.require(&event).map_err(js_error)?;
        let state = State::read(&self.core);
        let run = state
            .run
            .filter(|_| state.available)
            .ok_or_else(|| crate::err("A correction is not available for this active run."))?;
        let text = UserText::try_from(text.as_str()).map_err(js_error)?;
        let flow = self.core.flow();
        let pending = Pending {
            run: run.clone(),
            request: flow.correction_request_id().map_err(js_error)?,
        };
        self.last_request.replace(Some(pending.request.clone()));
        *self.core.correction_operation.borrow_mut() = Operation::Sending(pending.clone());
        self.core.page.notify_settings();
        let core = Rc::clone(&self.core);
        Ok(future_to_promise(async move {
            let result = flow
                .correct(pending.request.clone(), core.scope.clone(), run, text)
                .await;
            match result {
                Ok(()) => {
                    if core.correction_operation.borrow().owns(&pending) {
                        *core.correction_operation.borrow_mut() = Operation::Waiting(pending);
                    }
                    core.page.notify_settings();
                    let _ = core.current().await;
                    Ok(JsValue::UNDEFINED)
                }
                Err(error) => {
                    let reconciled = reconcile(&core, &flow, &pending).await;
                    if reconciled == Reconciled::Admitted {
                        Ok(JsValue::UNDEFINED)
                    } else {
                        if core.correction_operation.borrow().owns(&pending) {
                            *core.correction_operation.borrow_mut() =
                                if matches!(error, FlowError::Remote(_))
                                    && reconciled == Reconciled::Absent
                                {
                                    Operation::Idle
                                } else {
                                    Operation::Uncertain(pending)
                                };
                        }
                        core.page.notify_settings();
                        Err(js_error(error))
                    }
                }
            }
        }))
    }
    #[wasm_bindgen(unchecked_return_type = "Promise<undefined>")]
    pub fn refresh(&self) -> Promise {
        let core = Rc::clone(&self.core);
        future_to_promise(async move {
            core.ensure_current().map_err(js_error)?;
            if core.correction_operation.borrow().sending() {
                return Err(js_error(FlowError::HistoryBusy));
            }
            let pending = core.correction_operation.borrow().pending().cloned();
            if let Some(pending) = pending {
                let flow = core.flow();
                if reconcile(&core, &flow, &pending).await != Reconciled::Admitted {
                    return Err(crate::err(
                        "Correction status is unresolved. Check saved work before sending again.",
                    ));
                }
            } else {
                core.current().await.map_err(js_error)?;
            }
            Ok(JsValue::UNDEFINED)
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Reconciled {
    Admitted,
    Absent,
    Unknown,
}
fn receipt_proof(history: &CorrectionHistory, request: &str) -> Reconciled {
    if history
        .entries
        .iter()
        .any(|receipt| receipt.request == request)
    {
        Reconciled::Admitted
    } else if history.entries.len() == usize::from(history.total) {
        Reconciled::Absent
    } else {
        Reconciled::Unknown
    }
}
async fn reconcile(core: &Core, flow: &FlowClient, pending: &Pending) -> Reconciled {
    if core.correction_operation.borrow().recorded(pending) {
        return Reconciled::Admitted;
    }
    let Some((view, run)) = flow
        .correction_history(&core.scope, &pending.run, &pending.request)
        .await
    else {
        return Reconciled::Unknown;
    };
    if !flow.same_connection(&core.flow()) {
        return Reconciled::Unknown;
    }
    let observed_sequence = view.sequence.clone();
    let Ok(accepted) = core.accept(view) else {
        return Reconciled::Unknown;
    };
    if core.correction_operation.borrow().recorded(pending) {
        return Reconciled::Admitted;
    }
    let proof = receipt_proof(&run.corrections, &pending.request);
    if proof == Reconciled::Admitted {
        if core.correction_operation.borrow().owns(pending) {
            *core.correction_operation.borrow_mut() = Operation::Recorded(pending.clone());
        }
        core.page.notify_settings();
        Reconciled::Admitted
    } else if accepted.sequence != observed_sequence {
        Reconciled::Unknown
    } else {
        proof
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn partial_history_cannot_prove_a_correction_was_absent() {
        let mut history = CorrectionHistory {
            total: 2,
            entries: vec![CorrectionReceipt {
                ordinal: 1,
                request: "owned-request".into(),
                text: crate::history::Preview {
                    text: "Keep the source".into(),
                    truncated: false,
                },
                state: CorrectionState::Pending,
            }],
        };
        assert_eq!(
            receipt_proof(&history, "owned-request"),
            Reconciled::Admitted
        );
        assert_eq!(
            receipt_proof(&history, "other-request"),
            Reconciled::Unknown
        );
        history.total = 1;
        assert_eq!(receipt_proof(&history, "other-request"), Reconciled::Absent);
        assert_eq!(
            history.entries[0].state.label(),
            "Saved and waiting to send"
        );
    }
}
impl Core {
    pub(super) fn notify_corrections(&self) {
        let observers: Vec<_> = {
            let mut list = self.correction_subscriptions.borrow_mut();
            list.retain(|weak| weak.upgrade().is_some_and(|o| !o.closed()));
            list.iter().filter_map(std::rc::Weak::upgrade).collect()
        };
        for observer in observers {
            if observer.emit().is_err() {
                observer.close();
            }
            observer.wake();
        }
    }
}
