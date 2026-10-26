# metldr Responses client

This Rust client streams plain text, browser tool calls, or a single validated
table presentation through the OpenAI Responses protocol. Browser conversations
keep request history and finalized reasoning. Presentation conversations expose
only the source table catalog and keep their tool continuation in the client.
The application owns browser permissions, approvals and actions.

```no_run
use metldr_responses::{Client, Prompt, Result, TextEvent};
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
let invocation = conversation.next(&stop, |event| {
    if let TextEvent::Delta(text) = event { print!("{text}"); }
    Ok(())
}).await?;
if let Some(invocation) = invocation {
    println!("Application must handle tool call {}", invocation.id);
}
# Ok(())
# }
```

For a text response, use `Client::text` instead of starting a browser conversation.
The caller supplies system instructions and input separately.

```no_run
use metldr_responses::{Client, Result, TextEvent};
use std::sync::atomic::AtomicBool;

# async fn example() -> Result<()> {
let client = Client::new("https://api.openai.com/v1/responses", "your-supported-model",
    &std::env::var("OPENAI_API_KEY")?, None)?;
client.text("Answer using the supplied source.", "The report lists 17 units.",
    &AtomicBool::new(false), |event| {
        if let TextEvent::Delta(text) = event { print!("{text}"); }
        Ok(())
    }).await?;
# Ok(())
# }
```

The text request includes no tool definitions or browser policy and uses
`store: false`. It rejects tool calls, empty answers and incomplete streams.
Partial text can arrive before an error. The caller owns source selection,
history and the final run status. All modes share transport and cancellation
behavior, with mode-specific resource bounds.

After approving and executing a returned invocation, pass the actual browser
result to `conversation.submit_tool_result(&result)`, then call `next` again.
The conversation appends the finalized output and matching call identifier
before the result. A second submission or another request while a result is
pending fails. A final answer returns `None`, after its ordered text events
reach the callback. Cancellation or a request failure closes the conversation.
Partial text can be visible before a failure, so the application must show the
failure and must not treat partial text as a completed task.

The client uses HTTPS with rustls, a 45 second timeout, and disabled proxies,
redirects and retries. An optional PEM bundle replaces the default trust roots
and is limited to 16 KiB. Credentials enter only the sensitive Authorization
header. Endpoint user information, query strings and fragments are rejected.
The caller owns credential storage and must trust its configured endpoint.

Requests use `store: false`, encrypted reasoning, strict tool schemas and one
tool call at a time. Presentation mode offers only `render_a2ui` for
`copilotkit://open-generative-ui-tables`, with one root `Table` component.
It validates the surface id and converts table fields through the shared
validated table type. The client validates event ordering, identifiers, output
items and terminal output, then requires clean HTTP completion before returning
a tool call. It retains finalized opaque reasoning for the next request.
Browser and text requests remain limited to 128 KiB, 16 KiB per tool argument,
128 KiB per SSE line/event, and 512 KiB per stream. Presentation requests are
limited to 2 MiB, tool arguments to 384 KiB, SSE lines/events to 1 MiB, and
streams to 8 MiB. Text is limited to 8 KiB per response. The application must
also bound complete runs.

Browser and text responses allow at most 4096 events. Presentation responses
allow at most 65536 events.

Run `make check-responses` from the repository root. The compiled guide and
focused Rust checks cover request construction, streaming, finalized history
and conversation ordering. The application verification uses a controlled HTTPS
fixture and Chrome on Linux. Live provider behavior and native Windows and
macOS execution remain unverified. This package does not install a native host
or replace the production extension interface.
