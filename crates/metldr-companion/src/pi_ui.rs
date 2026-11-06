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

#[derive(Clone, Default)]
pub(super) struct Active {
    pending: BTreeMap<String, Pending>,
    title: Option<String>,
    statuses: BTreeMap<String, String>,
    widgets: BTreeMap<String, Value>,
}

#[derive(Clone, Default)]
pub(super) struct State {
    active: Active,
    editor: Option<Value>,
    notices: Vec<Value>,
    notices_omitted: u64,
}

impl State {
    pub(super) fn detach(&mut self) -> Active {
        std::mem::take(&mut self.active)
    }

    pub(super) fn restore(&mut self, active: Active) {
        self.active = active;
    }

    pub(super) fn public(&self, run: &str) -> Result<Value> {
        let value = json!({"run":run,
            "pending":self.active.pending.iter().map(|(request,p)| json!({"request":request,"dialog":p.dialog})).collect::<Vec<_>>(),
            "title":self.active.title,
            "statuses":self.active.statuses.iter().map(|(key,text)| json!({"key":key,"text":text})).collect::<Vec<_>>(),
            "widgets":self.active.widgets.iter().map(|(key,widget)| json!({"key":key,"lines":widget["lines"],"placement":widget["placement"]})).collect::<Vec<_>>(),
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
                    self.active.statuses.insert(key, text);
                }
                None => {
                    self.active.statuses.remove(&key);
                }
            },
            Display::Widget {
                key,
                lines,
                placement,
            } => match lines {
                Some(lines) => {
                    self.active
                        .widgets
                        .insert(key, json!({"lines":lines,"placement":placement}));
                }
                None => {
                    self.active.widgets.remove(&key);
                }
            },
            Display::Title(title) => self.active.title = Some(title),
            Display::EditorText { revision, text } => {
                self.editor = Some(json!({"revision":revision,"text":text}))
            }
        }
        if self.active.statuses.len() > 16 || self.active.widgets.len() > 8 {
            return Err("Pi UI display has too many entries".into());
        }
        Ok(())
    }
}

impl Owner {
    pub(super) fn pi_ui_change(&mut self, run: &str, change: Change) -> Result<()> {
        let status = self.native_input_status(run, crate::agent_settings::Kind::Pi)?;
        if status.is_none()
            || (status == Some(Status::StopRequested)
                && self.operations.get(run).is_none_or(|operation| {
                    !operation.stop.load(std::sync::atomic::Ordering::Acquire)
                }))
        {
            return Ok(());
        }
        let mut next = self.native_input_state(run)?;
        let active = self.operations.contains_key(run)
            && next
                .launches
                .get(run)
                .is_some_and(|launch| matches!(launch, crate::agent_model::Launch::Pi { .. }));
        if !active {
            return Ok(());
        }
        let previous = self.pi_ui.get(run).cloned();
        let result = (|| -> Result<()> {
            self.pi_ui.entry(run.into()).or_default();
            match change {
                Change::Offer {
                    key,
                    dialog,
                    answer,
                } => {
                    if !self.native_input_active(run, crate::agent_settings::Kind::Pi)? {
                        return Ok(());
                    }
                    let request = next.id()?;
                    let ui = self.pi_ui.get_mut(run).ok_or("missing Pi UI state")?;
                    if ui.active.pending.len() >= 8
                        || ui.active.pending.values().any(|p| p.key == key)
                    {
                        return Err("invalid pending Pi UI offer".into());
                    }
                    ui.active.pending.insert(
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
                        .get_mut(run)
                        .ok_or("missing Pi UI state")?
                        .active
                        .pending
                        .retain(|_, p| p.key != key);
                }
                Change::Display(display) => self
                    .pi_ui
                    .get_mut(run)
                    .ok_or("missing Pi UI state")?
                    .display(display)?,
            }
            self.pi_ui
                .get(run)
                .ok_or("missing Pi UI state")?
                .public(run)?;
            self.commit_native_input(run, crate::agent_settings::Kind::Pi, next)
        })();
        if let Err(error) = result {
            if crate::flow::fatal(error.as_ref()) {
                return Err(error);
            }
            match previous {
                Some(state) => {
                    self.pi_ui.insert(run.into(), state);
                }
                None => {
                    self.pi_ui.remove(run);
                }
            }
            self.stop_native_record_inputs(
                run,
                "Pi UI could not be presented",
                Some(&error.to_string()),
                true,
            )?;
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
        if !self.native_input_active(run, crate::agent_settings::Kind::Pi)? {
            return Err("Pi UI run is no longer accepting replies".into());
        }
        let ui = self.pi_ui.get(run).ok_or("stale Pi UI run")?;
        let pending = ui
            .active
            .pending
            .get(request)
            .ok_or("stale Pi UI request")?;
        if !pending.dialog.accepts(&answer) {
            return Err("Pi UI answer was not offered".into());
        }
        if let Answer::Text(value) = &answer
            && serde_json::to_vec(value)?.len() > 32768 - 512
        {
            return Err("Pi UI reply is too long".into());
        }
        let next = self.native_input_state(run)?;
        let pending = self
            .pi_ui
            .get_mut(run)
            .ok_or("missing Pi UI state")?
            .active
            .pending
            .remove(request)
            .ok_or("stale Pi UI request")?;
        if let Err(error) = self.commit_native_input(run, crate::agent_settings::Kind::Pi, next) {
            if !crate::flow::fatal(error.as_ref()) {
                self.pi_ui
                    .get_mut(run)
                    .ok_or("missing Pi UI state")?
                    .active
                    .pending
                    .insert(request.into(), pending);
            }
            return Err(error);
        }
        match pending.answer.try_send(answer) {
            Ok(()) => self.reply_view(Some(id), "view", true),
            Err(TrySendError::Disconnected(_)) => Err("Pi UI request has already ended".into()),
            Err(TrySendError::Full(_)) => Err("Pi UI reply is already queued".into()),
        }
    }

    pub(super) fn prune_pi_ui(&mut self) {
        let visible = self
            .state
            .view
            .runs
            .iter()
            .find(|run| self.pi_ui.contains_key(&run.id))
            .map(|run| run.id.as_str());
        let operations = &self.operations;
        self.pi_ui
            .retain(|run, _| operations.contains_key(run) || Some(run.as_str()) == visible);
    }

    pub(super) fn disconnect_pi_ui(&mut self) -> Result<()> {
        let runs: Vec<_> = self.operations.keys().cloned().collect();
        for run in runs {
            let pi = self.record_history.edit(|edit| {
                let Some(execution) = edit.native_execution(&run)? else {
                    return Ok(false);
                };
                Ok(matches!(
                    edit.native_task(&execution, &run)?.launch,
                    crate::agent_model::Launch::Pi { .. }
                ))
            })?;
            if !pi {
                continue;
            }
            self.stop_native_record_inputs(&run, "Pi controller disconnected", None, false)?;
        }
        Ok(())
    }
}
