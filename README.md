# type-python

Rust workspace for the TypePython compiler, CLI, and future LSP.

The existing `pyproject.toml` keeps the PyPI name reservation; the compiler
implementation itself now lives in the Rust workspace rooted at `Cargo.toml`.

## Status

Milestone 0 is initialized:

- pinned Rust toolchain and workspace-level lint/format/test conventions
- separate crates for config, diagnostics, syntax, lowering, binding, graph,
  checking, emit, incremental state, LSP, and CLI entrypoints
- `typepython init/check/build/watch/clean/lsp/verify/migrate` command skeletons
- example TypePython project under `examples/hello-world`

## Getting Started

1. `./scripts/bootstrap-rust.sh`
2. `make ci`
3. `cargo run -p typepython-cli -- check --project examples/hello-world`

## Architecture

See `docs/architecture.md` for the crate map, milestone breakdown, and the
official Rust references used for the initialization choices.
