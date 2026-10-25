use super::ReadError;
use crate::{Result, history::Selection};
use metldr_history_pages::{Counter, DocumentKind, Identity, Snapshot};
use serde::Serialize;
use std::time::{Duration, Instant};

const MAX_PINS: usize = 4;
const IDLE_LIMIT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Pins {
    scope: Option<Selection>,
    generation: Option<String>,
    next_ticket: u64,
    pins: Vec<Pin>,
}

struct Pin {
    request: String,
    identity: Identity,
    snapshot: Snapshot,
    last_used: Instant,
}

impl Pins {
    pub(super) fn context(&mut self, scope: &Selection, now: Instant) -> Result<String> {
        self.expire(now);
        if self.scope.as_ref() != Some(scope) || self.generation.is_none() {
            let mut random = [0; 16];
            getrandom::fill(&mut random)?;
            self.pins.clear();
            self.next_ticket = 0;
            self.scope = Some(scope.clone());
            let mut generation = String::with_capacity(32);
            for byte in random {
                use std::fmt::Write;
                write!(generation, "{byte:02x}").expect("writing to String cannot fail");
            }
            self.generation = Some(generation);
        }
        Ok(self
            .generation
            .as_ref()
            .expect("generation initialized")
            .clone())
    }

    pub(super) fn accept(
        &mut self,
        scope: &Selection,
        generation: &str,
        now: Instant,
    ) -> Result<()> {
        self.expire(now);
        if self.scope.as_ref() == Some(scope) && self.generation.as_deref() == Some(generation) {
            Ok(())
        } else {
            Err(ReadError::Stale.into())
        }
    }

    pub(super) fn clear(&mut self) {
        self.pins.clear();
        self.scope = None;
        self.generation = None;
        self.next_ticket = 0;
    }

    pub(super) fn expire(&mut self, now: Instant) {
        self.pins
            .retain(|pin| now.saturating_duration_since(pin.last_used) < IDLE_LIMIT);
    }

    pub(super) fn find(
        &mut self,
        request: &str,
        kind: DocumentKind,
        document: &str,
        revision: Counter,
        now: Instant,
    ) -> Option<Identity> {
        self.expire(now);
        let pin = self.pins.iter_mut().find(|pin| {
            pin.request == request
                && pin.identity.kind == kind
                && pin.identity.document == document
                && pin.identity.revision == revision
        })?;
        pin.last_used = now;
        Some(pin.identity.clone())
    }

    pub(super) fn insert(
        &mut self,
        request: &str,
        kind: DocumentKind,
        document: &str,
        revision: Counter,
        value: &impl Serialize,
        now: Instant,
    ) -> Result<Identity> {
        self.expire(now);
        if self.pins.len() >= MAX_PINS {
            return Err(ReadError::Busy.into());
        }
        let scope = self
            .scope
            .as_ref()
            .ok_or_else(|| error("pin context unavailable"))?;
        let generation = self
            .generation
            .as_ref()
            .ok_or_else(|| error("pin context unavailable"))?;
        let next = self
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| error("pin ticket capacity reached"))?;
        let identity = Identity {
            store: scope.store.clone(),
            conversation: scope.conversation.clone(),
            epoch: Counter(scope.epoch),
            kind,
            document: document.to_owned(),
            revision,
            generation: generation.clone(),
            ticket: format!("{generation}:{next}"),
        };
        let snapshot = Snapshot::new(identity.clone(), value)?;
        self.next_ticket = next;
        self.pins.push(Pin {
            request: request.to_owned(),
            identity: identity.clone(),
            snapshot,
            last_used: now,
        });
        Ok(identity)
    }

    pub(super) fn page(
        &mut self,
        identity: &Identity,
        request: &str,
        offset: u32,
        now: Instant,
    ) -> Result<Vec<u8>> {
        self.expire(now);
        let pin = self
            .pins
            .iter_mut()
            .find(|pin| pin.identity.ticket == identity.ticket)
            .ok_or(ReadError::Stale)?;
        if pin.identity != *identity {
            return Err(error("pin identity mismatch"));
        }
        let page = pin.snapshot.page(request, offset)?;
        pin.last_used = now;
        Ok(page)
    }

    pub(super) fn release(&mut self, identity: &Identity, now: Instant) -> Result<bool> {
        self.expire(now);
        if self.scope.as_ref().is_none_or(|scope| {
            scope.store != identity.store
                || scope.conversation != identity.conversation
                || scope.epoch != identity.epoch.0
        }) || self.generation.as_deref() != Some(identity.generation.as_str())
        {
            return Err(ReadError::Stale.into());
        }
        let Some(index) = self
            .pins
            .iter()
            .position(|pin| pin.identity.ticket == identity.ticket)
        else {
            return Ok(false);
        };
        if self.pins[index].identity != *identity {
            return Err(error("pin identity mismatch"));
        }
        self.pins.remove(index);
        Ok(true)
    }
}

fn error(message: &'static str) -> Box<dyn std::error::Error + Send + Sync> {
    message.into()
}

#[cfg(test)]
mod checks {
    use super::*;
    use serde_json::json;

    #[test]
    fn idle_pins_keep_one_revision_and_reject_stale_contexts() -> Result<()> {
        let scope = Selection {
            store: "0123456789abcdef0123456789abcdef".into(),
            conversation: "0123456789abcdef0123456789abcdef:60".into(),
            epoch: 1,
        };
        let now = Instant::now();
        let mut pins = Pins::default();
        let generation = pins.context(&scope, now)?;
        pins.accept(&scope, &generation, now)?;
        let identity = pins.insert(
            "first",
            DocumentKind::Run,
            &format!("{}:1", scope.store),
            Counter(1),
            &json!({"output":"old"}),
            now,
        )?;
        assert!(pins.page(&identity, "page", 1, now).is_ok());
        assert!(
            pins.page(&identity, "page", u32::MAX, now + Duration::from_secs(29))
                .is_err()
        );
        assert!(
            pins.page(&identity, "page", 0, now + IDLE_LIMIT).is_err(),
            "invalid requests must not extend idle lifetime"
        );
        let identity = pins.insert(
            "new",
            DocumentKind::Run,
            &format!("{}:1", scope.store),
            Counter(2),
            &json!({"output":"new"}),
            now + IDLE_LIMIT,
        )?;
        let mut changed = scope.clone();
        changed.epoch += 1;
        let next = pins.context(&changed, now + IDLE_LIMIT)?;
        assert_ne!(next, generation);
        assert!(pins.page(&identity, "page", 0, now + IDLE_LIMIT).is_err());
        assert!(pins.accept(&scope, &generation, now + IDLE_LIMIT).is_err());
        pins.clear();
        assert!(pins.accept(&changed, &next, now + IDLE_LIMIT).is_err());
        Ok(())
    }
}
