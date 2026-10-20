.PHONY: rust check-rust check-codex check-pi check-opencode check-browser-tools check-responses check-google check-messages check-chat check-credentials

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
