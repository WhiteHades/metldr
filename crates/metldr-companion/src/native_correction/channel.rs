use super::Phase;
use crate::{Result, agent_model::Launch, agent_settings::Kind, flow::Event};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    Codex(metldr_codex_session::TurnId),
    Pi,
}
impl Target {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Codex(_) => Kind::Codex,
            Self::Pi => Kind::Pi,
        }
    }
    pub fn matches(&self, launch: &Launch) -> bool {
        matches!(
            (self, launch),
            (Self::Codex(_), Launch::Codex { .. }) | (Self::Pi, Launch::Pi { .. })
        )
    }
}

pub(crate) struct Command {
    pub request: String,
    pub text: String,
    pub target: Target,
}

pub(crate) struct Slot {
    pub send: SyncSender<Command>,
    pub ready: Option<Target>,
}

pub(crate) struct Receiver {
    run: String,
    receive: mpsc::Receiver<Command>,
    tx: SyncSender<Event>,
    open: bool,
}

pub(crate) fn channel(run: &str, tx: &SyncSender<Event>) -> (Slot, Receiver) {
    let (send, receive) = mpsc::sync_channel(1);
    (
        Slot { send, ready: None },
        Receiver {
            run: run.into(),
            receive,
            tx: tx.clone(),
            open: false,
        },
    )
}

impl Receiver {
    pub fn ready(&mut self, target: Target) -> Result<()> {
        self.tx.send(Event::CorrectionReady {
            run: self.run.clone(),
            target,
        })?;
        self.open = true;
        Ok(())
    }

    pub fn take(&self) -> Result<Option<Command>> {
        match self.receive.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err("correction owner disconnected".into()),
        }
    }

    pub fn authorize(&self, command: &Command, target: &Target, stop: &AtomicBool) -> Result<bool> {
        if !self.open || command.target != *target || stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        let (ack, result) = mpsc::sync_channel(1);
        self.tx.send(Event::CorrectionDispatch {
            run: self.run.clone(),
            request: command.request.clone(),
            target: target.clone(),
            ack,
        })?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Ok(false);
            }
            match result.recv_timeout(Duration::from_millis(20)) {
                Ok(allowed) => return Ok(allowed && !stop.load(Ordering::Acquire)),
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(false),
            }
        }
    }

    pub fn finish(&self, request: &str, outcome: Phase) -> Result<()> {
        self.tx.send(Event::CorrectionFinished {
            run: self.run.clone(),
            request: request.into(),
            outcome,
        })?;
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        if self.open {
            self.open = false;
            self.tx.send(Event::CorrectionClosed {
                run: self.run.clone(),
            })?;
        }
        Ok(())
    }
}
