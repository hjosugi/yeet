# Contributing to Yeet

Yeet uses stable Rust, GTK 4 and platform-native drag-and-drop backends. Keep
changes portable: Linux code must continue to build on Wayland and the fallback
window backend, and Windows code is verified by the MSYS2 job in CI.

Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

Nix packaging changes should also pass `nix flake check`; Arch packaging changes
must regenerate both checked-in `.SRCINFO` files. User-visible English strings
belong in `src/i18n.rs` with a Japanese translation and are covered by the
translation-key unit test.

An optional pre-commit hook is included. Enable it for this checkout with:

```sh
git config core.hooksPath .githooks
```

Platform behavior that cannot be automated belongs in the compositor or
Windows manual test matrix. Do not mark those checks complete from a nested or
headless smoke test.
