# Rust panel module

The crate implements the Chrome side panel and its background message handling in Rust and WebAssembly. It provides streamed results, retained assistant messages and tool activity, browser action decisions, provider and installed agent settings, cancellation and saved run restoration. Native work and saved run ownership stay with the paired companion. The root extension continues to select its existing panel while integration proceeds.

Run `make check-panel` to check the Rust module. Run `make panel-wasm` to build its browser bindings into `dist/rust-panel/pkg`. Both commands use the pinned Rust toolchain. The build also needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` version 0.2.128, matching the crate dependency. Set `WASM_BINDGEN` to the generator executable if it is outside your path. Set `PANEL_TARGET_DIR` to choose the Cargo build directory.

The generated `metldr_panel.js` exports `start_panel` and `accept_panel`. Initialize the generated module before either call. The panel document must be `panel.html` within the same extension, with an `app` element and the expected browser event bindings. The background passes connected extension ports to `accept_panel`. The module validates the sender and owns the native connection to `io.metldr.first_flow` using wire version 3. A paired, registered companion and the extension permissions are required for native execution.

The module uses the shared `metldr-conversation` model. Streaming updates retain message identities and observed tool order. Copy reads current validated output and reports when the copied result has changed. The Chrome background validates browser context and grants before browser commands. Provider credentials belong to the native companion rather than page scripts.

The output contains generated JavaScript bindings and WebAssembly, rather than a complete extension installation. Packaging, companion registration and the remaining interface integration are separate delivery steps.
