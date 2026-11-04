use super::*;

const LIMIT: usize = 32;
const MAX_BYTES: usize = crate::protocol::MAX_INITIALIZE_BYTES + 256;

#[derive(Clone, Copy, Eq, PartialEq, Deserialize)]
#[serde(transparent)]
struct TaskId(NonZeroU32);

#[derive(Clone, Copy, Eq, PartialEq, Deserialize)]
#[serde(transparent)]
struct ProbeId(NonZeroU32);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Packet {
    binding: Binding,
    document_id: NonZeroU32,
    task: Transition,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Transition {
    Started { id: TaskId },
    Finished { id: TaskId },
    Failed { id: TaskId },
    Pong { id: ProbeId },
}

#[derive(Default)]
pub(super) struct Tasks {
    last_id: u32,
    active: [Option<TaskId>; LIMIT],
    issued_probe: u32,
    acknowledged_probe: u32,
}

impl Tasks {
    fn accept(&mut self, change: Transition) -> Result<TaskEvent, String> {
        let outcome = match change {
            Transition::Started { id } => {
                if self.last_id.checked_add(1) != Some(id.0.get()) {
                    return Err("answer task sequence is invalid".into());
                }
                let slot = self
                    .active
                    .iter_mut()
                    .find(|slot| slot.is_none())
                    .ok_or("answer has too many pending tasks")?;
                *slot = Some(id);
                self.last_id = id.0.get();
                Outcome::Started(id)
            }
            Transition::Finished { id } | Transition::Failed { id } => {
                let slot = self
                    .active
                    .iter_mut()
                    .find(|slot| **slot == Some(id))
                    .ok_or("answer task is not active")?;
                *slot = None;
                if matches!(change, Transition::Finished { .. }) {
                    Outcome::Finished(id)
                } else {
                    Outcome::Failed(id)
                }
            }
            Transition::Pong { id } => {
                if id.0.get() > self.issued_probe {
                    return Err("answer probe was not issued".into());
                }
                if id.0.get() == self.issued_probe && id.0.get() > self.acknowledged_probe {
                    self.acknowledged_probe = id.0.get();
                    Outcome::Pong(id)
                } else {
                    Outcome::Ignored(id)
                }
            }
        };
        Ok(TaskEvent(outcome))
    }
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskEventKind {
    Started,
    Finished,
    Failed,
    Pong,
    Ignored,
}

#[derive(Clone, Copy)]
enum Outcome {
    Started(TaskId),
    Finished(TaskId),
    Failed(TaskId),
    Pong(ProbeId),
    Ignored(ProbeId),
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
pub struct TaskEvent(Outcome);

#[cfg_attr(feature = "guest", wasm_bindgen)]
impl TaskEvent {
    pub fn kind(&self) -> TaskEventKind {
        match self.0 {
            Outcome::Started(_) => TaskEventKind::Started,
            Outcome::Finished(_) => TaskEventKind::Finished,
            Outcome::Failed(_) => TaskEventKind::Failed,
            Outcome::Pong(_) => TaskEventKind::Pong,
            Outcome::Ignored(_) => TaskEventKind::Ignored,
        }
    }
    pub fn id(&self) -> u32 {
        match self.0 {
            Outcome::Started(id) | Outcome::Finished(id) | Outcome::Failed(id) => id.0.get(),
            Outcome::Pong(id) | Outcome::Ignored(id) => id.0.get(),
        }
    }
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
impl DomSession {
    pub fn receive_task(&mut self, encoded: String) -> Result<TaskEvent, String> {
        if !self.is_active() || encoded.len() > MAX_BYTES {
            return Err("answer task session is closed or packet exceeds its limit".into());
        }
        let packet: Packet =
            serde_json::from_str(&encoded).map_err(|_| "invalid answer task packet".to_owned())?;
        if packet.binding != self.binding || packet.document_id != self.document_id {
            return Err("answer task packet is out of scope".into());
        }
        self.tasks.accept(packet.task)
    }
    pub fn task_count(&self) -> u32 {
        self.tasks.active.iter().flatten().count() as u32
    }
    pub fn task_limit(&self) -> u32 {
        LIMIT as u32
    }
    pub fn probe_timeout_ms(&self) -> u32 {
        5000
    }
    pub fn probe_interval_ms(&self) -> u32 {
        1000
    }
    pub fn probe(&mut self) -> Result<u32, String> {
        if !self.is_active() {
            return Err("answer document is closed".into());
        }
        let next = self
            .tasks
            .issued_probe
            .checked_add(1)
            .ok_or("answer probe sequence exhausted")?;
        self.tasks.issued_probe = next;
        Ok(next)
    }
}
