use super::{Edit, Selection, record, storage};
use crate::{
    Result,
    browser_action::Action,
    history::schema::*,
    model::{Source, Status},
    tool_report::Preview,
};
use metldr_history_pages::Counter;
use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub anchor: Counter,
    pub before: Counter,
}

#[derive(Serialize)]
pub(crate) struct RunSummary {
    pub id: String,
    pub source: String,
    pub source_available: bool,
    pub revision: Counter,
    pub kind: String,
    pub status: Status,
    pub question: Option<Preview>,
    pub output: Preview,
}

#[derive(Serialize)]
pub(crate) struct RunPage {
    pub store: String,
    pub conversation: String,
    pub epoch: Counter,
    pub sequence: Counter,
    pub anchor: Counter,
    pub runs: Vec<RunSummary>,
    pub next: Option<Cursor>,
}

pub(crate) struct Observed<T> {
    pub revision: Counter,
    pub value: T,
}

impl Edit<'_> {
    pub(crate) fn sequence(&self) -> Result<Counter> {
        self.checked(
            self.profile
                .sequence
                .checked_add(u64::from(self.changed))
                .map(Counter)
                .ok_or_else(|| "view sequence capacity reached".into()),
        )
    }
    pub(crate) fn runs_page(&self, scope: &Selection, cursor: Option<&Cursor>) -> Result<RunPage> {
        self.checked((|| {
            self.require_selection(scope)?;
            let anchor = cursor.map_or(self.profile.next - 1, |cursor| cursor.anchor.0);
            let before = cursor.map(|cursor| cursor.before.0);
            if anchor >= self.profile.next
                || cursor.is_some_and(|cursor| cursor.before.0 == 0 || cursor.before.0 > anchor)
            {
                return Err("invalid history cursor".into());
            }
            let mut page = RunPage {
                store: scope.store.clone(),
                conversation: scope.conversation.clone(),
                epoch: Counter(scope.epoch),
                sequence: self.sequence()?,
                anchor: Counter(anchor),
                runs: Vec::new(),
                next: None,
            };
            let mut end = before.map_or(anchor, |value| value - 1);
            let mut last = None;
            let mut more = false;
            loop {
                let entry = storage(|| {
                    let order = self.write.open_table(RUN_ORDER)?;
                    let entry = order
                        .range(
                            (scope.conversation.as_str(), 0)..=(scope.conversation.as_str(), end),
                        )?
                        .next_back()
                        .transpose()?;
                    Ok(entry.map(|(key, id)| (key.value().1, id.value().to_owned())))
                })?;
                let Some((number, id)) = entry else { break };
                if page.runs.len() == 16 {
                    more = true;
                    break;
                }
                let saved = storage(|| {
                    self.saved_run(&id)?
                        .ok_or_else(|| "listed run is absent".into())
                })?;
                storage(|| {
                    if saved.conversation != scope.conversation
                        || id
                            .rsplit(':')
                            .next()
                            .and_then(|value| value.parse::<u64>().ok())
                            != Some(number)
                    {
                        return Err("listed run order or ownership differs".into());
                    }
                    Ok(())
                })?;
                let run = saved.value;
                page.runs.push(RunSummary {
                    id: run.id,
                    source_available: self.source(&run.source)?.is_some(),
                    source: run.source,
                    revision: Counter(saved.revision),
                    kind: run.kind,
                    status: run.status,
                    question: run.question.as_deref().map(|text| Preview::text(text, 256)),
                    output: Preview::text(&run.output.text(), 512),
                });
                page.next = Some(Cursor {
                    anchor: Counter(anchor),
                    before: Counter(number),
                });
                if serde_json::to_vec(&page)?.len() > 32_768 {
                    page.runs.pop();
                    if page.runs.is_empty() {
                        return Err("history summary exceeds page capacity".into());
                    }
                    more = true;
                    break;
                }
                last = Some(number);
                end = number - 1;
            }
            page.next = if more {
                Some(Cursor {
                    anchor: Counter(anchor),
                    before: Counter(last.ok_or("history cursor is absent")?),
                })
            } else {
                None
            };
            Ok(page)
        })())
    }

    pub(crate) fn run_detail(&self, scope: &Selection, id: &str) -> Result<Observed<Value>> {
        self.checked((|| {
            self.require_selection(scope)?;
            let saved = self.saved_run(id)?.ok_or("unknown history run")?;
            let state = self.project(scope, Some(id))?;
            let run = state
                .view
                .runs
                .iter()
                .find(|run| run.id == id)
                .ok_or("projected run is absent")?;
            Ok(Observed {
                revision: Counter(saved.revision),
                value: crate::public_view::run_record(&state, run)?,
            })
        })())
    }

    pub(crate) fn source_detail(
        &self,
        scope: &Selection,
        id: &str,
    ) -> Result<Observed<Option<Source>>> {
        self.checked((|| {
            self.require_selection(scope)?;
            let value = self.source(id)?;
            let saved: Record<Provenance> =
                record(self.write, SOURCES, id)?.ok_or("source is absent")?;
            Ok(Observed {
                revision: Counter(saved.revision),
                value,
            })
        })())
    }

    pub(crate) fn action_detail(&self, scope: &Selection, id: &str) -> Result<Observed<Action>> {
        self.checked((|| {
            let value = self.browser_action(scope, id)?;
            let saved = self.browser_record(scope, id)?;
            Ok(Observed {
                revision: Counter(saved.revision),
                value,
            })
        })())
    }
}
