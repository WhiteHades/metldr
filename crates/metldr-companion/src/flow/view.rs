use super::{Operation, Owner, PendingPermission};
use crate::{
    Result,
    history::{History, NativeChoices, Selection},
    model::{Status, Store},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::atomic::Ordering};

pub(crate) struct View<'a> {
    pub(crate) state: &'a Store,
    pub(crate) browser_actions: &'a BTreeMap<String, crate::browser_action::Action>,
    pi_profile: Option<&'a crate::agent_model::Profile>,
    codex_profile: Option<&'a crate::codex_model::Profile>,
    opencode_profile: Option<&'a crate::opencode_model::Profile>,
    permissions: &'a BTreeMap<String, PendingPermission>,
    pi_ui: Option<&'a crate::pi_ui::State>,
    codex_input: Option<&'a crate::codex_input::State>,
    browser_route: Option<&'a crate::https_profile::Profile>,
    responses_route: Option<&'a crate::https_profile::Profile>,
    google_route: Option<&'a crate::https_profile::Profile>,
    chat_route: Option<&'a crate::https_profile::Profile>,
    fixture: bool,
    settings: &'a crate::provider_settings::Runtime,
    agent_runtime: &'a crate::agent_settings::Runtime,
    operations: &'a BTreeMap<String, Operation>,
}

impl Owner {
    pub(crate) fn view_parts(&mut self) -> (&mut History, View<'_>) {
        (
            &mut self.record_history,
            View {
                state: &self.state,
                browser_actions: &self.browser_view,
                pi_profile: self.pi_profile.as_ref(),
                codex_profile: self.codex_profile.as_ref(),
                opencode_profile: self.opencode_profile.as_ref(),
                permissions: &self.native_permissions,
                pi_ui: self.pi_ui.as_ref(),
                codex_input: self.codex_input.as_ref(),
                browser_route: self.browser_route.as_ref(),
                responses_route: self.responses_route.as_ref(),
                google_route: self.google_route.as_ref(),
                chat_route: self.chat_route.as_ref(),
                fixture: self.fixture.is_some(),
                settings: &self.settings,
                agent_runtime: &self.agent_runtime,
                operations: &self.operations,
            },
        )
    }
}

impl View<'_> {
    pub(crate) fn native_fingerprints(
        &self,
    ) -> Result<BTreeMap<crate::agent_settings::Kind, String>> {
        use crate::agent_settings::Kind;
        Ok([
            (
                Kind::Pi,
                self.pi_profile
                    .map(crate::agent_model::digest)
                    .transpose()?,
            ),
            (
                Kind::Codex,
                self.codex_profile
                    .map(crate::agent_model::digest)
                    .transpose()?,
            ),
            (
                Kind::Opencode,
                self.opencode_profile
                    .map(crate::agent_model::digest)
                    .transpose()?,
            ),
        ]
        .into_iter()
        .filter_map(|(kind, fingerprint)| fingerprint.map(|value| (kind, value)))
        .collect())
    }

    pub(crate) fn reply(
        &self,
        state: &Store,
        request: Option<&str>,
        kind: &str,
        ended: bool,
        browser_actions: &BTreeMap<String, crate::browser_action::Action>,
        selection: &Selection,
        choices: &NativeChoices,
    ) -> Result<Value> {
        let mut view = crate::public_view::render(
            state,
            self.pi_profile,
            self.codex_profile,
            self.opencode_profile,
            self.permissions,
            self.pi_ui,
            self.codex_input,
            self.browser_route,
            self.responses_route,
            self.google_route,
            self.chat_route,
            self.fixture,
            Some(choices),
        )?;
        if browser_actions.is_empty() {
            view.as_object_mut()
                .ok_or("invalid public view")?
                .remove("browser_actions");
        } else {
            view["browser_actions"] = crate::public_view::browser_actions(browser_actions);
        }
        view["selection"] = serde_json::to_value(crate::history_io::Scope::from(selection))?;
        view["native_correction"] = self.correction(state);
        view["provider_settings"] = self.settings.public(&state.providers);
        view["agent_settings"] = self.agent_runtime.public(
            &state.agent_settings,
            !self.operations.is_empty() || self.settings.job.is_some(),
        );
        let mut reply = json!({"v":crate::WIRE_VERSION,"request":request,"kind":kind,"view":view,"ended":ended});
        crate::tool_report::fit_wire(&mut reply)?;
        Ok(reply)
    }

    pub(crate) fn preflight(
        &self,
        state: &Store,
        actions: &BTreeMap<String, crate::browser_action::Action>,
        selection: &Selection,
        choices: &NativeChoices,
    ) -> Result<()> {
        self.reply(
            state,
            Some(&"x".repeat(96)),
            "view",
            false,
            actions,
            selection,
            choices,
        )?;
        Ok(())
    }

    fn correction(&self, state: &Store) -> Value {
        let Some((op, run, slot)) = state.view.runs.iter().find_map(|run| {
            if run.status.terminal() {
                return None;
            }
            let op = self.operations.get(&run.id)?;
            Some((op, run, op.correction.as_ref()?))
        }) else {
            return Value::Null;
        };
        json!({"run":run.id,"available":slot.ready.is_some()
            && !op.stop.load(Ordering::Acquire) && run.status == Status::Running
            && run.corrections.len() < crate::native_correction::LIMIT
            && run.corrections.last().is_none_or(|r| !r.state.unresolved())})
    }
}
