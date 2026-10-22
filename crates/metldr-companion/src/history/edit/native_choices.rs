use super::{Edit, History, NativeWorkflow, Selection, selection, storage};
use crate::{
    Result,
    agent_model::Launch,
    agent_settings::Kind,
    history::schema::*,
    model::{Run, Source, Store, View},
};
use redb::{
    ReadTransaction, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
    WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ops::Bound};

const KINDS: [Kind; 3] = [Kind::Pi, Kind::Codex, Kind::Opencode];
const LIMIT: usize = 16;

#[derive(Default)]
pub(crate) struct NativeChoices(BTreeMap<Kind, Vec<Choice>>);

#[derive(Serialize)]
struct Choice {
    workflow: String,
    source: String,
    profile: String,
    revision: u64,
    ready: bool,
}

impl NativeChoices {
    pub(crate) fn apply(&self, view: &mut Value) -> Result<()> {
        for kind in KINDS {
            let field = match kind {
                Kind::Pi => "agent_sessions",
                Kind::Codex => "codex_sessions",
                Kind::Opencode => "opencode_sessions",
            };
            view[field] = match self.0.get(&kind) {
                Some(choices) => serde_json::to_value(choices)?,
                None => json!([]),
            };
        }
        Ok(())
    }
}

fn table(kind: Kind) -> TableDefinition<'static, &'static str, &'static [u8]> {
    match kind {
        Kind::Pi => PI,
        Kind::Codex => CODEX,
        Kind::Opencode => OPENCODE,
    }
}

fn fingerprint(workflow: &NativeWorkflow) -> (Kind, &str) {
    match workflow {
        NativeWorkflow::Pi(w) => (Kind::Pi, &w.fingerprint),
        NativeWorkflow::Codex(w) => (Kind::Codex, &w.fingerprint),
        NativeWorkflow::OpenCode(w) => (Kind::Opencode, &w.fingerprint),
    }
}

fn bucket(conversation: &str, source: &str, kind: Kind, fingerprint: &str) -> Result<String> {
    crate::agent_model::digest(&(conversation, source, kind, fingerprint))
}

fn number(profile: &Profile, id: &str) -> Result<u64> {
    id.strip_prefix(&format!("{}:", profile.store))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|number| *number > 0 && *number < profile.next)
        .ok_or_else(|| "invalid native choice identity".into())
}

