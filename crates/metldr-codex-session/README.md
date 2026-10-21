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
            Event::Message { .. } | Event::ToolProgress { .. } => (),
            Event::ToolReport(report) => println!("{}: {:?}", report.tool, report.state),
            Event::Approval(request) => {
                session.reply_approval(&request, None).await?;
            }
            Event::ApprovalResolved(_) => (),
            Event::ApprovalsUpdated(requests) => {
                for (_, request) in requests {
                    session.reply_approval(&request, None).await?;
                }
            }
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

Resume requests metadata without returning prior turns. Codex restores the saved context for subsequent turns.

Each completed `Event::Message` carries its item ID and full text. Use it to reconcile streamed deltas for that item. An interrupt reply reports acknowledgement or rejection; `Event::Finished` determines the final turn outcome.

Cancelling an event wait preserves its partial frame. Cancelling an RPC or pipe write kills the process and makes the connection unusable. Frames, queued events, tool requests and RPC deadlines are bounded. Tool replies belong to their originating connection and turn.

Native command, file change and additional permission requests produce `Event::Approval`. Display the complete `review()` and each offered choice before calling `reply_approval`. Choice IDs are opaque and belong to that request. Persistent command rules and session grants have separate labels. The example cancels requests because it has no approval interface.

Approval handles belong to their connection, turn and request generation. A resolved request cannot be answered again. `Event::ApprovalResolved` clears its interface. `Event::ApprovalsUpdated` replaces file reviews when their patches change and invalidates the preceding handles. File approval requires a complete current diff, with a limit of 32 KiB. Ordinary tool reports remain truncated observations and cannot substitute for approval review.

An approval reply returns true when it requests native turn cancellation. Wait for an interrupted terminal event before treating the task as cancelled. Empty additional permission grants deny the request and let Codex continue. Callers must keep consuming events while review is pending. Native policy, authentication and configuration remain under Codex control.

Clean direct process exit and drained stdout do not establish cleanup of every possible descendant. Callers still own process supervision and recovery after host loss.

Run `make check-codex` from the repository root.

Native command execution, file changes and dynamic tools produce `Event::ToolReport` observations. Reports retain the native item ID, turn, lifecycle outcome and bounded plain text previews. Codex command output can also produce `Event::ToolProgress` with a bounded provisional preview. It may omit quick early bytes, and the terminal report remains authoritative. Unknown success stays unspecified. An observation cannot authorize a client tool or prove a browser effect. Native item IDs are independent of client tool call IDs. Unsupported native item kinds are ignored.

Progress output is a complete cumulative preview. Replace the previous preview for its item ID instead of appending it. The example ignores these optional previews and prints agent text separately.

The session retains at most 32 supported item identities per turn. Exact repeated reports are suppressed, and conflicting lifecycle reports fail the session. Preview limits are 256 bytes for titles, 1024 for inputs and 2048 for outputs, cut at UTF8 boundaries with explicit incomplete flags. Nontext dynamic results are omitted and marked incomplete. Optional native timestamps are metadata. Callers decide what to display or retain.
