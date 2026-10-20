use crate::{
    Result,
    browser_action::{Action, Outcome, Proposal, Target},
    browser_io::{Client, Notice, Operation},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};

mod actor;
mod observation;
mod runtime;
mod scripts;

const TIMEOUT: Duration = Duration::from_secs(8);
type Reply = oneshot::Sender<runtime::Result<Value>>;
struct Job {
    reply: Reply,
    deadline: Instant,
}
struct Request {
    kind: Kind,
    job: Job,
}
enum Kind {
    Observe {
        generation: u64,
    },
    PrepareChange {
        tool: String,
        handle: String,
        value: String,
    },
    Change(Action),
    PrepareNavigation {
        tool: String,
        destination: String,
    },
    Navigate(Action),
}

pub(super) struct AttachFailure {
    pub(super) message: String,
    pub(super) released: bool,
}
pub(super) struct Engine {
    requests: mpsc::Sender<Request>,
    stop: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    task: JoinHandle<Result<()>>,
    client: Client,
    tab: i32,
}

impl Engine {
    pub(super) async fn attach(
        client: Client,
        source: &Target,
        events: mpsc::Receiver<Notice>,
        stop: Arc<AtomicBool>,
    ) -> std::result::Result<Self, AttachFailure> {
        let deadline = Instant::now() + TIMEOUT;
        let setup = async {
            if stop.load(Ordering::Acquire) {
                return Err("browser task stopped".into());
            }
            let attached = client
                .call(Operation::Attach {
                    tab: source.tab,
                    document: source.document.clone(),
                    url: source.url.clone(),
                })
                .await?;
            if attached["grant_revision"] != 1 {
                return Err("invalid browser grant revision".into());
            }
            let target: chromiumoxide::cdp::browser_protocol::target::TargetInfo =
                serde_json::from_value(attached["targetInfo"].clone())?;
            if target.url != source.url || target.r#type != "page" {
                return Err("browser target changed during attachment".into());
            }
            let transport = runtime::Transport::connect(deadline).await?;
            let contexts = runtime::ContextMap::new(target.target_id.as_ref())?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((transport, contexts))
        };
        let (transport, contexts) = match tokio::time::timeout_at(deadline, setup).await {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                return Err(release_error(
                    &client,
                    source.tab,
                    format!("browser initialization failed: {error}"),
                )
                .await);
            }
            Err(_) => {
                return Err(release_error(
                    &client,
                    source.tab,
                    "browser initialization deadline expired".into(),
                )
                .await);
            }
        };
        let (requests, rx) = mpsc::channel(64);
        let (ready, readiness) = oneshot::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut task = tokio::spawn(
            actor::Actor::new(
                client.clone(),
                source.clone(),
                transport,
                contexts,
                events,
                rx,
                stop.clone(),
                shutdown.clone(),
                ready,
                deadline,
            )
            .run(),
        );
        match tokio::time::timeout_at(deadline, readiness).await {
            Ok(Ok(Ok(()))) => Ok(Self {
                requests,
                stop,
                shutdown,
                task,
                client,
                tab: source.tab,
            }),
            _ => {
                shutdown.store(true, Ordering::Release);
                task.abort();
                let _ = (&mut task).await;
                Err(release_error(&client, source.tab, "browser readiness failed".into()).await)
            }
        }
    }

    async fn request<T: DeserializeOwned>(&self, kind: Kind) -> Result<T> {
        if self.stop.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return Err("browser task stopped".into());
        }
        let deadline = Instant::now() + TIMEOUT;
        let (reply, rx) = oneshot::channel();
        self.requests
            .try_send(Request {
                kind,
                job: Job { reply, deadline },
            })
            .map_err(|_| "browser actor is unavailable or full")?;
        let value = tokio::time::timeout_at(deadline, rx)
            .await
            .map_err(|_| "browser operation deadline expired")?
            .map_err(|_| "browser actor ended")?
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
        Ok(serde_json::from_value(value)?)
    }
    pub(super) async fn observe(&self, generation: u64) -> Result<Value> {
        self.request(Kind::Observe { generation }).await
    }
    pub(super) async fn prepare_change(
        &self,
        tool: &str,
        handle: &str,
        value: &str,
    ) -> Result<Proposal> {
        self.request(Kind::PrepareChange {
            tool: tool.into(),
            handle: handle.into(),
            value: value.into(),
        })
        .await
    }
    pub(super) async fn change(&self, action: &Action) -> Result<Outcome> {
        self.request(Kind::Change(action.clone())).await
    }
    pub(super) async fn prepare_navigation(
        &self,
        tool: &str,
        destination: &str,
    ) -> Result<Proposal> {
        self.request(Kind::PrepareNavigation {
            tool: tool.into(),
            destination: destination.into(),
        })
        .await
    }
    pub(super) async fn navigate(&self, action: &Action) -> Result<Outcome> {
        self.request(Kind::Navigate(action.clone())).await
    }
    pub(super) async fn detach(mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::Release);
        let task = match tokio::time::timeout(TIMEOUT, &mut self.task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) if e.is_cancelled() => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => {
                self.task.abort();
                let _ = (&mut self.task).await;
                Err("browser actor cleanup deadline expired".into())
            }
        };
        let task = task.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("browser actor cleanup failed: {e}").into()
        });
        let detached = self
            .client
            .call(Operation::Detach { tab: self.tab })
            .await
            .map(|_| ())
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                format!("browser detach was not confirmed: {e}").into()
            });
        match (task, detached) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
            (Err(a), Err(b)) => Err(format!("{a}; browser detach failed: {b}").into()),
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.task.abort();
    }
}
async fn release_error(client: &Client, tab: i32, message: String) -> AttachFailure {
    match client.call(Operation::Detach { tab }).await {
        Ok(_) => AttachFailure {
            message,
            released: true,
        },
        Err(e) => AttachFailure {
            message: format!("{message}; browser detach failed: {e}"),
            released: false,
        },
    }
}
