# Contributing

Thank you for your interest in contributing to TypePython! This guide covers the development setup, code conventions, testing, and PR workflow.

## Development Setup

### Prerequisites

- **Rust 1.94.0** (pinned in `rust-toolchain.toml`; workspace MSRV is 1.85)
- **Git**
- **Python 3.9+** (optional, for testing the Python package bridge)

### Quick Setup

```bash
git clone https://github.com/type-python/type-python.git
cd type-python

# Install the pinned Rust toolchain with clippy and rustfmt
./scripts/bootstrap-rust.sh

# Verify everything builds and passes
make ci
```

The `make ci` target runs: format check -> clippy lint -> fast workspace tests -> CLI verification tests -> downstream checker smoke -> bench compile check -> Python packaging validation.

### Building

```bash
# Debug build (fast compile, slow runtime)
cargo build --workspace

# Release build (slow compile, fast runtime)
cargo build --release -p typepython-cli

# Check compilation without producing binaries
cargo check --workspace

# Build a single crate (useful during development)
cargo build -p typepython-checking
```

### Running

```bash
# Run via cargo (debug)
cargo run -p typepython-cli -- check --project examples/hello-world

# Run via cargo (release, much faster for large projects)
cargo run --release -p typepython-cli -- build --project examples/hello-world

# Run the release binary directly
./target/release/typepython check --project examples/hello-world
```

## Project Structure

```
type-python/
  Cargo.toml                 # Workspace root (members, deps, lints, profiles)
  rust-toolchain.toml        # Pinned Rust 1.94.0
  rustfmt.toml               # Formatting config
  Makefile                   # Dev targets
  crates/
    typepython_diagnostics/  # Foundation: diagnostic model (no internal deps)
    typepython_config/       # Configuration loading
    typepython_syntax/       # Parser boundary (ruff-python)
    typepython_binding/      # Symbol extraction
    typepython_graph/        # Module graph
    typepython_checking/     # Type checker (largest test suite)
    typepython_lowering/     # TypePython -> Python lowering
    typepython_emit/         # Output generation
    typepython_project/      # Shared project discovery/support utilities
    typepython_incremental/  # Incremental build state
    typepython_lsp/          # Language server
    typepython_cli/          # CLI binary (depends on all crates)
  stdlib/                    # Bundled stdlib/type stub snapshot
  typepython/                # Python package bridge
  templates/                 # Project init templates
  examples/                  # Example projects
  docs/                      # Documentation
  scripts/                   # Dev scripts
```

## Crate Dependency Graph

The 12 crates form a layered architecture. Most crates own a single compilation phase; `typepython_project` is shared support code used by the CLI and LSP layers:

```
typepython_diagnostics          <-- Foundation (no internal deps)
  |
  +-- typepython_config         <-- Config loading (+ serde, toml, thiserror)
  |
  +-- typepython_syntax         <-- Parser (+ ruff-python-parser)
  |     |
  |     +-- typepython_binding  <-- Symbol extraction
  |     |     |
  |     |     +-- typepython_graph       <-- Module graph
  |     |     |
  |     |     +-- typepython_checking    <-- Type checker (+ config, graph, incremental)
  |     |
  |     +-- typepython_lowering <-- Python lowering
  |           |
  |           +-- typepython_emit        <-- Output generation (+ config, syntax)
  |
  +-- typepython_project        <-- Discovery/support helpers (+ config, emit, syntax)
  |
  +-- typepython_incremental    <-- Fingerprinting (+ graph, binding, syntax)
  |
  +-- typepython_lsp            <-- Language server (depends on most crates)
  |
  +-- typepython_cli            <-- Binary entrypoint (depends on ALL crates)
```

**Key rule:** dependencies flow downward. `typepython_diagnostics` is the foundation, `typepython_cli` is the top.

## Code Conventions

### Rust Style

- **Edition:** 2024
- **Formatting:** enforced via `cargo fmt` (config in `rustfmt.toml`)
  - Unix newlines
  - Field init shorthand
  - Max heuristics for line breaking
- **Lints:** workspace-level in `Cargo.toml`
  - `unsafe_code` -- **forbidden** (no unsafe code anywhere in the workspace)
  - `unwrap_used` -- **denied** (use `?`, `.ok()`, `.unwrap_or()`, or `expect()` with context)
  - `todo` -- **denied** (complete implementations before merging)
  - `dbg_macro` -- **denied** (use `tracing` for logging)
  - All clippy lints -- warned

### Crate Boundaries

Crates keep narrow ownership boundaries. Key rules:

