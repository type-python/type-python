# Core v1 Beta Readiness

TypePython's Beta claim is intentionally narrow: **Core v1 Beta**. The package may include DX v1 and
Experimental v1 capabilities, but those surfaces are not part of the Beta compatibility promise until
they are promoted explicitly.

## Stable during Beta

The following surfaces are compatibility-stable for the Core v1 Beta line:

- `.tpy` Core syntax documented in the language spec.
- `typepython.toml` Core configuration fields for project discovery, resolution, typing, and emit
  behavior used by `init`, `check`, `build`, `clean`, and `verify`.
- CLI commands: `init`, `check`, `build`, `clean`, and `verify`.
- Diagnostic code identity: a `TPYxxxx` code keeps its meaning within the Beta line, though wording
  may improve.
- Emitted `.py` / `.pyi` compatibility contract: generated artifacts remain standard Python typing
  surfaces with no mandatory TypePython runtime dependency.

## Included but not compatibility-stable

The following features can be useful in Beta builds, but their UX, schemas, or heuristics may change
without a Beta-line compatibility guarantee:

- LSP UX details, command names, and editor affordances.
- Framework adapter manifest SDK and prototype adapter metadata.
- Runtime validators and boundary-validator adapter delegation.
- Migration heuristics, type-budget scoring, and adoption dashboard details.
- Cache internal schema, support indexes, and incremental snapshot layout.
- Conditional returns, pass-through inference, sync/async dual emit, and other Experimental v1 work.

## Release gate

A Beta release candidate must pass the tracked release gate before the classifier is kept at Beta:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- CLI verification suite: `cargo test -p typepython-cli tests::verification::`
- downstream checker smoke: `python scripts/downstream_checker_smoke.py` with mypy, pyright, and ty
- fuzz smoke: `parser`, `type_expr`, and `lowering_stub`
- package build and metadata check: `python -m build --sdist --wheel` and `python -m twine check dist/*`
- installed wheel quickstart smoke, including `typepython --help`, `init`, `check`, `build`, and `verify`
- Python 3.9 wrapper smoke
- Python 3.13 and 3.14 target smoke
- macOS and Windows platform smoke

The GitHub `rust` workflow has the authoritative `beta-release-gate` job and depends on these
families, including interpreter- and platform-specific smoke jobs that require GitHub-hosted Python
3.13/3.14, macOS, and Windows runners. `make beta-release-gate` is the portable local aggregate for
maintainers with the required checker/fuzz/package tooling installed; it is a preflight for the
cross-platform CI gate, not a replacement for the CI-only interpreter and platform matrix. The PyPI
publish workflow refuses to publish unless the release commit already has a successful `rust`
workflow run containing a successful `beta-release-gate` job.

## Packaging and install contract

- Published wheels cover the platforms claimed in the README: Windows AMD64, macOS x86_64, macOS
  arm64, and Linux x86_64.
- Source distribution fallback is supported but requires Rust and `cargo` because the Rust CLI binary
  must be built locally.
- `pip install type-python && typepython --help` is the supported entry path for users.
- The quickstart smoke creates a clean project and runs `check`, `build`, and `verify` before release.

## Checker interoperability baseline

The downstream checker matrix is the Beta baseline for emitted artifacts. It covers a library package,
standard typing/TypedDict transforms, Pydantic-like shapes, FastAPI-like routes, task decorator
transforms, native/compat emit styles, dual emit, and negative consumer cases. Known checker
disagreements must stay explicit in the matrix or allowlist with a reason and review horizon.
