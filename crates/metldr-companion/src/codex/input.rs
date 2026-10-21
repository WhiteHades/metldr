use super::*;
use metldr_codex_session::{Input, InputAnswer, InputId};

struct Pending {
    input: Input,
    answer: Option<mpsc::Receiver<InputAnswer>>,
    queued: Option<InputAnswer>,
    answered: bool,
    offered: bool,
}

#[derive(Default)]
pub(super) struct Inputs {
    pending: Vec<Pending>,
}

impl Inputs {
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn request(
        &mut self,
        input: Input,
        job: &Job,
        tx: &mpsc::SyncSender<Event>,
        stopping: bool,
    ) -> Result<()> {
        let answer = if stopping {
            None
        } else {
            let (answer, receive) = mpsc::sync_channel(1);
            tx.send(Event::CodexInput {
                run: job.run.clone(),
                key: input.id().clone(),
                prompt: input.prompt().clone(),
                answer,
            })?;
            Some(receive)
        };
        self.pending.push(Pending {
            input,
            answer,
            queued: None,
            answered: false,
            offered: !stopping,
        });
        Ok(())
    }

    pub fn has_answer(&mut self, stop: &AtomicBool) -> Result<bool> {
        let mut ready = false;
        for pending in &mut self.pending {
            if pending.queued.is_none() {
                pending.queued = receive(pending.answer.as_ref(), stop)?;
            }
            ready |= pending.queued.is_some();
        }
        Ok(ready)
    }

    pub async fn poll(
        &mut self,
        session: &mut CodexSession,
        job: &Job,
        tx: &mpsc::SyncSender<Event>,
        stopping: bool,
        stop: &AtomicBool,
    ) -> Result<()> {
        if stopping || stop.load(Ordering::Acquire) {
            self.withdraw(job, tx)?;
            for pending in &mut self.pending {
                if !pending.answered && pending.input.turn().is_none() {
                    session
                        .reply_input(&pending.input, InputAnswer::Cancel)
                        .await?;
                    pending.answered = true;
                }
            }
            return Ok(());
        }
        for pending in &mut self.pending {
            let answer = match pending.queued.take() {
                Some(answer) => Some(answer),
                None => receive(pending.answer.as_ref(), stop)?,
            };
            if let Some(answer) = answer {
                if stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                session.reply_input(&pending.input, answer).await?;
                pending.answered = true;
                pending.answer = None;
            }
        }
        Ok(())
    }

    pub fn resolved(
        &mut self,
        key: &InputId,
        job: &Job,
        tx: &mpsc::SyncSender<Event>,
    ) -> Result<()> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| pending.input.id() == key)
        {
            let pending = self.pending.swap_remove(index);
            if pending.offered {
                tx.send(Event::CodexInputWithdraw {
                    run: job.run.clone(),
                    key: key.clone(),
                })?;
            }
        }
        Ok(())
    }

    pub fn withdraw(&mut self, job: &Job, tx: &mpsc::SyncSender<Event>) -> Result<()> {
        for pending in &mut self.pending {
            pending.answer = None;
            pending.queued = None;
            if pending.offered {
                tx.send(Event::CodexInputWithdraw {
                    run: job.run.clone(),
                    key: pending.input.id().clone(),
                })?;
                pending.offered = false;
            }
        }
        Ok(())
    }
}

fn receive(
    answer: Option<&mpsc::Receiver<InputAnswer>>,
    stop: &AtomicBool,
) -> Result<Option<InputAnswer>> {
    let Some(answer) = answer else {
        return Ok(None);
    };
    match answer.try_recv() {
        Ok(answer) => Ok(Some(answer)),
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) if stop.load(Ordering::Acquire) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => Err("Codex input controller disconnected".into()),
    }
}
