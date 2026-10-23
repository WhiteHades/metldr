# metldr companion

The companion runs provider requests, installed agents and browser tasks for the [Rust panel](../metldr-panel/README.md). It keeps run state, approvals and recovery in one native process. Browser commands use the normal Chromiumoxide dependency and the extension's debugger bridge.

Page summaries and questions have a separate text path for configured Messages, Responses, Google Interactions and Chat Completions endpoints. The request contains the saved page and completed text answers from the same page and provider selection. It offers no browser tools. Changing the provider, model or connection revision starts a separate history. The companion keeps at most six prior turns and removes whole oldest turns when the context limit is reached. It refuses a source and question that cannot fit without truncation.

Text results use the same local run storage, streaming, Stop and restart handling. Partial output can remain after failure or cancellation. A native restart interrupts an active request and does not send it again. Stop closes the local stream; remote inference and billing cancellation depend on the provider.

Pi receives the shared browser tools through a temporary MCP registration for each run. Pi loads its native skills, and skill prompts receive the saved page and browser rules. Its installed tools, authentication and saved session remain under Pi control. Browser field changes use the same separate metldr approval as other execution routes. The registration file contains no endpoint secret and is removed after Pi exits. An uncertain action or unconfirmed release blocks that session from continuing automatically.

Pi extensions can request selection, confirmation, input and editor dialogs in the panel. Native notices, status, widgets, titles and editor suggestions are also shown. Dialog replies return the exact selected value or entered text. Suggestions require an explicit action before replacing the composer draft. Native timeouts remove expired controls; the companion adds no editor timeout. Stop or loss of the native controller retires pending dialogs. Native UI content and replies stay in bounded memory and are discarded when the companion restarts. Browser action approvals remain separate. Custom TUI components that Pi does not expose through RPC remain unavailable. Initial startup dialogs are rejected before showing a reply control. Installed Pi 1.3.0 does not consume replies while an extension awaits a dialog in its first `session_start` hook. Defer that dialog until after startup.

Codex and OpenCode share the panel's approval view. Codex requests can cover commands, file changes and additional permissions. Review the complete details before choosing an option. Saved command rules and session grants have separate labels. A changed file patch replaces its review and invalidates the old choice. The shared approval view is limited to 8192 encoded bytes. Larger reviews stop the task without approval. A native event backlog that prevents checking a queued decision also stops the task without applying that decision. Each agent retains its own policy and response format.

During active Codex or Pi work, use the composer to send a correction without starting another session. Stop remains available. Saved receipts distinguish native acceptance, Pi queue admission, extension handling and uncertain delivery. None confirms model use. An uncertain correction blocks session reuse and is never sent again automatically. Each run accepts at most 16 corrections of 1024 UTF8 bytes each.

Active Pi, Codex and OpenCode turns have no overall companion time limit. Startup, RPC, browser operations and resource limits still apply. After Stop is observed, the companion waits up to 15 seconds for native confirmation between protocol operations, then enters cleanup if confirmation is missing. Expiry is a failure, not confirmed cancellation. Native RPC and process cleanup retain their own deadlines. Codex and OpenCode approval prompts wait for your decision or Stop. MCP browser calls have a cumulative 30 second queue and execution limit. A saved browser approval pauses that limit until your decision or Stop. Clients that request progress and accept SSE receive updates during review. Their own request limits still apply.

Browser inspection and field changes run in a named isolated world. Page scripts cannot replace the JavaScript properties used to read or change a field. The companion still checks the current frame and execution context before using an approved action, and requests no universal access for the isolated world. A verified field value does not establish that a website accepted or saved it.

From the repository root, run:

```sh
make check-companion
make companion
```

The default output is `crates/metldr-companion/target/release/metldr-companion`. Cargo uses `CARGO_TARGET_DIR` instead when it is set. The checks write their local artifacts under this crate's `.tmp` directory.

Process checks that require companion executables are ignored by default. To run the five current startup checks on Linux, build the executable and provide its full path:

```sh
cargo build --locked --manifest-path crates/metldr-companion/Cargo.toml
METLDR_STARTUP_BINARY="$PWD/crates/metldr-companion/target/debug/metldr-companion" \
  cargo test --locked --manifest-path crates/metldr-companion/Cargo.toml \
  history::startup_checks -- --ignored --skip actual_old_private_host_upgrades_and_cannot_reopen_new_history
```

