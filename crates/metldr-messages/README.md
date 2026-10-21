# metldr Messages client

This Rust client uses the native Anthropic Messages API with metldr's browser
tool definitions. One conversation owns its request history and pending tool
result. The application owns browser permissions, approval and action execution.

```no_run
use metldr_messages::{Client, Prompt, Result};
use std::sync::atomic::AtomicBool;

# async fn example() -> Result<()> {
let key = std::env::var("ANTHROPIC_API_KEY")?;
let mut conversation = Client::new(
    "https://api.anthropic.com/v1/messages",
    "your-supported-model-id",
    &key,
    None,
)?.start(Prompt {
    instruction: "Read the current page and report the outstanding amount.",
    title: "Current page",
    url: "https://example.com/report",
    text: "Page text supplied by the application",
})?;
let stop = AtomicBool::new(false);
if let Some(invocation) = conversation.next(&stop, |text| {
    print!("{text}");
    Ok(())
}).await? {
    println!("Application must handle tool call {}", invocation.id);
}
# Ok(())
# }
```

After approving and executing a returned invocation, submit the actual result
with `conversation.submit_tool_result(&result)`, then call `next` again. The
conversation preserves complete finalized assistant content, including opaque
thinking signatures and redacted thinking. It appends each matching tool result
once. A final answer returns `None`, with text already delivered to the callback.
A missing result prevents another request. Cancellation and failures close the
conversation without replay.

Requests use `stream: true`, the native `x-api-key` header and API version
`2023-06-01`. The configured model must match the response model identifier.
The caller owns credential storage and trusts its configured endpoint.
Bearer tokens, workload identity, managed tools, server fallback and thinking
configuration are outside this client interface. Returned thinking content is
preserved; the client does not enable thinking or verify its signatures.
See the official [streaming guide](https://platform.claude.com/docs/en/build-with-claude/streaming)
and [tool continuation guide](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls).

Text arrives incrementally. A tool call requires complete content blocks,
consistent successful stop reason, `message_stop` and clean HTTP completion.
One browser call is accepted per turn. Unsupported content, incomplete output,
refusal, malformed streams and provider errors release no call. Partial text
can remain visible after failure, so the application must show the failure.
Unknown event types and pings are ignored within the event and byte limits.

The internal HTTPS client uses rustls, a 45 second timeout and disabled proxies,
redirects and retries. Optional PEM roots replace default roots and are limited
to 16 KiB. Request bodies are limited to 128 KiB and response streams to 512 KiB.
Visible text is limited to 8 KiB per response, with 16 KiB limits on block text,
function arguments and signatures. The application must also bound whole runs.
Stop closes local consumption and prevents returning a call; server computation
and billing cancellation are not established. Provider retention is separate
from local history, and a direct request does not establish zero retention.

Run `make check-messages` from the repository root. The Rust checks and compiled
guide cover request construction, incremental text, complete opaque history,
stream completion and conversation ordering. Live account and model behavior,
native Windows and macOS execution, and production extension installation
remain unverified.
