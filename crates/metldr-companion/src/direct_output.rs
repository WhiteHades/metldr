use crate::{Result, flow::Event};
use metldr_conversation::{Change, MessageKey, TextEvent};
use std::sync::mpsc::SyncSender;

pub(super) struct DirectOutput<'a> {
    run: &'a str,
    tx: &'a SyncSender<Event>,
    ordinal: u16,
    current: Option<MessageKey>,
    bytes: usize,
}

impl<'a> DirectOutput<'a> {
    pub(super) fn new(run: &'a str, tx: &'a SyncSender<Event>) -> Self {
        Self {
            run,
            tx,
            ordinal: 0,
            current: None,
            bytes: 0,
        }
    }

    pub(super) fn accept(&mut self, event: TextEvent) -> Result<()> {
        let change = match event {
            TextEvent::Delta(text) => {
                if text.is_empty() {
                    return Ok(());
                }
                self.bytes = self.bytes.saturating_add(text.len());
                if self.bytes > crate::TEXT_MAX {
                    return Err("provider output exceeds bound".into());
                }
                let key = match &self.current {
                    Some(key) => key.clone(),
                    None => {
                        let ordinal = self
                            .ordinal
                            .checked_add(1)
                            .ok_or("provider message capacity overflow")?;
                        let key = MessageKey::observed(ordinal)?;
                        self.ordinal = ordinal;
                        self.current = Some(key.clone());
                        key
                    }
                };
                Change::Append { key, text }
            }
            TextEvent::Final(text) => {
                let Some(key) = self.current.take() else {
                    if text.is_empty() {
                        return Ok(());
                    }
                    return Err("provider final message has no streamed text".into());
                };
                Change::Final { key, text }
            }
        };
        self.tx
            .try_send(Event::Text {
                run: self.run.into(),
                change,
            })
            .map_err(|_| "owner output channel unavailable".into())
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use metldr_conversation::{Completion, Content, Ending, EntryRef};
    use std::sync::mpsc;

    #[test]
    fn preserves_message_boundaries_and_the_latest_interrupted_text() {
        let (tx, rx) = mpsc::sync_channel(8);
        let mut output = DirectOutput::new("run", &tx);
        for event in [
            TextEvent::Delta(String::new()),
            TextEvent::Final(String::new()),
            TextEvent::Delta("North ".into()),
            TextEvent::Delta("🌍".into()),
            TextEvent::Final("North 🌍".into()),
            TextEvent::Delta("Second".into()),
        ] {
            output.accept(event).unwrap();
        }
        let mut content = Content::native();
        for event in rx.try_iter() {
            let Event::Text { run, change } = event else {
                panic!("unexpected output event")
            };
            assert_eq!(run, "run");
            content.change(change).unwrap();
        }
        content.end(Ending::Interrupted).unwrap();
        assert_eq!(content.text(), "North 🌍Second");
        let display = metldr_conversation::Display::new(content.projection()).unwrap();
        assert!(matches!(
            display.entries().collect::<Vec<_>>().as_slice(),
            [
                EntryRef::Message {
                    text: "North 🌍",
                    completion: Completion::FinalSnapshot,
                    ..
                },
                EntryRef::Message {
                    text: "Second",
                    completion: Completion::Interrupted,
                    ..
                },
            ]
        ));
    }

    #[test]
    fn rejects_unmatched_final_and_run_bounds_before_emission() {
        let (tx, rx) = mpsc::sync_channel(128);
        let mut output = DirectOutput::new("run", &tx);
        assert!(output.accept(TextEvent::Final("unmatched".into())).is_err());
        assert!(rx.try_recv().is_err());
        for _ in 0..64 {
            output.accept(TextEvent::Delta("x".into())).unwrap();
            output.accept(TextEvent::Final("x".into())).unwrap();
        }
        assert!(output.accept(TextEvent::Delta("65".into())).is_err());
        assert_eq!(rx.try_iter().count(), 128);
        let mut output = DirectOutput::new("other", &tx);
        assert!(
            output
                .accept(TextEvent::Delta("x".repeat(crate::TEXT_MAX + 1)))
                .is_err()
        );
        assert!(rx.try_recv().is_err());
    }
}