enum Reader<'a> {
    Staged(&'a WriteTransaction),
    Committed(&'a ReadTransaction),
}

struct Entry {
    conversation: String,
    head: u64,
    kind: Kind,
    choice: Choice,
}

impl Reader<'_> {
    fn bytes(&self, definition: TableDefinition<&str, &[u8]>, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(match self {
            Self::Staged(tx) => tx
                .open_table(definition)?
                .get(id)?
                .map(|v| v.value().to_vec()),
            Self::Committed(tx) => tx
                .open_table(definition)?
                .get(id)?
                .map(|v| v.value().to_vec()),
        })
    }

    fn value<T: DeserializeOwned>(
        &self,
        definition: TableDefinition<&str, &[u8]>,
        id: &str,
    ) -> Result<T> {
        decode(
            &self
                .bytes(definition, id)?
                .ok_or("native choice record is absent")?,
        )
    }

    fn record<T: DeserializeOwned>(
        &self,
        definition: TableDefinition<&str, &[u8]>,
        id: &str,
    ) -> Result<Record<T>> {
        let record: Record<T> = self.value(definition, id)?;
        if record.revision == 0 {
            return Err("invalid native choice record revision".into());
        }
        Ok(record)
    }

    fn workflow(&self, kind: Kind, id: &str) -> Result<Record<NativeWorkflow>> {
        macro_rules! load {
            ($table:ident, $ty:ty, $variant:ident) => {{
                let record: Record<$ty> = self.record($table, id)?;
                Record {
                    conversation: record.conversation,
                    revision: record.revision,
                    value: NativeWorkflow::$variant(record.value),
                }
            }};
        }
        Ok(match kind {
            Kind::Pi => load!(PI, crate::agent_model::Workflow, Pi),
            Kind::Codex => load!(CODEX, crate::codex_model::Workflow, Codex),
            Kind::Opencode => load!(OPENCODE, crate::opencode_model::Workflow, OpenCode),
        })
    }

    fn entry(&self, profile: &Profile, kind: Kind, id: &str) -> Result<Entry> {
        number(profile, id)?;
        let saved = self.workflow(kind, id)?;
        let workflow = saved.value;
        let source = workflow.source();
        let conversation: Conversation = self.value(CONVERSATIONS, &saved.conversation)?;
        conversation_number(profile, &saved.conversation)?;
        let provenance: Record<Provenance> = self.record(SOURCES, &source.id)?;
        let run: Record<Run> = self.record(RUNS, workflow.last_run())?;
        let launch: Record<Launch> = self.record(LAUNCHES, workflow.last_run())?;
        let head = number(profile, workflow.last_run())?;
        if conversation.id != saved.conversation
            || provenance.conversation != saved.conversation
            || run.conversation != saved.conversation
            || launch.conversation != saved.conversation
            || run.value.id != workflow.last_run()
            || run.value.source != source.id
            || super::native_task::workflow_id(&launch.value)? != id
            || !workflow.matches(&launch.value)
            || !matches!(&provenance.value, Provenance::Known { source: known } if known.id == source.id && known.page == source.page)
        {
            return Err("native choice ownership or head differs".into());
        }
        let terminal = run.value.status.terminal();
        let mut state = Store {
            schema: crate::STORE_VERSION,
            view: View {
                sequence: profile.sequence,
                store: profile.store.clone(),
                source: Some(source.clone()),
                runs: vec![run.value],
            },
            next: profile.next,
            starts: BTreeMap::new(),
            launches: BTreeMap::from([(workflow.last_run().to_owned(), launch.value)]),
            workflows: BTreeMap::new(),
            codex_workflows: BTreeMap::new(),
            opencode_workflows: BTreeMap::new(),
            browser_actions: BTreeMap::new(),
            providers: profile.providers.clone(),
            agent_settings: profile.agents.clone(),
        };
        workflow.install(id, &mut state);
        state.validate()?;
        let resumable = match &workflow {
            NativeWorkflow::Pi(w) => w.binding.resume_path().is_ok_and(|path| path.is_some()),
            NativeWorkflow::Codex(w) => w.binding.resume().is_ok_and(|value| value.is_some()),
            NativeWorkflow::OpenCode(w) => w.binding.resume().is_ok_and(|value| value.is_some()),
        };
        let (_, fingerprint) = fingerprint(&workflow);
        Ok(Entry {
            conversation: saved.conversation,
            head,
            kind,
            choice: Choice {
                workflow: id.into(),
                source: source.id.clone(),
                profile: fingerprint.into(),
                revision: workflow.revision(),
                ready: terminal && resumable,
            },
        })
    }

    fn rows(&self, bucket: &str) -> Result<Vec<(u64, String)>> {
        fn rows(
            table: &impl ReadableTable<(&'static str, u64), &'static str>,
            bucket: &str,
        ) -> Result<Vec<(u64, String)>> {
            table
                .range((bucket, 0)..=(bucket, u64::MAX))?
                .rev()
                .take(LIMIT)
                .map(|row| {
                    let (key, value) = row?;
                    Ok((key.value().1, value.value().to_owned()))
                })
                .collect()
        }
        match self {
            Self::Staged(tx) => rows(&tx.open_table(NATIVE_CHOICES)?, bucket),
            Self::Committed(tx) => rows(&tx.open_table(NATIVE_CHOICES)?, bucket),
        }
    }

    fn choices(
        &self,
        profile: &Profile,
        scope: &Selection,
        source: Option<&Source>,
        profiles: &BTreeMap<Kind, String>,
    ) -> Result<NativeChoices> {
        if selection(profile) != *scope {
            return Err("native choices selection changed".into());
        }
        let conversation: Conversation = self.value(CONVERSATIONS, &scope.conversation)?;
        if conversation.id != scope.conversation
            || conversation.source.as_deref() != source.map(|s| s.id.as_str())
        {
            return Err("native choices source changed".into());
        }
        let mut choices = NativeChoices::default();
        let Some(source) = source else {
            return Ok(choices);
        };
        let known: Record<Provenance> = self.record(SOURCES, &source.id)?;
        if known.conversation != scope.conversation
            || !matches!(known.value, Provenance::Known { source: saved } if saved.id == source.id && saved.page == source.page)
        {
            return Err("native choices saved source differs".into());
        }
        for kind in KINDS {
            let Some(fingerprint) = profiles.get(&kind) else {
                continue;
            };
            let key = bucket(&scope.conversation, &source.id, kind, fingerprint)?;
            let mut selected = Vec::new();
            for (head, id) in self.rows(&key)? {
                let entry = self.entry(profile, kind, &id)?;
                if entry.conversation != scope.conversation
                    || entry.kind != kind
                    || entry.head != head
                    || entry.choice.source != source.id
                    || &entry.choice.profile != fingerprint
                {
                    return Err("native choice index differs from saved workflow".into());
                }
                selected.push(entry.choice);
            }
            choices.0.insert(kind, selected);
        }
        Ok(choices)
    }
}

impl History {
    pub(crate) fn native_choices(
        &self,
        scope: &Selection,
        source: Option<&Source>,
        profiles: &BTreeMap<Kind, String>,
    ) -> Result<NativeChoices> {
        storage(|| {
            let read = self.db.begin_read()?;
            let reader = Reader::Committed(&read);
            let profile: Profile = reader.value(META, "profile")?;
            profile.validate()?;
            reader.choices(&profile, scope, source, profiles)
        })
    }
}

impl Edit<'_> {
    pub(crate) fn native_choices(
        &self,
        scope: &Selection,
        source: Option<&Source>,
        profiles: &BTreeMap<Kind, String>,
    ) -> Result<NativeChoices> {
        self.checked(storage(|| {
            Reader::Staged(self.write).choices(&self.profile, scope, source, profiles)
        }))
    }
}

