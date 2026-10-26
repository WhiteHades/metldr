use crate::{
    Result,
    flow::Owner,
    history::{ConversationBusy, ConversationChange, Cursor, Selection},
};
pub(crate) use metldr_history_pages::Scope;
use metldr_history_pages::{Counter, DocumentKind, Identity};
use serde_json::{Value, json};
use std::time::Instant;

mod pins;
pub(crate) use pins::Pins;

#[derive(Debug)]
pub(crate) enum ReadError {
    Busy,
    Stale,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "history is busy",
            Self::Stale => "history is stale",
        })
    }
}

impl std::error::Error for ReadError {}

impl From<&Selection> for Scope {
    fn from(value: &Selection) -> Self {
        Self {
            store: value.store.clone(),
            conversation: value.conversation.clone(),
            epoch: Counter(value.epoch),
        }
    }
}

impl Owner {
    pub(super) fn expire_history(&mut self) {
        self.history_pins.expire(Instant::now());
    }

    pub(super) fn invalidate_history(&mut self) {
        self.history_pins.clear();
    }

    fn history_context(&mut self) -> Result<(Selection, String)> {
        let (selection, _) = self.persist_records(|_, scope| Ok((scope.clone(), None)))?;
        let generation = self.history_pins.context(&selection, Instant::now())?;
        Ok((selection, generation))
    }

    fn check_history(&mut self, scope: &Scope, generation: &str) -> Result<Selection> {
        let (current, _) = self.history_context()?;
        if current.store != scope.store
            || current.conversation != scope.conversation
            || current.epoch != scope.epoch.0
        {
            return Err(ReadError::Stale.into());
        }
        self.history_pins
            .accept(&current, generation, Instant::now())?;
        Ok(current)
    }

    pub(super) fn history_current(&mut self, request: &str) -> Result<()> {
        let (scope, generation) = self.history_context()?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_current","scope":Scope::from(&scope),"generation":generation}))
    }

    pub(super) fn conversation_change(
        &mut self,
        request: &str,
        scope: Scope,
        change: ConversationChange,
    ) -> Result<()> {
        scope.validate()?;
        let idle = self.conversation_idle();
        let expected = Selection {
            store: scope.store,
            conversation: scope.conversation,
            epoch: scope.epoch.0,
        };
        let (receipt, sequence) = self.record_change(|edit, _| {
            let receipt = edit.change_conversation(&expected, request, change)?;
            if edit.changed() && !idle {
                return Err(ConversationBusy.into());
            }
            let projection = if edit.changed() {
                Some(edit.project_latest(&receipt.current)?)
            } else {
                None
            };
            Ok(((receipt, edit.sequence()?), projection))
        })?;
        let (current, generation) = self.history_context()?;
        if current != receipt.current {
            return Err(std::io::Error::other("selection changed outside its owner").into());
        }
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"conversation_changed","original":Scope::from(&receipt.original),"current":Scope::from(&current),"generation":generation,"sequence":sequence}))
    }

    pub(super) fn conversation_idle(&self) -> bool {
        self.operation.is_none()
            && self.settings.job.is_none()
            && self.native_permissions.is_empty()
            && self.pi_ui.is_none()
            && self.codex_input.is_none()
            && self.browser_approval.is_none()
            && self.browser_io.is_none()
            && self.subscribers.is_empty()
    }

    pub(super) fn history_conversations(
        &mut self,
        request: &str,
        scope: Scope,
        generation: &str,
        cursor: Option<Cursor>,
    ) -> Result<()> {
        let scope = self.check_history(&scope, generation)?;
        let (page, _) = self.persist_records(|edit, _| {
            Ok((edit.conversations_page(&scope, cursor.as_ref())?, None))
        })?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_conversations","generation":generation,"page":page}))
    }

    pub(super) fn history_runs(
        &mut self,
        request: &str,
        scope: Scope,
        generation: &str,
        cursor: Option<Cursor>,
    ) -> Result<()> {
        let scope = self.check_history(&scope, generation)?;
        let (page, _) =
            self.persist_records(|edit, _| Ok((edit.runs_page(&scope, cursor.as_ref())?, None)))?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_runs","generation":generation,"page":page}))
    }

    pub(super) fn history_pin(
        &mut self,
        request: &str,
        scope: Scope,
        generation: &str,
        kind: DocumentKind,
        document: &str,
        revision: Counter,
    ) -> Result<()> {
        let scope = self.check_history(&scope, generation)?;
        let now = Instant::now();
        if let Some(identity) = self
            .history_pins
            .find(request, kind, document, revision, now)
        {
            return self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_pinned","identity":identity}));
        }
        let ((observed, sequence), _) = self.persist_records(|edit, _| {
            let value = match kind {
                DocumentKind::Run => edit.run_detail(&scope, document)?,
                DocumentKind::Source => {
                    let observed = edit.source_detail(&scope, document)?;
                    crate::history::Observed {
                        revision: observed.revision,
                        value: serde_json::to_value(observed.value)?,
                    }
                }
                DocumentKind::Action => {
                    let observed = edit.action_detail(&scope, document)?;
                    crate::history::Observed {
                        revision: observed.revision,
                        value: serde_json::to_value(observed.value)?,
                    }
                }
                DocumentKind::Artifact => {
                    let observed = edit.artifact_detail(&scope, document)?;
                    crate::history::Observed {
                        revision: observed.revision,
                        value: serde_json::to_value(observed.value)?,
                    }
                }
            };
            Ok(((value, edit.sequence()?), None))
        })?;
        if observed.revision != revision {
            return self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_stale","observation":{"scope":Scope::from(&scope),"generation":generation,"sequence":sequence,"kind":kind,"document":document,"revision":observed.revision,"status":if kind == DocumentKind::Run { observed.value.get("status").cloned() } else {None}}}));
        }
        if kind == DocumentKind::Source && observed.value.is_null() {
            return self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_unavailable","scope":Scope::from(&scope),"generation":generation,"document":document,"revision":revision}));
        }
        let identity = self.history_pins.insert(
            request,
            kind,
            document,
            revision,
            &observed.value,
            Instant::now(),
        )?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_pinned","identity":identity}))
    }

    pub(super) fn history_page(
        &mut self,
        request: &str,
        identity: Identity,
        offset: u32,
    ) -> Result<()> {
        self.check_history(
            &Scope {
                store: identity.store.clone(),
                conversation: identity.conversation.clone(),
                epoch: identity.epoch,
            },
            &identity.generation,
        )?;
        let bytes = self
            .history_pins
            .page(&identity, request, offset, Instant::now())?;
        self.emit(serde_json::from_slice::<Value>(&bytes)?)
    }

    pub(super) fn history_release(&mut self, request: &str, identity: Identity) -> Result<()> {
        self.check_history(
            &Scope {
                store: identity.store.clone(),
                conversation: identity.conversation.clone(),
                epoch: identity.epoch,
            },
            &identity.generation,
        )?;
        let released = self.history_pins.release(&identity, Instant::now())?;
        self.emit(json!({"v":crate::WIRE_VERSION,"request":request,"kind":"history_released","identity":identity,"released":released}))
    }
}
