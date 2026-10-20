# MetLDR companion

The companion runs provider requests, installed agents and browser tasks for the [Rust panel](../metldr-panel/README.md). It keeps run state, approvals and recovery in one native process. Browser commands use the normal Chromiumoxide dependency and the extension's debugger bridge.

From the repository root, run:

```sh
make check-companion
make companion
```

The default output is `crates/metldr-companion/target/release/metldr-companion`. Cargo uses `CARGO_TARGET_DIR` instead when it is set. The checks write their local artifacts under this crate's `.tmp` directory.

Pair the executable with an extension that implements MetLDR's native wire version 3. The default Vue frontend and complete extension packaging are still being migrated. Linux is the verified native platform. Windows and macOS installation and runtime checks remain open.

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
  "description": "MetLDR Rust companion",
  "path": "/absolute/path/to/metldr-companion",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"]
}
```

For Google Chrome on Linux, the user manifest directory is `~/.config/google-chrome/NativeMessagingHosts`. Use [Chrome's native messaging guide](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging) for other browser channels and platform registration paths. Chrome starts the executable and supplies the extension origin. The companion checks the origin against its adjacent configuration and selects a separate state directory for each browser profile.

Provider keys stay in the native credential store. Installed agents keep their own authentication. Configure the desired provider or agent through the paired Rust panel before starting work. An action that loses confirmation remains uncertain and is not repeated automatically.

The three files in `test-tls` are synthetic fixture material. The loopback fixture profile embeds `ca.pem`; the server certificate and server key are used only by tests. Public provider requests use their configured trust settings. None of these files grants access to a live service.
