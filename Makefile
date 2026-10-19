.PHONY: rust check-rust check-codex check-pi check-browser-tools

rust:
	npm run build:rust

check-rust:
	node scripts/build-rust.mjs --check

check-codex:
	cargo test --locked --manifest-path crates/metldr-codex-session/Cargo.toml

check-pi:
	cargo test --locked --manifest-path crates/metldr-pi-session/Cargo.toml

check-browser-tools:
	cargo test --locked --manifest-path crates/metldr-browser-tools/Cargo.toml
