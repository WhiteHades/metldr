# Rune source

Source: https://github.com/anomalyco/rune
Revision: 781e2b6d87cd5f32d6f4365956fad4baaf24ee50

This directory keeps the Rune runtime, motion, staging, layout and optional profiling crates with their original relative paths. The workspace lists these crates and retains the upstream edition, license and lint settings. Dependency versions are owned by the consuming crate lockfile.

Local changes:

* The WebAssembly GPU instance cache uses thread local storage; native targets retain the upstream mutex.
* Browser field semantics share Rust paths and focus state with the existing text service bridge. Secure field values are omitted from the semantic mirror.
* Disabled fields cannot take focus through traversal or restoration, or retain offscreen editing focus.
* Text test fixtures use the identical font already bundled in the runtime.
* Focus and semantic mirror regressions cover these changes.

The MIT license and bundled Inter and IBM Plex Mono font licenses remain beside their source assets.

Line endings and trailing blank lines are normalized without changing source tokens or license wording.
