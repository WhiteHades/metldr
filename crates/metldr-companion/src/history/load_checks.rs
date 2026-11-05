use super::{ConversationChange, Edit, History, NativeWorkflow, Selection, fixture, schema::*};
use crate::{
    Result,
    agent_model::Launch,
    browser_action::{Binding, Intent, Proposal},
    model::{Page, Run, Source, Status},
};
use redb::{ReadableDatabase, ReadableTableMetadata};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Result<Self> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "history-load-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setting(name: &str, default: &str, allowed: &[&str]) -> Result<String> {
    let value = match std::env::var(name) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => default.into(),
        Err(error) => return Err(error.into()),
    };
    if !allowed.contains(&value.as_str()) {
        return Err(format!("{name} must be one of {allowed:?}").into());
    }
    Ok(value)
}
fn text(label: &str, bytes: usize) -> String {
    format!("{label}{}", "x".repeat(bytes - label.len()))
}
fn timed<T>(operation: impl FnOnce() -> Result<T>) -> (u128, Result<T>) {
    let start = Instant::now();
    let result = operation();
    (start.elapsed().as_nanos(), result)
}
fn record_query(
    history: &mut History,
    name: &str,
    query: impl Fn(&mut History) -> Result<usize>,
) -> Value {
    let mut observations = Vec::with_capacity(34);
    let mut failures = Vec::new();
    let mut expected = None;
    for iteration in 0..34 {
        let (elapsed, result) = timed(|| query(history));
        match result {
            Ok(count) => {
                if iteration == 0 {
                    expected = Some(count);
                }
                if expected != Some(count) {
                    failures.push(format!(
                        "observation {iteration}: work {count} differs from {expected:?}"
                    ));
                }
            }
            Err(error) => failures.push(format!("observation {iteration}: {error}")),
        }
        observations.push(elapsed);
    }
    json!({"name":name,"first_access_ns":observations[0],"warmup_3_ns":observations[1..4],"samples_30_ns":observations[4..],"work_count":expected,"failures":failures})
}

