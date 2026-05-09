# Core v1 Beta Readiness

TypePython's Beta claim is intentionally narrow: **Core v1 Beta**. The package may include Supported
DX, Experimental opt-in, and Roadmap / prototype capabilities, but those surfaces are not part of the
Beta compatibility promise until they are promoted explicitly. The canonical status vocabulary lives
in [TypePython Feature Status](feature-status.md). Shipped-but-unstable feature gates are tracked in
the [Experimental Feature Registry](experimental-features.md). Editor and workflow maturity is
tracked separately in [DX and LSP Stability](dx-stability.md).

## Stable Core v1 During Beta

The following surfaces are compatibility-stable for the Core v1 Beta line:

- `.tpy` Core syntax documented in the language spec.
- Core checker semantics for TypePython-checked author packages, including same-module `sealed`
  class closure, sealed-match exhaustiveness, `unknown` narrowing requirements, `unsafe:` fences,
  and supported `TypedDict` transforms.
- `typepython.toml` Core configuration fields for project discovery, resolution, typing, and emit
  behavior used by `init`, `check`, `build`, `clean`, and `verify`.
- CLI commands: `init`, `check`, `build`, `clean`, and `verify`.
- Diagnostic code identity: a `TPYxxxx` code keeps its meaning within the Beta line, though wording
  may improve.
- Emitted `.py` / `.pyi` compatibility contract: generated artifacts remain standard Python typing
  surfaces with no mandatory TypePython runtime dependency.

The Beta line deliberately separates these two promises: TypePython enforces stronger facts before
emit for the author package, while ordinary downstream consumers receive portable Python typing.
Extending sealed, `unknown`, `unsafe:`, or transform provenance semantics across package boundaries
requires an explicit TypePython-aware sidecar, checker plugin, or consumer mode; it is not part of
the default emitted-artifact compatibility promise.

## Core v1.0 RC Scope

The first v1.0 release candidate is scoped as **Core v1.0 RC**, not a full product-wide v1.0 RC.
It promotes the Stable Core v1 surfaces above from Beta trial status to release-candidate status
only when the final release commit passes the tracked release gate. The RC claim covers:

- Core source syntax, Core checker semantics, and Core `typepython.toml` fields.
- `init`, `check`, `build`, `clean`, and `verify` command behavior needed for authored-package
  checking and standard artifact publication.
- Diagnostic code identity and documented JSON diagnostic shape for Core commands.
- Emitted `.py` and `.pyi` portability, including the no-mandatory-TypePython-runtime boundary.
- Bundled stdlib and target-version compatibility for Python 3.10 through 3.14.

The Core v1.0 RC claim deliberately excludes the Supported DX, Experimental opt-in, and Roadmap /
prototype tiers. Those features may ship in the same package, but release notes must describe them
with their tier labels and must not imply that editor UX, watch behavior, migration heuristics,
framework adapters, or research slices are stable v1.0 surfaces.

## Full v1.0 / DX Promotion

A later full product-wide v1.0 claim can include the editor and workflow surfaces only after the
[DX and LSP Stability](dx-stability.md) gate is complete. Until then, a release may be a Core v1.0
RC while `typepython lsp`, VS Code packaging, `watch`, `compat`, `api-diff`, `type-health`, and
`migrate` remain Supported DX, non-stable.

## Supported DX, Non-Stable

The following features can be useful in Beta builds, but their UX, schemas, or heuristics may change
without a Beta-line compatibility guarantee:

- LSP UX details, command names, and editor affordances.
- LSP capability shape, formatter behavior, code-action details, and editor extension packaging
  until promoted by the DX v1.0 gate.
- Migration heuristics, type-budget scoring, and adoption dashboard details.
- Cache internal schema, support indexes, and incremental snapshot layout.
- `watch`, `compat`, `api-diff`, and `type-health` UX and report details.

## Experimental Opt-In

The following features are outside Core v1 conformance and must require explicit opt-in before they
can affect an ordinary project. Their feature ids, current gates, and promotion requirements are
canonical in the [Experimental Feature Registry](experimental-features.md):

- Runtime validators and boundary-validator adapter delegation.
- Conditional returns, pass-through `.py` inference, sync/async dual emit, and other Experimental v1
  work.
- Shape projections beyond `TypedDict`.

## Roadmap / Prototype

The following surfaces are not compatibility claims and must not appear as completed advantages in
the main mypy / pyright / PEP 695 comparison table:

- Framework adapter manifest SDK and prototype adapter metadata.
- Author-time semantic research slices: effect/capability rows beyond the stable `unsafe:` fence,
  restricted type-level evaluator forms, taint facts, and validator witnesses.
- Notebook ingestion and other future workflow experiments.

## Release gate

