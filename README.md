# type-python

Rust workspace for the TypePython compiler, CLI, and LSP.

The Rust workspace rooted at `Cargo.toml` contains the current TypePython
implementation. The Python packaging layer exposes the requested PyPI package
name `type-python` and the top-level import/command surface `typepython`.

## Status

The current tree includes:

- Rust crates for config, diagnostics, syntax, lowering, binding, graph,
  checking, emit, incremental state, LSP, and CLI entrypoints
- `typepython init/check/build/watch/clean/lsp/verify/migrate` commands
- `.tpy` parsing, lowering, checking, build/verify/watch flows, and `.py` /
  `.pyi` emission support
- bundled typing/stdlib surfaces and Core v1 diagnostic coverage from
  `TypePython_Spec_v1.md`
- Python package bridging for `import typepython`, `python -m typepython`, and
  the packaged `typepython` console command
- an example TypePython project under `examples/hello-world`

## Getting Started

1. `./scripts/bootstrap-rust.sh`
2. `make ci`
3. `cargo run -p typepython-cli -- check --project examples/hello-world`

## Architecture

See `docs/architecture.md` for the crate map, milestone breakdown, and the
official Rust references used for the initialization choices.
