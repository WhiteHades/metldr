use crate::{
    codex_input,
    flow::{Owner, PendingPermission},
    pi_ui,
};

pub(super) struct PendingInputs {
    run: String,
    pi_ui: Option<pi_ui::Active>,
    codex_input: Option<codex_input::State>,
    permissions: Vec<(String, PendingPermission)>,
}

impl PendingInputs {
    pub(super) fn cancel(self) {
        drop(self.pi_ui);
        drop(self.codex_input);
        for (_, pending) in self.permissions {
            let _ = pending.ack.send(None);
        }
    }
}

impl Owner {
    pub(super) fn detach_inputs(&mut self, run: &str) -> PendingInputs {
        let pi_ui = self.pi_ui.get_mut(run).map(pi_ui::State::detach);
        let codex_input = self.codex_input.remove(run);
        let permissions = self.take_native_permissions(run);
        PendingInputs {
            run: run.into(),
            pi_ui,
            codex_input,
            permissions,
        }
    }

    pub(super) fn restore_inputs(&mut self, pending: PendingInputs) {
        if let Some(pi_ui) = pending.pi_ui {
            self.pi_ui
                .entry(pending.run.clone())
                .or_default()
                .restore(pi_ui);
        }
        if let Some(codex_input) = pending.codex_input {
            self.codex_input.insert(pending.run, codex_input);
        }
        self.native_permissions.extend(pending.permissions);
    }
}
