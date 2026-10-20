# Extension asset preparation

From the repository root, run `cargo run --locked --release --manifest-path crates/metldr-extension-assets/Cargo.toml`. This prepares the 13 pinned font subsets and four dependency notices in `public/rust-panel/assets`. Each HTTPS response is limited to 256 KiB, checked against its accepted size and SHA-256, then atomically installed. Valid cached files are reused. Scratch files live in `.tmp/rust-panel-assets`.
