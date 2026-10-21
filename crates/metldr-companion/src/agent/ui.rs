mod protocol;

pub(crate) use protocol::{Answer, Dialog};
use protocol::{Kind, encode_reply, parse};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

const MAX_PENDING: usize = 8;
const MAX_PENDING_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key(u64);

pub(crate) enum Change {
    Offer {
        key: u64,
        dialog: Dialog,
        answer: SyncSender<Answer>,
    },
    Withdraw {
        key: u64,
    },
    Display(Display),
}

pub(crate) enum Display {
    Notify {
        message: String,
        level: String,
    },
    Status {
        key: String,
        text: Option<String>,
    },
    Widget {
        key: String,
        lines: Option<Vec<String>>,
        placement: String,
    },
    Title(String),
    EditorText {
        revision: u64,
        text: String,
    },
}

struct Pending {
    id: String,
    key: Key,
    dialog: Dialog,
    answer: Receiver<Answer>,
    deadline: Option<Instant>,
    bytes: usize,
}

pub(crate) struct Ui {
    run: String,
    tx: SyncSender<crate::flow::Event>,
    pending: HashMap<String, Pending>,
    pending_bytes: usize,
    next_key: u64,
    editor_revision: u64,
}

impl Ui {
    pub(crate) fn new(run: &str, tx: &SyncSender<crate::flow::Event>) -> Self {
        Self {
            run: run.to_owned(),
            tx: tx.clone(),
            pending: HashMap::new(),
            pending_bytes: 0,
            next_key: 1,
            editor_revision: 0,
        }
    }

    pub(crate) fn observe(
        &mut self,
        event: &Value,
        pi: &mut metldr_pi_session::PiSession,
        stopping: bool,
    ) -> crate::Result<bool> {
        if event["type"] != "extension_ui_request" {
            return Ok(false);
        }
        let request = parse(event).map_err(ui_error)?;
        match request.kind {
            Kind::Dialog(dialog) => {
                if stopping {
                    pi.send(encode_reply(&request.id, &dialog, Answer::Cancel).map_err(ui_error)?)?;
                    return Ok(true);
                }
                if self.pending.contains_key(&request.id) {
                    return Err(ui_error("duplicate native UI request identity"));
                }
                if self.pending.len() == MAX_PENDING {
                    return Err(ui_error("too many pending native UI requests"));
                }
                let bytes = serde_json::to_vec(&dialog)?.len();
                if self.pending_bytes.saturating_add(bytes) > MAX_PENDING_BYTES {
                    return Err(ui_error("pending native UI exceeds size limit"));
                }
                let key = Key(self.next_key);
                self.next_key = self
                    .next_key
                    .checked_add(1)
                    .ok_or_else(|| ui_error("native UI request key limit reached"))?;
                let (answer_tx, answer_rx) = mpsc::sync_channel(1);
                let deadline = match (&dialog, request.timeout) {
                    (Dialog::Editor { .. }, _) | (_, None) => None,
                    (_, Some(timeout)) => local_deadline(&timeout),
                };
                self.emit(Change::Offer {
                    key: key.0,
                    dialog: dialog.clone(),
                    answer: answer_tx,
                })?;
                self.pending_bytes += bytes;
                self.pending.insert(
                    request.id.clone(),
                    Pending {
                        id: request.id,
                        key,
                        dialog,
                        answer: answer_rx,
                        deadline,
                        bytes,
                    },
                );
            }
            Kind::Notify { message, level } => {
                self.emit(Change::Display(Display::Notify { message, level }))?;
            }
            Kind::Status { key, text } => {
                self.emit(Change::Display(Display::Status { key, text }))?;
            }
            Kind::Widget {
                key,
                lines,
                placement,
            } => {
                self.emit(Change::Display(Display::Widget {
                    key,
                    lines,
                    placement,
                }))?;
            }
            Kind::Title(title) => self.emit(Change::Display(Display::Title(title)))?,
            Kind::EditorText(text) => {
                self.editor_revision = self
                    .editor_revision
                    .checked_add(1)
                    .ok_or_else(|| ui_error("native UI update limit reached"))?;
                self.emit(Change::Display(Display::EditorText {
                    revision: self.editor_revision,
                    text,
                }))?;
            }
        }
        Ok(true)
    }

    pub(crate) fn poll(
        &mut self,
        pi: &mut metldr_pi_session::PiSession,
        stopping: bool,
    ) -> crate::Result<()> {
        if stopping {
            return self.cancel_pending(pi);
        }
        let now = Instant::now();
        let ids: Vec<String> = self.pending.keys().cloned().collect();
        for id in ids {
            let Some(pending) = self.pending.get(&id) else {
                continue;
            };
            if pending.deadline.is_some_and(|deadline| deadline <= now) {
                self.withdraw(&id)?;
                continue;
            }
            let answer = match pending.answer.try_recv() {
                Ok(answer) if pending.dialog.accepts(&answer) => answer,
                Err(TryRecvError::Empty) => continue,
                Ok(_) => return Err(ui_error("native UI answer does not match request")),
                Err(TryRecvError::Disconnected) => {
                    return Err(ui_error("native UI controller disconnected"));
                }
            };
            let Some(pending) = self.pending.get(&id) else {
                continue;
            };
            pi.send(encode_reply(&pending.id, &pending.dialog, answer).map_err(ui_error)?)?;
            self.withdraw(&id)?;
        }
        Ok(())
    }

    pub(crate) fn cancel_pending(
        &mut self,
        pi: &mut metldr_pi_session::PiSession,
    ) -> crate::Result<()> {
        let ids: Vec<String> = self.pending.keys().cloned().collect();
        for id in ids {
            let Some(pending) = self.pending.get(&id) else {
                continue;
            };
            pi.send(encode_reply(&pending.id, &pending.dialog, Answer::Cancel).map_err(ui_error)?)?;
            self.withdraw(&id)?;
        }
        Ok(())
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn withdraw(&mut self, id: &str) -> crate::Result<()> {
        if let Some(pending) = self.pending.remove(id) {
            self.pending_bytes -= pending.bytes;
            self.emit(Change::Withdraw { key: pending.key.0 })?;
        }
        Ok(())
    }

    fn emit(&self, change: Change) -> crate::Result<()> {
        self.tx
            .send(crate::flow::Event::PiUi {
                run: self.run.clone(),
                change,
            })
            .map_err(|_| ui_error("native UI event channel closed"))
    }
}

fn local_deadline(timeout: &serde_json::Number) -> Option<Instant> {
    let millis = timeout.as_f64()?;
    if millis == 0.0 {
        return None;
    }
    // Node clamps sub-millisecond and overlarge timers to one millisecond.
    let millis = if millis < 1.0 || millis > 2_147_483_647.0 {
        1
    } else {
        millis as u64
    };
    Instant::now().checked_add(Duration::from_millis(millis))
}

fn ui_error(message: &'static str) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_zero_timeout_has_no_local_deadline() {
        assert!(local_deadline(serde_json::json!(0).as_number().unwrap()).is_none());
        assert!(local_deadline(serde_json::json!(10).as_number().unwrap()).is_some());
    }
}
