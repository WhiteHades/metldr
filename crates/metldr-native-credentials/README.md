# metldr native credentials

This Rust crate stores provider keys for the native companion in the operating
system credential store. The caller owns provider selection and recovery of
interrupted settings changes.

`CredentialId::generate` creates a random scope or revision identifier.
`Vault::put`, `read` and `remove` operate on a revision within that scope.
`Key::parse` accepts 1 to 4096 ASCII graphic bytes. `Key` clears its owned
buffer when dropped and exposes its value only through `expose`.

Hold one process lock for the scope throughout writes and cleanup. The crate
serializes writes inside one process. Use a fresh revision identifier when a
key changes, and journal that identifier before writing. Existing identical
values are accepted; conflicting values return an error. Deletion is
idempotent. An interrupted partial write needs deliberate recovery by its caller.

Values use one or two bounded records. The head is written last and removed
first. Resolve a key before admitting work and keep that resolved value with
the run. Removing its vault record does not erase values already read into memory.

Linux uses an encrypted Secret Service session and requires an existing,
unlocked, persistent default collection. Missing or locked storage returns
an error. Windows uses Credential Manager. macOS uses Keychain.
The Windows and macOS backends still need runtime verification on those systems.

From the repository root, run `make check-credentials` for the ordinary checks.
Cargo remains available directly:

```sh
cargo fmt --manifest-path crates/metldr-native-credentials/Cargo.toml -- --check
cargo test --offline --locked --manifest-path crates/metldr-native-credentials/Cargo.toml
```

The ordinary checks validate inputs and record decoding without accessing a
credential store. The ignored Linux check requires the explicit owned fixture
marker and a separate Secret Service. It must never run against a personal store.
