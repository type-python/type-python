# TypePython Strategic TODO

This document tracks the highest-leverage work for making TypePython useful enough that strict Python teams have a concrete reason to adopt it.

The priority model is:

- `P0`: Foundational or adoption-critical work for the first vertical slice. Keep this category small.
- `P1`: Strong differentiators and productization work that should follow once the P0 path is credible.
- `P2`: Valuable but higher-risk, narrower-audience, or sequencing-sensitive work.
- `Deferred`: Real problems, but likely to fragment the language or distract from the north star if started too early.

## Community Signals

- Python typing is widely adopted, but users still report friction around complex dynamic patterns, decorators, framework-generated attributes, inconsistent checker behavior, runtime validation, utility types, ADTs, and structural dictionary/model typing.
- `dataclass_transform` standardizes a narrow framework interop lane, but intentionally does not cover every framework behavior.
- Mypy plugins remain framework-specific, checker-specific, and difficult to maintain. SQLAlchemy has deprecated its mypy plugin, and Django/Pydantic still rely on plugin-like approaches for important behaviors.
- TypePython's unique advantage is not being another checker. Its advantage is owning the authoring source and generated `.py`/`.pyi` artifacts.

Useful external references:

- Meta 2025 Python Typing Survey: https://engineering.fb.com/2025/12/22/developer-tools/python-typing-survey-2025-code-quality-flexibility-typing-adoption/
- Meta 2024 Typed Python Survey: https://engineering.fb.com/2024/12/09/developer-tools/typed-python-2024-survey-meta/
- PEP 681 `dataclass_transform`: https://peps.python.org/pep-0681/
- PEP 561 typed package distribution: https://peps.python.org/pep-0561/
- PEP 728 `TypedDict` `closed` / `extra_items`: https://peps.python.org/pep-0728/
- Python 3.14 typing and deferred annotation behavior: https://docs.python.org/3.14/library/typing.html
- Pyrefly typing conformance comparison: https://pyrefly.org/blog/typing-conformance-comparison/
- Pyrefly third-party stub bundling: https://pyrefly.org/blog/stubs/
- Pydantic Pyrefly integration: https://pydantic.dev/docs/validation/latest/integrations/dev-tools/pyrefly/
- ty type-system documentation: https://docs.astral.sh/ty/features/type-system/
- SQLAlchemy mypy plugin status: https://docs.sqlalchemy.org/en/14/orm/extensions/mypy.html
- Pydantic mypy plugin docs: https://docs.pydantic.dev/1.10/mypy_plugin/
- django-stubs plugin docs: https://github.com/typeddjango/django-stubs

## North Star

Position TypePython as:

> Python's static shape compiler and standard artifact generator.

The product should let framework-heavy Python projects describe runtime-generated shapes once, then emit ordinary Python and checker-agnostic `.pyi` files that work across mypy, pyright, ty, IDEs, and packaging tools. Emerging checkers such as pyrefly should be tracked as compatibility signals until they are stable enough for default gates.

This is stronger than "better Python syntax" because it solves a known ecosystem gap: framework magic is not portable across type checkers.

The broader product identity should be:

> The typed Python engineering layer: shape compilation, artifact generation, portability audit, migration control, and publication verification.

The first execution loop should be narrow:

1. Describe a framework transform.
2. Compile it into ordinary `.py` and authoritative `.pyi`.
3. Validate the generated artifacts with downstream checkers.
4. Preserve runtime framework behavior.
5. Explain unsupported dynamic behavior deterministically.

Everything else in this document should either strengthen that loop or wait until it is credible.

## P0: Framework Shape and Decorator Transform System

### Goal

Make TypePython able to express framework-level transformations that today require mypy plugins, checker-specific builtins, or hand-written stubs.

This includes:

- class decorators that rewrite constructor/member shape
- base classes and metaclasses that synthesize fields or methods
- function decorators that replace a function with a non-function object
- runtime framework entities such as Pydantic models, Django models, SQLAlchemy declarative models, Celery tasks, Click/Typer commands, and FastAPI dependencies

### Why This Matters

Framework-heavy Python is where static typing currently hurts most. The community has repeatedly worked around this with checker plugins, but those plugins do not port cleanly to pyright, ty, PyCharm, or other tooling.

TypePython can generate the final `.pyi` surface itself, so the output can be consumed by all downstream tools without custom plugin support.

### Design TODO

- [x] Write an RFC for framework transform declarations.
- [x] Define terminology:
  - [x] runtime declaration
  - [x] static shape
  - [x] transform provider
  - [x] transform target
  - [x] generated member
  - [x] generated constructor
  - [x] replacement callable/object
  - [x] emitted stub authority
- [x] Decide syntax for declaring transform providers.
- [x] Decide whether transform declarations live in `.tpy`, sidecar `.tpyi`, config, or all three.
- [x] Define a minimal transform metadata model that can represent:
  - [x] field collection
  - [x] constructor generation
  - [x] alias handling
  - [x] required vs optional fields
  - [x] readonly/frozen fields
  - [x] descriptor-backed attributes
  - [x] method synthesis
  - [x] function-to-object replacement
  - [x] generic preservation via `ParamSpec`, `TypeVar`, and `TypeVarTuple`
