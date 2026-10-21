use crate::{Result, agent_model::Launch, model::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PREVIEW_BUDGET: usize = 8_192;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preview {
    pub text: String,
    pub truncated: bool,
}

impl Preview {
    pub fn text(text: &str, limit: usize) -> Self {
        let mut preview = Self {
            text: text.into(),
            truncated: false,
        };
        preview.limit(limit);
        preview
    }

    pub fn input(value: &Value) -> Self {
        Self::text(
            &serde_json::to_string_pretty(value).expect("JSON value serializes"),
            1_024,
        )
    }

    pub fn pi_result(value: &Value) -> Result<Self> {
        value
            .get("content")
            .and_then(|content| Self::contents(content, false))
            .ok_or_else(|| "invalid Pi tool result".into())
    }

    fn contents(value: &Value, nested: bool) -> Option<Self> {
        let parts = value.as_array()?;
        let mut text = String::new();
        let mut incomplete = false;
        for part in parts {
            let content = if nested {
                part.get("content")
            } else {
                Some(part)
            };
            if let Some(value) = content
                .filter(|p| p["type"] == "text")
                .and_then(|p| p["text"].as_str())
            {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(value);
            } else {
                incomplete = true;
            }
        }
        let mut result = Self::text(&text, 2_048);
        result.truncated |= incomplete;
        Some(result)
    }

    fn limit(&mut self, limit: usize) {
        if self.text.len() <= limit {
            return;
        }
        let mut end = limit;
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        self.text.truncate(end);
        self.truncated = true;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Call {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<Preview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Preview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Preview>,
}

impl Call {
    pub fn new(id: &str) -> Result<Self> {
        let call = Self {
            id: id.into(),
            title: None,
            input: None,
            output: None,
        };
        if !call.valid() {
            return Err("invalid native tool call identity".into());
        }
        Ok(call)
    }

    pub fn merge_opencode(&mut self, update: &Value) {
        if let Some(title) = update["title"].as_str() {
            self.title = Some(Preview::text(title, 256));
        }
        if let Some(input) = update.get("rawInput").filter(|v| !v.is_null()) {
            self.input = Some(Preview::input(input));
        }
        if let Some(output) = update
            .get("content")
            .and_then(|v| Preview::contents(v, true))
        {
            self.output = Some(output);
        }
    }

    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && !self.id.chars().any(char::is_control)
            && [&self.title, &self.input, &self.output]
                .into_iter()
                .zip([256, 1_024, 2_048])
                .all(|(preview, limit)| preview.as_ref().is_none_or(|p| p.text.len() <= limit))
    }

    fn limit(&mut self, limit: usize) {
        for preview in [&mut self.title, &mut self.input, &mut self.output]
            .into_iter()
            .flatten()
        {
            preview.limit(limit);
        }
    }
}

fn coalesce(activity: &mut [crate::agent_model::Activity], update: Call) -> bool {
    if !update.valid() {
        return false;
    }
    let Some(report) = activity
        .iter_mut()
        .rev()
        .find(|report| {
            report
                .call
                .as_ref()
                .is_some_and(|call| call.id == update.id)
        })
        .filter(|report| report.phase == "started")
    else {
        return false;
    };
    let call = report.call.as_mut().expect("matched call");
    let before = call.clone();
    for (current, supplied) in [
        (&mut call.title, update.title),
        (&mut call.input, update.input),
        (&mut call.output, update.output),
    ] {
        if supplied.is_some() {
            *current = supplied;
        }
    }
    *call != before
}

pub(super) fn send_progress(
    tx: &std::sync::mpsc::SyncSender<crate::flow::Event>,
    run: &str,
    call: Call,
) -> Result<()> {
    match tx.try_send(crate::flow::Event::ToolProgress {
        run: run.into(),
        call,
    }) {
        Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => Ok(()),
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            Err("native progress owner is unavailable".into())
        }
    }
}

impl crate::flow::Owner {
    pub(super) fn tool_progress(&mut self, run: &str, call: Call) -> Result<()> {
        if self.operation.as_ref().is_none_or(|op| op.run != run)
            || self
                .state
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .is_none_or(|r| r.status != crate::model::Status::Running)
        {
            return Ok(());
        }
        let mut next = self.state.clone();
        let Some(
            Launch::Pi { activity, .. }
            | Launch::Codex { activity, .. }
            | Launch::OpenCode { activity, .. },
        ) = next.launches.get_mut(run)
        else {
            return Ok(());
        };
        if coalesce(activity, call) {
            self.commit(next)?;
        }
        Ok(())
    }
}

pub(super) fn fit_store(state: &mut Store) -> Result<()> {
    let mut fields = 0;
    let mut bytes = 0;
    for launch in state.launches.values() {
        if let Launch::Pi { activity, .. }
        | Launch::Codex { activity, .. }
        | Launch::OpenCode { activity, .. } = launch
        {
            for call in activity.iter().filter_map(|a| a.call.as_ref()) {
                for p in [&call.title, &call.input, &call.output]
                    .into_iter()
                    .flatten()
                {
                    fields += 1;
                    bytes += p.text.len();
                }
            }
        }
    }
    if bytes > PREVIEW_BUDGET {
        limit_store(state, PREVIEW_BUDGET / fields.max(1), false);
    }
    // Optional previews must not prevent core task state from being saved.
    if serde_json::to_vec(state)?.len() > crate::STORE_MAX {
        limit_store(state, 0, false);
    }
    if serde_json::to_vec(state)?.len() > crate::STORE_MAX {
        limit_store(state, 0, true);
    }
    Ok(())
}

fn limit_store(state: &mut Store, limit: usize, remove: bool) {
    for launch in state.launches.values_mut() {
        if let Launch::Pi { activity, .. }
        | Launch::Codex { activity, .. }
        | Launch::OpenCode { activity, .. } = launch
        {
            for activity in activity {
                if remove {
                    activity.call = None;
                } else if let Some(call) = &mut activity.call {
                    call.limit(limit);
                }
            }
        }
    }
}

pub(super) fn fit_wire(reply: &mut Value) -> Result<()> {
    if serde_json::to_vec(reply)?.len() <= crate::FRAME_MAX {
        return Ok(());
    }
    if let Some(runs) = reply["view"]["runs"].as_array_mut() {
        for run in runs {
            if let Some(reports) = run["activity"].as_array_mut() {
                for report in reports {
                    for name in ["title", "input", "output"] {
                        if let Some(preview) =
                            report.get_mut("call").and_then(|call| call.get_mut(name))
                        {
                            if !preview["text"].as_str().unwrap_or("").is_empty() {
                                preview["text"] = Value::String(String::new());
                                preview["truncated"] = Value::Bool(true);
                            }
                        }
                    }
                }
            }
        }
    }
    if serde_json::to_vec(reply)?.len() > crate::FRAME_MAX {
        if let Some(runs) = reply["view"]["runs"].as_array_mut() {
            for run in runs {
                if let Some(reports) = run["activity"].as_array_mut() {
                    for report in reports {
                        report
                            .as_object_mut()
                            .ok_or("invalid internal activity")?
                            .remove("call");
                    }
                }
            }
        }
    }
    if serde_json::to_vec(reply)?.len() > crate::FRAME_MAX {
        return Err("required native view exceeds frame bound".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_model::Activity;
    use serde_json::json;

    #[test]
    fn progress_updates_only_retained_active_previews_without_adding_reports() {
        let mut call = Call::new("owned-call").unwrap();
        call.title = Some(Preview::text("Native tool", 256));
        let mut activity = vec![Activity {
            tool: "native".into(),
            phase: "started".into(),
            outcome: None,
            call: Some(call.clone()),
        }];
        let update = |text: &str| {
            let mut call = Call::new("owned-call").unwrap();
            call.output = Some(Preview::text(text, 2048));
            call
        };
        assert!(coalesce(&mut activity, update("partial")));
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].phase, "started");
        assert_eq!(activity[0].outcome, None);
        assert_eq!(activity[0].call.as_ref().unwrap().title, call.title);
        assert!(!coalesce(&mut activity, update("partial")));
        assert!(coalesce(&mut activity, update("")));
        assert_eq!(
            activity[0]
                .call
                .as_ref()
                .unwrap()
                .output
                .as_ref()
                .unwrap()
                .text,
            ""
        );
        assert!(!coalesce(&mut activity, Call::new("unknown").unwrap()));
        call.output = Some(Preview::text("authoritative final", 2048));
        activity.push(Activity {
            tool: "native".into(),
            phase: "completed".into(),
            outcome: None,
            call: Some(call),
        });
        assert!(!coalesce(&mut activity, update("late")));
        assert_eq!(activity.len(), 2);
        assert_eq!(
            activity[1]
                .call
                .as_ref()
                .unwrap()
                .output
                .as_ref()
                .unwrap()
                .text,
            "authoritative final"
        );
        for report in &mut activity {
            report.call = None;
        }
        assert!(!coalesce(&mut activity, update("budget erased")));
        assert!(activity.iter().all(|report| report.call.is_none()));
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        send_progress(&tx, "run", update("first")).unwrap();
        send_progress(&tx, "run", update("full queue")).unwrap();
        assert!(
            matches!(rx.try_recv().unwrap(), crate::flow::Event::ToolProgress { call, .. } if call.output.as_ref().unwrap().text == "first")
        );
        assert!(matches!(
            rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        drop(rx);
        assert!(send_progress(&tx, "run", update("gone")).is_err());
        assert!(Preview::pi_result(&Value::Null).is_err());
        assert!(Preview::pi_result(&json!({"content":"invalid"})).is_err());
        assert_eq!(Preview::pi_result(&json!({"content":[]})).unwrap().text, "");
    }

    #[test]
    fn native_details_preserve_legacy_reports_and_partial_updates() {
        let old_codex: Launch = serde_json::from_value(json!({
            "kind":"codex", "input_digest":"d", "workflow":"w", "selection":null
        }))
        .unwrap();
        assert!(matches!(old_codex, Launch::Codex { activity, .. } if activity.is_empty()));
        let legacy: Activity =
            serde_json::from_value(json!({"tool":"read","phase":"started"})).unwrap();
        assert!(legacy.call.is_none());
        assert_eq!(
            serde_json::to_value(&legacy).unwrap(),
            json!({"tool":"read","phase":"started"})
        );
        for (phase, outcome, valid) in [
            ("started", None, true),
            ("started", Some("succeeded"), false),
            ("completed", Some("succeeded"), true),
            ("completed", Some("failed"), false),
            ("failed", Some("declined"), true),
            ("failed", Some("unspecified"), false),
        ] {
            let activity = Activity {
                tool: "read".into(),
                phase: phase.into(),
                outcome: outcome.map(str::to_owned),
                call: None,
            };
            assert_eq!(activity.valid(), valid);
        }
        let mut call = Call::new("native-17").unwrap();
        call.merge_opencode(&json!({"title":"Read note"}));
        call.merge_opencode(&json!({"rawInput":{"path":"owned-note.txt"}}));
        call.merge_opencode(&json!({"content":[{"type":"content","content":{"type":"text","text":"owned-note=17"}},{"type":"diff","path":"file"}]}));
        assert_eq!(call.title.unwrap().text, "Read note");
        assert!(call.input.unwrap().text.contains("owned-note.txt"));
        let output = call.output.unwrap();
        assert_eq!(output.text, "owned-note=17");
        assert!(output.truncated);
        let preview = Preview::text("éé", 3);
        assert_eq!(preview.text, "é");
        assert!(preview.truncated);
        assert!(Call::new("bad\nidentity").is_err());
        assert_eq!(
            Preview::pi_result(&json!({"content":[{"type":"text","text":"FILE_ONLY_73"}]}))
                .unwrap()
                .text,
            "FILE_ONLY_73"
        );
    }

    #[test]
    fn optional_details_degrade_before_core_store_or_wire_state() {
        let mut state = Store {
            schema: crate::STORE_VERSION,
            view: crate::model::View {
                sequence: 7,
                store: "a".repeat(32),
                source: None,
                runs: vec![],
            },
            next: 1,
            starts: Default::default(),
            launches: Default::default(),
            workflows: Default::default(),
            codex_workflows: Default::default(),
            opencode_workflows: Default::default(),
            browser_actions: Default::default(),
            providers: Default::default(),
            agent_settings: Default::default(),
        };
        for index in 0..5 {
            state.view.runs.push(crate::model::Run {
                id: format!("r{index}"),
                source: "s1".into(),
                kind: "agent".into(),
                question: None,
                status: crate::model::Status::Complete,
                output: metldr_conversation::Content::flat("x".repeat(8_000)).unwrap(),
                error: None,
            });
        }
        for index in 0..2 {
            let activity = (0..32)
                .map(|id| {
                    let mut call = Call::new(&format!("{id:0128}")).unwrap();
                    call.input = Some(Preview::text(&"i".repeat(1_024), 1_024));
                    call.output = Some(Preview::text(&"o".repeat(2_048), 2_048));
                    Activity {
                        tool: "read".into(),
                        phase: "completed".into(),
                        outcome: (index == 0).then(|| "succeeded".into()),
                        call: Some(call),
                    }
                })
                .collect();
            let launch = if index == 0 {
                Launch::Codex {
                    input_digest: "d".into(),
                    workflow: "w".into(),
                    selection: None,
                    activity,
                }
            } else {
                Launch::OpenCode {
                    input_digest: "d".into(),
                    workflow: "w".into(),
                    selection: None,
                    activity,
                }
            };
            state.launches.insert(format!("r{index}"), launch);
        }
        let core = serde_json::to_value(&state.view).unwrap();
        assert!(serde_json::to_vec(&state).unwrap().len() > crate::STORE_MAX);
        fit_store(&mut state).unwrap();
        assert!(serde_json::to_vec(&state).unwrap().len() <= crate::STORE_MAX);
        assert_eq!(serde_json::to_value(&state.view).unwrap(), core);
        assert!(state.launches.values().all(|launch| match launch {
            Launch::Codex { activity, .. } | Launch::OpenCode { activity, .. } =>
                activity.len() == 32,
            _ => false,
        }));
        assert!(matches!(
            state.launches.get("r0"),
            Some(Launch::Codex { activity, .. }) if activity[0].outcome.as_deref() == Some("succeeded")
        ));

        let mut report = json!({"tool":"read","phase":"completed","outcome":"succeeded","call":{"id":"c".repeat(128),"input":{"text":"a".repeat(1_024),"truncated":false}}});
        let mut wire = json!({"view":{"output":"x".repeat(crate::FRAME_MAX - 8_000),"runs":[{"activity":vec![report.clone();64]}]}});
        let output = wire["view"]["output"].clone();
        assert!(serde_json::to_vec(&wire).unwrap().len() > crate::FRAME_MAX);
        fit_wire(&mut wire).unwrap();
        assert!(serde_json::to_vec(&wire).unwrap().len() <= crate::FRAME_MAX);
        assert_eq!(wire["view"]["output"], output);
        assert!(fit_wire(&mut json!({"view":{"output":"x".repeat(crate::FRAME_MAX)}})).is_err());
        let reports = wire["view"]["runs"][0]["activity"].as_array().unwrap();
        assert_eq!(reports.len(), 64);
        assert!(reports.iter().all(|r| r["outcome"] == "succeeded"));
        report.as_object_mut().unwrap().remove("call");
        assert!(reports.iter().all(|r| r == &report));
    }
}
