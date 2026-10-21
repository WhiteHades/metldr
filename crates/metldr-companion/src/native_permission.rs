use crate::{
    Result,
    flow::{Owner, PendingPermission},
    model::Status,
};
use serde_json::{Value, json};
use std::sync::mpsc;

impl Owner {
    pub(super) fn native_permission(
        &mut self,
        run: &str,
        key: u64,
        details: Value,
        ack: mpsc::SyncSender<Option<String>>,
    ) -> Result<()> {
        let active = self.operation.as_ref().is_some_and(|op| op.run == run)
            && self
                .state
                .view
                .runs
                .iter()
                .any(|record| record.id == run && record.status == Status::Running)
            && self
                .state
                .launches
                .get(run)
                .is_some_and(|launch| match launch {
                    crate::agent_model::Launch::Codex { .. } => details["agent"] == "codex",
                    crate::agent_model::Launch::OpenCode { .. } => details["agent"] == "opencode",
                    _ => false,
                });
        let options = details["options"].as_array();
        let canonical_shape = details.as_object().is_some_and(|object| {
            object.len() == 5
                && ["agent", "tool", "title", "input", "options"]
                    .iter()
                    .all(|key| object.contains_key(*key))
        });
        let valid_options = options.is_some_and(|options| {
            !options.is_empty() && options.len() <= 16 && {
                let mut seen = std::collections::BTreeSet::new();
                options.iter().all(|option| {
                    let Some(fields) = option.as_object() else {
                        return false;
                    };
                    let id = option["id"].as_str().unwrap_or_default();
                    fields.len() == 2
                        && fields.contains_key("id")
                        && fields.contains_key("name")
                        && !id.is_empty()
                        && id.len() <= 128
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
                        && seen.insert(id)
                        && option["name"].as_str().is_some_and(|name| {
                            !name.is_empty()
                                && name.len() <= 256
                                && !name.chars().any(char::is_control)
                        })
                })
            }
        });
        let bytes = serde_json::to_vec(&details)?;
        if !active
            || !canonical_shape
            || bytes.len() > 8192
            || details["input"].as_str().is_none_or(|s| s.len() > 8192)
            || details["tool"]
                .as_str()
                .is_none_or(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
            || details["title"]
                .as_str()
                .is_none_or(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
            || !valid_options
            || self.native_permissions.len() >= 8
            || self
                .native_permissions
                .values()
                .any(|pending| pending.run == run && pending.key == key)
        {
            return self.reject_native_permission(
                run,
                "native permission details are invalid or exceed bounds",
                ack,
            );
        }

        let mut next = self.state.clone();
        let request = match next.id() {
            Ok(request) => request,
            Err(error) => return self.reject_native_permission(run, &error.to_string(), ack),
        };
        if self.native_permissions.contains_key(&request) {
            return self.reject_native_permission(run, "native permission identity repeated", ack);
        }
        self.native_permissions.insert(
            request.clone(),
            PendingPermission {
                run: run.into(),
                key,
                details,
                ack,
            },
        );
        let view = match crate::public_view::render(
            &next,
            self.pi_profile.as_ref(),
            self.codex_profile.as_ref(),
            self.opencode_profile.as_ref(),
            &self.native_permissions,
            self.browser_route.as_ref(),
            self.responses_route.as_ref(),
            self.google_route.as_ref(),
            self.chat_route.as_ref(),
            self.fixture.is_some(),
        ) {
            Ok(view) => view,
            Err(error) => {
                let ack = self
                    .native_permissions
                    .remove(&request)
                    .ok_or("new native permission disappeared")?
                    .ack;
                return self.reject_native_permission(run, &error.to_string(), ack);
            }
        };
        let projected = json!({"v":crate::WIRE_VERSION,"request":null,"kind":"changed","view":view,"ended":false});
        if serde_json::to_vec(&projected["view"]["native_permissions"])?.len() > 8192
            || serde_json::to_vec(&projected)?.len() > crate::FRAME_MAX.saturating_sub(256)
        {
            let ack = self
                .native_permissions
                .remove(&request)
                .ok_or("new native permission disappeared")?
                .ack;
            return self.reject_native_permission(
                run,
                "native permission view exceeds wire bounds",
                ack,
            );
        }
        if let Err(error) = self.commit(next) {
            let ack = self
                .native_permissions
                .remove(&request)
                .ok_or("new native permission disappeared")?
                .ack;
            if error.downcast_ref::<std::io::Error>().is_some() {
                let _ = ack.send(None);
                return Err(error);
            }
            return self.reject_native_permission(run, &error.to_string(), ack);
        }
        Ok(())
    }

    pub(super) fn native_permission_resolved(&mut self, run: &str, key: u64) -> Result<()> {
        let Some(request) = self
            .native_permissions
            .iter()
            .find_map(|(request, pending)| {
                (pending.run == run && pending.key == key).then(|| request.clone())
            })
        else {
            return Ok(());
        };
        let pending = self
            .native_permissions
            .remove(&request)
            .ok_or("resolved native permission disappeared")?;
        if let Err(error) = self.commit(self.state.clone()) {
            self.native_permissions.insert(request, pending);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn native_decide(
        &mut self,
        id: &str,
        run: &str,
        request: &str,
        option: Option<&str>,
    ) -> Result<()> {
        let index = request.to_owned();
        let pending = self
            .native_permissions
            .get(&index)
            .ok_or("stale native permission request")?;
        if pending.run != run {
            return Err("native permission run mismatch".into());
        }
        if let Some(option) = option {
            if !pending.details["options"]
                .as_array()
                .is_some_and(|choices| choices.iter().any(|o| o["id"] == option))
            {
                return Err("native permission choice was not offered".into());
            }
        }
        let pending = self
            .native_permissions
            .remove(&index)
            .ok_or("stale native permission request")?;
        if let Err(error) = self.commit(self.state.clone()) {
            self.native_permissions.insert(index, pending);
            return Err(error);
        }
        let _ = pending.ack.send(option.map(str::to_owned));
        self.reply_view(Some(id), "view", true)
    }

    pub(super) fn take_native_permissions(
        &mut self,
        run: &str,
    ) -> Vec<(String, PendingPermission)> {
        let keys: Vec<_> = self
            .native_permissions
            .iter()
            .filter(|(_, pending)| pending.run == run)
            .map(|(request, _)| request)
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|key| self.native_permissions.remove(&key).map(|p| (key, p)))
            .collect()
    }

    pub(super) fn cancel_native_permissions(&mut self, run: &str) {
        for (_, pending) in self.take_native_permissions(run) {
            let _ = pending.ack.send(None);
        }
    }

    fn reject_native_permission(
        &mut self,
        run: &str,
        reason: &str,
        ack: mpsc::SyncSender<Option<String>>,
    ) -> Result<()> {
        if self.operation.as_ref().is_none_or(|op| op.run != run)
            || self
                .state
                .view
                .runs
                .iter()
                .find(|r| r.id == run)
                .is_none_or(|r| r.status != Status::Running)
        {
            let _ = ack.send(None);
            return Ok(());
        }
        let mut next = self.state.clone();
        let record = next
            .view
            .runs
            .iter_mut()
            .find(|r| r.id == run)
            .ok_or("missing native run")?;
        record.status = Status::StopRequested;
        record.error = Some(reason.chars().take(200).collect());
        if let Err(error) = next.prevent_browser(run, "native permission request failed") {
            if error.downcast_ref::<std::io::Error>().is_some() {
                let _ = ack.send(None);
                return Err(error);
            }
        }
        let pending = self.take_native_permissions(run);
        if let Err(error) = self.commit(next) {
            self.native_permissions.extend(pending);
            if let Some(op) = &self.operation {
                op.stop.store(true, std::sync::atomic::Ordering::Release);
            }
            let _ = ack.send(None);
            return Err(error);
        }
        if let Some(op) = &self.operation {
            op.stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.cancel_browser_approval(run);
        let _ = ack.send(None);
        for (_, pending) in pending {
            let _ = pending.ack.send(None);
        }
        Ok(())
    }
}
