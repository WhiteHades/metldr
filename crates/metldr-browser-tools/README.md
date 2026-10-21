# Browser tool contract

This Rust crate defines the shared tools for observing a page and proposing navigation, text field changes, native button clicks and single select choices. API providers and installed agents can use the same definitions and argument decoder.

```rust
use metldr_browser_tools::{Call, decode_call};
use serde_json::json;

let call = decode_call(
    "set_field_value",
    json!({"handle":"field_1","value":"Report ready"}),
)?;
assert_eq!(call, Call::Change {
    handle: "field_1".into(),
    value: "Report ready".into(),
});
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

The decoder rejects unknown tools, extra arguments and handles outside the supported identifier format. Handles contain 1 to 64 ASCII letters, digits, underscores or hyphens. Field values have a limit of 1024 UTF8 bytes. Navigation URLs contain 1 to 2048 UTF8 bytes and cannot contain control characters. `definitions()` provides JSON schemas and tool descriptions. `BROWSER_POLICY` marks page text and labels as untrusted data.

Decoded calls do not grant browser access or approve effects. The caller must validate destination URLs against its navigation policy, resolve handles against the latest observation, bind actions to the current document and obtain approval before dispatch. The crate does not execute browser commands.

Run `make check-browser-tools` from the repository root.

`activate_button` proposes a native click on a button handle from the latest observation. The user must approve the exact control. `button_click_observed` reports one trusted click observed on that control; it does not certify a site result or identify who supplied concurrent input. An uncertain result stops automatic continuation and is never replayed. Model callers cannot supply selectors, scripts or coordinates.

`select_option` proposes selecting an option handle from the latest observation. Its `handle` and `option` arguments are opaque bounded identifiers, not selectors or option values. The user must approve the exact control and choice. A local selection receipt reports only the observed selection; it does not certify a site save or submission.
