<p align="center">
  <img src="public/assets/logo/metldr-dark-lavender.svg" width="250" alt="metldr">
</p>

<p align="center">
  <a href="https://chromewebstore.google.com/detail/kbfdmfgmmibkdnbfdaeganjckfgbfjlh">
    <img src="https://img.shields.io/chrome-web-store/v/kbfdmfgmmibkdnbfdaeganjckfgbfjlh?color=4285F4&style=flat-square&logo=google-chrome&logoColor=white" alt="version">
  </a>
  <a href="https://chromewebstore.google.com/detail/kbfdmfgmmibkdnbfdaeganjckfgbfjlh">
    <img src="https://img.shields.io/chrome-web-store/users/kbfdmfgmmibkdnbfdaeganjckfgbfjlh?color=4285F4&style=flat-square&logo=google-chrome&logoColor=white" alt="users">
  </a>
</p>

> **metldr** is a Chrome extension for page and email summaries, replies, PDF questions, dictionary lookups and browser tasks. Use local models, your configured provider, or supported installed agents.

## install

- **chrome web store**: https://chromewebstore.google.com/detail/kbfdmfgmmibkdnbfdaeganjckfgbfjlh

## features

- **email intelligence**: automatically summarises threads, extracts key facts (dates, amounts), and generates context aware reply suggestions in gmail.
- **page intelligence**: detects article content, provides bulleted summaries, and allows you to chat with any webpage.
- **pdf intelligence**: full featured pdf reader with summarisation, q&a (rag), and local file support.
- **instant definitions**: three tier dictionary lookup (local > api > llm) for instant word definitions.
- **model choice**: use Chrome AI or Ollama in the existing page tools, or configure a provider or installed agent in the Rust browser panel.

## usage

### setting up ai providers

#### option 1: chrome built in ai (recommended)

metldr uses chrome's built in gemini nano model by default.

1. ensure you are on chrome version 138+
2. enable "prompt api for gemini nano" in `chrome://flags`
3. the extension will automatically detect availability

#### option 2: ollama (for power users)

run larger models (llama 3, qwen, etc.) locally on your machine.

1. install [ollama](https://ollama.com)
2. run a model: `ollama run llama3.2`
3. go to metldr settings > ai provider > select "ollama"
4. ensure ollama is running at `http://127.0.0.1:11434`

## development

### Rust core

The Rust core runs through WebAssembly. It decodes Ollama chat streams while preserving text spacing and handling thinking tags across fragments. Invalid records and responses that end before a completion record report an error. Each stream record is limited to 256 KiB.

Retrieval text chunking also runs in the Rust core. It keeps the existing email, article and PDF window sizes and overlap. Offsets count UTF16 units in normalized text, and chunks keep Unicode characters intact at their boundaries.

The welcome page runs in Rust through WebAssembly, with static Help available while it loads. It checks Chrome AI or Ollama only when requested, saves setup before closing, and shows the extension's exact origin in Ollama setup commands. Its former Vue entry is removed. The existing page tools still use Vue. The Rust browser agent is also included in the same extension.

The [Rust conversation model](crates/metldr-conversation/README.md) preserves assistant message identities and observed tool order. It validates saved content and text projections, with explicit states for streaming, final snapshots and interruption. Run `make check-conversation` to check it.

The [Rust panel module](crates/metldr-panel/README.md) supplies the replacement panel logic and background message handling. Run `make check-panel` for its checks and `make panel-wasm` for its generated browser module.

The [Rust companion](crates/metldr-companion/README.md) runs provider requests, installed agents and browser tasks through native messaging. Run `make check-companion` for its checks and `make companion` for the executable. Register the companion separately for your loaded extension ID before using the Rust agent. Building the extension does not install a native host.

Install Rust and the matching binding generator before building the extension. The repository pins Rust 1.99.1.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
npm ci
```

Use the mise managed Portless CLI for local previews. `npm run dev` builds Rust and starts Portless with the project name `metldr`. Open the URL it prints, then add `/welcome.html` for welcome, `/panel.html` for the Rust agent layout or `/index.html` for the current page tools. Chrome extension APIs require the loaded extension, so a web preview does not establish working AI or browser controls.

To check the extension, build it and load the `dist` directory with Load unpacked at `chrome://extensions`. The toolbar icon initially opens the current page tools. Press Alt+Shift+M for the Rust browser agent or Ctrl+Shift+Y for the page tools. Change shortcuts at `chrome://extensions/shortcuts` if Chrome reports a conflict. Open welcome from the panel, or copy the extension ID from Chrome's extensions page and open `chrome-extension://YOUR_EXTENSION_ID/welcome.html`. After another build, reload the extension from Chrome's extensions page before checking changes.

The local embedding model loads when a retrieval request needs embeddings. Opening the browser agent does not prewarm that model.

The development server and the main and content builds prepare the Rust core, welcome and panel bindings, welcome generator and pinned panel assets automatically. After editing Rust during a development session, run `make rust` again. The open welcome web preview reloads when its styles or compiled Rust assets change. An unpacked extension still needs a reload from Chrome's extensions page after rebuilding.

The welcome HTML template is rendered by the Rust generator in both Vite development and production builds. Generated bindings and Cargo build output are local build products. Private working folders are excluded from development page reloads and file serving. Tailwind scans the application source directory.

```sh
make rust
make check-rust
cargo test --locked --manifest-path crates/metldr-core/Cargo.toml
cargo test --locked --manifest-path crates/metldr-welcome/Cargo.toml
```

### commands

| command                   | description                                |
| :------------------------ | :----------------------------------------- |
| `npm run dev`             | start development server (hmr for ui)      |
| `npm run build`           | build all targets (main, content, sandbox) |
| `npm run download:models` | download required ai models to public/     |

## privacy

Local models process content on your machine. Provider and installed agent requests can send selected page content and instructions to the configured provider. The Rust companion keeps provider credentials outside page scripts and retains local run state for recovery. Review the selected destination before sending content. External providers can charge for their service.

The extension does not require a MetLDR subscription. Local operation depends on the installed model, hardware and task. The build downloads dependencies and pinned assets when they are not already available.

---

_built with ❤️ by efaz_
