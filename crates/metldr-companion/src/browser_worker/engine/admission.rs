use super::{Operation, Result, runtime};
use crate::browser_io::Client;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Default)]
enum State {
    #[default]
    Pending,
    Admitted,
    Cancelled,
}

#[derive(Default)]
pub(super) struct Admission(Mutex<State>);

impl Admission {
    pub(super) fn begin(
        &self,
        client: &Client,
        operation: Operation,
    ) -> Result<oneshot::Receiver<runtime::Result<Value>>> {
        let mut state = self.0.lock().map_err(|_| "browser admission lock failed")?;
        if !matches!(*state, State::Pending) {
            return Err("browser caller expired before dispatch".into());
        }
        let reply = client.begin(operation);
        *state = if reply.is_ok() {
            State::Admitted
        } else {
            State::Cancelled
        };
        reply
    }

    pub(super) fn cancel(&self) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        match *state {
            State::Pending | State::Cancelled => {
                *state = State::Cancelled;
                true
            }
            State::Admitted => false,
        }
    }
}

#[derive(Default)]
pub(super) struct CancelOnDrop(pub(super) Arc<Admission>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::Event;
    use std::sync::{Barrier, mpsc};

    fn operation() -> Operation {
        Operation::Command {
            tab: 1,
            session: None,
            method: "Runtime.callFunctionOn".into(),
            params: serde_json::json!({}),
        }
    }

    #[test]
    fn cancelled_caller_and_failed_enqueue_cannot_admit_an_action() {
        let guard = CancelOnDrop::default();
        let admission = guard.0.clone();
        let (tx, rx) = mpsc::sync_channel::<Event>(1);
        let client = Client {
            run: "run".into(),
            tx,
        };
        drop(guard);
        assert!(admission.begin(&client, operation()).is_err());
        assert!(rx.try_recv().is_err());
        let admission = Admission::default();
        drop(rx);
        assert!(admission.begin(&client, operation()).is_err());
        assert!(admission.cancel());

        let (tx, rx) = mpsc::sync_channel::<Event>(1);
        let client = Client {
            run: "run".into(),
            tx,
        };
        let admitted = Admission::default();
        assert!(admitted.begin(&client, operation()).is_ok());
        let full = Admission::default();
        assert!(full.begin(&client, operation()).is_err());
        assert!(full.cancel());
        assert!(!admitted.cancel());
        assert!(admitted.begin(&client, operation()).is_err());
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cancellation_and_enqueue_have_exactly_one_winner() {
        for _ in 0..32 {
            let admission = Arc::new(Admission::default());
            let gate = Arc::new(Barrier::new(2));
            let (tx, rx) = mpsc::sync_channel::<Event>(1);
            let sender = admission.clone();
            let start = gate.clone();
            let worker = std::thread::spawn(move || {
                start.wait();
                sender
                    .begin(
                        &Client {
                            run: "run".into(),
                            tx,
                        },
                        operation(),
                    )
                    .is_ok()
            });
            gate.wait();
            let prevented = admission.cancel();
            let admitted = worker.join().unwrap();
            assert_ne!(prevented, admitted);
            assert_eq!(rx.try_recv().is_ok(), admitted);
            assert_eq!(admission.cancel(), prevented);
        }
    }
}