- [x] Specify fallback behavior when transform metadata depends on runtime-only values.
- [x] Define strict-mode diagnostics for unsupported transform behavior.
- [x] Define non-strict degradation behavior.
- [x] Add a spec section after the existing decorator/dataclass-transform rules.
- [x] Update `docs/spec/implementation-notes-v1.md` Appendix J after the design lands.

### Compiler TODO

- [x] Extend syntax metadata collection for transform declarations and transform applications.
- [x] Extend binding summaries to include transform provider metadata.
- [x] Extend checker semantic facts to resolve transform providers across imports.
- [x] Extend checker class-shape resolution beyond `dataclass_transform`.
- [x] Extend checker callable resolution for non-callable decorator replacement.
- [x] Extend stub generation to emit the transformed public surface rather than the raw source declaration.
- [x] Extend source maps so diagnostics point to the original framework declaration site.
- [x] Extend LSP hover/signature help to show transformed constructors and transformed callables.
- [x] Add `verify` checks that transformed `.pyi` surfaces remain compatible with emitted runtime names.

### First Concrete Feature: Function-to-Object Decorators

Example target:

```python
decorator def celery_task[**P, R](fn: Callable[P, R]) -> Task[P, R]: ...
```

Tasks:

- [x] Support a decorator transform whose static result is not `Callable`.
- [x] Preserve original function parameters through `ParamSpec`.
- [x] Preserve return type in task methods such as `delay()` and `apply_async()`.
- [x] Emit `.py` preserving the original framework decorator.
- [x] Emit `.pyi` exposing the transformed object type.
- [x] Add tests for plain functions, methods, overloads, async functions, and generic functions.
- [x] Add downstream checker fixtures for mypy, pyright, and ty.

### First Concrete Feature: Class Shape Rewriters

Example target:

```python
@model
class User:
    id: int
    name: str
```

Tasks:

- [x] Generalize existing dataclass-transform shape collection.
- [x] Represent generated `__init__` parameters as explicit synthetic signatures.
- [x] Support field aliases and keyword-only fields.
- [x] Support fields excluded from `__init__`.
- [x] Support frozen/readonly field diagnostics.
- [x] Support generated class attributes such as managers, metadata, and validators.
- [x] Ensure generated `.pyi` is checker-neutral.

### Acceptance Criteria

- [x] A small framework fixture can define a transform once and type-check generated stubs with mypy, pyright, and ty.
- [x] A decorated function can become a typed non-callable object in `.pyi`.
- [x] A transformed class can synthesize a constructor and generated members.
- [x] Unsupported dynamic transform behavior produces deterministic diagnostics.
- [x] No external checker plugin is required for generated artifacts.

## P0: First-Class Record and Shape Model

### Goal

Create a shared internal model for "field-bearing things" so utility transforms and framework integrations can reuse one semantic representation.

Phase 1 is an internal IR, not a public promise that every Python type can participate in `Pick`, `Partial`, or structural projection.

### Why This Matters

