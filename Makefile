.PHONY: rust check-rust check-codex check-pi check-opencode check-browser-tools check-responses check-google check-messages check-chat check-credentials check-conversation check-panel panel-wasm check-companion companion

rust:
	npm run build:rust

check-rust:
	node scripts/build-rust.mjs --check

check-codex:
	cargo test --locked --manifest-path crates/metldr-codex-session/Cargo.toml

check-pi:
	cargo test --locked --manifest-path crates/metldr-pi-session/Cargo.toml

check-opencode:
	cargo test --locked --manifest-path crates/metldr-opencode-session/Cargo.toml

check-browser-tools:
	cargo test --locked --manifest-path crates/metldr-browser-tools/Cargo.toml

check-responses:
	cargo test --locked --manifest-path crates/metldr-responses/Cargo.toml

check-google:
	cargo test --locked --manifest-path crates/metldr-google-interactions/Cargo.toml

check-messages:
	cargo test --locked --manifest-path crates/metldr-messages/Cargo.toml

check-chat:
	cargo test --locked --manifest-path crates/metldr-chat-completions/Cargo.toml

check-credentials:
	cargo test --locked --manifest-path crates/metldr-native-credentials/Cargo.toml

check-conversation:
	cargo test --locked --manifest-path crates/metldr-conversation/Cargo.toml

WASM_BINDGEN ?= wasm-bindgen
PANEL_TARGET_DIR ?= crates/metldr-panel/target

check-panel:
	cargo test --locked --manifest-path crates/metldr-answer-codec/Cargo.toml
	cargo test --locked --manifest-path crates/metldr-panel/Cargo.toml

panel-wasm:
	cargo build --locked --release --manifest-path crates/metldr-panel/Cargo.toml --target wasm32-unknown-unknown --target-dir "$(PANEL_TARGET_DIR)"
	mkdir -p crates/metldr-panel/pkg
	$(WASM_BINDGEN) --target web --out-dir crates/metldr-panel/pkg "$(PANEL_TARGET_DIR)/wasm32-unknown-unknown/release/metldr_panel.wasm"

check-companion:
	cargo test --locked --manifest-path crates/metldr-companion/Cargo.toml

companion:
	cargo build --locked --release --manifest-path crates/metldr-companion/Cargo.toml --bin metldr-companion
