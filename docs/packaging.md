# Packaging Contract

TypePython's Python package is a thin launcher around the Rust CLI binary.

## Wheel Strategy

Release wheels are platform-specific because they bundle the Rust executable
under `typepython/bin/`. The Python wrapper itself is pure Python and does not
bind to a CPython ABI, so the build uses a CPython host in cibuildwheel but
overrides the final wheel tag to `py3-none-<platform>`.

Current policy:

- cibuildwheel builds on `cp312-*` hosts;
- `setup.py` marks the wheel as platform-specific;
- `bdist_wheel.get_tag()` returns `py3-none-<platform>`;
- supported release wheels must run `scripts/quickstart_smoke.py` without
  requiring `cargo` at runtime.

## Source Distribution

The source distribution builds the Rust CLI locally. Users installing from
source need:

- Rust 1.94.0, the workspace MSRV;
- `cargo` on `PATH`;
- a platform that can build the Rust workspace.

The build error must mention Rust 1.94.0 and `cargo` rather than failing with a
missing binary later.

## Runtime Launcher Resolution

The Python launcher resolves the CLI in this order:

1. `TYPEPYTHON_BIN=/path/to/typepython`;
2. bundled `typepython/bin/typepython` or `typepython/bin/typepython.exe`;
3. `cargo run --manifest-path <checkout>/Cargo.toml -p typepython-cli --` when
   running from a repository checkout.

If no command can be resolved, the error must distinguish an installed package
missing its bundled binary from a source checkout missing `cargo`.
