use crate::{
    Result,
    agent::ui::{Answer, Change, Dialog, Display},
    flow::Owner,
    model::Status,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::mpsc::{SyncSender, TrySendError},
};

#[derive(Clone)]
struct Pending {
    key: u64,
    dialog: Dialog,
    answer: SyncSender<Answer>,
}

#[derive(Clone)]
pub(super) struct State {
    pub(super) run: String,
    pending: BTreeMap<String, Pending>,
    title: Option<String>,
    statuses: BTreeMap<String, String>,
    widgets: BTreeMap<String, Value>,
    editor: Option<Value>,
    notices: Vec<Value>,
    notices_omitted: u64,
}

impl State {
    fn new(run: &str) -> Self {
        Self {
            run: run.into(),
            pending: BTreeMap::new(),
            title: None,
            statuses: BTreeMap::new(),
            widgets: BTreeMap::new(),
            editor: None,
            notices: Vec::new(),
            notices_omitted: 0,
        }
    }

    pub(super) fn public(&self) -> Result<Value> {
        let value = json!({"run":self.run,
            "pending":self.pending.iter().map(|(request,p)| json!({"request":request,"dialog":p.dialog})).collect::<Vec<_>>(),
            "title":self.title,
            "statuses":self.statuses.iter().map(|(key,text)| json!({"key":key,"text":text})).collect::<Vec<_>>(),
            "widgets":self.widgets.iter().map(|(key,widget)| json!({"key":key,"lines":widget["lines"],"placement":widget["placement"]})).collect::<Vec<_>>(),
            "editor":self.editor,"notices":self.notices,"notices_omitted":self.notices_omitted});
        if serde_json::to_vec(&value)?.len() > 32768 {
            return Err("Pi UI display exceeds 32 KiB".into());
        }
        Ok(value)
    }

    fn display(&mut self, display: Display) -> Result<()> {
        match display {
            Display::Notify { message, level } => {
                if self.notices.len() == 32 {
                    self.notices.remove(0);
                    self.notices_omitted = self.notices_omitted.saturating_add(1);
                }
                self.notices.push(json!({"level":level,"message":message}));
            }
            Display::Status { key, text } => match text {
                Some(text) => {
                    self.statuses.insert(key, text);
                }
                None => {
                    self.statuses.remove(&key);
                }
            },
            Display::Widget {
                key,
                lines,
                placement,
            } => match lines {
                Some(lines) => {
                    self.widgets
                        .insert(key, json!({"lines":lines,"placement":placement}));
                }
                None => {
                    self.widgets.remove(&key);
                }
            },
            Display::Title(title) => self.title = Some(title),
            Display::EditorText { revision, text } => {
                self.editor = Some(json!({"revision":revision,"text":text}))
            }
        }
        if self.statuses.len() > 16 || self.widgets.len() > 8 {
            return Err("Pi UI display has too many entries".into());
        }
        self.public()?;
        Ok(())
    }
}

impl Owner {
    pub(super) fn pi_ui_change(&mut self, run: &str, change: Change) -> Result<()> {
        let active = self.operation.as_ref().is_some_and(|op| op.run == run)
            && self
                .state
                .launches
                .get(run)
                .is_some_and(|launch| matches!(launch, crate::agent_model::Launch::Pi { .. }));
        if !active {
            return Ok(());
        }
        let previous = self.pi_ui.clone();
        let result = (|| -> Result<()> {
            if self.pi_ui.as_ref().is_none_or(|ui| ui.run != run) {
                self.pi_ui = Some(State::new(run));
            }
            let mut next = self.state.clone();
            match change {
                Change::Offer {
                    key,
                    dialog,
                    answer,
                } => {
                    if !next
                        .view
                        .runs
                        .iter()
                        .any(|record| record.id == run && record.status == Status::Running)
                    {
                        return Ok(());
                    }
                    let request = next.id()?;
                    let ui = self.pi_ui.as_mut().ok_or("missing Pi UI state")?;
                    if ui.pending.len() >= 8 || ui.pending.values().any(|p| p.key == key) {
                        return Err("invalid pending Pi UI offer".into());
                    }
                    ui.pending.insert(
                        request,
                        Pending {
                            key,
                            dialog,
                            answer,
                        },
                    );
                }
                Change::Withdraw { key } => {
                    self.pi_ui
                        .as_mut()
                        .ok_or("missing Pi UI state")?
                        .pending
                        .retain(|_, p| p.key != key);
                }
                Change::Display(display) => self
                    .pi_ui
                    .as_mut()
                    .ok_or("missing Pi UI state")?
                    .display(display)?,
            }
            self.pi_ui.as_ref().ok_or("missing Pi UI state")?.public()?;
            self.commit(next)
        })();
        if let Err(error) = result {
            if let Some(op) = &self.operation {
                op.stop.store(true, std::sync::atomic::Ordering::Release);
            }
            self.pi_ui = previous;
            let mut next = self.state.clone();
            if let Some(record) = next.view.runs.iter_mut().find(|r| r.id == run) {
                record.status = Status::StopRequested;
                record.error = Some(error.to_string().chars().take(200).collect());
            }
            next.prevent_browser(run, "Pi UI could not be presented")?;
            self.cancel_browser_approval(run);
            self.clear_pi_ui(run);
            self.commit(next)?;
        }
        Ok(())
    }

