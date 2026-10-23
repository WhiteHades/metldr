use super::*;
use crate::browser_action::{Direction, Point, Scroll, ScrollKind};

impl Actor {
    pub(super) fn prepare_scroll(
        &mut self,
        tool: String,
        handle: String,
        direction: metldr_browser_tools::ScrollDirection,
        pixels: u16,
        job: Job,
    ) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("observe the page before scrolling")?;
            self.contexts.validate(&snapshot.context)?;
            let target = snapshot.scroll_target(&handle)?;
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::scroll::read(
                    target.slot,
                    kind_name(target.kind),
                    &target.label,
                    &self.source.url,
                    &target.before.x,
                    &target.before.y,
                ),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::ReadScroll {
                    tool,
                    handle,
                    direction,
                    pixels,
                    job,
                },
                Some(context),
                Some(root),
            ),
            Err(error) => {
                self.finish(job, Err(error));
                Ok(())
            }
        }
    }

    pub(super) fn scroll_read(
        &mut self,
        tool: String,
        handle: String,
        direction: metldr_browser_tools::ScrollDirection,
        pixels: u16,
        job: Job,
        value: Value,
    ) -> Result<()> {
        let proposal = (|| -> Result<_> {
            let before: Point = serde_json::from_value(results::evaluation_value(&value)?)?;
            if !before.valid() {
                return Err("invalid browser scroll position".into());
            }
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            let target = snapshot.scroll_target(&handle)?;
            if before != target.before {
                return Err("scroll target changed before proposal".into());
            }
            Ok(Proposal {
                run: self.client.run.clone(),
                tool_use_id: tool,
                url: self.source.url.clone(),
                binding: self.binding(&snapshot.context),
                intent: Intent::Scroll {
                    scroll: Scroll {
                        handle,
                        kind: target.kind,
                        label: target.label.clone(),
                        direction: Direction::from(direction),
                        pixels,
                        before,
                    },
                },
            })
        })();
        match proposal {
            Ok(proposal) => self.value(job, proposal),
            Err(error) => {
                self.finish(job, Err(error));
                Ok(())
            }
        }
    }

    pub(super) fn scroll(&mut self, action: Action, job: Job) -> Result<()> {
        let setup = (|| -> Result<_> {
            let snapshot = self
                .snapshot
                .as_ref()
                .ok_or("browser snapshot unavailable")?;
            self.validate_action(&action, &snapshot.context)?;
            let Intent::Scroll { scroll } = &action.proposal.intent else {
                return Err("not a scroll action".into());
            };
            if !scroll.valid() {
                return Err("invalid approved scroll".into());
            }
            let target = snapshot.scroll_target(&scroll.handle)?;
            if target.kind != scroll.kind
                || target.label != scroll.label
                || target.before != scroll.before
            {
                return Err("approved scroll does not match its observation".into());
            }
            if self.seen.len() == 64 || !self.seen.insert((action.id.clone(), action.revision)) {
                return Err("browser action was reused or action capacity reached".into());
            }
            Ok((
                snapshot.context.clone(),
                snapshot.root.clone(),
                scripts::scroll::write(
                    target.slot,
                    kind_name(target.kind),
                    &target.label,
                    &self.source.url,
                    &scroll.before.x,
                    &scroll.before.y,
                    scroll.direction,
                    scroll.pixels,
                ),
            ))
        })();
        match setup {
            Ok((context, root, script)) => self.queue(
                "Runtime.callFunctionOn",
                session(&context),
                call_params(&context, &root, script),
                Work::WriteScroll { action, job },
                Some(context),
                Some(root),
            ),
            Err(error) => self.value(
                job,
                Outcome::Prevented {
                    reason: error.to_string(),
                },
            ),
        }
    }

    pub(super) fn scroll_written(&mut self, action: Action, job: Job, value: Value) -> Result<()> {
        let after = match results::evaluation_value(&value) {
            Ok(Value::Bool(false)) => {
                return self.value(
                    job,
                    Outcome::Prevented {
                        reason: "scroll target changed before dispatch".into(),
                    },
                );
            }
            Ok(value) => serde_json::from_value::<Point>(value)
                .ok()
                .filter(Point::valid),
            Err(_) => None,
        };
        let Some(after) = after else {
            return self.value(
                job,
                Outcome::Uncertain {
                    reason: "scroll dispatch result was lost".into(),
                },
            );
        };
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or("browser snapshot disappeared after scroll")?;
        let Intent::Scroll { scroll } = &action.proposal.intent else {
            return Err("invalid scroll verification intent".into());
        };
        let target = snapshot.scroll_target(&scroll.handle)?;
        let context = snapshot.context.clone();
        let root = snapshot.root.clone();
        self.queue(
            "Runtime.callFunctionOn",
            session(&context),
            call_params(
                &context,
                &root,
                scripts::scroll::verify(
                    target.slot,
                    kind_name(target.kind),
                    &target.label,
                    &self.source.url,
                    &scroll.before.x,
                    &scroll.before.y,
                    &after.x,
                    &after.y,
                ),
            ),
            Work::VerifyScroll { after, job },
            Some(context),
            Some(root),
        )
    }

    pub(super) fn scroll_verified(&mut self, after: Point, job: Job, value: Value) -> Result<()> {
        let verified = results::evaluation_value(&value)
            .and_then(|value| Ok(serde_json::from_value::<Option<Point>>(value)?))
            .is_ok_and(|point| point.as_ref() == Some(&after));
        self.value(
            job,
            if verified {
                Outcome::ScrollObserved { after }
            } else {
                Outcome::Uncertain {
                    reason: "scroll position could not be verified".into(),
                }
            },
        )
    }
}

fn kind_name(kind: ScrollKind) -> &'static str {
    match kind {
        ScrollKind::Page => "page",
        ScrollKind::Region => "region",
    }
}
