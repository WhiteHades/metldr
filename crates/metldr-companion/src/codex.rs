use crate::{
    Result,
    agent::{self, Outcome},
    browser_io::{Client, Notice},
    browser_worker::{BROWSER_POLICY, Tools, decode_call, definitions},
    codex_model::{Binding, Identity, Profile, Selection},
    flow::Event,
    model::Source,
};
use metldr_codex_session::{CodexSession, Thread, ToolSpec};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

mod approval;
mod review;
mod stream;

pub(super) struct Job {
    pub run: String,
    pub workflow: String,
    pub profile: Profile,
    pub binding: Binding,
    pub source: Source,
    pub target: crate::browser_action::Target,
    pub text: String,
}

pub(super) fn execute(
    job: Job,
    stop: Arc<AtomicBool>,
    tx: mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })
        .map(|runtime| runtime.block_on(execute_inner(&job, &stop, &tx, events)));
    let (result, released) = match result {
        Ok(result) => result,
        Err(error) => (Outcome::Failed(error.to_string()), true),
    };
    let _ = tx.send(Event::CodexSettled {
        run: job.run,
        result,
        released,
    });
}

async fn execute_inner(
    job: &Job,
    stop: &Arc<AtomicBool>,
    tx: &mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
) -> (Outcome, bool) {
    if stop.load(Ordering::Acquire) {
        return (Outcome::Cancelled, true);
    }
    let thread = match job.binding.resume() {
        Ok(Some(id)) => Thread::Resume(id),
        Ok(None) => Thread::New {
            tools: definitions()
                .into_iter()
                .map(|definition| ToolSpec {
                    name: definition.name.into(),
                    description: definition.description.into(),
                    input_schema: definition.input_schema,
                })
                .collect(),
        },
        Err(error) => return (Outcome::Failed(error.to_string()), false),
    };
    let mut session =
        match CodexSession::open(&job.profile.executable, &job.profile.workspace, thread).await {
            Ok(session) => session,
            Err(error) => return (Outcome::Failed(error.to_string()), false),
        };
    let mut tools = Tools::new(
        Client {
            run: job.run.clone(),
            tx: tx.clone(),
        },
        job.target.clone(),
        events,
        stop.clone(),
    );
    let result = stream::run(job, &mut session, &mut tools, stop, tx).await;
    let browser_closed = tools.close().await;
    match (browser_closed, session.close().await) {
        (Ok(()), Ok(exit)) if exit.code == Some(0) && !exit.forced && exit.drained => (
            result.unwrap_or_else(|error| Outcome::Failed(error.to_string())),
            true,
        ),
        (Err(_), _) => (
            Outcome::Failed("Browser release was not confirmed after the Codex turn".into()),
            false,
        ),
        (Ok(()), Ok(exit)) => (
            Outcome::Failed(format!(
                "Codex shutdown was not confirmed: exit {:?}, forced {}, drained {}",
                exit.code, exit.forced, exit.drained
            )),
            false,
        ),
        (Ok(()), Err(_)) => (
            Outcome::Failed("Codex process shutdown failed".into()),
            false,
        ),
    }
}