fn native_group(
    edit: &mut Edit<'_>,
    scope: &Selection,
    source: &Source,
    templates: &[NativeWorkflow; 3],
    group: usize,
) -> Result<Vec<String>> {
    let mut runs = Vec::with_capacity(3);
    for (kind, template) in templates.iter().enumerate() {
        let workflow_id = edit.id()?;
        let run_id = edit.id()?;
        let mut workflow = template.clone();
        match &mut workflow {
            NativeWorkflow::Pi(value) => {
                value.source = source.clone();
                value.last_run = run_id.clone();
                value.revision = 2;
                value.binding = crate::agent_model::Binding::Pending;
            }
            NativeWorkflow::Codex(value) => {
                value.source = source.clone();
                value.last_run = run_id.clone();
                value.revision = 2;
                value.binding = crate::codex_model::Binding::Pending;
            }
            NativeWorkflow::OpenCode(value) => {
                value.source = source.clone();
                value.last_run = run_id.clone();
                value.revision = 2;
                value.binding = crate::opencode_model::Binding::Pending;
            }
        }
        let input_digest = format!("{:064x}", group * 3 + kind + 1);
        let launch = match &workflow {
            NativeWorkflow::Pi(_) => Launch::Pi {
                input_digest,
                workflow: workflow_id,
                activity: Vec::new(),
            },
            NativeWorkflow::Codex(_) => Launch::Codex {
                input_digest,
                workflow: workflow_id,
                selection: None,
                activity: Vec::new(),
            },
            NativeWorkflow::OpenCode(_) => Launch::OpenCode {
                input_digest,
                workflow: workflow_id,
                selection: None,
                activity: Vec::new(),
            },
        };
        edit.admit_native(
            scope,
            &format!("load-native-{group}-{kind}"),
            Run {
                id: run_id.clone(),
                source: source.id.clone(),
                kind: "agent".into(),
                question: Some("Owned load task".into()),
                status: Status::Running,
                output: metldr_conversation::Content::native(),
                error: None,
                corrections: Vec::new(),
            },
            launch,
            workflow,
        )?;
        let mut task = edit.native_task(scope, &run_id)?;
        match &mut task.workflow {
            NativeWorkflow::Pi(value) => {
                let id = format!("load-pi-{group}");
                value.binding = crate::agent_model::Binding::Bound {
                    id: id.clone(),
                    path: value
                        .profile
                        .session_dir
                        .join(format!("{id}.jsonl"))
                        .to_str()
                        .ok_or("non UTF8 fixture path")?
                        .into(),
                };
                value.revision += 1;
            }
            NativeWorkflow::Codex(value) => {
                value.binding = crate::codex_model::Binding::Bound {
                    identity: crate::codex_model::Identity {
                        thread: format!("thread-load-{group}"),
                        session: format!("session-load-{group}"),
                    },
                };
                value.revision += 1;
                let Launch::Codex { selection, .. } = &mut task.launch else {
                    return Err("native kind mismatch".into());
                };
                *selection = Some(crate::codex_model::Selection {
                    provider: "fixture-provider".into(),
                    model: "fixture-model".into(),
                });
            }
            NativeWorkflow::OpenCode(value) => {
                let selected = crate::opencode_model::Selection {
                    model: "fixture-model".into(),
                    mode: "build".into(),
                };
                value.binding = crate::opencode_model::Binding::Bound {
                    identity: crate::opencode_model::Identity {
                        session: format!("session_load_{group}"),
                        selection: selected.clone(),
                    },
                };
                value.revision += 1;
                let Launch::OpenCode { selection, .. } = &mut task.launch else {
                    return Err("native kind mismatch".into());
                };
                *selection = Some(selected);
            }
        }
        edit.put_native(scope, task)?;
        let (action, created) = edit.propose_browser(
            scope,
            Proposal {
                run: run_id.clone(),
                tool_use_id: "load-field-call".into(),
                binding: Binding {
                    tab: source.page.tab,
                    document: source.page.document.clone(),
                    target: format!("load-target-{group}"),
                    frame: "load-frame".into(),
                    session: "load-cdp-session".into(),
                    unique_context: format!("context-{group}-{kind}"),
                    grant_revision: 1,
                },
                url: source.page.url.clone(),
                intent: Intent::Field {
                    field_id: "load-field".into(),
                    field_label: "Load field".into(),
                    previous_value: String::new(),
                    value: "load".into(),
                },
            },
        )?;
        if !created {
            return Err("load browser proposal unexpectedly duplicated".into());
        }
        let rejected = edit.decide_browser(scope, &action.id, action.revision, false)?;
        if !matches!(
            rejected.state,
            crate::browser_action::Phase::Settled {
                outcome: crate::browser_action::Outcome::Prevented { .. }
            }
        ) {
            return Err("load browser action was not rejected".into());
        }
        let mut task = edit.native_task(scope, &run_id)?;
        task.run.output.change(metldr_conversation::Change::Final {
            key: metldr_conversation::MessageKey::native("load-message".into())?,
            text: format!("native {group}.{kind}"),
        })?;
        task.run.status = Status::Complete;
        edit.put_native(scope, task)?;
        runs.push(run_id);
    }
    Ok(runs)
}

