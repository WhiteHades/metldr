# metldr Responses client

This Rust client streams the OpenAI Responses protocol with metldr's browser
tool definitions. It keeps request history and finalized reasoning inside one
conversation. The application owns browser permissions, approvals and actions.

```no_run
use metldr_responses::{Client, Prompt, Result};
use std::sync::atomic::AtomicBool;

# async fn example() -> Result<()> {
let key = std::env::var("OPENAI_API_KEY")?;
let client = Client::new(
    "https://api.openai.com/v1/responses",
    "your-supported-model",
    &key,
    None,
)?;
let mut conversation = client.start(Prompt {
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

After approving and executing a returned invocation, pass the actual browser
result to `conversation.submit_tool_result(&result)`, then call `next` again.
The conversation appends the finalized output and matching call identifier
before the result. A second submission or another request while a result is
pending fails. A final answer returns `None`, with its text already delivered
to the callback. Cancellation or a request failure closes the conversation.
Partial text can be visible before a failure, so the application must show the
failure and must not treat partial text as a completed task.

The client uses HTTPS with rustls, a 45 second timeout, and disabled proxies,
redirects and retries. An optional PEM bundle replaces the default trust roots
and is limited to 16 KiB. Credentials enter only the sensitive Authorization
header. Endpoint user information, query strings and fragments are rejected.
The caller owns credential storage and must trust its configured endpoint.

Requests use `store: false`, encrypted reasoning, strict tool schemas and one
tool call at a time. The client validates event ordering, identifiers, output
items and terminal output, then requires clean HTTP completion before returning
a tool call. It retains finalized opaque reasoning for the next request.
Request bodies are limited to 128 KiB and response streams to 512 KiB. Text is
limited to 8 KiB per response. The application must also bound complete runs.

Run `make check-responses` from the repository root. The compiled guide and
focused Rust checks cover request construction, streaming, finalized history
and conversation ordering. The application verification uses a controlled HTTPS
fixture and Chrome on Linux. Live provider behavior and native Windows and
macOS execution remain unverified. This package does not install a native host
or replace the production extension interface.
