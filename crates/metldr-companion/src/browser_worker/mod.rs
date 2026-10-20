mod engine;
mod fields;
mod protocol;

pub(crate) use fields::Tools;
pub(crate) use metldr_browser_tools::{BROWSER_POLICY, Call, decode_call, definitions};

use crate::{
    Result,
    browser_io::{Client, Slot},
    flow::{Event, Operation, Owner},
    https_profile::Profile,
    model::{Run, Status},
};
use serde_json::json;
use std::{
    sync::{Arc, atomic::AtomicBool},
    thread,
};

use metldr_browser_tools::Invocation;

const MAX_TOOL_CALLS: usize = 8;

enum Route {
    Messages(Profile),
    Responses(Profile),
    Google(Profile),
    Chat(Profile),
}

impl Route {
    fn public(&self) -> serde_json::Value {
        match self {
            Self::Messages(profile) => profile.public(),
            Self::Responses(Profile::Https { binding, .. }) => binding.public_responses(),
            Self::Responses(Profile::Fixture { .. }) => unreachable!("Responses requires HTTPS"),
            Self::Google(Profile::Https { binding, .. }) => binding.public_google(),
            Self::Google(Profile::Fixture { .. }) => unreachable!("Google requires HTTPS"),
            Self::Chat(Profile::Https { binding, .. }) => binding.public_chat_completions(),
            Self::Chat(Profile::Fixture { .. }) => unreachable!("Chat requires HTTPS"),
        }
    }
    fn launch(&self, digest: String) -> crate::agent_model::Launch {
        use crate::agent_model::Launch;
        match self {
            Self::Messages(Profile::Fixture { .. }) => Launch::Messages {
                input_digest: digest,
                model: crate::https_profile::FIXTURE_MODEL.into(),
            },
            Self::Messages(Profile::Https { binding, .. }) => Launch::ConfiguredMessages {
                input_digest: digest,
                binding: binding.clone(),
            },
            Self::Responses(Profile::Https { binding, .. }) => Launch::ConfiguredResponses {
                input_digest: digest,
                binding: binding.clone(),
            },
            Self::Google(Profile::Https { binding, .. }) => Launch::ConfiguredGoogle {
                input_digest: digest,
                binding: binding.clone(),
            },
            Self::Chat(Profile::Https { binding, .. }) => Launch::ConfiguredChat {
                input_digest: digest,
                binding: binding.clone(),
            },
            Self::Google(Profile::Fixture { .. }) => unreachable!("Google requires HTTPS"),
            Self::Responses(Profile::Fixture { .. }) => unreachable!("Responses requires HTTPS"),
            Self::Chat(Profile::Fixture { .. }) => unreachable!("Chat requires HTTPS"),
        }
    }
}

impl Owner {
    pub(super) fn browser_start(
        &mut self,
        id: &str,
        source_id: &str,
        text: &str,
        fingerprint: Option<&str>,
    ) -> Result<()> {
        let profile = self
            .browser_route
            .clone()
            .ok_or("Messages browser route is unavailable")?;
        match &profile {
            Profile::Fixture { .. } if fingerprint.is_some() => {
                return Err("Messages fixture route does not accept a fingerprint".into());
            }
            Profile::Https { binding, .. } if fingerprint != Some(binding.fingerprint.as_str()) => {
                return Err("Messages route configuration changed".into());
            }
            _ => {}
        }
        self.start_browser(id, source_id, text, Route::Messages(profile))
    }

    pub(super) fn responses_start(
        &mut self,
        id: &str,
        source_id: &str,
        text: &str,
        fingerprint: &str,
    ) -> Result<()> {
        let profile = self
            .responses_route
            .clone()
            .ok_or("Responses browser route is unavailable")?;
        if !matches!(&profile, Profile::Https { binding, .. } if binding.fingerprint == fingerprint)
        {
            return Err("Responses route configuration changed".into());
        }
        self.start_browser(id, source_id, text, Route::Responses(profile))
    }

    pub(super) fn google_start(
        &mut self,
        id: &str,
        source_id: &str,
        text: &str,
        fingerprint: &str,
    ) -> Result<()> {
        let profile = self
            .google_route
            .clone()
            .ok_or("Google browser route is unavailable")?;
        if !matches!(&profile, Profile::Https { binding, .. } if binding.fingerprint == fingerprint)
        {
            return Err("Google route configuration changed".into());
        }
        self.start_browser(id, source_id, text, Route::Google(profile))
    }

