use super::{History, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    model::{Run, Source, Status, Store, View, valid_request_id},
};
use redb::{
    Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
    WriteTransaction,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{cell::Cell, collections::BTreeMap};

mod browser_read;
mod browser_view;
mod browser_write;
mod context;
mod conversations;
mod native_admit;
pub(super) mod native_choices;
mod native_start;
mod native_target;
mod native_task;
mod native_update;
mod native_workflow;
mod read;
mod recovery;
mod settings;
mod view;

pub(crate) use conversations::{
    ConversationBusy, ConversationChange, ConversationChanged, ConversationPage,
};
pub(crate) use native_choices::NativeChoices;
pub(crate) use native_task::NativeTask;
pub(crate) use native_workflow::NativeWorkflow;
pub(crate) use read::{Cursor, Observed, RunPage, RunSummary};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selection {
    pub store: String,
    pub conversation: String,
    pub epoch: u64,
}

#[derive(Debug)]
pub(crate) struct Failure(Box<dyn std::error::Error + Send + Sync>);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "history storage failure: {}", self.0)
    }
}

impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

fn storage<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
    operation().map_err(|error| Box::new(Failure(error)) as _)
}

pub(crate) struct Started {
    pub run: String,
    pub current: Selection,
}

pub(crate) struct Edit<'a> {
    write: &'a WriteTransaction,
    profile: Profile,
    first_id: u64,
    changed: bool,
    failed: Cell<bool>,
    fatal: Cell<bool>,
}

impl History {
    pub(crate) fn selection(&self) -> Result<Selection> {
        storage(|| {
            let read = self.db.begin_read()?;
            let meta = read.open_table(META)?;
            let profile: Profile = decode(meta.get("profile")?.ok_or("missing profile")?.value())?;
            profile.validate()?;
            let table = read.open_table(CONVERSATIONS)?;
            let conversation: Conversation = decode(
                table
                    .get(profile.selected.as_str())?
                    .ok_or("missing conversation")?
                    .value(),
            )?;
            if conversation.id != profile.selected {
                return Err("selected history identity differs".into());
            }
            Ok(selection(&profile))
        })
    }

    pub(crate) fn edit<T>(&mut self, change: impl FnOnce(&mut Edit<'_>) -> Result<T>) -> Result<T> {
        let mut write = storage(|| Ok(self.db.begin_write()?))?;
        storage(|| {
            write.set_durability(Durability::Immediate)?;
            write.set_two_phase_commit(true);
            Ok(())
        })?;
        let profile = storage(|| {
            let table = write.open_table(META)?;
            let profile: Profile = decode(table.get("profile")?.ok_or("missing profile")?.value())?;
            profile.validate()?;
            Ok(profile)
        })?;
        let mut edit = Edit {
            write: &write,
            first_id: profile.next,
            profile,
            changed: false,
            failed: Cell::new(false),
            fatal: Cell::new(false),
        };
        let result = change(&mut edit)?;
        if edit.failed.get() {
            return if edit.fatal.get() {
                Err(Box::new(Failure(
                    "history edit caught a storage failure".into(),
                )))
            } else {
                Err("history edit caught a rejected operation".into())
            };
        }
        if edit.changed {
            edit.profile.sequence = edit
                .profile
                .sequence
                .checked_add(1)
                .ok_or("view sequence capacity reached")?;
            storage(|| {
                write
                    .open_table(META)?
                    .insert("profile", encode(&edit.profile)?.as_slice())?;
                Ok(())
            })?;
        }
        drop(edit);
        storage(|| Ok(write.commit()?))?;
        Ok(result)
    }
}

fn selection(profile: &Profile) -> Selection {
    Selection {
        store: profile.store.clone(),
        conversation: profile.selected.clone(),
        epoch: profile.epoch,
    }
}

fn record<T: DeserializeOwned>(
    write: &WriteTransaction,
    table: TableDefinition<&str, &[u8]>,
    key: &str,
) -> Result<Option<Record<T>>> {
    storage(|| {
        let table = write.open_table(table)?;
        let Some(bytes) = table.get(key)? else {
            return Ok(None);
        };
        let value: Record<T> = decode(bytes.value())?;
        if value.revision == 0 {
            return Err("invalid saved record revision".into());
        }
        Ok(Some(value))
    })
}

impl Edit<'_> {
    pub(crate) fn selection(&self) -> Selection {
        selection(&self.profile)
    }

    pub(crate) fn require_selection(&self, expected: &Selection) -> Result<()> {
        self.checked(self.selection_inner(expected))
    }

    fn selection_inner(&self, expected: &Selection) -> Result<()> {
        if *expected != selection(&self.profile) {
            return Err("history selection changed".into());
        }
        Ok(())
    }

