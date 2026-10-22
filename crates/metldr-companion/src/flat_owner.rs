use crate::{
    Result,
    agent_model::{Launch, digest},
    flow::{Event, Operation, Owner},
    history::{Edit, Selection},
    model::{ConversationAction, Run, Source, Status, Store, check_page_fields},
};
use serde_json::{Value, json};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

impl Owner {
    pub(super) fn record_change<T>(
        &mut self,
        change: impl FnOnce(&mut Edit<'_>, &Selection) -> Result<(T, Option<Store>)>,
    ) -> Result<T> {
        let (result, changed) = self.persist_records(change)?;
        if changed {
            self.publish()?;
        }
        Ok(result)
    }

    pub(super) fn persist_records<T>(
        &mut self,
        change: impl FnOnce(&mut Edit<'_>, &Selection) -> Result<(T, Option<Store>)>,
    ) -> Result<(T, bool)> {
        let mut history = self
            .record_history
            .take()
            .ok_or("record history is unavailable")?;
        let result = (|| {
            let selection = history.selection()?;
            history.edit(|edit| {
                let (result, projection) = change(edit, &selection)?;
                let current = edit.selection();
                if edit.changed() && projection.is_none() {
                    return Err(
                        io::Error::other("record change has no publication projection").into(),
                    );
                }
                let projection = projection
                    .map(|state| -> Result<_> {
                        let run_ids: Vec<_> =
                            state.view.runs.iter().map(|run| run.id.as_str()).collect();
                        let browser_view = edit.browser_view(&current, &run_ids)?;
                        let choices = edit.native_choices(
                            &current,
                            state.view.source.as_ref(),
                            &self.native_fingerprints()?,
                        )?;
                        self.preflight_records(&state, &browser_view, &current, &choices)?;
                        Ok((state, browser_view))
                    })
                    .transpose()?;
                Ok((result, projection, edit.changed(), current != selection))
            })
        })();
        self.record_history = Some(history);
        let (result, projection, changed, selection_changed) = result?;
        if let Some((state, browser_view)) = projection {
            self.state = state;
            self.browser_view = browser_view;
        }
        if selection_changed {
            self.invalidate_history();
        }
        Ok((result, changed))
    }

    pub(super) fn fixture_start(
        &mut self,
        id: &str,
        scope: &Selection,
        start: ConversationAction,
    ) -> Result<()> {
        let fixture = self.fixture.as_ref().ok_or("fixture is not configured")?;
        let origin = fixture.origin.clone();
        let address = fixture.address;
        let input_digest =
            digest(&json!({"command":&start,"origin":origin,"model":"metldr-fixture"}))?;
        if self.duplicate(scope, id, &input_digest)? {
            return Ok(());
        }
        if self.record_history.is_some() {
            self.require_conversation(scope)?;
        }
        if self.operation.is_some() {
            return Err("another run is active".into());
        }
        let (run, body) = if self.record_history.is_some() {
            self.record_change(|edit, _| {
                let (kind, question) = match start {
                    ConversationAction::Summarize { page } => {
                        check_fixture_page(&page, &origin)?;
                        edit.new_source(scope, page)?;
                        ("summary", None)
                    }
                    ConversationAction::Ask { source, text } => {
                        check_question(&text)?;
                        if edit
                            .selected_source(scope)?
                            .is_none_or(|saved| saved.id != source)
                        {
                            return Err("source is not current saved source".into());
                        }
                        ("answer", Some(text))
                    }
                    _ => return Err("invalid fixture command".into()),
                };
                let source = edit.selected_source(scope)?.ok_or("no selected source")?;
                let context = edit.fixture_context(scope, &source.id)?;
                let body = fixture_body(&source, kind, question.as_deref(), context.iter())?;
                let run = edit.id()?;
                edit.admit(
                    scope,
                    id,
                    Run {
                        id: run.clone(),
                        source: source.id,
                        kind: kind.into(),
                        question,
                        status: Status::Running,
                        output: Default::default(),
                        error: None,
                    },
                    Launch::Fixture { input_digest },
                )?;
                Ok(((run.clone(), body), Some(edit.project(scope, Some(&run))?)))
            })?
        } else {
            // shortcut: remove snapshot admission when all production callers migrate to records.
            if self.state.view.runs.len() >= 16 {
                return Err("prototype run capacity reached".into());
            }
            let mut next = self.state.clone();
            let (kind, question) = match start {
                ConversationAction::Summarize { page } => {
                    check_fixture_page(&page, &origin)?;
                    let source_id = next.id()?;
                    next.view.source = Some(Source {
                        id: source_id,
                        page,
                    });
                    ("summary", None)
                }
                ConversationAction::Ask { source, text } => {
                    check_question(&text)?;
                    if next
                        .view
                        .source
                        .as_ref()
                        .is_none_or(|saved| saved.id != source)
                    {
                        return Err("source is not current saved source".into());
                    }
                    ("answer", Some(text))
                }
                _ => return Err("invalid fixture command".into()),
            };
            let source = next
                .view
                .source
                .as_ref()
                .ok_or("no selected source")?
                .clone();
            let context = next.view.runs.iter().filter(|run| {
                run.source == source.id
                    && run.status == Status::Complete
                    && matches!(next.launches.get(&run.id), Some(Launch::Fixture { .. }))
            });
            let body = fixture_body(&source, kind, question.as_deref(), context)?;
            let run = next.id()?;
            next.view.runs.push(Run {
                id: run.clone(),
                source: source.id,
                kind: kind.into(),
                question,
                status: Status::Running,
                output: Default::default(),
                error: None,
            });
            next.starts.insert(id.into(), run.clone());
            next.launches
                .insert(run.clone(), Launch::Fixture { input_digest });
            self.commit(next)?;
            (run, body)
        };
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_run = run.clone();
        let tx = self.tx.clone();
        let worker = thread::spawn(move || {
            let result = crate::provider::provider(address, &body, &worker_run, &worker_stop, &tx)
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
        self.emit(self.started_reply(id, scope, &run)?)
    }

    pub(super) fn stop_record(&mut self, id: &str, run: &str) -> Result<()> {
        let terminal =
            self.record_change(|edit, scope| Ok((edit.run(scope, run)?.status.terminal(), None)))?;
        if terminal {
            return self.reply_stop(id, "already_terminal", run);
        }
        let pending = self.detach_inputs(run);
        let saved = self.record_change(|edit, scope| {
            edit.prevent_browser(scope, run, "user stopped before dispatch")?;
            let mut record = edit.run(scope, run)?;
            record.status = Status::StopRequested;
            edit.put_run(scope, record)?;
            Ok(((), Some(edit.project(scope, Some(run))?)))
        });
        if let Some(operation) = &self.operation
            && operation.run == run
        {
            operation.stop.store(true, Ordering::Release);
        }
        if let Err(error) = saved {
            self.restore_inputs(pending);
            return Err(error);
        }
        self.cancel_browser_approval(run);
        self.cancel_inputs(run, pending);
        self.reply_stop(id, "stopping", run)
    }
}

fn check_question(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > 1024 {
        return Err("invalid question".into());
    }
    Ok(())
}

fn check_fixture_page(page: &crate::model::Page, origin: &str) -> Result<()> {
    check_page_fields(page)?;
    if page.url != format!("{origin}/article") && page.url != format!("{origin}/decoy") {
        return Err("page is outside the owned fixture grant".into());
    }
    Ok(())
}

fn fixture_body<'a>(
    source: &Source,
    kind: &str,
    question: Option<&str>,
    history: impl Iterator<Item = &'a Run>,
) -> Result<Value> {
    let mut history: Vec<_> = history
        .map(|run| json!({"kind":run.kind,"question":run.question,"output":run.output.text()}))
        .collect();
    loop {
        let body = json!({"model":"metldr-fixture","instruction":if kind == "summary" {"summarize"} else {"answer"},
            "source":{"title":source.page.title,"url":source.page.url,"text":source.page.text},"question":question,"history":history});
        if serde_json::to_vec(&body)?.len() <= 32768 {
            return Ok(body);
        }
        if history.is_empty() {
            return Err("fixture source and request exceed the input limit".into());
        }
        history.remove(0);
    }
}
