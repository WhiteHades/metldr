# Native Codex sessions

This Rust crate runs an installed Codex app server over stdio. It uses the caller's normal environment, configuration and authentication. It does not add provider overrides or read credentials itself.

The adapter supports saved threads, streamed messages, client tool replies, turn interruption and process shutdown. Codex 0.163.0 is verified on Linux. Native Windows and macOS execution still need verification. The app server's dynamic tool API is experimental.

```no_run
use metldr_codex_session::{CodexSession, Event, Thread};
use std::path::Path;

#[tokio::main(flavor = "current_thread")]
async fn main() -> metldr_codex_session::Result<()> {
    let workspace = std::env::current_dir()?;
    let mut session = CodexSession::open(
        Path::new("/absolute/path/to/codex"),
        &workspace,
        Thread::New { tools: vec![] },
    ).await?;

    session.prompt("Describe this workspace.").await?;
    loop {
        match session.next_event().await? {
            Event::TextDelta { text, .. } => print!("{text}"),
            Event::ToolCall(call) => {
                session.reply_tool(call, "No client tool handler is configured.", false).await?;
            }
            Event::Message { .. } => (),
            Event::Finished { .. } => break,
        }
    }
    let exit = session.close().await?;
    assert_eq!(exit.code, Some(0));
    assert!(!exit.forced && exit.drained);
    Ok(())
}
```

Save the thread identity from `info()` in the native task owner before dispatching a prompt. Reopen it with `Thread::Resume` and `ThreadId::saved`. Native identities and configuration should stay outside page contexts.

Each completed `Event::Message` carries its item ID and full text. Use it to reconcile streamed deltas for that item. An interrupt reply reports acknowledgement or rejection; `Event::Finished` determines the final turn outcome.

Cancelling an event wait preserves its partial frame. Cancelling an RPC or pipe write kills the process and makes the connection unusable. Frames, queued events, tool requests and RPC deadlines are bounded. Tool replies belong to their originating connection and turn.

Native approval requests are unsupported and fail without approval. Clean direct process exit and drained stdout do not establish cleanup of every possible descendant. Callers still own process supervision and recovery after host loss.

Run `make check-codex` from the repository root.
