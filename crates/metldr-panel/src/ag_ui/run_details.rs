use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::model::{ActivityOutcome, ActivityPhase, ActivityView, RunId, RunView};

use super::message::RunOrigin;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunDetailsReference {
    pub(crate) run_id: RunId,
    pub(crate) origin: RunOrigin,
    pub(crate) revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunReport {
    pub(crate) tool: String,
    pub(crate) phase: ActivityPhase,
    pub(crate) outcome: Option<ActivityOutcome>,
    pub(crate) call_id: Option<String>,
}

impl RunDetailsReference {
    pub(crate) fn new(run: &RunView, origin: RunOrigin) -> Self {
        let reports = reports(&run.activity);
        let mut hasher = DefaultHasher::new();
        run.id.as_str().hash(&mut hasher);
        run.route.hash(&mut hasher);
        run.status.hash(&mut hasher);
        reports.hash(&mut hasher);
        Self {
            run_id: run.id.clone(),
            origin,
            revision: format!("{:016x}", hasher.finish()),
        }
    }

    pub(crate) fn message_id(&self) -> String {
        format!("{}:details", self.run_id)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.revision.len() != 16
            || !self.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            || matches!(&self.origin, RunOrigin::Saved { reference } if reference.id != self.run_id)
        {
            return Err("Invalid run details reference.".into());
        }
        Ok(())
    }
}

pub(crate) fn resolve(
    run: &RunView,
    reference: &RunDetailsReference,
) -> Result<Vec<RunReport>, String> {
    reference.validate()?;
    if RunDetailsReference::new(run, reference.origin.clone()) != *reference {
        return Err("Run details changed. Refresh the conversation.".into());
    }
    Ok(reports(&run.activity))
}

pub(crate) fn reports(activity: &[ActivityView]) -> Vec<RunReport> {
    activity
        .iter()
        .map(|report| RunReport {
            tool: report.tool.clone(),
            phase: report.phase,
            outcome: report.outcome,
            call_id: report.call.as_ref().map(|call| call.id.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ag_ui::tools::checks;

    #[test]
    fn retains_every_ordered_report_without_copying_previews_into_the_reference() {
        let mut run = checks::run();
        let reports = reports(&run.activity);
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].call_id.as_deref(), Some("call-1"));
        assert_eq!(reports[2].call_id, None);

        let first = RunDetailsReference::new(&run, RunOrigin::Live);
        assert_eq!(first.message_id(), format!("{}:details", run.id));
        run.activity[0].tool = "renamed".into();
        let changed = RunDetailsReference::new(&run, RunOrigin::Live);
        assert_eq!(changed.message_id(), first.message_id());
        assert_ne!(changed.revision, first.revision);
        let value = serde_json::to_value(&changed).unwrap();
        assert!(value.get("reports").is_none());
        assert_eq!(value["revision"].as_str().unwrap().len(), 16);
    }
}
