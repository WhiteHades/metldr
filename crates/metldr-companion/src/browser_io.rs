use crate::{
    Result,
    flow::{Event, Owner},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::mpsc::SyncSender, time::Duration};
use tokio::sync::{mpsc, oneshot};

type Reply = std::result::Result<Value, String>;

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(super) enum Operation {
    Attach {
        tab: i32,
        document: String,
        url: String,
    },
    Command {
        tab: i32,
        session: Option<String>,
        method: String,
        params: Value,
    },
    Navigate {
        action: String,
        revision: u64,
        binding: crate::browser_action::Binding,
        url: String,
        destination: String,
        params: Value,
    },
    ActivateButton {
        action: String,
        revision: u64,
        binding: crate::browser_action::Binding,
        url: String,
        params: Value,
    },
    ConfirmNavigation {
        action: String,
        revision: u64,
        tab: i32,
        frame: String,
        loader: String,
    },
    Detach {
        tab: i32,
    },
}

pub(super) struct Call {
    pub run: String,
    pub operation: Operation,
    pub ack: oneshot::Sender<Reply>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum WireReply {
    Ok { value: Value },
    Error { message: String },
}

pub(super) struct Notice {
    pub session: Option<String>,
    pub method: String,
    pub params: Value,
}

pub(super) struct Slot {
    pub run: String,
    pub tab: i32,
    events: Option<mpsc::Sender<Notice>>,
    cleanup: bool,
    pending: BTreeMap<u64, oneshot::Sender<Reply>>,
    next: u64,
    navigations: std::collections::BTreeSet<String>,
    buttons: std::collections::BTreeSet<String>,
}

impl Slot {
    pub fn new(run: String, tab: i32) -> (Self, mpsc::Receiver<Notice>) {
        let (events, rx) = mpsc::channel(64);
        (
            Self {
                run,
                tab,
                events: Some(events),
                cleanup: false,
                pending: BTreeMap::new(),
                next: 0,
                navigations: Default::default(),
                buttons: Default::default(),
            },
            rx,
        )
    }

    fn end(&mut self) {
        self.events.take();
        for (_, ack) in std::mem::take(&mut self.pending) {
            let _ = ack.send(Err("browser transport ended".into()));
        }
    }
}

#[derive(Clone)]
pub(super) struct Client {
    pub run: String,
    pub tx: SyncSender<Event>,
}

impl Client {
    pub fn begin(&self, operation: Operation) -> Result<oneshot::Receiver<Reply>> {
        let (ack, reply) = oneshot::channel();
        self.tx
            .try_send(Event::BrowserIo(Call {
                run: self.run.clone(),
                operation,
                ack,
            }))
            .map_err(|_| "browser owner channel is unavailable or full")?;
        Ok(reply)
    }

