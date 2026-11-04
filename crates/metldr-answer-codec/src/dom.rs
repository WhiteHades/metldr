use std::{
    cell::Cell,
    num::NonZeroU32,
    rc::{Rc, Weak},
};

use serde::{Deserialize, Serialize};
#[cfg(feature = "guest")]
use wasm_bindgen::prelude::*;

use crate::protocol::Binding;

mod tasks;
pub use tasks::{TaskEvent, TaskEventKind};

const MAX_NODES: usize = 8_192;
const MAX_BATCH_BYTES: usize = 262_144;
const MAX_OPERATIONS: usize = 256;
const MAX_TEXT_BYTES: usize = 16_384;
const MAX_EVENT_BYTES: usize = 8_192;

#[cfg_attr(feature = "guest", wasm_bindgen)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DomEventKind {
    Input,
    Click,
}

impl DomEventKind {
    fn mask(self) -> u8 {
        match self {
            Self::Input => 1,
            Self::Click => 2,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerBatch {
    binding: Binding,
    document_id: NonZeroU32,
    sequence: NonZeroU32,
    operations: Vec<Operation>,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Operation {
    SetText {
        node: NonZeroU32,
        text: String,
    },
    SetValue {
        node: NonZeroU32,
        value: String,
    },
    SetDisabled {
        node: NonZeroU32,
        disabled: bool,
    },
    SetDataset {
        node: NonZeroU32,
        key: String,
        value: String,
    },
    Listen {
        node: NonZeroU32,
        event: DomEventKind,
    },
}

impl Operation {
    fn node(&self) -> NonZeroU32 {
        match self {
            Self::SetText { node, .. }
            | Self::SetValue { node, .. }
            | Self::SetDisabled { node, .. }
            | Self::SetDataset { node, .. }
            | Self::Listen { node, .. } => *node,
        }
    }
}

#[derive(Clone)]
struct Node {
    parent: u32,
    present: bool,
    listeners: u8,
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
pub struct DomSession {
    binding: Binding,
    document_id: NonZeroU32,
    nodes: Vec<Node>,
    last_batch: u32,
    last_event: u32,
    document: Weak<Cell<DocumentState>>,
    tasks: tasks::Tasks,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DocumentState {
    Empty,
    Ready(NonZeroU32),
    Mounted(NonZeroU32),
}

impl DomSession {
    pub(crate) fn mount(
        binding: Binding,
        document: &Rc<Cell<DocumentState>>,
        parents: Vec<u32>,
    ) -> Result<Self, String> {
        let DocumentState::Ready(document_id) = document.get() else {
            return Err("answer document is not ready to mount".into());
        };
        if parents.is_empty() || parents.len() > MAX_NODES {
            return Err("invalid answer node count".into());
        }
        let mut nodes = Vec::with_capacity(parents.len());
        for (index, parent) in parents.into_iter().enumerate() {
            if parent as usize > index {
                return Err("invalid answer node parent".into());
            }
            nodes.push(Node {
                parent,
                present: true,
                listeners: 0,
            });
        }
        let session = Self {
            binding,
            document_id,
            nodes,
            last_batch: 0,
            last_event: 0,
            document: Rc::downgrade(document),
            tasks: tasks::Tasks::default(),
        };
        document.set(DocumentState::Mounted(document_id));
        Ok(session)
    }

    fn is_active(&self) -> bool {
        self.document
            .upgrade()
            .is_some_and(|document| document.get() == DocumentState::Mounted(self.document_id))
    }
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
impl DomSession {
    pub fn receive(&mut self, encoded: String) -> Result<DomBatch, String> {
        if !self.is_active() || encoded.len() > MAX_BATCH_BYTES {
            return Err("answer DOM session is closed or packet exceeds its limit".into());
        }
        let batch: WorkerBatch =
            serde_json::from_str(&encoded).map_err(|_| "invalid answer DOM batch".to_owned())?;
        if batch.binding != self.binding
            || batch.document_id != self.document_id
            || self.last_batch.checked_add(1) != Some(batch.sequence.get())
            || batch.operations.len() > MAX_OPERATIONS
        {
            return Err("answer DOM batch is out of scope or sequence".into());
        }
        let mut nodes = self.nodes.clone();
        for operation in &batch.operations {
            let index = operation.node().get() as usize - 1;
            let node = nodes
                .get_mut(index)
                .filter(|node| node.present)
                .ok_or("answer DOM node is unavailable")?;
            match operation {
                Operation::SetText { text, .. } => {
                    bounded_text(text, MAX_TEXT_BYTES)?;
                    let id = operation.node().get();
                    for child in index + 1..nodes.len() {
                        let parent = nodes[child].parent;
                        if parent == id || (parent != 0 && !nodes[parent as usize - 1].present) {
                            nodes[child].present = false;
                            nodes[child].listeners = 0;
                        }
                    }
                }
                Operation::SetValue { value, .. } => bounded_text(value, MAX_TEXT_BYTES)?,
                Operation::SetDisabled { .. } => {}
                Operation::SetDataset { key, value, .. } => {
                    if key.is_empty()
                        || key.len() > 128
                        || !key
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    {
                        return Err("invalid answer dataset key".into());
                    }
                    bounded_text(value, MAX_TEXT_BYTES)?;
                }
                Operation::Listen { event, .. } => node.listeners |= event.mask(),
            }
        }
        self.nodes = nodes;
        self.last_batch = batch.sequence.get();
        Ok(DomBatch {
            operations: batch.operations.into_iter(),
        })
    }

    pub fn event(
        &mut self,
        node: u32,
        kind: DomEventKind,
        value: Option<String>,
    ) -> Result<String, String> {
        let index = node.checked_sub(1).ok_or("invalid answer event node")? as usize;
        if !self.is_active()
            || !self
                .nodes
                .get(index)
                .is_some_and(|node| node.present && node.listeners & kind.mask() != 0)
        {
            return Err("answer event has no active listener".into());
        }
        match (kind, &value) {
            (DomEventKind::Input, Some(value)) => bounded_text(value, MAX_EVENT_BYTES)?,
            (DomEventKind::Click, None) => {}
            _ => return Err("invalid answer event value".into()),
        }
        let sequence = self
            .last_event
            .checked_add(1)
            .ok_or("answer event sequence exhausted")?;
        let event = NativeEvent {
            binding: &self.binding,
            document_id: self.document_id,
            sequence,
            node,
            kind,
            value,
        };
        let encoded = serde_json::to_string(&event).map_err(|_| "cannot encode answer event")?;
        self.last_event = sequence;
        Ok(encoded)
    }

    pub fn close(&mut self) {
        if let Some(document) = self.document.upgrade()
            && document.get() == DocumentState::Mounted(self.document_id)
        {
            document.set(DocumentState::Empty);
        }
        self.nodes.clear();
        self.tasks = tasks::Tasks::default();
    }
}

fn bounded_text(text: &str, limit: usize) -> Result<(), String> {
    if text.len() > limit {
        Err("answer DOM text exceeds its limit".into())
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeEvent<'a> {
    binding: &'a Binding,
    document_id: NonZeroU32,
    sequence: u32,
    node: u32,
    kind: DomEventKind,
    value: Option<String>,
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
pub struct DomBatch {
    operations: std::vec::IntoIter<Operation>,
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
impl DomBatch {
    pub fn next(&mut self) -> Option<DomMutation> {
        self.operations
            .next()
            .map(|operation| DomMutation { operation })
    }
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomMutationKind {
    SetText,
    SetValue,
    SetDisabled,
    SetDataset,
    Listen,
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
pub struct DomMutation {
    operation: Operation,
}

#[cfg_attr(feature = "guest", wasm_bindgen)]
impl DomMutation {
    pub fn kind(&self) -> DomMutationKind {
        match self.operation {
            Operation::SetText { .. } => DomMutationKind::SetText,
            Operation::SetValue { .. } => DomMutationKind::SetValue,
            Operation::SetDisabled { .. } => DomMutationKind::SetDisabled,
            Operation::SetDataset { .. } => DomMutationKind::SetDataset,
            Operation::Listen { .. } => DomMutationKind::Listen,
        }
    }

    pub fn node(&self) -> u32 {
        self.operation.node().get()
    }

    pub fn text(&self) -> Result<String, String> {
        match &self.operation {
            Operation::SetText { text, .. } => Ok(text.clone()),
            Operation::SetValue { value, .. } | Operation::SetDataset { value, .. } => {
                Ok(value.clone())
            }
            _ => Err("answer DOM operation has no text".into()),
        }
    }

    pub fn key(&self) -> Result<String, String> {
        match &self.operation {
            Operation::SetDataset { key, .. } => Ok(key.clone()),
            _ => Err("answer DOM operation has no key".into()),
        }
    }

    pub fn disabled(&self) -> Result<bool, String> {
        match self.operation {
            Operation::SetDisabled { disabled, .. } => Ok(disabled),
            _ => Err("answer DOM operation has no disabled state".into()),
        }
    }

    pub fn event_kind(&self) -> Result<DomEventKind, String> {
        match self.operation {
            Operation::Listen { event, .. } => Ok(event),
            _ => Err("answer DOM operation has no event".into()),
        }
    }
}

#[cfg(test)]
mod checks;