- **No circular dependencies** between crates
- **typepython_diagnostics** is the only crate that every other crate may depend on
- **typepython_cli** depends on every other internal crate; **typepython_lsp** depends on most of them
- Keep public APIs minimal: expose only what downstream crates need
- Use `pub(crate)` for internal items

### Error Handling

- Use `thiserror` for typed error enums in library crates
- Use `anyhow` for ad-hoc errors in the CLI crate
- Return `Result` instead of panicking
- Diagnostics (type errors, warnings) go through `DiagnosticReport`, not `Result::Err`

### Naming Conventions

- Cargo package names: `typepython-{phase}`; Rust crate imports use `typepython_{phase}`
- Diagnostic codes: `TPY{category}{sequence}` (e.g., `TPY4001`)
- Source kinds: `TypePython`, `Python`, `Stub`
- Configuration keys: `snake_case` in TOML

## Testing

### Test Distribution

The counts below are a current snapshot from `cargo test -p <crate> -- --list`; they will drift as the workspace evolves.

| Crate                    | Tests    | Focus                                         |
| ------------------------ | -------- | --------------------------------------------- |
| `typepython_checking`    | 480      | Type checking rules, assignability, narrowing |
| `typepython_syntax`      | 106      | Parsing, metadata extraction, error recovery  |
| `typepython_cli`         | 105      | End-to-end pipeline, init, verify, watch      |
| `typepython_lowering`    | 65       | TypePython-to-Python lowering, source maps    |
| `typepython_lsp`         | 50       | LSP methods, diagnostics publishing           |
| `typepython_binding`     | 40       | Symbol extraction, declaration kinds          |
| `typepython_emit`        | 31       | Artifact planning, stub generation            |
| `typepython_config`      | 24       | Config discovery, validation, profiles        |
| `typepython_incremental` | 21       | Fingerprinting, snapshot diff                 |
| `typepython_diagnostics` | 16       | Diagnostic rendering and serialization        |
| `typepython_graph`       | 15       | Module graph, prelude injection               |
| `typepython_project`     | 13       | Project discovery, support-source utilities   |
| **Total**                | **~966** |                                               |

### Running Tests

```bash
# All tests
make test
# or
cargo test --workspace

# Fast workspace suite (skips the heavy CLI verification module)
make test-fast

# Heavy CLI verification suite only
make test-cli-verification

# Checker-backed end-to-end smoke suite (.tpy -> build -> mypy/pyright/ty)
make test-downstream-checkers

# Tests for a specific crate
cargo test -p typepython-checking

# A specific test by name
cargo test -p typepython-checking -- check_reports_missing_required_typed_dict_key

# With stdout visible
cargo test -p typepython-checking -- --nocapture

# Only tests matching a pattern
cargo test -p typepython-lowering -- sealed
```

### Writing Tests

Tests live alongside source code in each crate using standard Rust `#[cfg(test)]` modules. The typical pattern constructs in-memory TypePython source, runs it through the relevant pipeline stage, and asserts on the output.

**Checking test example** (tests type mismatch detection):

```rust
use std::path::PathBuf;

use typepython_binding::bind;
use typepython_checking::check;
use typepython_graph::build;
use typepython_syntax::{parse, SourceFile, SourceKind};

#[test]
fn test_type_mismatch_in_assignment() {
    let source = SourceFile {
        path: PathBuf::from("/project/src/app/__init__.tpy"),
        kind: SourceKind::TypePython,
        logical_module: "app".to_string(),
        text: r#"
x: int = "hello"
"#.to_string(),
    };
    let tree = parse(source);
    let table = bind(&tree);
    let graph = build(&[table]);
    let result = check(&graph);

    assert!(result.diagnostics.has_errors());
    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("str"));
    assert!(rendered.contains("int"));
}
```

**Lowering test example** (tests interface lowering):

```rust
use std::path::PathBuf;

use typepython_lowering::lower;
use typepython_syntax::{parse, SourceFile, SourceKind};

#[test]
fn test_interface_lowers_to_protocol() {
    let source = SourceFile {
        path: PathBuf::from("/project/src/app/__init__.tpy"),
        kind: SourceKind::TypePython,
        logical_module: "app".to_string(),
        text: r#"
interface Closeable:
    def close(self) -> None: ...
"#.to_string(),
    };
    let tree = parse(source);
    let lowered = lower(&tree);

    assert!(lowered.module.python_source.contains("Protocol"));
    assert!(lowered.module.python_source.contains("class Closeable"));
    assert!(lowered.module.python_source.contains("def close(self) -> None"));
}
```

**Key patterns:**

- Construct a `SourceFile` with inline `.tpy` source text
- Run through the pipeline stages needed for the test
- Assert on diagnostic codes (`TPY4001`), output text, or structural properties
- Use `has_errors()` to check for build-blocking diagnostics
- Use `as_text()` to get human-readable diagnostic output for assertion matching