    pub async fn call(&self, operation: Operation) -> Result<Value> {
        let reply = self.begin(operation)?;
        let value = tokio::time::timeout(Duration::from_secs(8), reply)
            .await
            .map_err(|_| "browser RPC deadline expired")?
            .map_err(|_| "browser RPC owner closed")?;
        value.map_err(Into::into)
    }
}

impl Owner {
    pub(super) fn browser_io_call(&mut self, call: Call) -> Result<()> {
        let result = (|| -> Result<u64> {
            let slot = self
                .browser_io
                .as_mut()
                .filter(|s| s.run == call.run)
                .ok_or("browser transport is unavailable")?;
            if slot.cleanup {
                return Err("browser cleanup is already pending".into());
            }
            if slot.events.is_none() && !matches!(call.operation, Operation::Detach { .. }) {
                return Err("browser event stream ended; only cleanup is available".into());
            }
            let operation = self
                .operation
                .as_ref()
                .filter(|op| op.run == call.run)
                .ok_or("browser transport run is stale")?;
            if operation.stop.load(std::sync::atomic::Ordering::Acquire)
                && !matches!(call.operation, Operation::Detach { .. })
            {
                return Err("browser operation stopped before command".into());
            }
            match &call.operation {
                Operation::Navigate {
                    action,
                    revision,
                    binding,
                    url,
                    destination,
                    ..
                } => {
                    let record = self
                        .state
                        .browser_actions
                        .get(action)
                        .ok_or("unknown navigation action")?;
                    if record.proposal.run != call.run
                        || record.revision != *revision
                        || record.state != crate::browser_action::Phase::Dispatching
                        || &record.proposal.binding != binding
                        || &record.proposal.url != url
                        || !matches!(&record.proposal.intent,crate::browser_action::Intent::Navigate { destination:approved } if approved==destination)
                        || binding.tab != slot.tab
                        || !slot.navigations.insert(action.clone())
                    {
                        return Err(
                            "navigation dispatch does not match a fresh durable approval".into(),
                        );
                    }
                }
                Operation::ActivateButton {
                    action,
                    revision,
                    binding,
                    url,
                    ..
                } => {
                    let record = self
                        .state
                        .browser_actions
                        .get(action)
                        .ok_or("unknown button action")?;
                    if record.proposal.run != call.run
                        || record.revision != *revision
                        || record.state != crate::browser_action::Phase::Dispatching
                        || &record.proposal.binding != binding
                        || &record.proposal.url != url
                        || !matches!(
                            &record.proposal.intent,
                            crate::browser_action::Intent::ActivateButton { .. }
                        )
                        || binding.tab != slot.tab
                        || !slot.buttons.insert(action.clone())
                    {
                        return Err(
                            "button dispatch does not match a fresh durable approval".into()
                        );
                    }
                }
                Operation::ConfirmNavigation {
                    action,
                    revision,
                    tab,
                    ..
                } => {
                    let record = self
                        .state
                        .browser_actions
                        .get(action)
                        .ok_or("unknown navigation action")?;
                    if record.proposal.run != call.run
                        || record.revision != *revision
                        || record.state != crate::browser_action::Phase::Dispatching
                        || *tab != slot.tab
                        || !slot.navigations.contains(action)
                    {
                        return Err("navigation confirmation is stale or was not dispatched".into());
                    }
                }
                Operation::Attach { tab, .. }
                | Operation::Command { tab, .. }
                | Operation::Detach { tab }
                    if *tab != slot.tab =>
                {
                    return Err("browser command does not match the owned tab".into());
                }
                _ => (),
            }
            if slot.pending.len() >= 64 {
                return Err("browser pending command limit".into());
            }
            slot.next = slot
                .next
                .checked_add(1)
                .ok_or("browser call identity exhausted")?;
            Ok(slot.next)
        })();
        let id = match result {
            Ok(id) => id,
            Err(error) => {
                let _ = call.ack.send(Err(error.to_string()));
                return Ok(());
            }
        };
        let slot = self.browser_io.as_mut().unwrap();
        if matches!(call.operation, Operation::Detach { .. }) {
            slot.cleanup = true;
            slot.end();
        }
        // Install the waiter before exposing the command to the extension.
        slot.pending.insert(id, call.ack);
        if let Err(error) = self.emit(
            json!({"v":crate::WIRE_VERSION,"kind":"browser_rpc","call":id,
            "run":call.run,"operation":call.operation}),
        ) {
            self.browser_io_finish(&call.run);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn browser_io_reply(&mut self, run: &str, call: u64, result: WireReply) {
        if let Some(slot) = self.browser_io.as_mut().filter(|s| s.run == run)
            && let Some(ack) = slot.pending.remove(&call)
        {
            let result = match result {
                WireReply::Ok { value } => Ok(value),
                WireReply::Error { message } => Err(message.chars().take(200).collect()),
            };
            let _ = ack.send(result);
        }
    }

    pub(super) fn browser_io_event(
        &mut self,
        run: &str,
        tab: i32,
        session: Option<String>,
        method: String,
        params: Value,
    ) {
        let Some(slot) = self
            .browser_io
            .as_ref()
            .filter(|s| s.run == run && !s.cleanup)
        else {
            return;
        };
        if slot.tab != tab
            || method.is_empty()
            || method.len() > 128
            || !params.is_object()
            || session
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 128)
            || method == "MetLDR.detached"
            || slot.events.as_ref().is_none_or(|events| {
                events
                    .try_send(Notice {
                        session,
                        method,
                        params,
                    })
                    .is_err()
            })
        {
            if let Some(slot) = self.browser_io.as_mut() {
                slot.end();
            }
        }
    }

    pub(super) fn browser_io_finish(&mut self, run: &str) {
        if self.browser_io.as_ref().is_some_and(|s| s.run == run)
            && let Some(mut slot) = self.browser_io.take()
        {
            slot.end();
        }
    }
}