A Beta release candidate must pass the tracked release gate before the classifier is kept at Beta:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- CLI verification suite: `cargo test -p typepython-cli tests::verification::`
- downstream checker smoke: `python scripts/downstream_checker_smoke.py` with mypy strict,
  pyright strict, basedpyright strict, and ty strict
- roadmap demo smoke: `python scripts/research_roadmap_demo_smoke.py`, which checks and builds the
  P0-P4 author-time semantics example and asserts portable output
- fuzz smoke: `parser`, `type_expr`, and `lowering_stub`
- package build and metadata check: `python -m build --sdist --wheel` and `python -m twine check dist/*`
- installed wheel quickstart smoke, including `typepython --help`, `init`, `check`, `build`, and
  `verify`, proving the installed wheel uses its bundled Rust CLI without `cargo`
- Python 3.9 wrapper smoke
- Python 3.13 and 3.14 target smoke
- macOS and Windows platform smoke
- industrial performance evidence: `python scripts/industrial_perf_smoke.py` on the release
  candidate, recording cold check, warm check, single-file implementation edit, public surface edit,
  and peak RSS; plus the 512-module `typepython_lsp` incremental Criterion suite with p95/p99
  hover-session latency evidence
- experimental scope contract: `python3 -m unittest scripts.test_repo_contracts.RepoContractsTests.test_experimental_scope_is_guarded`
  so Experimental opt-in and Roadmap / prototype features cannot drift into Core v1 claims silently

The GitHub `rust` workflow has the authoritative `beta-release-gate` job and depends on these
families, including interpreter- and platform-specific smoke jobs that require GitHub-hosted Python
3.13/3.14, macOS, Windows runners, and the industrial performance smoke artifact. The local
`make beta-release-gate` target is the portable aggregate for maintainers with the required
checker/fuzz/package/performance tooling installed; it is a preflight for the cross-platform CI
gate, not a replacement for the CI-only interpreter, platform, and uploaded-artifact matrix. The
PyPI publish workflow refuses to publish unless the release commit already has a successful `rust`
workflow run containing a successful `beta-release-gate` job.

Local release-gate evidence and CI follow-up requirements are recorded in
[Release Evidence](release-evidence.md).

## Packaging and install contract

- Published wheels cover the platforms claimed in the README: Windows AMD64, macOS x86_64, macOS
  arm64, and Linux x86_64.
- Source distribution fallback is supported but requires Rust and `cargo` because the Rust CLI binary
  must be built locally.
- Wheels are platform-specific `py3-none-<platform>` artifacts: they are not tied to a CPython ABI,
  but they do include the Rust CLI for the target platform.
- `pip install type-python && typepython --help` is the supported entry path for users.
- The quickstart smoke creates a clean project and runs `check`, `build`, and `verify` before release.

## Checker interoperability baseline

The downstream checker matrix is the Beta baseline for emitted artifacts. It covers basic and rich
packages, standard typing/TypedDict transforms, Python 3.10 through 3.14 target lowering, Pydantic-like
shapes, FastAPI-like routes, task decorator transforms, native/compat emit styles, sync/async dual
emit, typeshed-heavy imports, implicit namespace packages, PEP 561 typed-package and partial-stub
metadata, attrs/SQLAlchemy/Django-style framework patterns, TypedDict-heavy SDK clients,
overload-heavy APIs, and negative consumer cases.

The matrix carries an `ecosystem_corpus.baseline_categories` map so the v1 corpus obligation stays
machine-checkable. The categories cover attrs/dataclass-heavy code, Pydantic v2-style models,
FastAPI route/dependency structures, SQLAlchemy-style mapped attributes, Protocol/ParamSpec-heavy
APIs, TypedDict-heavy SDK clients, namespace packages, partial stubs, large `py.typed` package
surfaces, and overload-heavy APIs.

Known checker disagreements must stay explicit in `test-fixtures/downstream-checkers/matrix.json`
with `allowlist_reason` and a non-expired `allowlist_expires` date. The smoke runner rejects expired
allowlists before invoking external checkers so release candidates cannot silently carry stale
checker disagreements.

## Industrial performance baseline

The repository distinguishes micro-benchmark coverage from industrial-scale evidence. The
parse/lower/graph/checker Criterion suites are useful regression sentinels, but they are not enough
to claim monorepo maturity. Before v1.0, release notes must include a checked artifact from
`scripts/industrial_perf_smoke.py` and the 512-module LSP incremental bench.

The minimum recorded fields are cold check time, warm check time, single-file implementation edit
latency, public surface edit latency, peak RSS, target Python version, module count, external stub
package count, and the TypePython command used. If a platform cannot report RSS or p95/p99, the
release note must say so explicitly instead of implying coverage.