Use the actual output path if `CARGO_TARGET_DIR` is set. The two older executable compatibility checks also require fixture binaries. `METLDR_CUTOVER_OLD_BINARY` must name the earlier wire 9 companion that writes saved schema 8. `METLDR_HISTORY_OLD_BINARY` must name a companion that writes history format 10 through 13. Run those checks explicitly with `--ignored` only when the matching fixtures are available. A skipped compatibility check does not establish upgrade support.

Pair the executable with an extension that implements metldr's native wire version 14. The remaining Vue page tools and complete extension packaging are still being migrated. Linux is the verified native platform. The history loader refuses to start on Windows and macOS until their directory durability and installation checks are complete.

Conversations and complete saved results are stored in a local record database. Startup preserves the original snapshot backup before installing the new format. New chat creates an empty conversation, while selecting a saved conversation restores its page and results. Switching is unavailable during active work. A lost confirmation requires an explicit Retry using the same request rather than another creation. Viewing saved work does not change or replay an active task. History search covers the loaded conversation pages, and More loads the next page.

Saved native sessions remain selectable after a model task and browser restart. The chooser shows up to 16 recent sessions for each agent, saved page and current profile in the selected conversation. Earlier results stay in saved history. History format 12 adds the session index and upgrades formats 10 and 11 in one transaction. An older companion refuses the new format.

To register the companion with Chrome:

1. Copy the executable to a stable location and create a private state directory owned by your user. On Linux, give the state directory mode `700` and use a directory without a symbolic link.
2. Save `metldr-companion.json` beside the executable with the configuration below. Replace the example extension ID and absolute paths with your own values. JSON paths must be complete paths.

```json
{
  "schema": 1,
  "extension_origin": "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
  "state_root": "/absolute/private/metldr-state"
}
```

3. Save the host manifest below as `io.metldr.first_flow.json` in Chrome's user `NativeMessagingHosts` directory. Use the same extension origin in both files. The existing registration name must match the Rust panel's `connectNative` call.

```json
{
  "name": "io.metldr.first_flow",
  "description": "metldr Rust companion",
  "path": "/absolute/path/to/metldr-companion",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"]
}
```

For Google Chrome on Linux, the user manifest directory is `~/.config/google-chrome/NativeMessagingHosts`. Use [Chrome's native messaging guide](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging) for other browser channels and platform registration paths. Chrome starts the executable and supplies the extension origin. The companion checks the origin against its adjacent configuration and selects a separate state directory for each browser profile.

Provider keys stay in the native credential store. Installed agents keep their own authentication. Configure the desired provider or agent through the paired Rust panel before starting work. An action that loses confirmation remains uncertain and is not repeated automatically.

If an action request ends before its browser command is queued, the companion records that the action was prevented. Cancellation and command delivery share one admission check, so an expired request cannot queue a write later. After a command is queued, a lost result remains uncertain. Closing the browser connection still requires confirmed detach and transport cleanup.

The three files in `test-tls` are synthetic fixture material. The loopback fixture profile embeds `ca.pem`; the server certificate and server key are used only by tests. Public provider requests use their configured trust settings. None of these files grants access to a live service.

Browser button actions and single-select option choices use the existing saved approval and Chromiumoxide engine. Selection receipts report the exact option observed in the local DOM; they do not claim site persistence or form submission. Native button controls in the observed main document receive one bounded Chrome mouse gesture. Selection changes emit synthetic input and change events. Approval checks the task tab and exact document even when another tab is active. Changing tabs does not redirect the action. The button receipt uses the isolated automation world and reports an observed trusted click separately from site success. A page can move a control between the check and input. Missing or conflicting evidence after admission stays uncertain, and an effect may already have occurred. Button navigation does not adopt a new document grant.

Browser scrolling uses the same saved approval and Chromiumoxide engine. The page and nested regions have handles from the current observation. Receipts show the actual position after an instant request and a second read, including no movement. Changed targets and positions prevent dispatch. A lost admitted result is uncertain and is never replayed. Main document targets are supported; frame and shadow root scrolling remain unavailable.
