# Development

## Install and run

Install Node.js, the mise managed Portless command, and the pinned Rust toolchain. The repository uses Rust 1.99.1 and wasm-bindgen-cli 0.2.128. Add the WebAssembly target and install the matching binding generator, then install JavaScript packages:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
npm ci
```

Run `npm run dev` to build the Rust modules and start the named metldr preview through Portless. Open the URL printed by Portless. Add `/welcome.html`, `/panel.html`, or `/index.html` to view the welcome page, browser agent, or current page tools. A web preview does not have Chrome extension access.

Run `npm run build` to create the extension in `dist`. In Chrome, open `chrome://extensions`, enable Developer mode, choose **Load unpacked**, and select `dist`. After another build, reload the extension from the same page.

The toolbar opens the current page tools. Press Alt+Shift+M to open the browser agent. Press Ctrl+Shift+Y to open the page tools. Change either shortcut at `chrome://extensions/shortcuts` if Chrome reports a conflict.

## Native companion

Provider and installed agent tasks need the native companion, built and registered separately for the extension ID. Linux is the verified native platform. Native installation on Windows and macOS is not verified. Follow the [companion setup guide](../crates/metldr-companion/README.md) to build and register it.

## Local AI setup

Open Help on the welcome page for Chrome AI and Ollama setup. Its Ollama guide includes the extension origin and platform commands. Check reads availability without sending a prompt.

## Checks

Run `make check-rust`, `make check-panel` or `make check-companion` for the affected code. Run `make rust` after Rust changes during a preview session. See the [panel guide](../crates/metldr-panel/README.md) for browser setup and module details.
