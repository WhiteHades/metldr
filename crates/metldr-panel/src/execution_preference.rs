const STORAGE_KEY: &str = "metldr.composer.destination";
pub(crate) const MAX_EXECUTION_CHOICE_BYTES: usize = 1024;

#[derive(Default)]
pub(crate) struct ExecutionPreference {
    current: Option<String>,
    persisted: Option<String>,
}

impl ExecutionPreference {
    pub(crate) fn load() -> Self {
        let value = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(STORAGE_KEY).ok().flatten())
            .filter(|value| value.len() <= MAX_EXECUTION_CHOICE_BYTES);
        Self {
            current: value.clone(),
            persisted: value,
        }
    }

    pub(crate) fn value(&self) -> Option<String> {
        self.current.clone()
    }

    pub(crate) fn remember(&mut self, value: &str) {
        self.remember_with(value, |value| {
            let storage = web_sys::window()
                .and_then(|window| window.local_storage().ok().flatten())
                .ok_or(())?;
            storage.set_item(STORAGE_KEY, value).map_err(|_| ())
        });
    }

    fn remember_with(&mut self, value: &str, write: impl FnOnce(&str) -> Result<(), ()>) {
        if self.current.as_deref() == Some(value) && self.persisted.as_deref() == Some(value) {
            return;
        }
        self.current = Some(value.to_owned());
        if self.persisted.as_deref() != Some(value) && write(value).is_ok() {
            self.persisted = Some(value.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionPreference;

    #[test]
    fn memory_survives_storage_failure_and_failed_write_can_retry() {
        let mut preference = ExecutionPreference::default();
        let mut writes = 0;
        preference.remember_with("codex:workflow", |_| {
            writes += 1;
            Err(())
        });
        assert_eq!(preference.value().as_deref(), Some("codex:workflow"));
        preference.remember_with("codex:workflow", |_| {
            writes += 1;
            Ok(())
        });
        preference.remember_with("codex:workflow", |_| {
            writes += 1;
            Ok(())
        });
        assert_eq!(preference.value().as_deref(), Some("codex:workflow"));
        assert_eq!(writes, 2);
    }
}