### Integration Tests

The CLI crate (`typepython_cli`) contains end-to-end tests that exercise the full pipeline. These tests create temporary project directories with `typepython.toml` and `.tpy` source files, then run the full init/check/build/verify flow.

## Makefile Targets

| Target                            | Command                                                                                        | Description                           |
| --------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------- |
| `make bootstrap`                  | `./scripts/bootstrap-rust.sh`                                                                  | Install Rust toolchain                |
| `make fmt`                        | `cargo fmt --all`                                                                              | Format all code                       |
| `make fmt-check`                  | `cargo fmt --all --check`                                                                      | Check formatting (CI)                 |
| `make check`                      | `cargo check --workspace`                                                                      | Check compilation                     |
| `make lint`                       | `cargo clippy --workspace --all-targets -- -D warnings`                                        | Lint with clippy                      |
| `make test`                       | `cargo test --workspace`                                                                       | Run all tests                         |
| `make docs`                       | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`                                   | Generate rustdoc                      |
| `make package-check`              | `python3 -m build --sdist --wheel` + `python3 -m twine check dist/*`                           | Validate Python package artifacts     |
| `make bump-version VERSION=0.0.8` | `python3 scripts/bump_version.py 0.0.8`                                                        | Sync Rust and Python package versions |
| `make ci`                         | `fmt-check` + `lint` + `test-fast` + `test-cli-verification` + `bench-check` + `package-check` | Repository validation target          |

## Python Packaging

The Python package (`type-python` on PyPI, `import typepython`) is a thin bridge that locates and invokes the compiled Rust CLI binary. Understanding this is useful when testing the pip-installable workflow.

### Build flow

1. A wheel build or non-editable install (`python -m build --wheel`, `python -m pip install .`) triggers the custom `build_py` path in `setup.py`
2. That path runs `cargo build --release -p typepython-cli`
3. The built binary is copied to `typepython/bin/typepython` (or `.exe` on Windows)
4. Permissions are set to 0o755
5. `bdist_wheel` is marked as non-pure and tagged as `py3-none-<platform>`

Editable installs (`python -m pip install -e .`) are different: they install the Python bridge from the checkout, but they do not guarantee a bundled CLI binary. In a repository checkout, the bridge usually falls back to `cargo run`.

### Binary resolution order

When `typepython` or `python -m typepython` is invoked:

1. `TYPEPYTHON_BIN` environment variable (if set)
2. Bundled binary at `typepython/bin/typepython` (wheel deployment)
3. `cargo run --manifest-path <repo>/Cargo.toml -p typepython-cli --` (development from a repo checkout)
4. `RuntimeError` if none available

During development, option 3 means you can run `python -m typepython check --project .` without rebuilding the wheel -- cargo compiles on the fly.

### Release hygiene

- Build release artifacts from a clean checkout. The source distribution uses `MANIFEST.in` with `graft` rules over the Rust workspace and bundled stdlib snapshot, so untracked files under packaged directories can be swept into a locally-built sdist.
- Use `make bump-version VERSION=X.Y.Z` for version updates. This synchronizes `Cargo.toml`, `Cargo.lock`, `pyproject.toml`, and `typepython/__init__.py` in one step.
- Validate both artifacts before publishing: `python -m build --sdist --wheel` and `python -m twine check dist/*`.
- If you intend `pip install type-python` to work without a Rust toolchain, publish platform wheels for each supported target in addition to the sdist. The release workflow uses `cibuildwheel` to publish Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64 wheels.

### Publishing to PyPI

The repository publishes to PyPI through GitHub Actions Trusted Publishing in the `pypi` environment. No PyPI API token is required in repository secrets or workflow `env`.

1. Choose the release version `X.Y.Z`.
2. Run `make bump-version VERSION=X.Y.Z`.
3. Optionally verify the sync guard with `cargo test -p typepython-cli packaged_versions_stay_in_sync`.
4. Commit the version bump and push it to GitHub.
5. Create a GitHub release from a tag named `vX.Y.Z`, where `X.Y.Z` exactly matches `pyproject.toml`.
6. The `publish` workflow validates the tag/version match, builds the sdist, builds wheel artifacts with `cibuildwheel`, smoke-tests each wheel with a Quick Start install/build flow, runs `twine check`, and then publishes to PyPI.

Linux wheel publishing uses the `manylinux2014` image through `cibuildwheel`, which gives PyPI-compatible Linux wheel tags for the bundled CLI binary. If you expand wheel coverage to more architectures later, keep that manylinux or musllinux compatibility requirement in place.

## Pull Request Workflow

### Before opening a PR

1. **Format**: `make fmt`
2. **Lint**: `make lint` (fix all warnings)
3. **Test**: `make test` (all tests pass)
4. **Full CI**: `make ci` (all steps pass)

### PR Guidelines

- Keep PRs focused: one feature, bug fix, or refactor per PR
- Write a clear title and description explaining **what** and **why**
- Reference relevant issues
- Add tests for new functionality
- Update documentation if behavior changes

### CI Pipeline

GitHub Actions runs on every push to `main` and every PR:

1. **workspace-fast** -- format, lint, `cargo test --workspace -- --skip tests::verification::`, bench compile check, downstream checker smoke, and Python package validation
2. **cli-verification** -- `cargo test -p typepython-cli tests::verification::`

Together these jobs cover the repository validation path that `make ci` approximates locally.

The downstream checker smoke suite currently covers:

- `basic-package` on Python 3.10
- `rich-package` on Python 3.10 and 3.12
- `compat-package` on Python 3.10, 3.11, and 3.12, with emitted import-source assertions before mypy / pyright / ty
- `native-package` on Python 3.13+, with emitted native typing syntax assertions and publication-metadata checks
- Parser and bundled stdlib baselines are recorded in `stdlib/BASELINE.toml`; keep that file in sync when touching parser dependency versions or refreshing the bundled stdlib snapshot

For focused local runs, `scripts/downstream_checker_smoke.py` accepts:

- `TYPEPYTHON_DOWNSTREAM_FIXTURES=compat-package`
- `TYPEPYTHON_DOWNSTREAM_CHECKERS=pyright,ty`

## Common Contribution Tasks

### Adding a Diagnostic

To add a new diagnostic code:

1. Choose a code in the appropriate range:
   - `TPY1xxx` -- configuration
   - `TPY2xxx` -- parsing/lowering
   - `TPY3xxx` -- imports/modules
   - `TPY4xxx` -- type checking
   - `TPY5xxx` -- emit
   - `TPY6xxx` -- infrastructure

2. Create the diagnostic using the builder from `typepython_diagnostics`:

```rust
Diagnostic::error("TPY4XXX", "Description of the issue")
    .with_span(Span::new(path, line, column, end_line, end_column))
    .with_note("Additional context")
    .with_suggestion(DiagnosticSuggestion {
        message: "Suggested fix".to_string(),
        span: replacement_span,
        replacement: "fixed code".to_string(),
    })
```

3. Add the check logic in the appropriate crate
4. Add a test case verifying the diagnostic fires (and does not false-positive)
5. Document the code in `docs/diagnostics.md`

### Adding a Type Checking Rule

Type checking rules live in the `typepython_checking` crate (with the main orchestration in `src/lib.rs` and supporting logic split across focused modules). Each rule contributes diagnostics over the module graph and related semantic caches.

1. Define the check function:

```rust
fn my_new_check_diagnostics(graph: &ModuleGraph, /* options */) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for node in &graph.nodes {
        // Inspect declarations, calls, returns, etc.
        // Push diagnostics when violations are found
    }
    diagnostics
}
```

2. Call it from the `check_with_options()` function
3. Add test cases (this crate has the largest test suite in the workspace -- follow existing patterns)
4. Document any new diagnostic codes

### Adding a CLI Command

1. Add the command variant/args to `typepython_cli/src/cli.rs`
2. Add clap derive attributes for flags and arguments
3. Implement the handler function
4. Wire it into the command dispatch in `typepython_cli/src/main.rs`
5. Add integration tests
6. Document in `docs/cli-reference.md`

### Modifying the Lowering

Lowering transformations live in the `typepython_lowering` crate (primarily `src/core.rs` plus helper modules such as `typeddict.rs`). When modifying how TypePython constructs lower to Python:

1. Update the lowering logic in the `lower()` or `lower_with_options()` path
2. Ensure source maps are maintained (every output line should map back to a source line)
3. Test that the lowered output is valid Python
4. If the change affects `.pyi` stubs, verify interoperability (see `docs/interop.md`)

### Adding a Syntax Extension

New syntax goes through multiple crates:

1. **typepython_syntax** -- parse the new construct into `SyntaxStatement`
2. **typepython_binding** -- extract declarations/calls/metadata from it
3. **typepython_checking** -- add type checking rules
4. **typepython_lowering** -- define how it lowers to Python
5. **typepython_emit** -- handle it in stub generation
6. Update documentation: `syntax-guide.md`, `type-system.md`, `interop.md`

## Useful Resources

- [Cargo Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/)
- [rustfmt Config](https://rust-lang.github.io/rustfmt/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html)
- [TypePython Language Specification](spec/language-spec-v1.md)