#[test]
#[ignore = "explicit local history database load workload"]
fn history_load_real_relationships() -> Result<()> {
    let scale = setting(
        "METLDR_HISTORY_LOAD_SCALE",
        "1",
        &["1", "10", "100", "1000"],
    )?
    .parse::<usize>()?;
    let layout = setting(
        "METLDR_HISTORY_LOAD_LAYOUT",
        "spread",
        &["spread", "fanout"],
    )?;
    let bytes = setting(
        "METLDR_HISTORY_LOAD_BYTES",
        "1024",
        &["128", "1024", "8192"],
    )?
    .parse::<usize>()?;
    let directory = Directory::new()?;
    let (disk, mut empty) = crate::store::Disk::open(directory.0.clone())?;
    empty.view.store = "0123456789abcdef0123456789abcdef".into();
    empty.validate()?;
    let templates = fixture::legacy();
    let native_templates = [
        NativeWorkflow::Pi(
            templates
                .workflows
                .values()
                .next()
                .ok_or("Pi template absent")?
                .clone(),
        ),
        NativeWorkflow::Codex(
            templates
                .codex_workflows
                .values()
                .next()
                .ok_or("Codex template absent")?
                .clone(),
        ),
        NativeWorkflow::OpenCode(
            templates
                .opencode_workflows
                .values()
                .next()
                .ok_or("OpenCode template absent")?
                .clone(),
        ),
    ];
    let native_profiles = BTreeMap::from([
        (
            crate::agent_settings::Kind::Pi,
            templates
                .workflows
                .values()
                .next()
                .unwrap()
                .fingerprint
                .clone(),
        ),
        (
            crate::agent_settings::Kind::Codex,
            templates
                .codex_workflows
                .values()
                .next()
                .unwrap()
                .fingerprint
                .clone(),
        ),
        (
            crate::agent_settings::Kind::Opencode,
            templates
                .opencode_workflows
                .values()
                .next()
                .unwrap()
                .fingerprint
                .clone(),
        ),
    ]);
    let path = directory.0.join("history.redb");
    let mut history = History::import_new(&path, &serde_json::to_vec(&empty)?, disk.lease())?;
    let mut latest_response = String::new();
    let mut latest_artifact = String::new();
    let mut latest_native = Vec::new();
    let mut latest_fixtures = Vec::new();
    let mut latest_text = Vec::new();
    let seed_start = Instant::now();
    for group in 0..scale {
        let scope = if layout == "spread" && group != 0 {
            history.edit(|edit| {
                let current = edit.selection();
                Ok(edit
                    .change_conversation(
                        &current,
                        &format!("load-conversation-{group}"),
                        ConversationChange::Create,
                    )?
                    .current)
            })?
        } else {
            history.selection()?
        };
        let source = history.edit(|edit| {
            edit.new_source(
                &scope,
                Page {
                    tab: group as i32 + 100,
                    document: format!("load-document-{group}"),
                    url: "http://127.0.0.1:4311/load".into(),
                    title: format!("Load source {group}"),
                    text: text("Owned load source: ", bytes),
                },
            )
        })?;
        history.edit(|edit| {
            latest_fixtures.clear();
            latest_text.clear();
            for item in 0..16 {
                let id = edit.id()?;
                edit.admit(
                    &scope,
                    &format!("load-fixture-{group}-{item}"),
                    Run {
                        id: id.clone(),
                        source: source.id.clone(),
                        kind: "summary".into(),
                        question: None,
                        status: Status::Running,
                        output: Default::default(),
                        error: None,
                        corrections: Vec::new(),
                    },
                    Launch::Fixture {
                        input_digest: format!("{:064x}", group * 16 + item + 1),
                    },
                )?;
                let mut run = edit.run(&scope, &id)?;
                run.output
                    .change(metldr_conversation::Change::FlatAppend(text(
                        &format!("Summary {group}.{item}: "),
                        bytes,
                    )))?;
                run.status = Status::Complete;
                edit.put_run(&scope, run)?;
                latest_fixtures.push(id);
            }
            for item in 0..6 {
                let id = edit.id()?;
                edit.admit(
                    &scope,
                    &format!("load-response-{group}-{item}"),
                    Run {
                        id: id.clone(),
                        source: source.id.clone(),
                        kind: "answer".into(),
                        question: Some(format!("Load question {group}.{item}?")),
                        status: Status::Running,
                        output: metldr_conversation::Content::native(),
                        error: None,
                        corrections: Vec::new(),
                    },
                    Launch::Text {
                        input_digest: format!("{:064x}", group * 6 + item + 1),
                        protocol: crate::provider_settings::Protocol::Responses,
                        binding: crate::https_profile::Binding {
                            endpoint: "https://example.test/v1/responses".into(),
                            model: "fixture-responses".into(),
                            revision: 1,
                            fingerprint: "a".repeat(64),
                        },
                    },
                )?;
                let mut run = edit.run(&scope, &id)?;
                run.output.change(metldr_conversation::Change::Final {
                    key: metldr_conversation::MessageKey::observed(1)?,
                    text: text(&format!("Answer {group}.{item}: "), bytes),
                })?;
                edit.put_run(&scope, run)?;
                edit.accept_chart(
                    &scope,
                    &id,
                    &format!("load-chart-{group}-{item}"),
                    "load-surface",
                    metldr_conversation::Chart::new(
                        metldr_conversation::ChartKind::Bar,
                        "Load chart".into(),
                        String::new(),
                        vec![metldr_conversation::ChartDatum {
                            label: "Rows".into(),
                            value: 1.into(),
                        }],
                    )?,
                )?;
                latest_artifact = edit
                    .accept_table(
                        &scope,
                        &id,
                        &format!("load-table-{group}-{item}"),
                        "load-surface",
                        metldr_conversation::Table::new(
                            "Load table".into(),
                            vec!["Index".into()],
                            vec![vec![group.to_string()]],
                            "Owned load data".into(),
                        )?,
                    )?
                    .as_str()
                    .into();
                let mut run = edit.run(&scope, &id)?;
                run.status = Status::Complete;
                edit.put_run(&scope, run)?;
                latest_response = id.clone();
                latest_text.push(id);
            }
            latest_native = native_group(edit, &scope, &source, &native_templates, group)?;
            Ok(())
        })?;
    }
    let seed_ns = seed_start.elapsed().as_nanos();
    drop(history);
    let (reopen_ns, reopened) = timed(|| History::open(&path, disk.lease()));
    let mut history = reopened?;
    let scope = history.selection()?;
    let counts = {
        let read = history.db.begin_read()?;
        let mut counts = BTreeMap::new();
        for (name, table) in [
            ("conversations", CONVERSATIONS),
            ("sources", SOURCES),
            ("runs", RUNS),
            ("launches", LAUNCHES),
            ("starts", STARTS),
            ("artifacts", ARTIFACTS),
            ("actions", ACTIONS),
            ("native_targets", NATIVE_TARGETS),
            ("pi_workflows", PI),
            ("codex_workflows", CODEX),
            ("opencode_workflows", OPENCODE),
        ] {
            counts.insert(name, read.open_table(table)?.len()?);
        }
        counts.insert(
            "run_headers",
            read.open_table(super::summary::RUN_HEADERS)?.len()?,
        );
        counts.insert(
            "source_headers",
            read.open_table(super::summary::SOURCE_HEADERS)?.len()?,
        );
        counts.insert("native_choices", read.open_table(NATIVE_CHOICES)?.len()?);
        counts.insert("completed_context", read.open_table(CONTEXT)?.len()?);
        counts.insert("workflow_runs", read.open_table(WORKFLOW_RUNS)?.len()?);
        assert_eq!(read.open_table(ACTIVE_RUNS)?.len()?, 0);
        assert_eq!(read.open_table(OPEN_ACTIONS)?.len()?, 0);
        counts
    };
    for (name, per_group) in [
        ("sources", 1),
        ("source_headers", 1),
        ("runs", 25),
        ("run_headers", 25),
        ("launches", 25),
        ("starts", 25),
        ("artifacts", 12),
        ("actions", 3),
        ("native_targets", 3),
        ("native_choices", 3),
        ("pi_workflows", 1),
        ("codex_workflows", 1),
        ("opencode_workflows", 1),
        ("completed_context", 22),
        ("workflow_runs", 3),
    ] {
        assert_eq!(
            counts[name],
            scale as u64 * per_group,
            "{name} did not scale"
        );
    }
    assert_eq!(
        counts["conversations"],
        if layout == "spread" { scale as u64 } else { 1 }
    );
    let latest_native_id = latest_native.last().ok_or("native seed absent")?;
    let source = history
        .edit(|edit| edit.selected_source(&scope))?
        .ok_or("load source absent")?;
    let mut queries = serde_json::Map::new();
    queries.insert(
        "runs_page".into(),
        record_query(&mut history, "runs_page", |history| {
            history.edit(|edit| {
                let page = edit.runs_page(&scope, None)?;
                if page.runs.len() != 16 || page.runs[0].id != *latest_native_id {
                    return Err("run page work or latest identity differs".into());
                }
                Ok(page.runs.len())
            })
        }),
    );
    queries.insert(
        "conversations_page".into(),
        record_query(&mut history, "conversations_page", |history| {
            history.edit(|edit| {
                let page = edit.conversations_page(&scope, None)?;
                let expected = if layout == "spread" { scale.min(16) } else { 1 };
                if page.conversations.len() != expected {
                    return Err("conversation page work differs".into());
                }
                Ok(page.conversations.len())
            })
        }),
    );
    queries.insert(
        "project_latest".into(),
        record_query(&mut history, "project_latest", |history| {
            history.edit(|edit| {
                let store = edit.project_latest(&scope)?;
                if store.view.runs.first().map(|r| r.id.as_str()) != Some(latest_native_id.as_str())
                {
                    return Err("latest projection identity differs".into());
                }
                Ok(store.view.runs.len())
            })
        }),
    );
    queries.insert(
        "run_detail".into(),
        record_query(&mut history, "run_detail", |history| {
            history.edit(|edit| {
                let detail = edit.run_detail(&scope, &latest_response)?;
                if detail.value["id"] != latest_response {
                    return Err("run detail identity differs".into());
                }
                Ok(1)
            })
        }),
    );
    queries.insert(
        "artifact_detail".into(),
        record_query(&mut history, "artifact_detail", |history| {
            history.edit(|edit| {
                let detail = edit.artifact_detail(&scope, &latest_artifact)?;
                if detail.value.run != latest_response {
                    return Err("artifact run ownership differs".into());
                }
                Ok(1)
            })
        }),
    );
    queries.insert(
        "native_task".into(),
        record_query(&mut history, "native_task", |history| {
            history.edit(|edit| {
                for id in &latest_native {
                    let task = edit.native_task(&scope, id)?;
                    if task.run.id != *id || task.run.status != Status::Complete {
                        return Err("native task identity or status differs".into());
                    }
                }
                Ok(latest_native.len())
            })
        }),
    );
    queries.insert(
        "native_choices".into(),
        record_query(&mut history, "native_choices", |history| {
            let choices = history.native_choices(&scope, Some(&source), &native_profiles)?;
            let mut view = json!({});
            choices.apply(&mut view)?;
            let count = ["agent_sessions", "codex_sessions", "opencode_sessions"]
                .iter()
                .map(|key| view[*key].as_array().map_or(0, Vec::len))
                .sum::<usize>();
            if count != 3 {
                return Err("native choices work differs".into());
            }
            Ok(count)
        }),
    );
    queries.insert(
        "browser_by_tool".into(),
        record_query(&mut history, "browser_by_tool", |history| {
            history.edit(|edit| {
                for id in &latest_native {
                    let action = edit
                        .browser_by_tool(&scope, id, "load-field-call")?
                        .ok_or("browser proposal lookup absent")?;
                    if action.proposal.run != *id {
                        return Err("browser proposal ownership differs".into());
                    }
                }
                Ok(latest_native.len())
            })
        }),
    );
    queries.insert(
        "fixture_context".into(),
        record_query(&mut history, "fixture_context", |history| {
            history.edit(|edit| {
                let runs = edit.fixture_context(&scope, &source.id)?;
                if runs.iter().map(|r| &r.id).collect::<Vec<_>>()
                    != latest_fixtures.iter().collect::<Vec<_>>()
                {
                    return Err("fixture context work differs".into());
                }
                Ok(runs.len())
            })
        }),
    );
    queries.insert(
        "text_context".into(),
        record_query(&mut history, "text_context", |history| {
            history.edit(|edit| {
                let runs = edit.text_context(
                    &scope,
                    &source.id,
                    crate::provider_settings::Protocol::Responses,
                    &"a".repeat(64),
                )?;
                if runs.iter().map(|r| &r.id).collect::<Vec<_>>()
                    != latest_text.iter().collect::<Vec<_>>()
                {
                    return Err("text context work differs".into());
                }
                Ok(runs.len())
            })
        }),
    );
    let failures = queries
        .values()
        .map(|q| q["failures"].as_array().unwrap().len())
        .sum::<usize>();
    let result = json!({"scope":"Production queries, validation, and transaction setup and close. Seeding and result serialization excluded. Native choices uses its read transaction.","scale":scale,"layout":layout,"payload_bytes":bytes,"seed":"deterministic empty store and proportional groups admitted only through real Edit APIs","counts":counts,"seed_ns":seed_ns,"reopen_validation_ns":reopen_ns,"reopen_scope":"Full History::open audit after closing existing handle, not cold OS cache","file_bytes":fs::metadata(&path)?.len(),"queries":queries,"failure_count":failures,"effects":"browser proposals are rejected before dispatch; no agent or network effect runs","statistics":"Within process observations are not independent replicates. Compare separately repeated paired processes."});
    println!(
        "METLDR_HISTORY_LOAD_JSON {}",
        serde_json::to_string(&result)?
    );
    if failures != 0 {
        return Err(format!("{failures} database load query observations failed").into());
    }
    Ok(())
}
