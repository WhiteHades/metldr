# Conversation model

This crate owns assistant message text and records observed tool positions. It has no browser, network, process or storage dependency. Native hosts and WebAssembly interfaces can use the same validated representation.

```rust
use metldr_conversation::{Change, Content, Display, Ending, EntryRef, MessageKey, ToolId};

let message = MessageKey::native("reply-1".into())?;
let mut content = Content::native();
content.change(Change::Append {
    key: message.clone(),
    text: "Working.".into(),
})?;
content.observe_tool(ToolId::new("lookup-1".into())?)?;
content.change(Change::Final {
    key: message,
    text: "Finished.".into(),
})?;
content.end(Ending::Complete)?;

let display = Display::new(content.projection())?;
assert_eq!(display.text(), "Finished.");
assert!(matches!(display.entries().nth(1), Some(EntryRef::Tool { .. })));
# Ok::<(), String>(())
```

`Content::flat` preserves existing plain text without inventing message boundaries. Native content keeps each message key, its text and its completion state. A first final snapshot can replace provisional text. An identical repeated final snapshot changes nothing, while a conflicting final snapshot or an append to a completed message returns an error. Rejected changes leave content unchanged.

Tools are recorded by their actual call IDs in order of first observation. Recording the same call again changes nothing. Tool positions describe first observation, rather than the timing of every token in a continuing message.

`Ending::Complete` marks remaining streamed messages as `StreamEnded`. `Ending::Interrupted` marks them as `Interrupted`. Both preserve messages that already have an authoritative final snapshot.

`Content::projection` joins message text without inserting separators and includes ordered message ranges and tool IDs. Ranges count UTF8 bytes. `Display::new` rejects duplicate IDs, invalid ranges, incomplete text coverage and ranges that split a Unicode character. `Display::entries` borrows text from the validated projection. Tools never contribute text to the projection.

Content is limited to 8192 UTF8 bytes, 64 messages, 64 tools and 128 entries. IDs must be nonempty, contain no control characters and use at most 128 UTF8 bytes. Callers must bound incoming frames before deserialization. Serialized flat content is a string; native content is an object with an `entries` array. Unknown fields are rejected.

Use `make check-conversation` from the repository root to run the checks and example with the pinned Rust toolchain.
