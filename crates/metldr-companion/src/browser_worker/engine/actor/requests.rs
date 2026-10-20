use super::*;
impl Actor {
    pub(super) fn request(&mut self, request: Request) -> Result<()> {
        let Request { kind, job } = request;
        if self.busy {
            let _ = job
                .reply
                .send(Err("another browser operation is pending".into()));
            return Ok(());
        }
        self.busy = true;
        match kind {
            Kind::Observe { generation } => {
                if generation <= self.generation {
                    self.finish(job, Err("observation generation was reused".into()));
                    return Ok(());
                }
                self.generation = generation;
                let context = match self.context() {
                    Ok(c) => c,
                    Err(e) => {
                        self.finish(job, Err(e));
                        return Ok(());
                    }
                };
                if let Some(snapshot) = &self.snapshot {
                    if snapshot.context == context {
                        return self.queue(
                            "Runtime.releaseObject",
                            session(&context),
                            json!({"objectId":snapshot.root}),
                            Work::ReleaseObserve { generation, job },
                            Some(context),
                            Some(snapshot.root.clone()),
                        );
                    }
                }
                self.snapshot = None;
                self.capture(context, generation, job)
            }
            Kind::PrepareChange {
                tool,
                handle,
                value,
            } => {
                let setup = (|| -> Result<_> {
                    let snapshot = self
                        .snapshot
                        .as_ref()
                        .ok_or("observe the page before changing a field")?;
                    self.contexts.validate(&snapshot.context)?;
                    let field = snapshot.field(&handle)?;
                    Ok((
                        snapshot.context.clone(),
                        snapshot.root.clone(),
                        scripts::read_value(field.slot, &field.check_label),
                    ))
                })();
                match setup {
                    Ok((context, root, script)) => self.queue(
                        "Runtime.callFunctionOn",
                        session(&context),
                        call_params(&context, &root, script),
                        Work::ReadField {
                            tool,
                            handle,
                            value,
                            job,
                        },
                        Some(context),
                        Some(root),
                    ),
                    Err(e) => {
                        self.finish(job, Err(e));
                        Ok(())
                    }
                }
            }
            Kind::Change(action) => {
                let setup = (|| -> Result<_> {
                    let snapshot = self
                        .snapshot
                        .as_ref()
                        .ok_or("browser snapshot unavailable")?;
                    self.validate_action(&action, &snapshot.context)?;
                    let Intent::Field {
                        field_id,
                        field_label,
                        previous_value,
                        value,
                    } = &action.proposal.intent
                    else {
                        return Err("not a field action".into());
                    };
                    let field = snapshot.field(field_id)?;
                    if &field.label != field_label {
                        return Err("approved field label changed".into());
                    }
                    if self.seen.len() == 64
                        || !self.seen.insert((action.id.clone(), action.revision))
                    {
                        return Err("browser action was reused or action capacity reached".into());
                    }
                    Ok((
                        snapshot.context.clone(),
                        snapshot.root.clone(),
                        scripts::write_value(field.slot, &field.check_label, previous_value, value),
                    ))
                })();
                match setup {
                    Ok((context, root, script)) => self.queue(
                        "Runtime.callFunctionOn",
                        session(&context),
                        call_params(&context, &root, script),
                        Work::Write { action, job },
                        Some(context),
                        Some(root),
                    ),
                    Err(e) => self.value(
                        job,
                        Outcome::Prevented {
                            reason: e.to_string(),
                        },
                    ),
                }
            }
            Kind::PrepareNavigation { tool, destination } => {
                let result = (|| -> Result<Value> {
                    let context = self.context()?;
                    if !crate::browser_action::valid_destination(&self.source.url, &destination) {
                        return Err("navigation destination is not permitted".into());
                    }
                    Ok(serde_json::to_value(Proposal {
                        run: self.client.run.clone(),
                        tool_use_id: tool,
                        url: self.source.url.clone(),
                        binding: self.binding(&context),
                        intent: Intent::Navigate { destination },
                    })?)
                })();
                self.finish(job, result);
                Ok(())
            }
            Kind::Navigate(action) => {
                let setup = (|| -> Result<_> {
                    let context = self.context()?;
                    self.validate_action(&action, &context)?;
                    let Intent::Navigate { destination } = &action.proposal.intent else {
                        return Err("not a navigation action".into());
                    };
                    if !crate::browser_action::valid_destination(&self.source.url, destination) {
                        return Err("navigation destination is not permitted".into());
                    }
                    if self.seen.len() == 64
                        || !self.seen.insert((action.id.clone(), action.revision))
                    {
                        return Err("browser action was reused or action capacity reached".into());
                    }
                    Ok((context, destination.clone()))
                })();
                match setup {
                    Ok((context, destination)) => {
                        self.snapshot = None;
                        self.queue(
                            "Page.navigate",
                            None,
                            json!({"url":destination}),
                            Work::Navigate {
                                action,
                                old: context.clone(),
                                job,
                            },
                            Some(context),
                            None,
                        )
                    }
                    Err(e) => self.value(
                        job,
                        Outcome::Prevented {
                            reason: e.to_string(),
                        },
                    ),
                }
            }
        }
    }
    pub(super) fn capture(
        &mut self,
        context: FrameContext,
        generation: u64,
        job: Job,
    ) -> Result<()> {
        self.queue("Runtime.evaluate",session(&context),json!({"expression":format!("({})()",scripts::capture()),"returnByValue":false,"awaitPromise":true,"uniqueContextId":context.unique_context_id()}),Work::Capture{generation,job},Some(context),None)
    }
}
