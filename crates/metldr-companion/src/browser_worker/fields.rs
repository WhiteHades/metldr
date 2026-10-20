use super::{Call, engine::Engine};
use crate::{
    Result,
    browser_action::Target,
    browser_action::{Action, Outcome, Phase},
    browser_io::{Client, Notice},
    browser_owner,
    flow::Event,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

mod navigation;
pub(crate) struct Tools {
    engine: Option<Engine>,
    events: Option<tokio::sync::mpsc::Receiver<Notice>>,
    client: Client,
    current: Target,
    stop: Arc<AtomicBool>,
    observation_generation: u64,
    completed: BTreeMap<String, (Call, Value)>,
    release_unknown: bool,
}

impl Tools {
    pub(crate) fn new(
        client: Client,
        source: Target,
        events: tokio::sync::mpsc::Receiver<Notice>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            engine: None,
            events: Some(events),
            client,
            current: source,
            stop,
            observation_generation: 0,
            completed: BTreeMap::new(),
            release_unknown: false,
        }
    }

    pub(crate) async fn dispatch(
        &mut self,
        call_id: &str,
        call: Call,
        stop: &AtomicBool,
    ) -> Result<Value> {
        if stop.load(Ordering::Acquire) {
            return Err("browser task stopped".into());
        }
        validate_call(call_id, &call)?;
        if let Some((previous, result)) = self.completed.get(call_id) {
            if previous == &call {
                return Ok(result.clone());
            }
            return Err("model reused a tool identity with changed arguments".into());
        }
        if self.completed.len() == super::MAX_TOOL_CALLS {
            return Err("browser task step limit reached".into());
        }
        self.attach().await?;
        if stop.load(Ordering::Acquire) {
            return Err("browser task stopped".into());
        }
        let result = match &call {
            Call::Observe => self.observe().await?,
            Call::Navigate { destination } => self.navigate(call_id, destination, stop).await?,
            Call::Change { handle, value } => self.change(call_id, handle, value, stop).await?,
        };
        self.completed
            .insert(call_id.to_owned(), (call, result.clone()));
        Ok(result)
    }

    async fn attach(&mut self) -> Result<()> {
        if self.engine.is_some() {
            return Ok(());
        }
        match Engine::attach(
            self.client.clone(),
            &self.current,
            self.events.take().ok_or("browser events unavailable")?,
            self.stop.clone(),
        )
        .await
        {
            Ok(engine) => {
                self.engine = Some(engine);
                Ok(())
            }
            Err(failure) => {
                self.release_unknown = !failure.released;
                Err(failure.message.into())
            }
        }
    }

    async fn observe(&mut self) -> Result<Value> {
        self.observation_generation = self
            .observation_generation
            .checked_add(1)
            .ok_or("browser observation capacity reached")?;
        self.engine
            .as_ref()
            .ok_or("browser is not attached")?
            .observe(self.observation_generation)
            .await
    }

    async fn change(
        &mut self,
        tool_use_id: &str,
        handle: &str,
        value: &str,
        stop: &AtomicBool,
    ) -> Result<Value> {
        let proposal = self
            .engine
            .as_ref()
            .ok_or("browser is not attached")?
            .prepare_change(tool_use_id, handle, value)
            .await?;
        let approved = action(&self.client, stop, |ack| browser_owner::Event::Proposed {
            proposal,
            ack,
        })
        .await?;
        let settled = match &approved.state {
            Phase::Approved => {
                let dispatched = action(&self.client, stop, |ack| browser_owner::Event::Dispatch {
                    action: approved.id.clone(),
                    revision: approved.revision,
                    ack,
                })
                .await?;
                let outcome = match self
                    .engine
                    .as_ref()
                    .ok_or("browser is not attached")?
                    .change(&dispatched)
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(_) => Outcome::Uncertain {
                        reason: "field dispatch result was lost".into(),
                    },
                };
                action_observed(&self.client, &dispatched, outcome).await?
            }
            Phase::Settled { .. } => approved,
            _ => return Err("cached browser action is not settled or approved".into()),
        };
        let Phase::Settled { outcome } = settled.state else {
            return Err("browser result not settled".into());
        };
        if matches!(outcome, Outcome::Uncertain { .. }) {
            return Err("browser effect is uncertain; automatic continuation stopped".into());
        }
        Ok(serde_json::to_value(outcome)?)
    }

    pub(crate) async fn close(mut self) -> Result<()> {
        let engine_result = match self.engine.take() {
            Some(engine) => engine.detach().await,
            None => Ok(()),
        };
        engine_result?;
        if self.release_unknown {
            return Err("browser release was not confirmed after failed attachment".into());
        }
        Ok(())
    }
}

