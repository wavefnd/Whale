# Contributing

Whale is split into the assembler, object, linker, and shared IR crates. Keep
changes focused on one component and add a regression test beside the affected
crate when behaviour changes.

## Build and test

Install Rust through rustup, then run the workspace checks:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo build --workspace
cargo test --workspace --all-features
```

The assembler's NASM comparison tests require `nasm` on `PATH`; mention its
version in the PR when those tests are run. Changes to the CLI should update
the matching page under `docs/cli`.

## Pull requests

Use one branch per issue, describe the observable change, and include the
commands that passed. Keep formatting-only changes separate from functional
patches.
