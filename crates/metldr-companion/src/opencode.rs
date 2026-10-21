mod stream;

use crate::{
    Result,
    agent::{self, Outcome},
    browser_io::{Client, Notice},
    browser_mcp::Server,
    browser_worker::{BROWSER_POLICY, Tools},
    flow::Event,
    model::Source,
    opencode_model::{Binding, Identity, Profile, Selection},
};
use metldr_opencode_session::{McpServer, OpenCodeSession, Session};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

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
    let result = (|| -> Result<(Outcome, bool)> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(runtime.block_on(execute_inner(&job, stop, &tx, events)))
    })();
    let (result, released) =
        result.unwrap_or_else(|error| (Outcome::Failed(error.to_string()), true));
    let _ = tx.send(Event::OpenCodeSettled {
        run: job.run,
        result,
        released,
    });
}

async fn execute_inner(
    job: &Job,
    stop: Arc<AtomicBool>,
    tx: &mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
) -> (Outcome, bool) {
    if stop.load(Ordering::Acquire) {
        return (Outcome::Cancelled, true);
    }
    let server = match Server::bind(&job.run).await {
        Ok(server) => server,
        Err(error) => return (Outcome::Failed(error.to_string()), true),
    };
    let (address, token) = server.endpoint();
    let declarations = [McpServer::Http {
        name: "metldr".into(),
        url: format!("http://{address}/mcp"),
        headers: vec![("Authorization".into(), format!("Bearer {token}"))],
    }];
    let done = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let tools = Tools::new(
        Client {
            run: job.run.clone(),
            tx: tx.clone(),
        },
        job.target.clone(),
        events,
        stop.clone(),
    );
    let native = async {
        let result = native(job, &declarations, &stop, &failed, tx).await;
        done.store(true, Ordering::Release);
        result
    };
    let browser = async {
        let (result, released) = server
            .serve(tools, stop.clone(), done.clone(), failed.clone())
            .await;
        if result.is_err() || (!done.load(Ordering::Acquire) && !stop.load(Ordering::Acquire)) {
            failed.store(true, Ordering::Release);
        }
        (result, released)
    };
    let (native, (browser, browser_released)) = tokio::join!(native, browser);
    let released = browser_released && native.as_ref().is_ok_and(|(_, released)| *released);
    let outcome = match (native, browser) {
        (Ok((outcome, _)), Ok(())) => outcome,
        (Err(error), Ok(())) => Outcome::Failed(error.to_string()),
        (_, Err(error)) => Outcome::Failed(format!("OpenCode browser task failed: {error}")),
    };
    (outcome, released)
}

async fn native(
    job: &Job,
    servers: &[McpServer],
    stop: &AtomicBool,
    failed: &AtomicBool,
    tx: &mpsc::SyncSender<Event>,
) -> Result<(Outcome, bool)> {
    let session = match job.binding.resume()? {
        Some(id) => Session::Resume(id),
        None => Session::New,
    };
    let mut session = OpenCodeSession::open(
        &job.profile.executable,
        &job.profile.workspace,
        session,
        servers,
    )
    .await?;
    let result=async {
        if failed.load(Ordering::Acquire) { return Err("OpenCode browser service ended before its prompt".into()); }
        let identity=Identity {session:session.id().as_str().into(),selection:Selection::from_options(session.config_options())?};
        let (ack,receipt)=mpsc::sync_channel(1);
        tx.try_send(Event::OpenCodeBound {run:job.run.clone(),workflow:job.workflow.clone(),identity:identity.clone(),ack})
            .map_err(|_|"OpenCode binding owner is unavailable")?;
        let bound_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let ready = loop {
            match receipt.try_recv() {
                Ok(ready) => break ready?,
                Err(mpsc::TryRecvError::Disconnected) => return Err("OpenCode binding owner ended".into()),
                Err(mpsc::TryRecvError::Empty) => (),
            }
            if stop.load(Ordering::Acquire) { return Ok(Outcome::Cancelled); }
            if failed.load(Ordering::Acquire) || tokio::time::Instant::now() >= bound_deadline {
                return Err("OpenCode binding was not confirmed".into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        if !ready || stop.load(Ordering::Acquire) { return Ok(Outcome::Cancelled); }
        let message=format!("{}\n\n{BROWSER_POLICY}\nmetldr browser tools are available through tools.metldr in your normal execute tool. Native tool permissions and metldr browser approvals are separate decisions.",agent::message(&job.text,&job.source)?);
        session.prompt(&message).await?;
        stream::run(job,&mut session,&identity.selection,stop,failed,tx).await
    }.await;
    if result.is_err() {
        let _ = session.cancel().await;
    }
    let exit = session.close().await?;
    if exit.code != Some(0) || exit.forced || !exit.drained {
        return Err("OpenCode shutdown was not confirmed with drained output".into());
    }
    Ok((
        result.unwrap_or_else(|error: Box<dyn std::error::Error + Send + Sync>| {
            Outcome::Failed(error.to_string())
        }),
        true,
    ))
}
