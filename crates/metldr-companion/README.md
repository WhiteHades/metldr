# metldr companion

The companion runs provider requests, installed agents and browser tasks for the [Rust panel](../metldr-panel/README.md). It keeps run state, approvals and recovery in one native process. Browser commands use the normal Chromiumoxide dependency and the extension's debugger bridge.

Pi receives the shared browser tools through a temporary MCP registration for each run. Its installed tools, authentication and saved session remain under Pi control. Browser field changes use the same separate metldr approval as other execution routes. The registration file contains no endpoint secret and is removed after Pi exits. An uncertain action or unconfirmed release blocks that session from continuing automatically.

Pi extensions can request selection, confirmation, input and editor dialogs in the panel. Native notices, status, widgets, titles and editor suggestions are also shown. Dialog replies return the exact selected value or entered text. Suggestions require an explicit action before replacing the composer draft. Native timeouts remove expired controls; the companion adds no editor timeout. Stop or loss of the native controller retires pending dialogs. Native UI content and replies stay in bounded memory and are discarded when the companion restarts. Browser action approvals remain separate. Custom TUI components that Pi does not expose through RPC remain unavailable. Initial startup dialogs are rejected before showing a reply control. Installed Pi 1.3.0 does not consume replies while an extension awaits a dialog in its first `session_start` hook. Defer that dialog until after startup.

Codex and OpenCode share the panel's approval view. Codex requests can cover commands, file changes and additional permissions. Review the complete details before choosing an option. Saved command rules and session grants have separate labels. A changed file patch replaces its review and invalidates the old choice. The shared approval view is limited to 8192 encoded bytes. Larger reviews stop the task without approval. A native event backlog that prevents checking a queued decision also stops the task without applying that decision. Each agent retains its own policy and response format.

Active Pi, Codex and OpenCode turns have no overall companion time limit. Startup, RPC, browser operations and resource limits still apply. After Stop is observed, the companion waits up to 15 seconds for native confirmation between protocol operations, then enters cleanup if confirmation is missing. Expiry is a failure, not confirmed cancellation. Native RPC and process cleanup retain their own deadlines. Codex and OpenCode approval prompts wait for your decision or Stop. MCP browser calls have a cumulative 30 second queue and execution limit. A saved browser approval pauses that limit until your decision or Stop. Clients that request progress and accept SSE receive updates during review. Their own request limits still apply.

Browser inspection and field changes run in a named isolated world. Page scripts cannot replace the JavaScript properties used to read or change a field. The companion still checks the current frame and execution context before using an approved action, and requests no universal access for the isolated world. A verified field value does not establish that a website accepted or saved it.

From the repository root, run:

```sh
make check-companion
make companion
```

The default output is `crates/metldr-companion/target/release/metldr-companion`. Cargo uses `CARGO_TARGET_DIR` instead when it is set. The checks write their local artifacts under this crate's `.tmp` directory.

Pair the executable with an extension that implements metldr's native wire version 6. The default Vue frontend and complete extension packaging are still being migrated. Linux is the verified native platform. Windows and macOS installation and runtime checks remain open.

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

Browser button actions use the existing saved approval and Chromiumoxide engine. Native controls in the observed main document receive one bounded Chrome mouse gesture. Approval checks the task tab and exact document even when another tab is active. Changing tabs does not redirect the action. The receipt uses the isolated automation world and reports an observed trusted click separately from site success. A page can move a control between the check and input. Missing or conflicting evidence after admission stays uncertain, and an effect may already have occurred. Button navigation does not adopt a new document grant. Saved schema 6 migrates older stores with their exact backups; pair the host and extension wire version 6 before use.
