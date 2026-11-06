# Rune input checks

This consumer exercises named text fields, secure input, disabled focus, virtual fields, modal restoration and remounting with the pinned Rune source in [vendor/rune](../../vendor/rune/UPSTREAM.md).

Run the four native behavior checks from the repository root:

```sh
make check-rune
```

The browser entry uses generated WebAssembly bindings. Its mount function starts asynchronous GPU setup, and unmount requests asynchronous teardown. Wait for teardown to settle before checking field removal.

These checks cover the runtime and input bridge. The app frontend is a separate consumer.
