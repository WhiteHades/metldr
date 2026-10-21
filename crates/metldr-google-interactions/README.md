# metldr Google Interactions client

This Rust client uses Google's native Interactions API with metldr's browser
tool definitions. One conversation owns the full request history and pending
tool result. The application owns browser permissions, approvals and actions.

```no_run
use metldr_google_interactions::{Client, Prompt, Result};
use std::sync::atomic::AtomicBool;

# async fn example() -> Result<()> {
let key = std::env::var("GEMINI_API_KEY")?;
let mut conversation = Client::new(
    "https://generativelanguage.googleapis.com/v1/interactions",
    "your-supported-model",
    &key,
    None,
)?.start(Prompt {
    instruction: "Read the current page and report the outstanding amount.",
    title: "Current page",
    url: "https://example.com/report",
    text: "Page text supplied by the application",
})?;
let stop = AtomicBool::new(false);
let invocation = conversation.next(&stop, |text| {
    print!("{text}");
    Ok(())
}).await?;
if let Some(invocation) = invocation {
    println!("Application must handle tool call {}", invocation.id);
}
# Ok(())
# }
```

After approving and executing a returned invocation, submit the actual browser
result with `conversation.submit_tool_result(&result)`, then call `next` again.
The conversation retains complete thought steps and signatures, model text and
function calls. It appends each result using the matching function name and
call identifier. Duplicate submission and another request while a result is
pending fail. A final answer returns `None`, with text already delivered to the
callback. Cancellation or a request failure closes the conversation.

Requests use `store: false` and full local history. They repeat the browser
policy, tools and generation settings on every turn. The client uses Google's
`validated` tool choice and accepts one browser call per turn. It does not use
server conversation identifiers, background execution or automatic replay.
Setting `store: false` does not establish zero provider processing retention.
See Google's [stateless function calling guide](https://ai.google.dev/gemini-api/docs/function-calling)
and [stable API reference](https://ai.google.dev/api/interactions-api-v1).

The client uses HTTPS with rustls, a 45 second timeout, and disabled proxies,
redirects and retries. An optional PEM bundle replaces default trust roots and
is limited to 16 KiB. The key enters only the sensitive `x-goog-api-key` header.
The caller owns credential storage and must trust its configured endpoint.
Endpoint user information, query strings and fragments are rejected.

Tool calls are released only after complete steps, a successful terminal
`requires_action` status and clean HTTP completion. A final answer requires
`completed` status. Partial text can be visible before failure, so the
application must show failures and must not treat partial text as completed
work. Stop closes the local request; server execution or billing cancellation
is not established.

Request bodies are limited to 128 KiB and response streams to 512 KiB. Visible
text is limited to 8 KiB per response. Function arguments and signatures are
limited to 16 KiB. The application must also bound complete runs.

Run `make check-google` from the repository root. The focused Rust checks and
compiled guide cover request construction, stream validation, complete history
and conversation ordering. Controlled application checks use an owned HTTPS
fixture and Chrome on Linux. Live Google account behavior, model compatibility
and native Windows and macOS execution remain unverified. This package does
not install a native host or replace the production extension interface.