fn validate_call(call_id: &str, call: &Call) -> Result<()> {
    if call_id.is_empty() || call_id.len() > 128 || call_id.chars().any(char::is_control) {
        return Err("invalid browser tool id".into());
    }
    match call {
        Call::Observe => Ok(()),
        Call::Navigate { destination }
            if !destination.is_empty()
                && destination.len() <= 2048
                && !destination.chars().any(char::is_control) =>
        {
            Ok(())
        }
        Call::Navigate { .. } => Err("invalid browser navigation URL".into()),
        Call::Change { handle, value }
            if !handle.is_empty()
                && handle.len() <= 64
                && handle
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                && value.len() <= 1024 =>
        {
            Ok(())
        }
        Call::Change { .. } => Err("invalid browser field change".into()),
    }
}

async fn action(
    client: &Client,
    stop: &AtomicBool,
    event: impl FnOnce(mpsc::SyncSender<std::result::Result<Action, String>>) -> browser_owner::Event,
) -> Result<Action> {
    let (ack, rx) = mpsc::sync_channel(1);
    client
        .tx
        .try_send(Event::Browser(event(ack)))
        .map_err(|_| "browser owner channel unavailable")?;
    loop {
        match rx.try_recv() {
            Ok(value) => return value.map_err(Into::into),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("browser approval owner ended".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if stop.load(Ordering::Acquire) {
            return Err("browser action cancelled".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn action_observed(client: &Client, action: &Action, outcome: Outcome) -> Result<Action> {
    let (ack, rx) = mpsc::sync_channel(1);
    client
        .tx
        .try_send(Event::Browser(browser_owner::Event::Observed {
            action: action.id.clone(),
            revision: action.revision,
            outcome,
            ack,
        }))
        .map_err(|_| "browser owner channel unavailable")?;
    rx.recv()
        .map_err(|_| "browser approval owner ended")?
        .map_err(Into::into)
}

#[cfg(test)]
mod failed_attach_check {
    use super::*;
    use crate::{browser_io::Operation, flow::Event as FlowEvent, model::Page};
    use std::sync::mpsc;

    fn source() -> Page {
        Page {
            tab: 7,
            url: "https://example.test/".into(),
            title: "Example".into(),
            document: "document".into(),
            text: "page text".into(),
        }
    }

    #[tokio::test]
    async fn stopped_calls_never_attach_or_return_cached_results() {
        let (tx, rx) = mpsc::sync_channel::<FlowEvent>(8);
        let client = Client {
            run: "stopped".into(),
            tx,
        };
        let (_notices, events) = tokio::sync::mpsc::channel(1);
        let mut tools = Tools::new(
            client,
            Target::from(&source()),
            events,
            Arc::new(AtomicBool::new(false)),
        );
        tools.completed.insert(
            "cached".into(),
            (Call::Observe, serde_json::json!({"cached":true})),
        );
        let stop = AtomicBool::new(true);
        assert!(
            tools
                .dispatch("cached", Call::Observe, &stop)
                .await
                .is_err()
        );
        assert!(tools.dispatch("fresh", Call::Observe, &stop).await.is_err());
        assert!(rx.try_recv().is_err(), "Stop allowed browser I/O");
    }

    #[tokio::test]
    async fn failed_attach_reports_whether_browser_release_was_confirmed() {
        for (release_succeeds, close_succeeds) in [(true, true), (false, false)] {
            let run = format!("failed-attach-{release_succeeds}");
            let (tx, rx) = mpsc::sync_channel::<FlowEvent>(8);
            let client = Client { run, tx };
            let owner = std::thread::spawn(move || {
                while let Ok(event) = rx.recv() {
                    let FlowEvent::BrowserIo(call) = event else {
                        panic!("unexpected event")
                    };
                    match call.operation {
                        Operation::Attach { .. } => {
                            let _ = call.ack.send(Err("attach failure".into()));
                        }
                        Operation::Detach { .. } => {
                            let result = if release_succeeds {
                                Ok(serde_json::json!({"released":true}))
                            } else {
                                Err("release failure".into())
                            };
                            let _ = call.ack.send(result);
                            break;
                        }
                        _ => panic!("unexpected command"),
                    }
                }
            });
            let (_notice_tx, notice_rx) = tokio::sync::mpsc::channel(1);
            let mut tools = Tools::new(
                client,
                Target::from(&source()),
                notice_rx,
                Arc::new(AtomicBool::new(false)),
            );
            let result = tools
                .dispatch("call-1", Call::Observe, &AtomicBool::new(false))
                .await;
            assert!(result.is_err());
            assert_eq!(
                tools.close().await.is_ok(),
                close_succeeds,
                "failed attachment cleanup result"
            );
            owner.join().unwrap();
        }
    }
}