pub(super) fn update_index(
    write: &WriteTransaction,
    profile: &Profile,
    scope: &Selection,
    id: &str,
    workflow: &NativeWorkflow,
) -> Result<()> {
    let (kind, fingerprint) = fingerprint(workflow);
    let new_bucket = bucket(
        &scope.conversation,
        &workflow.source().id,
        kind,
        fingerprint,
    )?;
    let new_head = number(profile, workflow.last_run())?;
    let reader = Reader::Staged(write);
    let old = reader
        .bytes(table(kind), id)?
        .map(|_| reader.workflow(kind, id))
        .transpose()?;
    let mut order = write.open_table(NATIVE_CHOICES)?;
    if let Some(old) = old {
        let (old_kind, old_fingerprint) = self::fingerprint(&old.value);
        if old.conversation != scope.conversation
            || old_kind != kind
            || old_fingerprint != fingerprint
            || old.value.source().id != workflow.source().id
        {
            return Err("native choice update changed ownership".into());
        }
        let old_head = number(profile, old.value.last_run())?;
        if order
            .get((new_bucket.as_str(), old_head))?
            .map(|value| value.value() == id)
            != Some(true)
        {
            return Err("native choice previous index is absent or changed".into());
        }
        if old_head != new_head {
            order.remove((new_bucket.as_str(), old_head))?;
        }
    }
    if let Some(previous) = order.insert((new_bucket.as_str(), new_head), id)?
        && previous.value() != id
    {
        return Err("native choice index collision".into());
    }
    Ok(())
}

pub(in crate::history) fn backfill(write: &WriteTransaction, profile: &Profile) -> Result<()> {
    if write.open_table(NATIVE_CHOICES)?.len()? != 0 {
        return Err("native choices upgrade index is not empty".into());
    }
    let reader = Reader::Staged(write);
    for kind in KINDS {
        let mut after: Option<String> = None;
        loop {
            let ids = {
                let table = write.open_table(table(kind))?;
                table
                    .range::<&str>((
                        after.as_deref().map_or(Bound::Unbounded, Bound::Excluded),
                        Bound::Unbounded,
                    ))?
                    .take(LIMIT)
                    .map(|row| row.map(|(key, _)| key.value().to_owned()))
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            if ids.is_empty() {
                break;
            }
            for id in &ids {
                let entry = reader.entry(profile, kind, id)?;
                let key = bucket(
                    &entry.conversation,
                    &entry.choice.source,
                    kind,
                    &entry.choice.profile,
                )?;
                if write
                    .open_table(NATIVE_CHOICES)?
                    .insert((key.as_str(), entry.head), id.as_str())?
                    .is_some()
                {
                    return Err("native choices upgrade index collision".into());
                }
            }
            after = ids.last().cloned();
        }
    }
    Ok(())
}
