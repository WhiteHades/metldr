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
mod admission;
mod observation;
mod runtime;
mod scripts;

const TIMEOUT: Duration = Duration::from_secs(8);
type Reply = oneshot::Sender<runtime::Result<Value>>;
struct Job {
    reply: Reply,
    deadline: Instant,
    admission: Option<Arc<admission::Admission>>,
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
    PrepareSelection {
        tool: String,
        handle: String,
        option: String,
    },
    SelectOption(Action),
    PrepareButton {
        tool: String,
        handle: String,
    },
    ActivateButton(Action),
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
    task: JoinHandle<(Result<()>, Result<()>)>,
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
        self.request_with(kind, None).await
    }
    async fn request_with<T: DeserializeOwned>(
        &self,
        kind: Kind,
        admission: Option<Arc<admission::Admission>>,
    ) -> Result<T> {
        if self.stop.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return Err("browser task stopped".into());
        }
        let deadline = Instant::now() + TIMEOUT;
        let (reply, rx) = oneshot::channel();
        self.requests
            .try_send(Request {
                kind,
                job: Job {
                    reply,
                    deadline,
                    admission,
                },
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
    pub(super) async fn prepare_selection(
        &self,
        tool: &str,
        handle: &str,
        option: &str,
    ) -> Result<Proposal> {
        self.request(Kind::PrepareSelection {
            tool: tool.into(),
            handle: handle.into(),
            option: option.into(),
        })
        .await
    }
    async fn effect(&self, kind: Kind, lost_reason: &str) -> Outcome {
        let guard = admission::CancelOnDrop::default();
        match self.request_with(kind, Some(guard.0.clone())).await {
            Ok(outcome) => outcome,
            Err(error) if guard.0.cancel() => Outcome::Prevented {
                reason: format!("browser action was not dispatched: {error}"),
            },
            Err(_) => Outcome::Uncertain {
                reason: lost_reason.into(),
            },
        }
    }
    pub(super) async fn change(&self, action: &Action) -> Outcome {
        self.effect(
            Kind::Change(action.clone()),
            "field dispatch result was lost",
        )
        .await
    }
    pub(super) async fn select_option(&self, action: &Action) -> Outcome {
        self.effect(
            Kind::SelectOption(action.clone()),
            "selection dispatch result was lost",
        )
        .await
    }
    pub(super) async fn prepare_button(&self, tool: &str, handle: &str) -> Result<Proposal> {
        self.request(Kind::PrepareButton {
            tool: tool.into(),
            handle: handle.into(),
        })
        .await
    }
    pub(super) async fn activate_button(&self, action: &Action) -> Outcome {
        self.effect(
            Kind::ActivateButton(action.clone()),
            "button input result was lost",
        )
        .await
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
    pub(super) async fn navigate(&self, action: &Action) -> Outcome {
        self.effect(
            Kind::Navigate(action.clone()),
            "navigation destination or effect could not be verified",
        )
        .await
    }
    pub(super) async fn detach(mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::Release);
        let task = match tokio::time::timeout(TIMEOUT, &mut self.task).await {
            // Requests own execution outcomes; release still requires both cleanup checks.
            Ok(Ok((_, cleanup))) => cleanup,
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