    pub(super) fn chat_start(
        &mut self,
        id: &str,
        source_id: &str,
        text: &str,
        fingerprint: &str,
    ) -> Result<()> {
        let profile = self
            .chat_route
            .clone()
            .ok_or("Chat Completions browser route is unavailable")?;
        if !matches!(&profile, Profile::Https { binding, .. } if binding.fingerprint == fingerprint)
        {
            return Err("Chat Completions route configuration changed".into());
        }
        self.start_browser(id, source_id, text, Route::Chat(profile))
    }

    fn start_browser(&mut self, id: &str, source_id: &str, text: &str, route: Route) -> Result<()> {
        let identity = match &route {
            Route::Messages(Profile::Fixture { .. }) => {
                json!({"source":source_id,"text":text,"origin":self.fixture.as_ref().map(|fixture| fixture.origin.as_str()),"model":crate::https_profile::FIXTURE_MODEL})
            }
            _ => {
                json!({"source":source_id,"text":text,"origin":self.fixture.as_ref().map(|fixture| fixture.origin.as_str()),"route":route.public()})
            }
        };
        let digest = crate::agent_model::digest(&identity)?;
        if self.duplicate(id, &digest)? {
            return Ok(());
        }
        if self.operation.is_some() || self.state.view.runs.len() >= 16 {
            return Err("another run is active or prototype capacity reached".into());
        }
        if text.trim().is_empty() || text.len() > 1024 {
            return Err("invalid browser task instruction".into());
        }
        let source = self
            .state
            .view
            .source
            .as_ref()
            .filter(|s| s.id == source_id)
            .ok_or("browser task source is stale")?
            .clone();
        let mut next = self.state.clone();
        let run = next.id()?;
        next.view.runs.push(Run {
            id: run.clone(),
            source: source.id,
            kind: "browser".into(),
            question: Some(text.into()),
            status: Status::Running,
            output: Default::default(),
            error: None,
        });
        next.starts.insert(id.into(), run.clone());
        next.launches.insert(run.clone(), route.launch(digest));
        self.commit(next)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let client = Client {
            run: run.clone(),
            tx: self.tx.clone(),
        };
        let (slot, events) = Slot::new(run.clone(), source.page.tab);
        self.browser_io = Some(slot);
        let worker_run = run.clone();
        let instruction = text.to_owned();
        let tx = self.tx.clone();
        let worker = thread::spawn(move || {
            let result = (|| -> Result<()> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(async {
                    let protocol =
                        protocol::Protocol::open(&route, &source.page, &instruction, &worker_stop)?;
                    let mut tools = Tools::new(
                        client.clone(),
                        (&source.page).into(),
                        events,
                        worker_stop.clone(),
                    );
                    let result = execute(protocol, &mut tools, &client, &worker_stop).await;
                    let detached = tools.close().await;
                    match (result, detached) {
                        (_, Err(error)) => Err(error),
                        (Err(error), Ok(())) => Err(error),
                        (Ok(()), Ok(())) => Ok(()),
                    }
                })
            })()
            .map_err(|e| e.to_string());
            let _ = tx.send(Event::Settled {
                run: worker_run,
                result,
            });
        });
        self.operation = Some(Operation {
            run: run.clone(),
            stop,
            worker,
        });
        self.emit(json!({"v":4,"request":id,"kind":"started","run":run}))
    }
}

async fn execute(
    mut protocol: protocol::Protocol,
    tools: &mut Tools,
    client: &Client,
    stop: &AtomicBool,
) -> Result<()> {
    let mut emitted = 0usize;
    for used in 0..=MAX_TOOL_CALLS {
        let invocation = protocol
            .next(stop, MAX_TOOL_CALLS - used, |text| {
                emitted = emitted.saturating_add(text.len());
                if emitted > crate::TEXT_MAX {
                    return Err("browser output exceeds bound".into());
                }
                if text.is_empty() {
                    return Ok(());
                }
                client
                    .tx
                    .try_send(Event::Text {
                        run: client.run.clone(),
                        change: metldr_conversation::Change::FlatAppend(text),
                    })
                    .map_err(|_| "owner output channel unavailable".into())
            })
            .await?;
        let Some(invocation) = invocation else {
            return Ok(());
        };
        let result = tools
            .dispatch(&invocation.id, invocation.call, stop)
            .await?;
        protocol.continuation(&result)?;
    }
    Err("browser task step limit reached".into())
}
