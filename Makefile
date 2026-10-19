.PHONY: rust check-rust check-codex

rust:
	npm run build:rust

check-rust:
	node scripts/build-rust.mjs --check

check-codex:
	cargo test --locked --manifest-path crates/metldr-codex-session/Cargo.toml