    pub(crate) fn id(&mut self) -> Result<String> {
        let result = self.id_inner();
        self.checked(result)
    }

    fn id_inner(&mut self) -> Result<String> {
        let id = self.profile.next;
        self.profile.next = id.checked_add(1).ok_or("identity capacity reached")?;
        self.changed = true;
        Ok(format!("{}:{id}", self.profile.store))
    }

    pub(crate) fn duplicate(
        &self,
        expected: &Selection,
        request: &str,
        input_digest: &str,
    ) -> Result<Option<Started>> {
        self.checked(self.duplicate_inner(expected, request, input_digest))
    }

    fn duplicate_inner(
        &self,
        expected: &Selection,
        request: &str,
        input_digest: &str,
    ) -> Result<Option<Started>> {
        let Some((start, launch)) = self.request_launch_inner(expected, request)? else {
            return Ok(None);
        };
        let digest = launch.input_digest().ok_or("legacy start identity has no verifiable input or route; inspect its saved view instead")?;
        let same_scope =
            expected.store == self.profile.store && expected.conversation == start.conversation;
        let same_input = match start.format {
            ReceiptFormat::LegacyUnscoped => digest == input_digest,
            ReceiptFormat::Scoped {
                epoch,
                digest: saved,
            } => {
                storage(|| {
                    if epoch == 0
                        || saved.len() != 64
                        || !saved.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err("invalid saved scoped receipt".into());
                    }
                    Ok(())
                })?;
                epoch == expected.epoch
                    && saved == scoped_digest(expected, input_digest)?
                    && digest == input_digest
            }
        };
        if !same_scope || !same_input {
            return Err("request identity already names a different input route or scope".into());
        }
        Ok(Some(Started {
            run: start.run,
            current: selection(&self.profile),
        }))
    }

    pub(crate) fn request_launch(
        &self,
        expected: &Selection,
        request: &str,
    ) -> Result<Option<Launch>> {
        self.checked(
            self.request_launch_inner(expected, request)
                .map(|saved| saved.map(|(_, launch)| launch)),
        )
    }

    fn request_launch_inner(
        &self,
        expected: &Selection,
        request: &str,
    ) -> Result<Option<(Start, Launch)>> {
        if !valid_request_id(request) {
            return Err("invalid request identity".into());
        }
        self.require_run_request(request)?;
        let start = storage(|| {
            let table = self.write.open_table(STARTS)?;
            table
                .get(request)?
                .map(|bytes| decode::<Start>(bytes.value()))
                .transpose()
        })?;
        let Some(start) = start else { return Ok(None) };
        let run = storage(|| {
            self.saved_run(&start.run)?
                .ok_or_else(|| "saved start has no run".into())
        })?;
        let launch: Record<Launch> = storage(|| {
            record(self.write, LAUNCHES, &start.run)?
                .ok_or_else(|| "saved start has no launch".into())
        })?;
        storage(|| {
            if run.conversation != start.conversation || launch.conversation != start.conversation {
                return Err("saved start ownership differs".into());
            }
            if launch.value.input_digest().is_some_and(|digest| {
                digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            }) {
                return Err("invalid saved launch digest".into());
            }
            Ok(())
        })?;

        storage(|| {
            if let ReceiptFormat::Scoped { epoch, digest } = &start.format
                && (*epoch == 0
                    || digest.len() != 64
                    || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
            {
                return Err("invalid saved scoped receipt".into());
            }
            Ok(())
        })?;
        if expected.store != self.profile.store
            || expected.conversation != start.conversation
            || matches!(&start.format, ReceiptFormat::Scoped{epoch,..} if *epoch!=expected.epoch)
        {
            return Err("request identity already names a different input route or scope".into());
        }
        Ok(Some((start, launch.value)))
    }

    pub(crate) fn run(&self, expected: &Selection, id: &str) -> Result<Run> {
        self.checked(self.run_inner(expected, id))
    }

    fn run_inner(&self, expected: &Selection, id: &str) -> Result<Run> {
        self.require_selection(expected)?;
        let run = self.saved_run(id)?.ok_or("unknown history run")?;
        if run.conversation != expected.conversation {
            return Err("run belongs to a different conversation".into());
        }
        Ok(run.value)
    }

    pub(crate) fn admit(
        &mut self,
        expected: &Selection,
        request: &str,
        run: Run,
        launch: Launch,
    ) -> Result<()> {
        let result = self.admit_inner(expected, request, run, launch);
        self.checked(result)
    }

    fn admit_inner(
        &mut self,
        expected: &Selection,
        request: &str,
        run: Run,
        launch: Launch,
    ) -> Result<()> {
        self.require_selection(expected)?;
        if !valid_request_id(request)
            || self
                .duplicate(expected, request, launch.input_digest().unwrap_or(""))?
                .is_some()
        {
            return Err("request identity already admitted or invalid".into());
        }
        let number = run
            .id
            .strip_prefix(&format!("{}:", self.profile.store))
            .and_then(|id| id.parse::<u64>().ok())
            .ok_or("invalid new run identity")?;
        if number < self.first_id
            || number >= self.profile.next
            || run.id != format!("{}:{number}", self.profile.store)
            || run.status != Status::Running
            || run.error.is_some()
            || (run.output != Default::default()
                && run.output != metldr_conversation::Content::native())
            || launch.input_digest().is_none()
        {
            return Err("invalid new run admission".into());
        }
        let idle = storage(|| Ok(self.write.open_table(ACTIVE_RUNS)?.len()? == 0))?;
        if !idle {
            return Err("another run is active".into());
        }
        if self.saved_run(&run.id)?.is_some() {
            return Err("run identity already exists".into());
        }
        let conversation = self.conversation()?;
        if conversation.source.as_deref() != Some(&run.source) {
            return Err("source is not current saved source".into());
        }
        let source = self
            .source(&run.source)?
            .ok_or("historical source content is unavailable")?;
        self.validate_flat(&run, &launch, Some(source))?;
        let start = Start {
            conversation: expected.conversation.clone(),
            run: run.id.clone(),
            format: ReceiptFormat::Scoped {
                epoch: expected.epoch,
                digest: scoped_digest(expected, launch.input_digest().unwrap())?,
            },
        };
        storage(|| {
            let owned_run = Record {
                conversation: expected.conversation.clone(),
                revision: 1,
                value: &run,
            };
            let owned_launch = Record {
                conversation: expected.conversation.clone(),
                revision: 1,
                value: &launch,
            };
            self.write
                .open_table(RUNS)?
                .insert(run.id.as_str(), encode(&owned_run)?.as_slice())?;
            if self
                .write
                .open_table(LAUNCHES)?
                .insert(run.id.as_str(), encode(&owned_launch)?.as_slice())?
                .is_some()
                || self
                    .write
                    .open_table(STARTS)?
                    .insert(request, encode(&start)?.as_slice())?
                    .is_some()
                || self
                    .write
                    .open_table(RUN_ORDER)?
                    .insert((expected.conversation.as_str(), number), run.id.as_str())?
                    .is_some()
            {
                return Err("new admission collides with saved history".into());
            }
            self.write
                .open_table(ACTIVE_RUNS)?
                .insert(run.id.as_str(), ())?;
            Ok(())
        })?;
        self.changed = true;
        Ok(())
    }

    pub(crate) fn put_run(&mut self, expected: &Selection, run: Run) -> Result<()> {
        let result = self.put_run_inner(expected, run);
        self.checked(result)
    }

    fn put_run_inner(&mut self, expected: &Selection, mut run: Run) -> Result<()> {
        self.require_selection(expected)?;
        self.require_browser_finalized(expected, &run)?;
        let before = self.saved_run(&run.id)?.ok_or("unknown history run")?;
        if before.conversation != expected.conversation
            || before.value.source != run.source
            || before.value.kind != run.kind
            || before.value.question != run.question
            || before.value.status.terminal()
        {
            return Err("run identity or completed state changed".into());
        }
        let launch: Record<Launch> = storage(|| {
            record(self.write, LAUNCHES, &run.id)?.ok_or_else(|| "run has no launch".into())
        })?;
        storage(|| {
            if launch.conversation != before.conversation {
                return Err("launch ownership differs".into());
            }
            Ok(())
        })?;
        if matches!(
            launch.value,
            Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
        ) {
            let mut task = self.native_task(expected, &run.id)?;
            task.run = run;
            return self.put_native(expected, task);
        }
        let source = self.source(&run.source)?;
        storage(|| self.validate_flat(&before.value, &launch.value, source.clone()))?;
        if run.status.terminal() {
            run.output.end(if run.status == Status::Complete {
                metldr_conversation::Ending::Complete
            } else {
                metldr_conversation::Ending::Interrupted
            })?;
        }
        self.validate_flat(&run, &launch.value, source)?;
        let revision = before
            .revision
            .checked_add(1)
            .ok_or("run revision capacity reached")?;
        storage(|| {
            if run.status == Status::Complete
                && let Some(route) = super::context::Route::from_launch(&launch.value)
            {
                let key = route.key(&before.conversation, &run.source)?;
                let number = run
                    .id
                    .rsplit(':')
                    .next()
                    .ok_or("invalid run identity")?
                    .parse::<u64>()?;
                if self
                    .write
                    .open_table(CONTEXT)?
                    .insert((key.as_str(), number), run.id.as_str())?
                    .is_some()
                {
                    return Err("completed context index collision".into());
                }
            }
            let after = Record {
                conversation: before.conversation,
                revision,
                value: &run,
            };
            self.write
                .open_table(RUNS)?
                .insert(run.id.as_str(), encode(&after)?.as_slice())?;
            let mut active = self.write.open_table(ACTIVE_RUNS)?;
            if run.status.terminal() {
                active.remove(run.id.as_str())?;
            } else {
                active.insert(run.id.as_str(), ())?;
            }
            Ok(())
        })?;
        self.changed = true;
        Ok(())
    }

    fn saved_run(&self, id: &str) -> Result<Option<Record<Run>>> {
        let run: Option<Record<Run>> = record(self.write, RUNS, id)?;
        storage(|| {
            if let Some(run) = &run {
                if run.value.id != id {
                    return Err("saved run key differs".into());
                }
                run.value.validate(|id| self.valid_id(id))?;
                let number = id
                    .rsplit(':')
                    .next()
                    .ok_or("invalid saved run identity")?
                    .parse::<u64>()?;
                let order = self.write.open_table(RUN_ORDER)?;
                if order
                    .get((run.conversation.as_str(), number))?
                    .is_none_or(|saved| saved.value() != id)
                {
                    return Err("saved run order differs".into());
                }
                let active = self.write.open_table(ACTIVE_RUNS)?;
                if active.get(id)?.is_some() == run.value.status.terminal() {
                    return Err("saved active run index differs".into());
                }
            }
            Ok(run)
        })
    }

    fn valid_id(&self, id: &str) -> bool {
        id.strip_prefix(&format!("{}:", self.profile.store))
            .and_then(|id| id.parse::<u64>().ok())
            .is_some_and(|id| id > 0 && id < self.profile.next)
    }

    fn checked<T>(&self, result: Result<T>) -> Result<T> {
        if let Err(error) = &result {
            self.failed.set(true);
            self.fatal
                .set(self.fatal.get() || error.downcast_ref::<Failure>().is_some());
        }
        result
    }

    fn conversation(&self) -> Result<Conversation> {
        storage(|| {
            let table = self.write.open_table(CONVERSATIONS)?;
            let conversation: Conversation = decode(
                table
                    .get(self.profile.selected.as_str())?
                    .ok_or("missing conversation")?
                    .value(),
            )?;
            if conversation.id != self.profile.selected {
                return Err("conversation identity differs".into());
            }
            Ok(conversation)
        })
    }

    fn source(&self, id: &str) -> Result<Option<Source>> {
        self.source_in(&self.profile.selected, id)
    }

    fn source_in(&self, conversation: &str, id: &str) -> Result<Option<Source>> {
        storage(|| {
            let source: Record<Provenance> =
                record(self.write, SOURCES, id)?.ok_or("run source is absent")?;
            if source.conversation != conversation {
                return Err("source ownership differs".into());
            }
            match source.value {
                Provenance::Known { source } => {
                    if source.id != id || !self.valid_id(id) {
                        return Err("source identity differs".into());
                    }
                    crate::model::check_page_fields(&source.page)?;
                    Ok(Some(source))
                }
                Provenance::LegacyUnavailable { id: saved } if saved == id && self.valid_id(id) => {
                    Ok(None)
                }
                _ => Err("invalid unavailable source identity".into()),
            }
        })
    }

    fn validate_flat(&self, run: &Run, launch: &Launch, source: Option<Source>) -> Result<()> {
        if matches!(
            launch,
            Launch::Pi { .. } | Launch::Codex { .. } | Launch::OpenCode { .. }
        ) {
            return Err("flat task cannot use a native launch".into());
        }
        Store {
            schema: crate::STORE_VERSION,
            view: View {
                store: self.profile.store.clone(),
                sequence: self.profile.sequence,
                source,
                runs: vec![run.clone()],
            },
            next: self.profile.next,
            starts: BTreeMap::new(),
            launches: BTreeMap::from([(run.id.clone(), launch.clone())]),
            workflows: BTreeMap::new(),
            codex_workflows: BTreeMap::new(),
            opencode_workflows: BTreeMap::new(),
            browser_actions: BTreeMap::new(),
            providers: self.profile.providers.clone(),
            agent_settings: self.profile.agents.clone(),
        }
        .validate()
    }
}

fn scoped_digest(selection: &Selection, input_digest: &str) -> Result<String> {
    crate::agent_model::digest(&(selection, input_digest))
}
