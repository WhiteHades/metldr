use super::schema::*;
use crate::{
    Result,
    model::{Source, Store, View},
};
use redb::{Database, Durability, ReadableTable};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) struct Prepared {
    state: Store,
    conversation: String,
    next: u64,
    sources: BTreeMap<String, Provenance>,
}

impl Prepared {
    pub(super) fn new(original: &[u8]) -> Result<Self> {
        if original.len() > crate::STORE_MAX {
            return Err("legacy snapshot exceeds bound".into());
        }
        let state: Store = serde_json::from_slice(original)?;
        state.validate()?;
        if state.view.runs.iter().any(|run| {
            run.output
                .projection()
                .conversation
                .iter()
                .any(|entry| matches!(entry, metldr_conversation::ProjectedEntry::Artifact { .. }))
        }) {
            return Err("legacy snapshot cannot contain artifact references".into());
        }
        let conversation = format!("{}:{}", state.view.store, state.next);
        let next = state
            .next
            .checked_add(1)
            .ok_or("identity capacity reached")?;
        let mut known = BTreeMap::<String, Source>::new();
        let sources = state
            .view
            .source
            .iter()
            .chain(state.workflows.values().map(|w| &w.source))
            .chain(state.codex_workflows.values().map(|w| &w.source))
            .chain(state.opencode_workflows.values().map(|w| &w.source));
        for source in sources {
            match known.get(&source.id) {
                Some(prior) if prior.page != source.page => {
                    return Err("legacy source copies conflict".into());
                }
                Some(_) => (),
                None => {
                    known.insert(source.id.clone(), source.clone());
                }
            }
        }
        let mut sources: BTreeMap<_, _> = known
            .into_iter()
            .map(|(id, source)| (id, Provenance::Known { source }))
            .collect();
        for run in &state.view.runs {
            sources
                .entry(run.source.clone())
                .or_insert_with(|| Provenance::LegacyUnavailable {
                    id: run.source.clone(),
                });
        }
        Ok(Self {
            state,
            conversation,
            next,
            sources,
        })
    }

