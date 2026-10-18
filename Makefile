.PHONY: rust check-rust

rust:
	npm run build:rust

check-rust:
	node scripts/build-rust.mjs --check
