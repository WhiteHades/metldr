use super::*;
use metldr_codex_session::{Approval, ApprovalId, ApprovalKind};
use serde_json::json;

struct Pending {
    request: Approval,
    key: u64,
    answer: mpsc::Receiver<Option<String>>,
    queued_answer: Option<Option<String>>,
}

#[derive(Default)]
pub(super) struct Review {
    pending: Vec<Pending>,
    next_key: u64,
    pub cancelled: bool,
}

impl Review {
    pub(super) fn has_answer(&mut self) -> Result<bool> {
        let mut ready = false;
        for pending in &mut self.pending {
            if pending.queued_answer.is_none() {
                match pending.answer.try_recv() {
                    Ok(answer) => pending.queued_answer = Some(answer),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("Codex approval owner ended without a decision".into());
                    }
                }
            }
            ready |= pending.queued_answer.is_some();
        }
        Ok(ready)
    }

    pub async fn request(
        &mut self,
        request: Approval,
        session: &mut CodexSession,
        job: &Job,
        tx: &mpsc::SyncSender<Event>,
        stopping: bool,
    ) -> Result<()> {
        if stopping {
            self.cancelled |= session.reply_approval(&request, None).await?;
            return Ok(());
        }
        let input = super::review::render(request.kind(), request.review());
        let title = match request.kind() {
            ApprovalKind::Command => "Codex wants to run a command",
            ApprovalKind::FileChange => "Codex wants to change files",
            ApprovalKind::Permissions => "Codex requests additional permissions",
        };
        let details = json!({"agent":"codex","tool":match request.kind() { ApprovalKind::Command=>"Command",ApprovalKind::FileChange=>"File change",ApprovalKind::Permissions=>"Permissions" },"title":title,"input":input,"options":request.choices().iter().map(|choice|json!({"id":choice.id,"name":choice.name})).collect::<Vec<_>>()});
        if self.pending.len() >= 8 || serde_json::to_vec(&details)?.len() > 8192 {
            session.reply_approval(&request, None).await?;
            return Err("Complete Codex approval review exceeds application bounds".into());
        }
        self.next_key = self
            .next_key
            .checked_add(1)
            .ok_or("Codex approval key exhausted")?;
        let (ack, answer) = mpsc::sync_channel(1);
        tx.send(Event::NativePermission {
            run: job.run.clone(),
            key: self.next_key,
            details,
            ack,
        })?;
        self.pending.push(Pending {
            request,
            key: self.next_key,
            answer,
            queued_answer: None,
        });
        Ok(())
    }

    pub fn resolved(
        &mut self,
        id: &ApprovalId,
        job: &Job,
        tx: &mpsc::SyncSender<Event>,
    ) -> Result<()> {
        if let Some(index) = self.pending.iter().position(|p| p.request.id() == id) {
            let pending = self.pending.swap_remove(index);
            tx.send(Event::NativePermissionResolved {
                run: job.run.clone(),
                key: pending.key,
            })?;
        }
        Ok(())
    }

    pub async fn poll(&mut self, session: &mut CodexSession, stopping: bool) -> Result<()> {
        if self.cancelled {
            return Ok(());
        }
        let mut index = 0;
        while index < self.pending.len() {
            let answer = if stopping {
                Some(None)
            } else if let Some(answer) = self.pending[index].queued_answer.take() {
                Some(answer)
            } else {
                match self.pending[index].answer.try_recv() {
                    Ok(answer) => Some(answer),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("Codex approval owner ended without a decision".into());
                    }
                }
            };
            if let Some(answer) = answer {
                let pending = self.pending.swap_remove(index);
                self.cancelled |= session
                    .reply_approval(&pending.request, answer.as_deref())
                    .await?;
                if self.cancelled {
                    return Ok(());
                }
            } else {
                index += 1;
            }
        }
        Ok(())
    }

    pub fn finish(
        &mut self,
        job: &Job,
        tx: &mpsc::SyncSender<Event>,
        completed: bool,
    ) -> Result<()> {
        if completed && !self.pending.is_empty() {
            return Err("Codex completed with unanswered native approvals".into());
        }
        for pending in self.pending.drain(..) {
            tx.send(Event::NativePermissionResolved {
                run: job.run.clone(),
                key: pending.key,
            })?;
        }
        Ok(())
    }
}
