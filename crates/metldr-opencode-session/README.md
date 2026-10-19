# Native OpenCode sessions

The Rust crate runs an installed OpenCode executable with `acp` over stdio. It inherits the caller's environment, authentication and OpenCode configuration. OpenCode owns its native tools, model requests and saved sessions.

The adapter supports new sessions, saved session resume, streamed updates, native permission requests and cancellation. It uses ACP protocol 1, verified with OpenCode 2.0.32 on Linux through a controlled model provider. Native Windows, macOS and live provider behavior still need verification.

HTTP MCP attachment is verified across process restart with an owned Rust server, including authenticated requests, native tool execution and permission replies. Stdio declarations have source and boundary checks; installed stdio server execution remains unverified.

```no_run
use metldr_opencode_session::{Event, OpenCodeSession, Session};
use std::path::Path;

#[tokio::main(flavor = "current_thread")]
async fn main() -> metldr_opencode_session::Result<()> {
    let workspace = std::env::current_dir()?;
    let mut session = OpenCodeSession::open(
        Path::new("/absolute/path/to/opencode"),
        &workspace,
        Session::New,
        &[],
    ).await?;

    session.prompt("Describe this workspace.").await?;
    loop {
        match session.next_event().await? {
            Event::Update(update) => println!("{update}"),
            Event::Permission(request) => {
                session.reply_permission(&request, None).await?;
            }
            Event::Finished { stop_reason } => {
                println!("{stop_reason}");
                break;
            }
        }
    }
    let exit = session.close().await?;
    assert_eq!(exit.code, Some(0));
    assert!(!exit.forced && exit.drained);
    Ok(())
}
```

Prompts accept up to 128 KiB of UTF8 text, including the complete supplied context. Oversized prompts return an error without truncation.

Save the identity from `id()` before sending a prompt. Resume it with `Session::Resume` and `SessionId::saved` in the same workspace. Resume attaches the stored session without replaying its earlier messages or sending a prompt. Supply trusted `McpServer` declarations as the fourth argument, including again on resume. An empty list adds no MCP servers. HTTP headers and stdio environment values can contain credentials, so keep declarations in the native owner and out of page contexts and logs. OpenCode retains responsibility for connecting these servers and applying its native tool permissions.

`Event::Update` carries the native session update. The caller renders its text, tool status and configuration updates. A permission request exposes the native tool details and offered choices through `details()`. Pass an offered option ID to `reply_permission`, or `None` to cancel the request. The example cancels every request. Permission tokens belong to their connection and request, including when a native child task asks after a parent reply finishes. Invalid choices can be corrected, and answered tokens cannot be reused.

`cancel()` sends a cancellation notification. Wait for `Event::Finished` to establish the turn outcome. Cancelling an event wait preserves its partial frame. Cancelling an RPC or pipe write kills the direct process and prevents connection reuse. Frames, queued events, pending permission requests and RPC deadlines are bounded. Native errors during a prompt end the connection without assuming that its work completed.

`close()` detaches the native session when idle, closes stdin and waits for direct process exit and stdout drain. Saved history remains in OpenCode's database. Forced exit and incomplete drain are reported separately. Callers own descendant cleanup and supervision. The adapter does not sandbox OpenCode tools or connect the browser interface itself.

Run `make check-opencode` from the repository root.
