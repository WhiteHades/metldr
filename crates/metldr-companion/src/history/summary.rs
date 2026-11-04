use super::schema::{Profile, Provenance, RUNS, Record, SOURCES, decode, encode};
use crate::{
    Result,
    model::{Run, Status, check_page_fields},
    tool_report::Preview,
};
use redb::{ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction};

pub(super) type RunFields<'a> = (
    &'a str,
    u64,
    &'a str,
    &'a str,
    u8,
    Option<(&'a str, bool)>,
    (&'a str, bool),
);
pub(super) type SourceFields<'a> = (&'a str, u64, bool);

pub(super) const RUN_HEADERS: TableDefinition<&str, RunFields<'static>> =
    TableDefinition::new("run_headers");
pub(super) const SOURCE_HEADERS: TableDefinition<&str, SourceFields<'static>> =
    TableDefinition::new("source_headers");

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RunHeader {
    pub conversation: String,
    pub revision: u64,
    pub source: String,
    pub kind: String,
    pub status: Status,
    pub question: Option<Preview>,
    pub output: Preview,
}

pub(super) fn put_run(write: &WriteTransaction, record: &Record<&Run>) -> Result<()> {
    if record.revision == 0 {
        return Err("invalid saved record revision".into());
    }
    let fields = run_fields(record);
    write
        .open_table(RUNS)?
        .insert(record.value.id.as_str(), encode(record)?.as_slice())?;
    write
        .open_table(RUN_HEADERS)?
        .insert(record.value.id.as_str(), stored_run_fields(&fields))?;
    Ok(())
}

pub(super) fn put_source(
    write: &WriteTransaction,
    id: &str,
    record: &Record<Provenance>,
) -> Result<bool> {
    if record.revision == 0 {
        return Err("invalid saved record revision".into());
    }
    let known = source_fields(id, record)?;
    let collision = write
        .open_table(SOURCES)?
        .insert(id, encode(record)?.as_slice())?
        .is_some();
    let header_collision = if collision {
        false
    } else {
        write
            .open_table(SOURCE_HEADERS)?
            .insert(id, known)?
            .is_some()
    };
    Ok(collision || header_collision)
}

pub(super) fn require(read: &redb::ReadTransaction) -> Result<()> {
    read.open_table(RUN_HEADERS)?;
    read.open_table(SOURCE_HEADERS)?;
    Ok(())
}

pub(super) fn run(write: &WriteTransaction, id: &str) -> Result<Option<RunHeader>> {
    let table = write.open_table(RUN_HEADERS)?;
    let Some(fields) = table.get(id)? else {
        return Ok(None);
    };
    let (conversation, revision, source, kind, status, question, output) = fields.value();
    if revision == 0
        || conversation.is_empty()
        || conversation.len() > 53
        || source.is_empty()
        || source.len() > 2 * 1024 * 1024
        || !matches!(kind, "summary" | "answer" | "agent" | "browser")
        || question.is_some_and(|(text, _)| text.len() > 256)
        || output.0.len() > 512
    {
        return Err("invalid saved run header".into());
    }
    Ok(Some(RunHeader {
        conversation: conversation.to_owned(),
        revision,
        source: source.to_owned(),
        kind: kind.to_owned(),
        status: status_value(status)?,
        question: question.map(|(text, truncated)| Preview {
            text: text.into(),
            truncated,
        }),
        output: Preview {
            text: output.0.into(),
            truncated: output.1,
        },
    }))
}

pub(super) fn source(write: &WriteTransaction, conversation: &str, id: &str) -> Result<bool> {
    let table = write.open_table(SOURCE_HEADERS)?;
    let Some(fields) = table.get(id)? else {
        return Err("run source header is absent".into());
    };
    let (owner, revision, known) = fields.value();
    if owner != conversation || owner.is_empty() || owner.len() > 53 || revision == 0 {
        return Err("invalid saved source header".into());
    }
    Ok(known)
}

