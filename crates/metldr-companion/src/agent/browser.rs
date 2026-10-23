use super::{Job, Outcome};
use crate::{
    Result,
    browser_io::{Client, Notice},
    browser_mcp::Server,
    browser_worker::Tools,
    flow::Event,
    native_correction::Receiver,
};
use metldr_pi_session::McpServer;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(super) fn name(run: &str) -> String {
    format!("metldr_{}", run.replace(':', "_"))
}

pub(crate) fn execute(
    job: Job,
    stop: Arc<AtomicBool>,
    tx: mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
    corrections: Receiver,
) {
    let run = job.run.clone();
    let result = (|| -> Result<(Outcome, bool)> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(runtime.block_on(execute_inner(job, stop, &tx, events, corrections)))
    })();
    let (result, released) =
        result.unwrap_or_else(|error| (Outcome::Failed(error.to_string()), true));
    let _ = tx.send(Event::PiSettled {
        run,
        result,
        released,
    });
}

async fn execute_inner(
    job: Job,
    stop: Arc<AtomicBool>,
    tx: &mpsc::SyncSender<Event>,
    events: tokio::sync::mpsc::Receiver<Notice>,
    corrections: Receiver,
) -> (Outcome, bool) {
    if stop.load(Ordering::Acquire) {
        return (Outcome::Cancelled, true);
    }
    let server = match Server::bind(&job.run).await {
        Ok(server) => server,
        Err(error) => return (Outcome::Failed(error.to_string()), true),
    };
    let (address, token) = server.endpoint();
    let ready = server.ready();
    let registration =
        McpServer::http(name(&job.run), address, token, job.registration_dir.clone());
    let registration = match registration {
        Ok(registration) => registration,
        Err(error) => return (Outcome::Failed(error.to_string()), true),
    };
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
    let worker_stop = stop.clone();
    let worker_failed = failed.clone();
    let worker_done = done.clone();
    let worker_tx = tx.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let mut corrections = corrections;
        let result = super::native(
            &job,
            &worker_stop,
            &worker_failed,
            &ready,
            &mut corrections,
            registration,
            &worker_tx,
        );
        let result = match (result, corrections.close()) {
            (Ok(result), Ok(())) => Ok(result),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        };
        worker_done.store(true, Ordering::Release);
        result
    });
    let native = async {
        match worker.await {
            Ok(result) => result,
            Err(error) => {
                failed.store(true, Ordering::Release);
                done.store(true, Ordering::Release);
                Err(error.into())
            }
        }
    };
    let browser = async {
        let result = server
            .serve(tools, stop.clone(), done.clone(), failed.clone())
            .await;
        if result.0.is_err() || (!done.load(Ordering::Acquire) && !stop.load(Ordering::Acquire)) {
            failed.store(true, Ordering::Release);
        }
        result
    };
    let (native, (browser, browser_released)) = tokio::join!(native, browser);
    let released = browser_released && native.as_ref().is_ok_and(|(_, released)| *released);
    let outcome = match (native, browser) {
        (Ok((outcome, _)), Ok(())) => outcome,
        (Err(error), Ok(())) => Outcome::Failed(error.to_string()),
        (_, Err(error)) => Outcome::Failed(format!("Pi browser task failed: {error}")),
    };
    (outcome, released)
}
