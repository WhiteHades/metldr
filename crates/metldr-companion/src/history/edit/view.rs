use super::{Edit, Selection, record, storage};
use crate::{
    Result,
    agent_model::Launch,
    history::schema::*,
    model::{Page, Source, Store, View},
};
use redb::{ReadableTable, ReadableTableMetadata};
use std::collections::BTreeMap;

impl Edit<'_> {
    pub(crate) fn project_latest(&self, expected: &Selection) -> Result<Store> {
        let result = (|| {
            self.require_selection(expected)?;
            let run = storage(|| {
                let order = self.write.open_table(RUN_ORDER)?;
                let last = order
                    .range(
                        (expected.conversation.as_str(), 0)
                            ..=(expected.conversation.as_str(), u64::MAX),
                    )?
                    .next_back()
                    .transpose()?;
                last.map(|(key, id)| {
                    let id = id.value();
                    if id.rsplit(':').next().and_then(|n| n.parse::<u64>().ok())
                        != Some(key.value().1)
                    {
                        return Err("latest run order differs".into());
                    }
                    Ok(id.to_owned())
                })
                .transpose()
            })?;
            self.project(expected, run.as_deref())
        })();
        self.checked(result)
    }

    pub(crate) fn changed(&self) -> bool {
        self.changed
    }

    pub(crate) fn refresh_view(
        &mut self,
        expected: &Selection,
        run: Option<&str>,
    ) -> Result<Store> {
        let result = (|| {
            self.require_selection(expected)?;
            self.changed = true;
            self.project(expected, run)
        })();
        self.checked(result)
    }

    pub(crate) fn selected_source(&self, expected: &Selection) -> Result<Option<Source>> {
        let result = (|| {
            self.require_selection(expected)?;
            let conversation = self.conversation()?;
            match conversation.source {
                Some(id) => storage(|| {
                    self.source(&id)?
                        .map(Some)
                        .ok_or_else(|| "current source content is unavailable".into())
                }),
                None => Ok(None),
            }
        })();
        self.checked(result)
    }

    pub(crate) fn capture(
        &mut self,
        expected: &Selection,
        expected_source: Option<&str>,
        page: Page,
    ) -> Result<()> {
        let result = (|| {
            self.require_selection(expected)?;
            crate::model::check_page_fields(&page)?;
            if storage(|| Ok(self.write.open_table(ACTIVE_RUNS)?.len()? != 0))? {
                return Err("another run is active".into());
            }
            let conversation = self.conversation()?;
            if let Some(source) = self.selected_source(expected)?
                && source.page == page
            {
                return Ok(());
            }
            if conversation.source.as_deref() != expected_source {
                return Err("source is not current saved source".into());
            }
            self.new_source(expected, page)?;
            Ok(())
        })();
        self.checked(result)
    }

    pub(crate) fn new_source(&mut self, expected: &Selection, page: Page) -> Result<Source> {
        let result = (|| {
            self.require_selection(expected)?;
            crate::model::check_page_fields(&page)?;
            if storage(|| Ok(self.write.open_table(ACTIVE_RUNS)?.len()? != 0))? {
                return Err("another run is active".into());
            }
            let mut conversation = self.conversation()?;
            let source = Source {
                id: self.id()?,
                page,
            };
            if conversation.source.is_none() {
                conversation.title = source.page.title.clone();
            }
            conversation.source = Some(source.id.clone());
            storage(|| {
                let record = Record {
                    conversation: expected.conversation.clone(),
                    revision: 1,
                    value: Provenance::Known {
                        source: source.clone(),
                    },
                };
                if self
                    .write
                    .open_table(SOURCES)?
                    .insert(source.id.as_str(), encode(&record)?.as_slice())?
                    .is_some()
                {
                    return Err("new source collides with saved history".into());
                }
                self.write.open_table(CONVERSATIONS)?.insert(
                    expected.conversation.as_str(),
                    encode(&conversation)?.as_slice(),
                )?;
                Ok(())
            })?;
            self.changed = true;
            Ok(source)
        })();
        self.checked(result)
    }

    pub(crate) fn project(&self, expected: &Selection, run: Option<&str>) -> Result<Store> {
        let result = (|| {
            self.require_selection(expected)?;
            let mut runs = Vec::new();
            let mut launches = BTreeMap::new();
            if let Some(id) = run {
                let value = self.run(expected, id)?;
                let launch: Record<Launch> = storage(|| {
                    record(self.write, LAUNCHES, id)?
                        .ok_or_else(|| "projected launch is absent".into())
                })?;
                if matches!(
                    launch.value,
                    Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
                ) {
                    return self.project_native(expected, id);
                }
                storage(|| {
                    if launch.conversation != expected.conversation {
                        return Err("projected launch ownership differs".into());
                    }
                    self.validate_flat(&value, &launch.value, self.source(&value.source)?)
                })?;
                launches.insert(id.to_owned(), launch.value);
                runs.push(value);
            }
            let state = Store {
                schema: crate::STORE_VERSION,
                view: View {
                    store: self.profile.store.clone(),
                    sequence: self
                        .profile
                        .sequence
                        .checked_add(u64::from(self.changed))
                        .ok_or("view sequence capacity reached")?,
                    source: self.selected_source(expected)?,
                    runs,
                },
                next: self.profile.next,
                starts: BTreeMap::new(),
                launches,
                workflows: BTreeMap::new(),
                codex_workflows: BTreeMap::new(),
                opencode_workflows: BTreeMap::new(),
                browser_actions: BTreeMap::new(),
                providers: self.profile.providers.clone(),
                agent_settings: self.profile.agents.clone(),
            };
            state.validate()?;
            Ok(state)
        })();
        self.checked(result)
    }
}
