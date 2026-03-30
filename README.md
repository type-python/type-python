# type-python

Rust workspace for the TypePython compiler, CLI, and LSP.

The Rust workspace rooted at `Cargo.toml` contains the current TypePython
implementation. The Python packaging layer exposes the requested PyPI package
name `type-python` and the top-level import/command surface `typepython`.

## Status

The current repository targets the following `TypePython_Spec_v1.md` tiers:

- **Core v1**: implemented in the Rust workspace and exercised by the default
  `typepython init/check/build/clean/verify` flows.
- **DX v1**: implemented for `watch`, `lsp`, `migrate --report`, and the
  current enhanced diagnostic/code-action set for missing `| None`, missing
  `@override`, non-exhaustive `match` arms, and `Pick`/`Omit` key corrections.
- **Experimental v1**: implemented behind explicit opt-in config only; ordinary
  builds keep these features disabled by default. The current experimental
  surface includes conditional return lowering, pass-through `.py` inference,
  and runtime validator emission for `data class` output. These opt in through
  config keys such as `typing.conditional_returns = true`,
  `typing.infer_passthrough = true`, and `emit.runtime_validators = true`.

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