pub(super) fn backfill(write: &WriteTransaction, profile: &Profile) -> Result<()> {
    if write.open_table(RUN_HEADERS)?.len()? != 0 || write.open_table(SOURCE_HEADERS)?.len()? != 0 {
        return Err("history headers are not empty".into());
    }
    for row in write.open_table(RUNS)?.iter()? {
        let (id, bytes) = row?;
        let record: Record<Run> = decode(bytes.value())?;
        valid_id(profile, id.value())?;
        super::schema::conversation_number(profile, &record.conversation)?;
        if record.value.id != id.value() || record.revision == 0 {
            return Err("saved run identity or revision differs".into());
        }
        record
            .value
            .validate(|candidate| valid_id(profile, candidate).is_ok())?;
        let fields = run_fields(&Record {
            conversation: record.conversation,
            revision: record.revision,
            value: &record.value,
        });
        write
            .open_table(RUN_HEADERS)?
            .insert(id.value(), stored_run_fields(&fields))?;
    }
    for row in write.open_table(SOURCES)?.iter()? {
        let (id, bytes) = row?;
        let record: Record<Provenance> = decode(bytes.value())?;
        valid_id(profile, id.value())?;
        super::schema::conversation_number(profile, &record.conversation)?;
        let fields = source_fields(id.value(), &record)?;
        write
            .open_table(SOURCE_HEADERS)?
            .insert(id.value(), fields)?;
    }
    Ok(())
}

pub(super) fn audit_run(
    headers: &impl ReadableTable<&'static str, RunFields<'static>>,
    id: &str,
    record: &Record<Run>,
) -> Result<()> {
    let actual = headers.get(id)?.ok_or("run header is absent")?;
    let expected = run_fields(&Record {
        conversation: record.conversation.clone(),
        revision: record.revision,
        value: &record.value,
    });
    if actual.value() != stored_run_fields(&expected) {
        return Err("run header differs from saved run".into());
    }
    Ok(())
}

pub(super) fn audit_sources(read: &redb::ReadTransaction, profile: &Profile) -> Result<()> {
    let headers = read.open_table(SOURCE_HEADERS)?;
    let sources = read.open_table(SOURCES)?;
    for row in sources.iter()? {
        let (id, bytes) = row?;
        let record: Record<Provenance> = decode(bytes.value())?;
        valid_id(profile, id.value())?;
        super::schema::conversation_number(profile, &record.conversation)?;
        let expected = source_fields(id.value(), &record)?;
        let actual = headers.get(id.value())?.ok_or("source header is absent")?;
        if actual.value() != expected {
            return Err("source header differs from saved source".into());
        }
    }
    if headers.len()? != sources.len()? {
        return Err("source header count differs from saved sources".into());
    }
    Ok(())
}

fn valid_id(profile: &Profile, id: &str) -> Result<()> {
    id.strip_prefix(&format!("{}:", profile.store))
        .and_then(|suffix| suffix.parse::<u64>().ok())
        .filter(|number| *number > 0 && *number < profile.next)
        .ok_or("invalid saved source identity")?;
    Ok(())
}

fn run_fields(record: &Record<&Run>) -> RunHeader {
    RunHeader {
        conversation: record.conversation.clone(),
        revision: record.revision,
        source: record.value.source.clone(),
        kind: record.value.kind.clone(),
        status: record.value.status,
        question: record
            .value
            .question
            .as_deref()
            .map(|text| Preview::text(text, 256)),
        output: Preview::parts(record.value.output.text_parts(), 512),
    }
}

fn stored_run_fields(fields: &RunHeader) -> RunFields<'_> {
    (
        fields.conversation.as_str(),
        fields.revision,
        fields.source.as_str(),
        fields.kind.as_str(),
        status_code(fields.status),
        fields
            .question
            .as_ref()
            .map(|preview| (preview.text.as_str(), preview.truncated)),
        (fields.output.text.as_str(), fields.output.truncated),
    )
}

fn source_fields<'a>(id: &str, record: &'a Record<Provenance>) -> Result<SourceFields<'a>> {
    if id.is_empty()
        || id.len() > 2 * 1024 * 1024
        || record.conversation.is_empty()
        || record.conversation.len() > 53
        || record.revision == 0
    {
        return Err("invalid saved source header fields".into());
    }
    let known = match &record.value {
        Provenance::Known { source } => {
            if source.id != id {
                return Err("saved source identity differs".into());
            }
            check_page_fields(&source.page)?;
            true
        }
        Provenance::LegacyUnavailable { id: saved } => {
            if saved != id {
                return Err("saved source identity differs".into());
            }
            false
        }
    };
    Ok((&record.conversation, record.revision, known))
}

fn status_code(status: Status) -> u8 {
    match status {
        Status::Running => 0,
        Status::StopRequested => 1,
        Status::Complete => 2,
        Status::Cancelled => 3,
        Status::Failed => 4,
        Status::Interrupted => 5,
    }
}

fn status_value(code: u8) -> Result<Status> {
    match code {
        0 => Ok(Status::Running),
        1 => Ok(Status::StopRequested),
        2 => Ok(Status::Complete),
        3 => Ok(Status::Cancelled),
        4 => Ok(Status::Failed),
        5 => Ok(Status::Interrupted),
        _ => Err("invalid saved run status".into()),
    }
}
