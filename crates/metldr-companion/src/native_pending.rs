use crate::{
    codex_input,
    flow::{Owner, PendingPermission},
    pi_ui,
};

pub(super) struct PendingInputs {
    pi_ui: Option<pi_ui::State>,
    codex_input: Option<codex_input::State>,
    permissions: Vec<(String, PendingPermission)>,
}

impl Owner {
    pub(super) fn detach_inputs(&mut self, run: &str) -> PendingInputs {
        let pi_ui = self
            .pi_ui
            .as_ref()
            .filter(|state| state.run == run)
            .cloned();
        if pi_ui.is_some() {
            self.clear_pi_ui(run);
        }
        let codex_input = if self
            .codex_input
            .as_ref()
            .is_some_and(|state| state.run == run)
        {
            self.codex_input.take()
        } else {
            None
        };
        let permissions = self.take_native_permissions(run);

        PendingInputs {
            pi_ui,
            codex_input,
            permissions,
        }
    }

    pub(super) fn restore_inputs(&mut self, pending: PendingInputs) {
        if let Some(pi_ui) = pending.pi_ui {
            self.pi_ui = Some(pi_ui);
        }
        if let Some(codex_input) = pending.codex_input {
            self.codex_input = Some(codex_input);
        }
        self.native_permissions.extend(pending.permissions);
    }

    pub(super) fn cancel_inputs(&mut self, run: &str, pending: PendingInputs) {
        self.restore_inputs(pending);
        self.clear_pi_ui(run);
        self.clear_codex_input(run);
        self.cancel_native_permissions(run);
    }
}