The spec already limits `Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, and `Required_` to `TypedDict`. It explicitly says transforms over classes, interfaces, and protocols need a first-class record/shape model first.

### Shape Model TODO

- [x] Define `Shape` as an internal semantic object.
- [x] Define the phase-1 scope:
  - [x] `TypedDict`
  - [x] TypePython `data class`
  - [x] standard `@dataclass`
  - [x] `dataclass_transform`
  - [x] transformed framework classes
- [x] Explicitly defer public transforms over arbitrary classes, protocols, and interfaces until assignability rules are proven.
- [x] Include field metadata:
  - [x] field name
  - [x] public alias
  - [x] type
  - [x] required/optional
  - [x] readonly/mutable
  - [x] constructor participation
  - [x] default/default factory
  - [x] descriptor behavior
  - [x] source declaration span
- [x] Define phase-1 shape source behavior for:
  - [x] `TypedDict`
  - [x] `data class`
  - [x] standard `@dataclass`
  - [x] `dataclass_transform`
  - [x] transformed framework class
- [x] Document future shape source candidates without enabling them by default:
  - [x] `interface` / `Protocol`
  - [x] ordinary class with annotated instance fields
- [x] Define shape operations:
  - [x] `Partial`
  - [x] `Required_`
  - [x] `Readonly`
  - [x] `Mutable`
  - [x] `Pick`
  - [x] `Omit`
  - [x] shape composition
  - [x] shape projection
- [x] Define shape assignability.
- [x] Define how shape aliases lower into `.pyi`.
- [x] Define when a shape remains nominal and when it becomes structural.
- [x] Document which parts of the model are compiler-internal and which are user-visible.

### Implementation TODO

- [x] Add semantic shape structs in `typepython_checking`.
- [x] Move current `TypedDictShape` behavior toward shared shape primitives.
- [x] Keep existing TypedDict diagnostics stable during refactor.
- [x] Extend lowering transform expansion to consume shared shapes.
- [x] Emit stable names for generated shape aliases.
- [x] Add hover rendering for projected shapes.
- [x] Add diagnostics for unknown keys with suggestions.

### Acceptance Criteria

- [x] Existing TypedDict transform tests continue to pass.
- [x] `Pick` and `Partial` can operate on at least one non-TypedDict source behind an experimental flag.
- [x] Transformed class shapes can be reused by framework integrations.
- [x] Generated stubs remain standard Python typing.
- [x] No public "arbitrary type transform" guarantee is made before assignability semantics are stable.

## P0: Trustworthiness and Test Infrastructure

### Goal

Make TypePython credible as a compiler that users can place in a release pipeline.

### stdlib / typeshed TODO

- [x] Add an upstream typeshed commit pin.
- [x] Add a `scripts/refresh_stdlib_stubs.py` or equivalent sync tool.
- [x] Record generated diff statistics during refresh.
- [x] Validate `stdlib/VERSIONS` against refreshed files.
- [x] Add CI that fails when `stdlib/BASELINE.toml` is stale.
- [x] Document the refresh process in `docs/contributing.md`.
- [x] Preserve local TypePython-specific patches in a reproducible patch directory if needed.

### Coverage TODO

- [x] Add `cargo llvm-cov` or an equivalent coverage workflow.
- [x] Track coverage for:
  - [x] parser/syntax extraction
  - [x] binding
  - [x] checker semantic rules
  - [x] lowering
  - [x] emit/stub generation
  - [x] CLI verify
- [x] Publish coverage artifact in CI.
- [x] Establish minimum coverage thresholds only after baseline stabilization.

### Fuzzing TODO

- [x] Add `cargo-fuzz`.
- [x] Fuzz parser entrypoints.
- [x] Fuzz TypeExpr parsing.
- [x] Fuzz lowering on syntactically valid `.tpy` snippets.
- [x] Fuzz stub generation from lowered Python.
- [x] Fuzz TypedDict/shape transform composition.
- [x] Add corpus seeds from examples and test fixtures.
- [x] Run fuzz smoke in CI with short duration.
- [x] Run long fuzz in scheduled CI.

### Differential and External Checker TODO

- [x] Expand downstream checker smoke into a formal matrix.
- [x] Add fixtures where TypePython emits `.pyi`, then mypy/pyright/ty validate expected success.
- [x] Add negative fixtures where downstream checkers should reject intentionally bad consumer code.
- [x] Add differential tests for standard Python typing cases where TypePython should agree with mypy/pyright/ty.
- [x] Track known checker disagreements in a documented allowlist.
- [x] Add a `typepython verify --checker-preset all` convenience mode.

### Conformance TODO

- [x] Build a mapping from spec `MUST` rules to test names.
- [x] Add a generated conformance report.
- [x] Ensure every diagnostic code has positive and negative tests.
- [x] Split giant checker fixtures into thematic files where practical.
- [x] Keep insta snapshots limited to emission/golden output tests.

### Acceptance Criteria

- [x] stdlib stubs can be refreshed reproducibly from a pinned source.
- [x] Coverage and fuzz smoke run locally with documented commands.
- [x] CI has at least one checker-neutral downstream compatibility gate.
- [x] The conformance report identifies implemented, partial, and missing rule coverage.

## P0: Checker Portability and Compatibility Audit

### Goal

Make TypePython the tool that tells users whether their generated public type surface is portable across the major Python type checkers.

The required first matrix is mypy, pyright, and ty. Track pyrefly as experimental until its CLI behavior and conformance profile are stable enough for a default CI gate. basedpyright and zuban can be optional local checks.

### Why This Matters

Python's type checker ecosystem is fragmenting. Checkers differ in conformance, inference, narrowing, strictness defaults, library-specific behavior, and support for emerging features.

For application teams, this creates migration risk. For library authors, it creates publishing risk: a stub surface can look correct in one checker and fail, degrade to `Any`, or lose IDE value in another.

TypePython should not join the checker war. It should sit upstream and generate artifacts that are as checker-neutral as practical.

### TODO

- [x] Add a `typepython compat` command.
- [x] Support `typepython compat --checkers mypy,pyright,ty`.
- [x] Support optional `pyrefly`, `basedpyright`, and `zuban` checks when installed.
- [x] Run each configured checker against generated `.py` and `.pyi` artifacts.
- [x] Distinguish:
  - [x] TypePython compiler diagnostics
  - [x] downstream checker diagnostics
  - [x] checker disagreement
  - [x] known checker limitation
  - [x] invalid generated artifact
- [x] Produce machine-readable JSON output for CI.
- [x] Produce a human-readable portability report.
- [x] Add a checker disagreement allowlist with expiration dates and linked issues.
- [x] Add a `--strict-portability` mode that fails on any unallowlisted checker disagreement.
- [x] Add profiles:
  - [x] `library-portable`
  - [x] `app-strict`
  - [x] `pyright-first`
  - [x] `mypy-compatible`
  - [x] `experimental-checkers`
- [x] Detect non-portable constructs in emitted stubs before invoking external checkers.
- [x] Explain suggested rewrites for known portability problems.
- [x] Track a "type portability score" in reports.

### Acceptance Criteria

- [x] A package can run one command to validate emitted artifacts across mypy, pyright, and ty.
- [x] Experimental checker results can be reported without making CI flaky by default.
- [x] Known checker disagreements are visible, reproducible, and reviewable.
- [x] CI can fail on portability regressions without requiring every checker to behave identically.

## P0: Python 3.14+ Annotation Runtime Compatibility

### Goal

Make TypePython-generated artifacts safe and predictable for libraries that inspect annotations at runtime under Python 3.14+ deferred annotation semantics.

### Why This Matters

Frameworks such as FastAPI, Pydantic, dataclasses-like libraries, ORMs, serializers, dependency injection tools, and CLI frameworks often inspect annotations at runtime. Python 3.14 changes the default annotation evaluation model through deferred annotations and `annotationlib`.

Static correctness is not enough for TypePython's target users. Generated `.py` must also preserve framework runtime behavior across supported Python versions.

### TODO

- [x] Add an `annotations` compatibility audit pass.
- [x] Detect annotations that are safe statically but fragile at runtime.
- [x] Detect annotation consumers such as:
  - [x] `typing.get_type_hints`
  - [x] `inspect.get_annotations`
  - [x] `annotationlib.get_annotations`
  - [x] framework-specific model/route/command registration
- [x] Define target-version emit rules for annotations in generated `.py`.
- [x] Preserve enough runtime metadata for frameworks without forcing eager imports.
- [x] Add diagnostics for annotations that rely on local-scope names unavailable to runtime consumers.
- [x] Add diagnostics for circular imports caused only by runtime annotation evaluation.
- [x] Add guidance for when to emit string annotations, native annotations, or sidecar metadata.
- [x] Add test fixtures for Python 3.10 through 3.14 behavior.
- [x] Add runtime probes for representative Pydantic/FastAPI-like consumers.

### Acceptance Criteria

- [x] Generated `.py` behaves predictably under Python 3.14 deferred annotation semantics.
- [x] Runtime frameworks can consume TypePython-generated annotations without hidden import failures.
- [x] The compiler warns when a statically valid annotation would be unsafe for a configured runtime target.

## P1: Public API Surface Diff

### Goal

Let library authors answer one release-critical question: "Did this release change the public typing surface in a way users need to review?"

### Why This Matters

Typed Python packages can introduce type-breaking changes without breaking runtime tests. Changing a parameter from optional to required, modifying overload order, narrowing a return type incorrectly, changing `TypedDict` requiredness, or dropping `py.typed` can break downstream users even when the runtime still imports.

TypePython already treats emitted `.pyi` as authoritative. That makes it well-positioned to compare public type surfaces across releases.

The first version should be a conservative surface diff, not a complete semantic type-compatibility prover.

### TODO

- [x] Add `typepython api-diff <old> <new>`.
- [x] Support inputs:
  - [x] source directories
  - [x] generated build directories
  - [x] wheels
  - [x] sdists
  - [x] `.pyi` trees
- [x] Compare public modules, classes, functions, aliases, protocols, and `TypedDict` definitions.
- [x] Detect clear type-surface breaking changes:
  - [x] removed public symbol
  - [x] required parameter added
  - [x] parameter annotation changed
  - [x] return type widened to `Any` or `Unknown`
  - [x] return annotation changed
  - [x] overload order or coverage change
  - [x] generic parameter arity/default change
  - [x] `TypedDict` required/optional/readonly/closed/extra-items change
  - [x] protocol member removal or incompatible change
  - [x] `py.typed` or partial-stub metadata regression
- [x] Classify changes as:
  - [x] source-compatible
  - [x] likely type-compatible
  - [x] likely type-breaking
  - [x] runtime-breaking signal
  - [x] unknown risk
- [x] Defer full semantic compatibility checks until checker-backed diffing is designed.
- [x] Generate release-note snippets.
- [x] Add SemVer policy gates.
- [x] Integrate with `typepython verify` and publication workflow.

### Acceptance Criteria

- [x] A library maintainer can compare two artifacts and get a concrete type-compatibility report.
- [x] CI can block accidental type-breaking changes in minor or patch releases.
- [x] Reports are precise enough to review without reading raw generated stubs.

## P1: Typed Dependency and Stub Health Supply Chain

### Goal

Give users and library authors a clear view of whether their dependency graph has reliable type information.

### Why This Matters

Third-party library typing remains one of the most practical pain points in Python typing. PEP 561 defines `py.typed`, stub-only packages, partial stubs, and resolution order, but projects still run into missing stubs, stale stubs, version mismatches, partial coverage, and `Unknown` leakage.

Fast checkers and better syntax do not solve a bad type supply chain. TypePython can provide a package-level health layer around the artifacts it consumes and emits.

This is high-value productization work, but it should not block the first framework compiler vertical slice.

### TODO

- [x] Add a `typepython type-health` command.
- [x] Inspect installed dependencies for `py.typed`.
- [x] Detect stub-only packages following the `*-stubs` convention.
- [x] Detect partial stub packages.
- [x] Report runtime package version vs stub package version compatibility when metadata is available.
- [x] Detect common sources of poor typing:
  - [x] missing `py.typed`
  - [x] partial stubs with broad `Any`
  - [x] public functions returning `Any`
  - [x] public classes with untyped attributes
  - [x] overloaded APIs with fallback `Any`
  - [x] unsupported `typing_extensions` features for the configured target
- [x] Generate `.typepython/type-lock.toml` to record:
  - [x] Python target versions
  - [x] `typing_extensions` version
  - [x] typeshed commit
  - [x] stub package versions
  - [x] checker versions used for validation
- [x] Add a `--fail-under` option for type-health score.
- [x] Add a package maintainer mode that validates wheel/sdist type metadata before release.
- [x] Integrate with `typepython verify`.

### Acceptance Criteria

- [x] Users can identify which dependencies are degrading their typing precision.
- [x] Library authors can validate that published wheels expose correct PEP 561 metadata.
- [x] CI can lock and review the typing inputs used for a release.

## P1: Pydantic and FastAPI Integration

### Goal

Make TypePython compelling for API and data-validation teams by connecting static shape generation to runtime validation frameworks.

This integration should prove the framework transform and shape model. It must not become a separate path of Pydantic-specific compiler hacks.

### Why This Matters

Pydantic and FastAPI are high-signal adoption targets because they combine static annotations, runtime validation, decorators, generated constructors, aliases, defaults, and framework-owned runtime behavior.

If TypePython can model a Pydantic/FastAPI-style app through the general transform system and emit checker-neutral stubs, the north star becomes concrete.

### Pydantic TODO

- [x] Support Pydantic-style model field collection.
- [x] Implement support through generic framework transforms and shapes, not hard-coded checker behavior.
- [x] Support `Field(...)` default and alias metadata.
- [x] Support required vs optional field inference.
- [x] Support `default_factory`.
- [x] Support frozen fields or model-level immutability where statically known.
- [x] Support `computed_field` in emitted stubs.
- [x] Support validators and serializers as checked decorators where feasible.
- [x] Emit accurate `__init__` and `model_construct` signatures.
- [x] Diagnose dynamic aliases that prevent accurate static constructor typing.
- [x] Add fixture parity with common Pydantic mypy plugin strictness settings:
  - [x] typed init
  - [x] forbid extra
  - [x] warn untyped fields
  - [x] warn required dynamic aliases

### FastAPI TODO

- [x] Model endpoint request body shape.
- [x] Model response model shape.
- [x] Model dependency-injected parameters.
- [x] Preserve runtime FastAPI decorators in emitted `.py`.
- [x] Emit `.pyi` surfaces that IDEs can understand.
- [x] Add examples for typical API endpoints.

### Acceptance Criteria

- [x] A small FastAPI/Pydantic app written in `.tpy` builds to ordinary Python.
- [x] Generated artifacts pass mypy, pyright, and ty without Pydantic-specific checker plugins.
- [x] Runtime behavior remains delegated to Pydantic/FastAPI, not a TypePython runtime.
- [x] The implementation path can be reused by at least one non-Pydantic framework fixture.

## P1: Migration and Publication Workflow

### Goal

Reduce the cost of adopting TypePython in existing projects and increase confidence before publishing generated artifacts.

### Migration TODO

- [x] Improve `typepython migrate --report` into an adoption dashboard.
- [x] Report public API annotation completeness.
- [x] Report `dynamic` and `unknown` boundaries.
- [x] Report untyped imports.
- [x] Report modules with high downstream dependency impact.
- [x] Report framework patterns that would benefit from transform declarations.
- [x] Add JSON output suitable for CI dashboards.

### Baseline TODO

- [x] Add a diagnostic baseline file format.
- [x] Support "no new diagnostics" mode.
- [x] Support per-rule severity configuration.
- [x] Support inline suppression with diagnostic codes.
- [x] Add docs for strict migration strategy.

### Publication TODO

- [x] Extend `typepython verify` with checker presets.
- [x] Validate `py.typed`, `.pyi`, wheel, and sdist consistency.
- [x] Add richer public surface drift reports.
- [x] Add optional runtime import probes with clear trust model.
- [x] Add output that explains why a package is or is not PEP 561 ready.

### Acceptance Criteria

- [x] A partially typed project can adopt TypePython without fixing every diagnostic on day one.
- [x] CI can fail only on new errors.
- [x] Library authors can run one command before publishing to validate generated artifacts.

## P1: IDE-First Migration and Refactor UX

### Goal

Make TypePython adoption feel incremental inside the editor, not like a one-time command-line rewrite.

### Why This Matters

Typing adoption is now mainstream, but teams still struggle with learning cost, noisy diagnostics, migration planning, and understanding where `Any` or `Unknown` came from. The editor is where these problems are easiest to explain and fix.

TypePython already has LSP infrastructure. It should use that position to make migration discoverable symbol-by-symbol and file-by-file.

### TODO

- [ ] Add LSP code actions for common migrations:
  - [ ] convert class to TypePython `data class`
  - [ ] extract `interface` from implementation
  - [ ] extract `TypedDict` or shape from dict literal usage
  - [ ] convert hand-written DTO class to shape-backed model
  - [ ] generate public `.pyi` preview for a selected symbol
  - [ ] insert minimal annotation to remove a downstream `unknown`
- [ ] Add "Explain this type" hover support.
- [ ] Add "Find source of `Any`" and "Find source of `Unknown`" navigation.
- [ ] Add quick fixes for portable typing rewrites:
  - [ ] `typing.List` to `list` when target allows it
  - [ ] target-compatible `typing_extensions` import selection
  - [ ] overload normalization
  - [ ] `TypedDict` key requiredness fixes
- [ ] Add preview UI for emitted `.py` and `.pyi` for the current file.
- [ ] Add diagnostics that show whether a fix is TypePython-only or checker-portable.
- [x] Add editor commands that run `migrate --report`, `compat`, and `type-health` for the current project.

### Acceptance Criteria

- [ ] A user can migrate a small module from the editor without reading the full CLI docs.
- [ ] LSP explains why a public type is unknown and which upstream file caused it.
- [ ] Quick fixes do not introduce TypePython-only constructs when a standard Python rewrite is enough.

## P1: Type Coverage Budget and Incremental Gate

### Goal

Let teams improve typing gradually by preventing regressions instead of demanding full strictness immediately.

### Why This Matters

Most real Python codebases cannot become fully typed in a single migration. A strict all-or-nothing gate causes teams to abandon type checking or suppress too much. A budget model gives teams a ratchet: new code must be better, old code can be paid down intentionally.

### TODO

- [x] Extend `typepython migrate --report` into a type coverage budget system.
- [x] Add baseline files that record current:
  - [x] diagnostics
  - [x] public `Any`
  - [x] public `Unknown`
  - [x] untyped imports
  - [x] dynamic framework boundaries
  - [x] checker portability issues
- [x] Add "no new public `Any`" mode.
- [x] Add "no new `Unknown` exported from package" mode.
- [ ] Add per-path budgets for:
  - [ ] library public surface
  - [ ] application code
  - [ ] generated code
  - [ ] tests
  - [ ] migrations/scripts
- [x] Rank untyped modules by downstream blast radius.
- [ ] Add CI annotations for only the lines changed in a PR.
- [ ] Add JSON and SARIF output for dashboards.
- [ ] Add trend output so teams can track type coverage over time.

### Acceptance Criteria

- [x] A partially typed project can enforce "no new type debt" in CI.
- [x] Reports identify the smallest set of files that would unlock the largest downstream precision gain.
- [ ] Teams can ratchet strictness without mass suppressions.

## P1: Framework Adapter SDK Prototype

### Goal

Allow framework authors and advanced users to define TypePython transform adapters without writing checker plugins or modifying the compiler for every framework.

Start with a prototype SDK for first-party fixtures. Do not build a public registry until at least two real adapters have forced the abstraction to prove itself.

### Why This Matters

The framework-shape system will not scale if TypePython hard-codes every behavior for Django, SQLAlchemy, Pydantic, Celery, FastAPI, Click, Typer, and the long tail of internal frameworks.

The ecosystem needs a constrained adapter model: expressive enough for common runtime shape patterns, but declarative and testable enough to avoid recreating arbitrary mypy plugins.

### TODO

- [x] Design a framework adapter manifest, such as `typepython-framework.toml`.
- [x] Define which adapter declarations are allowed:
  - [x] class decorator transforms
  - [x] base class transforms
  - [x] metaclass transforms
  - [x] function-to-object decorator transforms
  - [x] field collector rules
  - [x] constructor synthesis rules
  - [x] descriptor-backed attribute rules
  - [x] alias/default/frozen metadata mapping
- [x] Prohibit arbitrary Python execution in adapter definitions.
- [x] Add an adapter validation command.
- [x] Require adapter golden tests:
  - [x] input `.tpy`
  - [x] emitted `.py`
  - [x] emitted `.pyi`
  - [x] downstream checker expectations
  - [x] runtime smoke expectations where relevant
- [x] Add minimal adapter compatibility metadata for local validation.
- [x] Defer a public registry and broad versioning policy until after:
  - [x] a Pydantic-like adapter works
  - [x] a non-validation framework adapter works
  - [x] checker portability reports cover adapter output
- [x] Add documentation for building first-party and third-party adapters.
- [x] Add examples for a toy ORM, task queue, and validation model.

### Acceptance Criteria

- [x] A framework can ship a TypePython adapter without depending on a mypy plugin.
- [x] Adapter behavior is deterministic, reviewable, and testable.
- [x] TypePython can reject unsafe or unsupported adapter declarations before compilation.
- [x] The SDK is not treated as stable until multiple adapters have been implemented.

## P2: Sync/Async Dual Emit

### Goal

Let library authors maintain one source body for closely related sync and async APIs while emitting two accurate Python surfaces.

This is a strong codegen differentiator, but it should not start until the framework-shape path has real users. The semantic risk is higher than the first framework transform loop.

### Scope Rules

Start narrow. This is code generation, not a general async-to-sync semantic converter.

Initial supported cases:

- explicit `dual async def`
- direct `await` over mapped client/session calls
- simple `async with` over mapped context managers
- simple `async for` only after a design pass

Initial unsupported cases:

- task groups
- cancellation-sensitive logic
- streaming generators
- arbitrary event loop interaction
- concurrent scheduling
- async callbacks whose sync equivalent is not declared

### Design TODO

- [x] Define `dual async def` syntax or decorator form.
- [x] Define name generation policy, such as `fetch_user` and `afetch_user`.
- [x] Define sync/async type mapping declarations.
- [x] Define how `await` is lowered in sync output.
- [x] Define how errors are reported when no sync mapping exists.
- [x] Define output ordering and stub generation.

### Implementation TODO

- [ ] Extend parser metadata for dual functions.
- [ ] Extend lowering to emit two function bodies.
- [ ] Extend source maps for dual emitted bodies.
- [ ] Extend stub generation for both functions.
- [x] Add examples for HTTP client and database access.
- [ ] Add downstream checker fixtures.

### Acceptance Criteria

- [ ] A simple async client API can generate a sync and async pair.
- [ ] Both emitted functions type-check externally.
- [ ] Unsupported async constructs fail with clear diagnostics.

## P2: Result, ADT, and Effect Experiment

### Goal

Explore safer error modeling using TypePython's existing sealed-class and exhaustive-match strengths.

### TODO

- [x] Define a standard `Result[T, E]` pattern using sealed classes.
- [x] Add examples for `Ok[T]` and `Err[E]`.
- [x] Add exhaustiveness examples using `match`.
- [x] Explore optional `raises` syntax behind an experimental flag.
- [x] Decide whether `raises` lowers to metadata, `Result`, or ordinary exceptions.
- [x] Avoid requiring throws annotations for arbitrary Python imports.
- [x] Avoid Java-style checked exceptions in Core v1.

### Acceptance Criteria

- [x] Users can opt into a Rust-like error style in `.tpy`.
- [x] Existing Python exception behavior is preserved by default.
- [x] The feature remains optional and does not burden ordinary interop.

## P2: Boundary Validator Generation

### Goal

Generate or connect runtime validation only at trust boundaries, while keeping TypePython itself free of a mandatory runtime.

### Why This Matters

Typed Python teams often need both static guarantees and runtime validation for untrusted data. The highest-value boundaries are API requests, CLI inputs, config files, message queues, plugin systems, and serialized payloads.

Full runtime type checking would be too invasive and too slow. Boundary-only generation is narrower, easier to explain, and aligns with existing tools such as Pydantic, msgspec, cattrs, attrs, dataclasses, beartype, and framework validators.

### TODO

- [x] Define what counts as a validation boundary.
- [x] Support boundary annotations for:
  - [x] HTTP request and response payloads
  - [x] CLI command parameters
  - [x] config files
  - [x] message queue payloads
  - [x] plugin entrypoints
  - [x] JSON/YAML/TOML serialization boundaries
- [x] Decide whether TypePython generates validators directly or delegates to existing libraries.
- [ ] Add an adapter interface for Pydantic/msgspec/cattrs-style validators.
- [ ] Generate validation code only when explicitly requested.
- [ ] Keep generated validators out of public `.pyi` unless intentionally exported.
- [ ] Add diagnostics when a type cannot be faithfully validated at runtime.
- [x] Add examples for API payloads and config loading.

### Acceptance Criteria

- [ ] A project can opt into runtime validation at selected boundaries.
- [x] The default TypePython workflow still has no mandatory runtime dependency.
- [ ] Generated validation behavior is explicit and reviewable.

## P2: Must-Use, Must-Await, and Must-Close Diagnostics

### Goal

Catch common resource and async lifecycle bugs without committing to a full typestate system.

### Why This Matters

Python developers frequently hit bugs that are not advanced type-theory problems:

- a coroutine is created but not awaited
- a resource is opened but not closed
- a transaction is started but not committed or rolled back
- a response stream is not consumed
- an async context manager is used incorrectly

These are high-impact diagnostics that can be modeled with lightweight attributes before TypePython attempts general typestate.

### TODO

- [x] Define opt-in annotations or decorators:
  - [x] `@must_use`
  - [x] `@must_await`
  - [x] `@must_close`
  - [x] `@must_consume`
- [x] Define how diagnostics behave for:
  - [x] ignored return values
  - [x] assigned-but-never-used resources
  - [x] context manager usage
  - [x] async context manager usage
  - [x] early returns
  - [x] exceptions
- [x] Add conservative intra-procedural analysis first.
- [x] Add clear false-positive escape hatches.
- [x] Add framework adapters for common resources:
  - [x] files
  - [x] HTTP responses
  - [x] database sessions
  - [x] transactions
  - [x] async tasks
- [x] Keep this separate from full typestate until the simpler diagnostics prove useful.

### Acceptance Criteria

- [x] TypePython can catch an ignored coroutine/task-like result in `.tpy`.
- [x] TypePython can warn when an annotated resource is created but not closed or consumed.
- [x] The analysis is conservative enough to avoid blocking normal Python resource patterns.

## Deferred: Notebook-to-Package Typing Workflow

### Why Deferred

Notebook migration is valuable for data and ML teams, but it introduces `.ipynb` parsing, cell dependency graphs, side-effect analysis, and DataFrame schema modeling. Those are adjacent to the north star rather than necessary for the first framework compiler loop.

Do not start until TypePython has a credible story for ordinary package/module migration.

Possible later work:

- [x] Add a notebook ingestion prototype for `.ipynb`.
- [x] Extract code cells into an ordered module graph.
- [x] Detect cross-cell variable dependencies.
- [x] Identify symbols that should become module-level public API.
- [x] Generate a migration report for:
  - [x] implicit globals
  - [x] untyped function boundaries
  - [x] dict-like records
  - [x] DataFrame-like schema boundaries
  - [x] side-effectful cells
- [x] Generate candidate `.tpy` modules from selected cells.
- [x] Generate `.pyi` previews for extracted modules.
- [x] Add lightweight schema annotations for pandas/polars-like tabular boundaries.
- [x] Avoid committing to full tensor shape algebra in this workflow.

## Deferred: Typestate

### Why Deferred

Typestate is useful for protocols, resources, and drivers, but it requires a state-transition model that could become its own language.

Do not start until the framework-shape path is stable.

Possible later work:

- [x] Model finite states with sealed classes.
- [x] Model method transitions.
- [x] Emit runtime assertions optionally.
- [x] Explore whether external `.pyi` can expose useful receiver-state changes.

## Deferred: Tensor Shape Types

### Why Deferred

ML users need tensor shape checking, but shape algebra moves toward dependent typing and domain-specific DSL design.

Possible later work:

- [ ] Survey jaxtyping, torchtyping, beartype, PyTorch, JAX, and NumPy typing.
- [ ] Decide whether TypePython should emit runtime asserts or only static metadata.
- [ ] Avoid adding general dependent types to Core.

## Deferred: Taint and Provenance Types

### Why Deferred

Security taint tracking is valuable, but the adoption path competes with SAST tools and requires framework-specific source/sink models.

Possible later work:

- [ ] Define `Tainted[T]` / `Untrusted[T]`.
- [ ] Define sanitizer declarations.
- [ ] Define source/sink config.
- [ ] Integrate with web framework route/request models.

## 90-Day Execution Plan

### Weeks 1-2: Design and Credibility Foundation

- [x] Draft framework transform RFC.
- [x] Draft phase-1 internal shape IR RFC.
- [x] Draft checker portability audit design for mypy, pyright, and ty.
- [x] Draft Python 3.14+ annotation runtime compatibility notes for generated `.py`.
- [x] Add typeshed pin and stdlib refresh design.
- [x] Add coverage tool choice and local command docs.

### Weeks 3-5: Minimal Transform Prototype

- [x] Implement function-to-object decorator transform prototype.
- [x] Emit transformed `.pyi` for a toy `Task[P, R]`.
- [x] Add mypy/pyright/ty downstream fixture for the toy task framework.
- [x] Add strict-mode diagnostic for unsupported non-callable decorator transforms.
- [x] Add a first `typepython compat` prototype over the toy generated artifacts.

### Weeks 6-8: Shape Model Prototype

- [x] Introduce internal `Shape` representation.
- [x] Bridge existing `TypedDictShape` to the new model.
- [x] Keep current TypedDict transform tests green.
- [x] Implement a class-shape rewriter for a toy model framework.
- [x] Emit a generated constructor and generated members in `.pyi`.
- [x] Validate emitted stubs with mypy, pyright, and ty.

### Weeks 9-10: Pydantic/FastAPI Spike

- [x] Build a tiny Pydantic-like fixture using the transform system.
- [x] Generate `__init__` stub with aliases and defaults.
- [x] Validate generated artifacts with mypy/pyright/ty.
- [x] Document known unsupported Pydantic features.
- [x] Add annotation runtime probes for Python 3.14-style deferred annotations.
- [x] Sketch the framework adapter manifest only from the toy and Pydantic-like fixtures.

### Weeks 11-12: Harden and Publish Direction

- [x] Add conformance mapping skeleton.
- [x] Add coverage CI artifact.
- [x] Add first parser or lowering fuzz smoke CI.
- [x] Add checker portability report to CI artifacts.
- [x] Write user-facing roadmap in `docs/`.
- [x] Add a showcase example for framework transforms.
- [x] Decide whether type-health, API diff, or migration baseline is the next P1 productization step.

## Definition of Done for the North Star

- [x] A framework author can describe a runtime transform without writing a mypy plugin.
- [x] A TypePython user can write framework-heavy `.tpy` and emit ordinary Python.
- [x] Generated `.pyi` works in mypy, pyright, ty, and IDEs.
- [x] pyrefly and other emerging checkers are reported as optional compatibility signals until stable enough for default gating.
- [x] TypePython can explain checker portability risks before users publish artifacts.
- [x] Generated runtime annotations behave predictably across supported Python targets, including Python 3.14+.
- [x] Runtime behavior remains framework-owned.
- [x] Unsupported dynamic behavior is diagnosed, not guessed.
- [x] The implementation has coverage, fuzz, downstream checker, and conformance evidence.

## Definition of Done for Productization

- [x] TypePython can report dependency and stub health for a project.
- [x] Library authors can diff public type surfaces across releases.
- [ ] Teams can adopt TypePython incrementally with baselines and type coverage budgets.
- [ ] IDE and CLI workflows explain `Any`, `Unknown`, generated stubs, and checker portability without requiring users to read compiler internals.

## Things Not To Do Yet

- [x] Do not add broad dependent typing.
- [x] Do not add TypeScript-style full conditional or mapped types before the shape model.
- [x] Do not make TypePython require a runtime.
- [x] Do not make mypy plugins part of the success path.
- [x] Do not build framework-specific hacks that cannot be expressed through a general transform/shape model.
- [x] Do not execute arbitrary framework adapter code during compilation.
- [x] Do not stabilize a public framework adapter registry before multiple adapters prove the abstraction.
- [x] Do not start sync/async dual emit before the framework-shape path has real users.
- [x] Do not turn boundary validation into whole-program runtime type checking.
- [x] Do not make notebook support depend on executing untrusted notebook cells.
- [x] Do not make AI-generated annotations authoritative without compiler and checker validation.
- [x] Do not expand into typestate, tensor shape types, or taint tracking before the framework path has real users.
