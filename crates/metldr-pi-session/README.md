# Native Pi sessions

This Rust crate runs an installed Pi executable in RPC mode over stdio. It uses the caller's environment and Pi configuration. The caller chooses the workspace, provider, model and session directory through `PiProfile`.

The transport sends JSON commands and receives JSON events. Pi owns its tools, Skills, authentication and saved session files. Resume a session by setting `PiProfile.resume` to its saved session file. Reopening a session does not itself send a prompt.

Set `PiProfile.mcp` to bind one temporary HTTP MCP server. The descriptor accepts only a named server on IPv4 loopback with a token of 64 hex characters. It creates a temporary extension registration for each session and passes the token only to the Pi child environment. The registration is removed after the child exits or the session is dropped. Each HTTP request asks for its connection to close. This does not add arbitrary Pi arguments or change Pi configuration.

```no_run
use metldr_pi_session::{PiProfile, PiSession, Result};
use serde_json::json;
use std::time::Duration;

fn run(profile: PiProfile) -> Result<()> {
    let mut session = PiSession::spawn(profile)?;
    session.send(json!({
        "id": "question",
        "type": "prompt",
        "message": "Describe this workspace."
    }))?;
    loop {
        let event = session.recv(Duration::from_secs(30))?;
        println!("{event}");
        if event["type"] == "agent_settled" {
            break;
        }
    }
    let exit = session.close()?;
    assert_eq!(exit.code, Some(0));
    assert!(!exit.forced && exit.drained);
    Ok(())
}
```

Use command IDs to match replies. An accepted `abort` reply acknowledges the request; wait for `agent_settled` before treating the turn as finished. A receive timeout leaves the session open.

Writes wait up to 250 milliseconds for the pipe writer. A failed or expired write kills the direct child and prevents session reuse. Delivery can be uncertain, so do not send the command again automatically. Run this transport outside an interaction thread and provide external process supervision. Commands are limited to 32 KiB, frames to 64 KiB, and session output to 1 MiB or 2048 records. Reaching an output limit ends the reader with an error.

`close` closes stdin, waits up to three seconds, then kills the direct child if necessary. It allows 200 milliseconds for pipe workers to finish. A false `drained` value means workers remain active, and repeated calls retain that state until the workers finish. Once the child is forcibly killed, later calls still report `forced` as true. The caller owns descendant cleanup. Dropping a session kills and waits for its direct child.

Pi 1.3.0 is verified on Linux with a controlled provider for Skills, native file reading, streaming, session resume and abort. Native Windows, macOS and live provider behavior still need verification. The adapter is a transport component and does not provide a sandbox for Pi tools.

Run `make check-pi` from the repository root.
