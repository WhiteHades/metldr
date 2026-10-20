use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirmation {
    document: String,
    url: String,
    grant_revision: u64,
    frame: String,
    loader: String,
}
impl Actor {
    pub(super) fn navigated(
        &mut self,
        action: Action,
        old: FrameContext,
        job: Job,
        value: Value,
    ) -> Result<()> {
        if value.get("errorText").is_some() || value["isDownload"] == true {
            self.finish(job, Err("navigation failed or started a download".into()));
            return Ok(());
        }
        let frame = value["frameId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let loader = value["loaderId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        match (frame, loader) {
            (Some(frame), Some(loader)) => {
                self.navigation = Some(Navigation {
                    action,
                    old,
                    frame,
                    loader,
                    job,
                });
                Ok(())
            }
            _ => {
                self.finish(job, Err("navigation did not create a new document".into()));
                Ok(())
            }
        }
    }
    pub(super) fn advance_navigation(&mut self) -> Result<()> {
        let Some(nav) = &self.navigation else {
            return Ok(());
        };
        let Some(context) = self.contexts.main_context() else {
            return Ok(());
        };
        if context.unique_context_id() == nav.old.unique_context_id() || !self.contexts.is_ready() {
            return Ok(());
        }
        let nav = self.navigation.take().unwrap();
        if context.target_id() != nav.old.target_id()
            || context.frame_id() != nav.old.frame_id()
            || context.session_id() != nav.old.session_id()
        {
            self.finish(
                nav.job,
                Err("navigation did not produce the expected fresh context".into()),
            );
            return Ok(());
        }
        self.queue(
            "Page.getFrameTree",
            None,
            json!({}),
            Work::FrameTree(nav),
            Some(context),
            None,
        )
    }
    pub(super) fn navigation_tree(
        &mut self,
        nav: Navigation,
        context: FrameContext,
        value: Value,
    ) -> Result<()> {
        let Intent::Navigate { destination } = &nav.action.proposal.intent else {
            return Err("invalid navigation intent".into());
        };
        let frame = &value["frameTree"]["frame"];
        if frame["id"] != nav.frame
            || frame["loaderId"] != nav.loader
            || frame["url"] != destination.as_str()
            || nav.frame != context.frame_id()
        {
            self.finish(
                nav.job,
                Err("navigation response does not match the destination".into()),
            );
            return Ok(());
        }
        self.contexts.validate(&context)?;
        if self.stop.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return Err("browser stopped before destination confirmation".into());
        }
        let reply = self.client.begin(Operation::ConfirmNavigation {
            action: nav.action.id.clone(),
            revision: nav.action.revision,
            tab: self.source.tab,
            frame: nav.frame.clone(),
            loader: nav.loader.clone(),
        })?;
        let deadline = nav.job.deadline;
        self.inflight.push(
            async move {
                (
                    Completion::Confirmation(nav, context),
                    wait_reply(deadline, reply).await,
                )
            }
            .boxed(),
        );
        Ok(())
    }
    pub(super) fn confirmed(
        &mut self,
        nav: Navigation,
        context: FrameContext,
        result: runtime::Result<Value>,
    ) -> Result<()> {
        let outcome = (|| -> Result<Outcome> {
            self.contexts.validate(&context)?;
            let confirmed: Confirmation = serde_json::from_value(
                result.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?,
            )?;
            let Intent::Navigate { destination } = &nav.action.proposal.intent else {
                return Err("invalid navigation intent".into());
            };
            let from = &nav.action.proposal.binding;
            if confirmed.url != *destination
                || confirmed.frame != from.frame
                || confirmed.loader != nav.loader
                || confirmed.document.is_empty()
                || confirmed.document.len() > 128
                || confirmed.document == from.document
                || from.grant_revision.checked_add(1) != Some(confirmed.grant_revision)
            {
                return Err("invalid browser navigation confirmation".into());
            }
            self.source.document = confirmed.document.clone();
            self.source.url = confirmed.url.clone();
            self.grant_revision = confirmed.grant_revision;
            Ok(Outcome::Navigated {
                binding: Binding {
                    document: confirmed.document,
                    unique_context: context.unique_context_id().into(),
                    grant_revision: confirmed.grant_revision,
                    ..from.clone()
                },
                url: confirmed.url,
                loader: confirmed.loader,
            })
        })();
        match outcome {
            Ok(value) => self.value(nav.job, value),
            Err(e) => {
                self.finish(nav.job, Err(e));
                Ok(())
            }
        }
    }
}