    pub(super) fn write(self, db: &Database, original: &[u8]) -> Result<()> {
        let mut navigation_heads = BTreeMap::<String, (u64, String)>::new();
        for (id, action) in &self.state.browser_actions {
            if matches!(
                action.state,
                crate::browser_action::Phase::Settled {
                    outcome: crate::browser_action::Outcome::Navigated { .. }
                }
            ) {
                let head = navigation_heads
                    .entry(action.proposal.run.clone())
                    .or_insert((action.proposal.binding.grant_revision, id.clone()));
                if action.proposal.binding.grant_revision > head.0 {
                    *head = (action.proposal.binding.grant_revision, id.clone());
                }
            }
        }
        let mut native_targets = Vec::new();
        for run in &self.state.view.runs {
            let workflow = match self.state.launches.get(&run.id) {
                Some(
                    crate::agent_model::Launch::Pi { workflow, .. }
                    | crate::agent_model::Launch::Codex { workflow, .. }
                    | crate::agent_model::Launch::OpenCode { workflow, .. },
                ) => workflow,
                _ => continue,
            };
            let initial = self.state.native_browser_target(&run.id)?;
            let current = self.state.native_browser_target_after(&run.id)?;
            initial.validate()?;
            current.validate()?;
            let navigation = navigation_heads.get(&run.id).map(|(_, id)| id.clone());
            native_targets.push((
                run.id.clone(),
                NativeTarget {
                    workflow: workflow.clone(),
                    initial,
                    current,
                    navigation,
                },
            ));
        }
        let Store {
            schema: _,
            view,
            next: _,
            starts,
            launches,
            workflows,
            codex_workflows,
            opencode_workflows,
            browser_actions,
            providers,
            agent_settings,
        } = self.state;
        let View {
            sequence,
            store,
            source,
            runs,
        } = view;
        let profile = Profile {
            format: FORMAT,
            store: store.clone(),
            next: self.next,
            sequence,
            selected: self.conversation.clone(),
            epoch: 1,
            legacy_sha256: format!("{:x}", Sha256::digest(original)),
            providers,
            agents: agent_settings,
        };
        let conversation = Conversation {
            id: self.conversation.clone(),
            title: source
                .as_ref()
                .map(|s| s.page.title.clone())
                .unwrap_or_else(|| "New conversation".into()),
            source: source.map(|s| s.id),
        };
        let mut write = db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        write.set_two_phase_commit(true);
        {
            let mut meta = write.open_table(META)?;
            meta.insert("profile", encode(&profile)?.as_slice())?;
            meta.insert("legacy_snapshot", original)?;
            write.open_table(ARTIFACTS)?;
            write.open_table(CONVERSATIONS)?.insert(
                self.conversation.as_str(),
                encode(&conversation)?.as_slice(),
            )?;
            write.open_table(CONVERSATION_ORDER)?.insert(
                conversation_number(&profile, &self.conversation)?,
                self.conversation.as_str(),
            )?;
            write.open_table(SELECTIONS)?;
            let owned = |value| Record {
                conversation: self.conversation.clone(),
                revision: 1,
                value,
            };
            let prefix = format!("{store}:");
            let number = |id: &str| -> Result<u64> {
                Ok(id
                    .strip_prefix(&prefix)
                    .ok_or("invalid legacy identity prefix")?
                    .parse()?)
            };
            {
                let mut heads = write.open_table(NAVIGATION_HEADS)?;
                for (run, (_, action)) in navigation_heads {
                    heads.insert(run.as_str(), action.as_str())?;
                }
                let mut order = write.open_table(WORKFLOW_RUNS)?;
                let mut targets = write.open_table(NATIVE_TARGETS)?;
                for (run, target) in native_targets {
                    if order
                        .insert((target.workflow.as_str(), number(&run)?), run.as_str())?
                        .is_some()
                    {
                        return Err("native workflow order collision".into());
                    }
                    targets.insert(
                        run.as_str(),
                        encode(&Record {
                            conversation: self.conversation.clone(),
                            revision: 1,
                            value: target,
                        })?
                        .as_slice(),
                    )?;
                }
            }
            {
                let mut records = write.open_table(RUNS)?;
                let mut order = write.open_table(RUN_ORDER)?;
                let mut active = write.open_table(ACTIVE_RUNS)?;
                let mut context = write.open_table(CONTEXT)?;
                for run in runs {
                    let id = run.id.clone();
                    if run.status == crate::model::Status::Complete
                        && let Some(route) = launches
                            .get(&id)
                            .and_then(super::context::Route::from_launch)
                    {
                        let key = route.key(&self.conversation, &run.source)?;
                        if context
                            .insert((key.as_str(), number(&id)?), id.as_str())?
                            .is_some()
                        {
                            return Err("legacy completed context collision".into());
                        }
                    }
                    if !run.status.terminal() {
                        active.insert(id.as_str(), ())?;
                    }
                    if order
                        .insert((self.conversation.as_str(), number(&id)?), id.as_str())?
                        .is_some()
                    {
                        return Err("legacy run order collision".into());
                    }
                    records.insert(id.as_str(), encode(&owned(run))?.as_slice())?;
                }
            }
            macro_rules! copy_map {
                ($table:ident, $values:expr) => {{
                    let mut table = write.open_table($table)?;
                    for (id, value) in $values {
                        let record = Record {
                            conversation: self.conversation.clone(),
                            revision: 1,
                            value,
                        };
                        if table
                            .insert(id.as_str(), encode(&record)?.as_slice())?
                            .is_some()
                        {
                            return Err("legacy record identity collision".into());
                        }
                    }
                }};
            }
            copy_map!(SOURCES, self.sources);
            copy_map!(LAUNCHES, launches);
            copy_map!(PI, workflows);
            copy_map!(CODEX, codex_workflows);
            copy_map!(OPENCODE, opencode_workflows);
            {
                let mut order = write.open_table(ACTION_ORDER)?;
                let mut by_tool = write.open_table(ACTION_BY_TOOL)?;
                let mut open = write.open_table(OPEN_ACTIONS)?;
                let mut grants = write.open_table(NAVIGATION_GRANTS)?;
                let mut uncertain = write.open_table(UNCERTAIN_RUNS)?;
                let mut has_open_action = false;
                for action in browser_actions.values() {
                    if order
                        .insert(
                            (
                                action.proposal.run.as_str(),
                                number(&action.id)?,
                                action.id.as_str(),
                            ),
                            action.id.as_str(),
                        )?
                        .is_some()
                    {
                        return Err("legacy action order collision".into());
                    }
                    if by_tool
                        .insert(
                            (
                                action.proposal.run.as_str(),
                                action.proposal.tool_use_id.as_str(),
                            ),
                            action.id.as_str(),
                        )?
                        .is_some()
                    {
                        return Err("legacy action tool index collision".into());
                    }
                    if !matches!(action.state, crate::browser_action::Phase::Settled { .. }) {
                        if has_open_action {
                            return Err("legacy multiple open actions".into());
                        }
                        has_open_action = true;
                        if open
                            .insert(action.id.as_str(), action.proposal.run.as_str())?
                            .is_some()
                        {
                            return Err("legacy open action collision".into());
                        }
                    }
                    if let crate::browser_action::Phase::Settled {
                        outcome: crate::browser_action::Outcome::Navigated { binding, .. },
                    } = &action.state
                    {
                        if grants
                            .insert(
                                (action.proposal.run.as_str(), binding.grant_revision),
                                action.id.as_str(),
                            )?
                            .is_some()
                        {
                            return Err("legacy navigation grant index collision".into());
                        }
                    }
                    if matches!(
                        action.state,
                        crate::browser_action::Phase::Settled {
                            outcome: crate::browser_action::Outcome::Uncertain { .. }
                        }
                    ) && uncertain.get(action.proposal.run.as_str())?.is_none()
                    {
                        uncertain.insert(action.proposal.run.as_str(), action.id.as_str())?;
                    }
                }
            }
            copy_map!(ACTIONS, browser_actions);
            let mut receipts = write.open_table(STARTS)?;
            for (request, run) in starts {
                let receipt = Start {
                    conversation: self.conversation.clone(),
                    run,
                    format: ReceiptFormat::LegacyUnscoped,
                };
                receipts.insert(request.as_str(), encode(&receipt)?.as_slice())?;
            }
            super::edit::native_choices::backfill(&write, &profile)?;
        }
        write.commit()?;
        Ok(())
    }
}
