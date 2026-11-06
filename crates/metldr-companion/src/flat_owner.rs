use crate::{
    Result,
    agent_model::{Launch, digest},
    flow::{Event, Operation, Owner},
    history::{Edit, Execution, Selection},
    model::{ConversationAction, Run, Source, Status, Store, check_page_fields},
};

enum RecordProjection {
    View(Option<Store>),
    Execution(Execution),
}
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
        self.persist_record_change(|edit, selection| {
            let (result, projection) = change(edit, selection)?;
            Ok((result, RecordProjection::View(projection)))
        })
    }

    pub(super) fn record_execution<T>(
        &mut self,
        run: &str,
        change: impl FnOnce(&mut Edit<'_>, &Execution) -> Result<T>,
    ) -> Result<T> {
        let (result, publish) = self.persist_execution(run, change)?;
        if publish {
            self.publish()?;
        }
        Ok(result)
    }

    pub(super) fn persist_execution<T>(
        &mut self,
        run: &str,
        change: impl FnOnce(&mut Edit<'_>, &Execution) -> Result<T>,
    ) -> Result<(T, bool)> {
        self.persist_record_change(|edit, _| {
            let execution = edit.execution(run)?;
            let result = change(edit, &execution)?;
            Ok((result, RecordProjection::Execution(execution)))
        })
    }

    fn persist_record_change<T>(
        &mut self,
        change: impl FnOnce(&mut Edit<'_>, &Selection) -> Result<(T, RecordProjection)>,
    ) -> Result<(T, bool)> {
        let (history, view) = self.view_parts();
        let result = (|| {
            let selection = history.selection()?;
            history.edit(|edit| {
                let (result, request) = change(edit, &selection)?;
                let current = edit.selection();
                let projection = match request {
                    RecordProjection::View(projection) => {
                        if edit.changed() && projection.is_none() {
                            return Err(io::Error::other(
                                "record change has no publication projection",
                            )
                            .into());
                        }
                        projection
                    }
                    RecordProjection::Execution(execution) => {
                        if edit.changed() && execution.conversation() == current.conversation {
                            Some(edit.project(&current, Some(execution.run()))?)
                        } else {
                            None
                        }
                    }
                };
                let publish = edit.changed() && projection.is_some();
                let projection = projection
                    .map(|state| -> Result<_> {
                        let run_ids: Vec<_> =
                            state.view.runs.iter().map(|run| run.id.as_str()).collect();
                        let browser_view = edit.browser_view(&current, &run_ids)?;
                        let choices = edit.native_choices(
                            &current,
                            state.view.source.as_ref(),
                            &view.native_fingerprints()?,
                        )?;
                        view.preflight(&state, &browser_view, &current, &choices)?;
                        Ok((state, browser_view))
                    })
                    .transpose()?;
                Ok((result, projection, publish, current != selection))
            })
        })();
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
        self.require_conversation(scope)?;
        if !self.operations.is_empty() {
            return Err("another run is active".into());
        }
        let (run, body) = self.record_change(|edit, _| {
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
                    corrections: Vec::new(),
                },
                Launch::Fixture { input_digest },
            )?;
            Ok(((run.clone(), body), Some(edit.project(scope, Some(&run))?)))
        })?;
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
        self.operations.insert(
            run.clone(),
            Operation {
                stop,
                worker,
                correction: None,
            },
        );
        self.emit(self.started_reply(id, scope, &run)?)
    }

    pub(super) fn stop_run(&mut self, id: &str, run: &str) -> Result<()> {
        let terminal = self.record_change(|edit, scope| {
            let execution = edit.current_execution(scope, run)?;
            Ok((edit.run(&execution, run)?.status.terminal(), None))
        })?;
        if terminal {
            return self.reply_stop(id, "already_terminal", run);
        }
        let pending = self.detach_inputs(run);
        let saved = self.record_change(|edit, scope| {
            let execution = edit.current_execution(scope, run)?;
            edit.prevent_browser(&execution, run, "user stopped before dispatch")?;
            let mut record = edit.run(&execution, run)?;
            record.status = Status::StopRequested;
            edit.put_run(&execution, record)?;
            Ok(((), Some(edit.project(scope, Some(run))?)))
        });
        if let Some(operation) = self.operations.get(run) {
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
