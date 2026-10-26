use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use metldr_conversation::{EntryRef, MAX_TOOLS, ToolId};
use serde::{Deserialize, Serialize};

use crate::model::{
    ActivityOutcome, ActivityPhase, ActivityView, MAX_ACTIVITY, RunId, RunView, ToolPreviewView,
};

use super::message::RunOrigin;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ToolGroupReference {
    pub(crate) run_id: RunId,
    pub(crate) group_id: ToolId,
    pub(crate) tool_ids: Vec<ToolId>,
    pub(crate) report_count: u32,
    pub(crate) revision: String,
    pub(crate) origin: RunOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct ToolRow {
    pub(crate) id: ToolId,
    pub(crate) details: Option<RetainedTool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct RetainedTool {
    pub(crate) name: String,
    pub(crate) phase: ActivityPhase,
    pub(crate) outcome: Option<ActivityOutcome>,
    pub(crate) title: Option<ToolPreviewView>,
    pub(crate) input: Option<ToolPreviewView>,
    pub(crate) output: Option<ToolPreviewView>,
    pub(crate) history: Vec<ToolReport>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct ToolReport {
    pub(crate) phase: ActivityPhase,
    pub(crate) outcome: Option<ActivityOutcome>,
}

impl ToolGroupReference {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.tool_ids.is_empty()
            || self.tool_ids.len() > MAX_TOOLS
            || self.tool_ids.first() != Some(&self.group_id)
            || self.tool_ids.iter().any(|id| !ids.insert(id.as_str()))
            || self.report_count > MAX_ACTIVITY as u32
            || self.revision.len() != 16
            || !self.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            || matches!(&self.origin, RunOrigin::Saved { reference } if reference.id != self.run_id)
        {
            return Err("Invalid native tool group reference.".into());
        }
        Ok(())
    }

    pub(crate) fn message_id(&self) -> String {
        format!("{}:tools:{}", self.run_id, self.group_id.as_str())
    }
}

pub(crate) fn groups(run: &RunView, origin: RunOrigin) -> Vec<ToolGroupReference> {
    let mut groups = Vec::new();
    let mut ids = Vec::new();
    for entry in run.output.entries() {
        match entry {
            EntryRef::Tool { id } => ids.push(id.clone()),
            _ if !ids.is_empty() => {
                groups.push(group(run, std::mem::take(&mut ids), origin.clone()));
            }
            _ => (),
        }
    }
    if !ids.is_empty() {
        groups.push(group(run, ids, origin));
    }
    groups
}

fn group(run: &RunView, ids: Vec<ToolId>, origin: RunOrigin) -> ToolGroupReference {
    let rows = rows(&ids, &run.activity);
    // The revision is a display cache hint. Scope and native identities guard reads.
    let mut revision = DefaultHasher::new();
    run.status.hash(&mut revision);
    rows.hash(&mut revision);
    ToolGroupReference {
        run_id: run.id.clone(),
        group_id: ids[0].clone(),
        tool_ids: ids,
        report_count: rows
            .iter()
            .filter_map(|row| row.details.as_ref())
            .map(|details| details.history.len() as u32)
            .sum(),
        revision: format!("{:016x}", revision.finish()),
        origin,
    }
}

pub(crate) fn rows(ids: &[ToolId], reports: &[ActivityView]) -> Vec<ToolRow> {
    ids.iter()
        .map(|id| {
            let mut details: Option<RetainedTool> = None;
            for report in reports {
                let Some(call) = report.call.as_ref().filter(|call| call.id == id.as_str()) else {
                    continue;
                };
                let row = details.get_or_insert_with(|| RetainedTool {
                    name: report.tool.clone(),
                    phase: report.phase,
                    outcome: report.outcome,
                    title: None,
                    input: None,
                    output: None,
                    history: Vec::new(),
                });
                row.name.clone_from(&report.tool);
                row.phase = report.phase;
                row.outcome = report.outcome;
                row.history.push(ToolReport {
                    phase: report.phase,
                    outcome: report.outcome,
                });
                if call.title.is_some() {
                    row.title.clone_from(&call.title);
                }
                if call.input.is_some() {
                    row.input.clone_from(&call.input);
                }
                if call.output.is_some() {
                    row.output.clone_from(&call.output);
                }
            }
            ToolRow {
                id: id.clone(),
                details,
            }
        })
        .collect()
}

pub(crate) fn resolve(
    run: &RunView,
    reference: &ToolGroupReference,
) -> Result<Vec<ToolRow>, String> {
    reference.validate()?;
    let current = groups(run, reference.origin.clone());
    if run.id != reference.run_id || !current.contains(reference) {
        return Err("Native tool details changed. Refresh the conversation.".into());
    }
    Ok(rows(&reference.tool_ids, &run.activity))
}

#[cfg(test)]
pub(crate) mod checks {
    use super::*;
    use crate::model::{CorrectionHistory, RunKind, RunRoute, RunStatus};
    use metldr_conversation::{Completion, Display, MessageKey, ProjectedEntry, Projection};

    pub(crate) fn run() -> RunView {
        let id: RunId = serde_json::from_str("\"0123456789abcdef0123456789abcdef:1\"").unwrap();
        RunView {
            id: id.clone(),
            source: serde_json::from_str("\"0123456789abcdef0123456789abcdef:2\"").unwrap(),
            kind: RunKind::Agent,
            question: Some("Read the note".into()),
            status: RunStatus::Running,
            output: Display::new(Projection {
                output: "BeforeAfter".into(),
                conversation: vec![
                    ProjectedEntry::Message {key: MessageKey::observed(1).unwrap(), start: 0, end: 6, completion: Completion::FinalSnapshot},
                    ProjectedEntry::Tool {id: ToolId::new("call-1".into()).unwrap()},
                    ProjectedEntry::Tool {id: ToolId::new("call-2".into()).unwrap()},
                    ProjectedEntry::Message {key: MessageKey::observed(2).unwrap(), start: 6, end: 11, completion: Completion::FinalSnapshot},
                    ProjectedEntry::Tool {id: ToolId::new("call-3".into()).unwrap()},
                ],
            }).unwrap(),
            error: None,
            route: RunRoute::OpenCode {workflow: id, model: Some("test/probe".into()), mode: Some("build".into())},
            activity: serde_json::from_value(serde_json::json!([
                {"tool":"read","phase":"started","call":{"id":"call-1","input":{"text":"مرحبا 🌱","truncated":true}}},
                {"tool":"read","phase":"completed","outcome":"succeeded","call":{"id":"call-1","output":{"text":"17 units","truncated":false}}},
                {"tool":"execute","phase":"started"}
            ])).unwrap(),
            corrections: CorrectionHistory { total: 0, entries: vec![] },
        }
    }

    #[test]
    fn groups_ordered_calls_and_preserves_missing_metadata_and_revisions() {
        let mut run = run();
        let initial = groups(&run, RunOrigin::Live);
        assert_eq!(initial.len(), 2);
        assert_eq!(
            initial[0]
                .tool_ids
                .iter()
                .map(ToolId::as_str)
                .collect::<Vec<_>>(),
            ["call-1", "call-2"]
        );
        assert_eq!(initial[1].tool_ids[0].as_str(), "call-3");
        assert_eq!(initial[0].report_count, 2);
        let rows = resolve(&run, &initial[0]).unwrap();
        let retained = rows[0].details.as_ref().unwrap();
        assert_eq!(
            retained.history,
            [
                ToolReport {
                    phase: ActivityPhase::Started,
                    outcome: None
                },
                ToolReport {
                    phase: ActivityPhase::Completed,
                    outcome: Some(ActivityOutcome::Succeeded)
                },
            ]
        );
        assert_eq!(retained.phase, ActivityPhase::Completed);
        assert_eq!(retained.outcome, Some(ActivityOutcome::Succeeded));
        assert_eq!(retained.input.as_ref().unwrap().text, "مرحبا 🌱");
        assert!(retained.input.as_ref().unwrap().truncated);
        assert_eq!(retained.output.as_ref().unwrap().text, "17 units");
        assert!(rows[1].details.is_none());
        run.activity[1]
            .call
            .as_mut()
            .unwrap()
            .output
            .as_mut()
            .unwrap()
            .truncated = true;
        let changed = groups(&run, RunOrigin::Live);
        assert_eq!(changed[0].message_id(), initial[0].message_id());
        assert_ne!(changed[0].revision, initial[0].revision);
        assert!(resolve(&run, &initial[0]).is_err());
        assert!(resolve(&run, &changed[0]).is_ok());
        assert_eq!(changed[1], initial[1]);

        let original = run.activity[0].phase;
        run.activity[0].phase = ActivityPhase::Completed;
        let earlier_report_changed = groups(&run, RunOrigin::Live);
        assert_eq!(
            earlier_report_changed[0].report_count,
            changed[0].report_count
        );
        assert_ne!(earlier_report_changed[0].revision, changed[0].revision);
        run.activity[0].phase = original;
    }

    #[test]
    fn native_tool_groups_reach_the_live_conversation_in_order() {
        let mut run = run();
        let mut stream =
            super::super::RunStream::new("thread".into(), "ui-run".into(), vec![]).unwrap();
        let events = serde_json::to_value(stream.push(&run).unwrap()).unwrap();
        let groups = events
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["activityType"] == "metldr.native-tools")
            .map(|event| event["messageId"].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            groups,
            serde_json::json!([
                "0123456789abcdef0123456789abcdef:1:tools:call-1",
                "0123456789abcdef0123456789abcdef:1:tools:call-3"
            ])
            .as_array()
            .unwrap()
            .clone()
        );
        run.activity[1]
            .call
            .as_mut()
            .unwrap()
            .output
            .as_mut()
            .unwrap()
            .text = "18 units".into();
        let changed = serde_json::to_value(stream.push(&run).unwrap()).unwrap();
        assert_eq!(changed.as_array().unwrap().len(), 1);
        assert_eq!(changed[0]["type"], "ACTIVITY_SNAPSHOT");
        assert_eq!(
            changed[0]["messageId"],
            "0123456789abcdef0123456789abcdef:1:tools:call-1"
        );
        assert_eq!(changed[0]["replace"], true);
        assert!(stream.push(&run).unwrap().is_empty());
    }
}