    pub(super) fn pi_ui_reply(
        &mut self,
        id: &str,
        run: &str,
        request: &str,
        answer: Answer,
    ) -> Result<()> {
        if self
            .operation
            .as_ref()
            .is_none_or(|op| op.run != run || op.stop.load(std::sync::atomic::Ordering::Acquire))
            || !self
                .state
                .view
                .runs
                .iter()
                .any(|r| r.id == run && r.status == Status::Running)
        {
            return Err("Pi UI run is no longer accepting replies".into());
        }
        let ui = self
            .pi_ui
            .as_ref()
            .filter(|ui| ui.run == run)
            .ok_or("stale Pi UI run")?;
        let pending = ui.pending.get(request).ok_or("stale Pi UI request")?;
        if !pending.dialog.accepts(&answer) {
            return Err("Pi UI answer was not offered".into());
        }
        if let Answer::Text(value) = &answer
            && serde_json::to_vec(value)?.len() > 32768 - 512
        {
            return Err("Pi UI reply is too long".into());
        }
        let pending = self
            .pi_ui
            .as_mut()
            .ok_or("missing Pi UI state")?
            .pending
            .remove(request)
            .ok_or("stale Pi UI request")?;
        if let Err(error) = self.commit(self.state.clone()) {
            self.pi_ui
                .as_mut()
                .ok_or("missing Pi UI state")?
                .pending
                .insert(request.into(), pending);
            return Err(error);
        }
        match pending.answer.try_send(answer) {
            Ok(()) => self.reply_view(Some(id), "view", true),
            Err(TrySendError::Disconnected(_)) => Err("Pi UI request has already ended".into()),
            Err(TrySendError::Full(_)) => Err("Pi UI reply is already queued".into()),
        }
    }

    pub(super) fn clear_pi_ui(&mut self, run: &str) {
        if let Some(ui) = self.pi_ui.as_mut().filter(|ui| ui.run == run) {
            ui.pending.clear();
            ui.title = None;
            ui.statuses.clear();
            ui.widgets.clear();
        }
    }

    pub(super) fn disconnect_pi_ui(&mut self) -> Result<()> {
        let Some(op) = self.operation.as_ref().filter(|op| {
            matches!(
                self.state.launches.get(&op.run),
                Some(crate::agent_model::Launch::Pi { .. })
            )
        }) else {
            return Ok(());
        };
        let run = op.run.clone();
        // Stop must be visible to the worker before its reply channels disappear.
        op.stop.store(true, std::sync::atomic::Ordering::Release);
        let mut next = self.state.clone();
        if let Some(record) = next.view.runs.iter_mut().find(|r| r.id == run)
            && !record.status.terminal()
        {
            record.status = Status::StopRequested;
        }
        next.prevent_browser(&run, "Pi controller disconnected")?;
        self.cancel_browser_approval(&run);
        self.cancel_native_permissions(&run);
        self.clear_pi_ui(&run);
        self.persist(next)
    }
}
