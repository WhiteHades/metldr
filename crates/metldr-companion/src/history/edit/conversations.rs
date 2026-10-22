use super::{Cursor, Edit, Selection, selection, storage};
use crate::{Result, history::schema::*, model::valid_request_id, tool_report::Preview};
use metldr_history_pages::Counter;
use redb::{ReadableTable, ReadableTableMetadata};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ConversationChange {
    Create,
    Select { conversation: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    expected: Selection,
    change: ConversationChange,
    original: Selection,
}

#[derive(Debug)]
pub(crate) struct ConversationChanged {
    pub original: Selection,
    pub current: Selection,
}

#[derive(Debug)]
pub(crate) struct ConversationBusy;
impl std::fmt::Display for ConversationBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("conversation has unfinished work")
    }
}
impl std::error::Error for ConversationBusy {}

#[derive(Serialize)]
pub(crate) struct ConversationSummary {
    pub id: String,
    pub title: Preview,
}

#[derive(Serialize)]
pub(crate) struct ConversationPage {
    pub store: String,
    pub conversation: String,
    pub epoch: Counter,
    pub sequence: Counter,
    pub anchor: Counter,
    pub conversations: Vec<ConversationSummary>,
    pub next: Option<Cursor>,
}

impl Edit<'_> {
    pub(crate) fn change_conversation(
        &mut self,
        expected: &Selection,
        request: &str,
        change: ConversationChange,
    ) -> Result<ConversationChanged> {
        let result = (|| {
            if !valid_request_id(request) {
                return Err("invalid request identity".into());
            }
            let receipt = storage(|| {
                self.write
                    .open_table(SELECTIONS)?
                    .get(request)?
                    .map(|bytes| decode::<Receipt>(bytes.value()))
                    .transpose()
            })?;
            if let Some(receipt) = receipt {
                storage(|| {
                    if receipt.expected.store != self.profile.store
                        || receipt.original.store != self.profile.store
                        || receipt.expected.epoch == 0
                        || receipt.expected.epoch.checked_add(1) != Some(receipt.original.epoch)
                        || receipt.original.epoch > self.profile.epoch
                        || self.write.open_table(STARTS)?.get(request)?.is_some()
                    {
                        return Err("saved selection receipt differs".into());
                    }
                    self.saved_conversation(&receipt.expected.conversation)?;
                    self.saved_conversation(&receipt.original.conversation)?;
                    match &receipt.change {
                        ConversationChange::Create
                            if receipt.original.conversation != receipt.expected.conversation =>
                        {
                            ()
                        }
                        ConversationChange::Select { conversation }
                            if *conversation == receipt.original.conversation =>
                        {
                            ()
                        }
                        _ => return Err("saved selection outcome differs".into()),
                    }
                    Ok(())
                })?;
                if receipt.expected != *expected || receipt.change != change {
                    return Err("request identity already names another conversation change".into());
                }
                return Ok(ConversationChanged {
                    original: receipt.original,
                    current: selection(&self.profile),
                });
            }
            if storage(|| Ok(self.write.open_table(STARTS)?.get(request)?.is_some()))? {
                return Err("request identity already names a run".into());
            }
            self.require_selection(expected)?;
            let idle = storage(|| {
                Ok(self.write.open_table(ACTIVE_RUNS)?.len()? == 0
                    && self.write.open_table(OPEN_ACTIONS)?.len()? == 0)
            })?;
            if !idle {
                return Err(ConversationBusy.into());
            }
            let epoch = self
                .profile
                .epoch
                .checked_add(1)
                .ok_or("selection epoch capacity reached")?;
            let conversation = match &change {
                ConversationChange::Create => {
                    let id = self.id()?;
                    let row = Conversation {
                        id: id.clone(),
                        title: "New conversation".into(),
                        source: None,
                    };
                    storage(|| {
                        let number = conversation_number(&self.profile, &id)?;
                        if self
                            .write
                            .open_table(CONVERSATIONS)?
                            .insert(id.as_str(), encode(&row)?.as_slice())?
                            .is_some()
                            || self
                                .write
                                .open_table(CONVERSATION_ORDER)?
                                .insert(number, id.as_str())?
                                .is_some()
                        {
                            return Err("conversation identity already exists".into());
                        }
                        Ok(())
                    })?;
                    id
                }
                ConversationChange::Select { conversation } => {
                    conversation_number(&self.profile, conversation)?;
                    if !storage(|| {
                        Ok(self
                            .write
                            .open_table(CONVERSATIONS)?
                            .get(conversation.as_str())?
                            .is_some())
                    })? {
                        return Err("unknown conversation".into());
                    }
                    self.saved_conversation(conversation)?.id
                }
            };
            self.profile.selected = conversation;
            self.profile.epoch = epoch;
            self.changed = true;
            let original = selection(&self.profile);
            let receipt = Receipt {
                expected: expected.clone(),
                change,
                original: original.clone(),
            };
            storage(|| {
                if self
                    .write
                    .open_table(SELECTIONS)?
                    .insert(request, encode(&receipt)?.as_slice())?
                    .is_some()
                {
                    return Err("selection receipt already exists".into());
                }
                Ok(())
            })?;
            Ok(ConversationChanged {
                original: original.clone(),
                current: original,
            })
        })();
        self.checked(result)
    }

    pub(crate) fn conversations_page(
        &self,
        scope: &Selection,
        cursor: Option<&Cursor>,
    ) -> Result<ConversationPage> {
        self.checked((|| {
            self.require_selection(scope)?;
            let anchor = cursor.map_or(self.profile.next - 1, |cursor| cursor.anchor.0);
            if anchor >= self.profile.next
                || cursor.is_some_and(|cursor| cursor.before.0 == 0 || cursor.before.0 > anchor)
            {
                return Err("invalid conversation cursor".into());
            }
            let mut page = ConversationPage {
                store: scope.store.clone(),
                conversation: scope.conversation.clone(),
                epoch: Counter(scope.epoch),
                sequence: self.sequence()?,
                anchor: Counter(anchor),
                conversations: Vec::new(),
                next: None,
            };
            let mut end = cursor.map_or(anchor, |cursor| cursor.before.0 - 1);
            let mut last = None;
            loop {
                let entry = storage(|| {
                    let table = self.write.open_table(CONVERSATION_ORDER)?;
                    Ok(table
                        .range(..=end)?
                        .next_back()
                        .transpose()?
                        .map(|(number, id)| (number.value(), id.value().to_owned())))
                })?;
                let Some((number, id)) = entry else { break };
                if page.conversations.len() == 16 {
                    page.next = Some(Cursor {
                        anchor: Counter(anchor),
                        before: Counter(last.ok_or("conversation cursor absent")?),
                    });
                    break;
                }
                let conversation = self.saved_conversation(&id)?;
                if storage(|| conversation_number(&self.profile, &id))? != number {
                    return storage(|| Err("conversation order differs".into()));
                }
                page.conversations.push(ConversationSummary {
                    id,
                    title: Preview::text(&conversation.title, 256),
                });
                page.next = Some(Cursor {
                    anchor: Counter(anchor),
                    before: Counter(number),
                });
                if serde_json::to_vec(&page)?.len() > 32_768 {
                    page.conversations.pop();
                    if page.conversations.is_empty() {
                        return Err("conversation summary exceeds capacity".into());
                    }
                    page.next = Some(Cursor {
                        anchor: Counter(anchor),
                        before: Counter(last.ok_or("conversation cursor absent")?),
                    });
                    break;
                }
                page.next = None;
                last = Some(number);
                end = number - 1;
            }
            Ok(page)
        })())
    }

    pub(super) fn require_run_request(&self, request: &str) -> Result<()> {
        if storage(|| Ok(self.write.open_table(SELECTIONS)?.get(request)?.is_some()))? {
            return Err("request identity already names a conversation change".into());
        }
        Ok(())
    }

    pub(super) fn saved_conversation(&self, id: &str) -> Result<Conversation> {
        storage(|| {
            let number = conversation_number(&self.profile, id)?;
            let row: Conversation = decode(
                self.write
                    .open_table(CONVERSATIONS)?
                    .get(id)?
                    .ok_or("conversation is absent")?
                    .value(),
            )?;
            if row.id != id
                || row.title.len() > 256
                || row
                    .source
                    .as_ref()
                    .is_some_and(|source| !self.valid_id(source))
                || self
                    .write
                    .open_table(CONVERSATION_ORDER)?
                    .get(number)?
                    .map(|value| value.value() == id)
                    != Some(true)
            {
                return Err("saved conversation differs".into());
            }
            Ok(row)
        })
    }
}
