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

- [ ] Write an RFC for framework transform declarations.
- [ ] Define terminology:
  - [ ] runtime declaration
  - [ ] static shape
  - [ ] transform provider
  - [ ] transform target
  - [ ] generated member
  - [ ] generated constructor
  - [ ] replacement callable/object
  - [ ] emitted stub authority
- [ ] Decide syntax for declaring transform providers.
- [ ] Decide whether transform declarations live in `.tpy`, sidecar `.tpyi`, config, or all three.
- [ ] Define a minimal transform metadata model that can represent:
  - [ ] field collection
  - [ ] constructor generation
  - [ ] alias handling
  - [ ] required vs optional fields
  - [ ] readonly/frozen fields
  - [ ] descriptor-backed attributes
  - [ ] method synthesis
  - [ ] function-to-object replacement
  - [ ] generic preservation via `ParamSpec`, `TypeVar`, and `TypeVarTuple`
- [ ] Specify fallback behavior when transform metadata depends on runtime-only values.
- [ ] Define strict-mode diagnostics for unsupported transform behavior.
- [ ] Define non-strict degradation behavior.
- [ ] Add a spec section after the existing decorator/dataclass-transform rules.
- [ ] Update `docs/spec/implementation-notes-v1.md` Appendix J after the design lands.

### Compiler TODO

- [x] Extend syntax metadata collection for transform declarations and transform applications.
- [x] Extend binding summaries to include transform provider metadata.
- [x] Extend checker semantic facts to resolve transform providers across imports.
- [x] Extend checker class-shape resolution beyond `dataclass_transform`.
- [x] Extend checker callable resolution for non-callable decorator replacement.
- [x] Extend stub generation to emit the transformed public surface rather than the raw source declaration.
- [ ] Extend source maps so diagnostics point to the original framework declaration site.
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
  - [ ] TypePython `data class`
  - [ ] standard `@dataclass`
  - [x] `dataclass_transform`
  - [x] transformed framework classes
- [x] Explicitly defer public transforms over arbitrary classes, protocols, and interfaces until assignability rules are proven.
- [x] Include field metadata:
  - [x] field name
  - [ ] public alias
  - [x] type
  - [x] required/optional
  - [x] readonly/mutable
  - [x] constructor participation
  - [x] default/default factory
  - [x] descriptor behavior
  - [x] source declaration span
- [ ] Define phase-1 shape source behavior for:
  - [ ] `TypedDict`
  - [ ] `data class`
  - [ ] standard `@dataclass`
  - [ ] `dataclass_transform`
  - [ ] transformed framework class
- [ ] Document future shape source candidates without enabling them by default:
  - [ ] `interface` / `Protocol`
  - [ ] ordinary class with annotated instance fields
- [ ] Define shape operations:
  - [ ] `Partial`
  - [ ] `Required_`
  - [ ] `Readonly`
  - [ ] `Mutable`
  - [ ] `Pick`
  - [ ] `Omit`
  - [ ] shape composition
  - [ ] shape projection
- [ ] Define shape assignability.
- [ ] Define how shape aliases lower into `.pyi`.
- [ ] Define when a shape remains nominal and when it becomes structural.
- [ ] Document which parts of the model are compiler-internal and which are user-visible.

### Implementation TODO

- [x] Add semantic shape structs in `typepython_checking`.
- [x] Move current `TypedDictShape` behavior toward shared shape primitives.
- [x] Keep existing TypedDict diagnostics stable during refactor.
- [ ] Extend lowering transform expansion to consume shared shapes.
- [ ] Emit stable names for generated shape aliases.
- [ ] Add hover rendering for projected shapes.
- [x] Add diagnostics for unknown keys with suggestions.

### Acceptance Criteria

- [x] Existing TypedDict transform tests continue to pass.
- [ ] `Pick` and `Partial` can operate on at least one non-TypedDict source behind an experimental flag.
- [x] Transformed class shapes can be reused by framework integrations.
- [x] Generated stubs remain standard Python typing.
- [ ] No public "arbitrary type transform" guarantee is made before assignability semantics are stable.

## P0: Trustworthiness and Test Infrastructure

### Goal

Make TypePython credible as a compiler that users can place in a release pipeline.

### stdlib / typeshed TODO

- [ ] Add an upstream typeshed commit pin.
- [ ] Add a `scripts/refresh_stdlib_stubs.py` or equivalent sync tool.
- [ ] Record generated diff statistics during refresh.
- [ ] Validate `stdlib/VERSIONS` against refreshed files.
- [ ] Add CI that fails when `stdlib/BASELINE.toml` is stale.
- [ ] Document the refresh process in `docs/contributing.md`.
- [ ] Preserve local TypePython-specific patches in a reproducible patch directory if needed.

### Coverage TODO

- [ ] Add `cargo llvm-cov` or an equivalent coverage workflow.
- [ ] Track coverage for:
  - [ ] parser/syntax extraction
  - [ ] binding
  - [ ] checker semantic rules
  - [ ] lowering
  - [ ] emit/stub generation
  - [ ] CLI verify
- [ ] Publish coverage artifact in CI.
- [ ] Establish minimum coverage thresholds only after baseline stabilization.

### Fuzzing TODO

- [ ] Add `cargo-fuzz`.
- [ ] Fuzz parser entrypoints.
- [ ] Fuzz TypeExpr parsing.
- [ ] Fuzz lowering on syntactically valid `.tpy` snippets.
- [ ] Fuzz stub generation from lowered Python.
- [ ] Fuzz TypedDict/shape transform composition.
- [ ] Add corpus seeds from examples and test fixtures.
- [ ] Run fuzz smoke in CI with short duration.
- [ ] Run long fuzz in scheduled CI.

### Differential and External Checker TODO

- [ ] Expand downstream checker smoke into a formal matrix.
- [ ] Add fixtures where TypePython emits `.pyi`, then mypy/pyright/ty validate expected success.
- [ ] Add negative fixtures where downstream checkers should reject intentionally bad consumer code.
- [ ] Add differential tests for standard Python typing cases where TypePython should agree with mypy/pyright/ty.
- [ ] Track known checker disagreements in a documented allowlist.
- [ ] Add a `typepython verify --checker-preset all` convenience mode.

### Conformance TODO

- [ ] Build a mapping from spec `MUST` rules to test names.
- [ ] Add a generated conformance report.
- [ ] Ensure every diagnostic code has positive and negative tests.
- [ ] Split giant checker fixtures into thematic files where practical.
- [ ] Keep insta snapshots limited to emission/golden output tests.

### Acceptance Criteria

- [ ] stdlib stubs can be refreshed reproducibly from a pinned source.
- [ ] Coverage and fuzz smoke run locally with documented commands.
- [ ] CI has at least one checker-neutral downstream compatibility gate.
- [ ] The conformance report identifies implemented, partial, and missing rule coverage.

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

- [ ] Add an `annotations` compatibility audit pass.
- [ ] Detect annotations that are safe statically but fragile at runtime.
- [ ] Detect annotation consumers such as:
  - [ ] `typing.get_type_hints`
  - [ ] `inspect.get_annotations`
  - [ ] `annotationlib.get_annotations`
  - [ ] framework-specific model/route/command registration
- [ ] Define target-version emit rules for annotations in generated `.py`.
- [ ] Preserve enough runtime metadata for frameworks without forcing eager imports.
- [ ] Add diagnostics for annotations that rely on local-scope names unavailable to runtime consumers.
- [ ] Add diagnostics for circular imports caused only by runtime annotation evaluation.
- [ ] Add guidance for when to emit string annotations, native annotations, or sidecar metadata.
- [ ] Add test fixtures for Python 3.10 through 3.14 behavior.
- [ ] Add runtime probes for representative Pydantic/FastAPI-like consumers.

### Acceptance Criteria

- [ ] Generated `.py` behaves predictably under Python 3.14 deferred annotation semantics.
- [ ] Runtime frameworks can consume TypePython-generated annotations without hidden import failures.
- [ ] The compiler warns when a statically valid annotation would be unsafe for a configured runtime target.

## P1: Public API Surface Diff

### Goal

Let library authors answer one release-critical question: "Did this release change the public typing surface in a way users need to review?"

### Why This Matters

Typed Python packages can introduce type-breaking changes without breaking runtime tests. Changing a parameter from optional to required, modifying overload order, narrowing a return type incorrectly, changing `TypedDict` requiredness, or dropping `py.typed` can break downstream users even when the runtime still imports.

TypePython already treats emitted `.pyi` as authoritative. That makes it well-positioned to compare public type surfaces across releases.

The first version should be a conservative surface diff, not a complete semantic type-compatibility prover.

### TODO

- [ ] Add `typepython api-diff <old> <new>`.
- [ ] Support inputs:
  - [ ] source directories
  - [ ] generated build directories
  - [ ] wheels
  - [ ] sdists
  - [ ] `.pyi` trees
- [ ] Compare public modules, classes, functions, aliases, protocols, and `TypedDict` definitions.
- [ ] Detect clear type-surface breaking changes:
  - [ ] removed public symbol
  - [ ] required parameter added
  - [ ] parameter annotation changed
  - [ ] return type widened to `Any` or `Unknown`
  - [ ] return annotation changed
  - [ ] overload order or coverage change
  - [ ] generic parameter arity/default change
  - [ ] `TypedDict` required/optional/readonly/closed/extra-items change
  - [ ] protocol member removal or incompatible change
  - [ ] `py.typed` or partial-stub metadata regression
- [ ] Classify changes as:
  - [ ] source-compatible
  - [ ] likely type-compatible
  - [ ] likely type-breaking
  - [ ] runtime-breaking signal
  - [ ] unknown risk
- [ ] Defer full semantic compatibility checks until checker-backed diffing is designed.
- [ ] Generate release-note snippets.
- [ ] Add SemVer policy gates.
- [ ] Integrate with `typepython verify` and publication workflow.

### Acceptance Criteria

- [ ] A library maintainer can compare two artifacts and get a concrete type-compatibility report.
- [ ] CI can block accidental type-breaking changes in minor or patch releases.
- [ ] Reports are precise enough to review without reading raw generated stubs.

## P1: Typed Dependency and Stub Health Supply Chain

### Goal

Give users and library authors a clear view of whether their dependency graph has reliable type information.

### Why This Matters

Third-party library typing remains one of the most practical pain points in Python typing. PEP 561 defines `py.typed`, stub-only packages, partial stubs, and resolution order, but projects still run into missing stubs, stale stubs, version mismatches, partial coverage, and `Unknown` leakage.

Fast checkers and better syntax do not solve a bad type supply chain. TypePython can provide a package-level health layer around the artifacts it consumes and emits.

This is high-value productization work, but it should not block the first framework compiler vertical slice.

### TODO

- [ ] Add a `typepython type-health` command.
- [ ] Inspect installed dependencies for `py.typed`.
- [ ] Detect stub-only packages following the `*-stubs` convention.
- [ ] Detect partial stub packages.
- [ ] Report runtime package version vs stub package version compatibility when metadata is available.
- [ ] Detect common sources of poor typing:
  - [ ] missing `py.typed`
  - [ ] partial stubs with broad `Any`
  - [ ] public functions returning `Any`
  - [ ] public classes with untyped attributes
  - [ ] overloaded APIs with fallback `Any`
  - [ ] unsupported `typing_extensions` features for the configured target
- [ ] Generate `.typepython/type-lock.toml` to record:
  - [ ] Python target versions
  - [ ] `typing_extensions` version
  - [ ] typeshed commit
  - [ ] stub package versions
  - [ ] checker versions used for validation
- [ ] Add a `--fail-under` option for type-health score.
- [ ] Add a package maintainer mode that validates wheel/sdist type metadata before release.
- [ ] Integrate with `typepython verify`.

### Acceptance Criteria

- [ ] Users can identify which dependencies are degrading their typing precision.
- [ ] Library authors can validate that published wheels expose correct PEP 561 metadata.
- [ ] CI can lock and review the typing inputs used for a release.

## P1: Pydantic and FastAPI Integration

### Goal

Make TypePython compelling for API and data-validation teams by connecting static shape generation to runtime validation frameworks.

This integration should prove the framework transform and shape model. It must not become a separate path of Pydantic-specific compiler hacks.

### Why This Matters

Pydantic and FastAPI are high-signal adoption targets because they combine static annotations, runtime validation, decorators, generated constructors, aliases, defaults, and framework-owned runtime behavior.

If TypePython can model a Pydantic/FastAPI-style app through the general transform system and emit checker-neutral stubs, the north star becomes concrete.

### Pydantic TODO

- [ ] Support Pydantic-style model field collection.
- [ ] Implement support through generic framework transforms and shapes, not hard-coded checker behavior.
- [ ] Support `Field(...)` default and alias metadata.
- [ ] Support required vs optional field inference.
- [ ] Support `default_factory`.
- [ ] Support frozen fields or model-level immutability where statically known.
- [ ] Support `computed_field` in emitted stubs.
- [ ] Support validators and serializers as checked decorators where feasible.
- [ ] Emit accurate `__init__` and `model_construct` signatures.
- [ ] Diagnose dynamic aliases that prevent accurate static constructor typing.
- [ ] Add fixture parity with common Pydantic mypy plugin strictness settings:
  - [ ] typed init
  - [ ] forbid extra
  - [ ] warn untyped fields
  - [ ] warn required dynamic aliases

### FastAPI TODO

- [ ] Model endpoint request body shape.
- [ ] Model response model shape.
- [ ] Model dependency-injected parameters.
- [ ] Preserve runtime FastAPI decorators in emitted `.py`.
- [ ] Emit `.pyi` surfaces that IDEs can understand.
- [ ] Add examples for typical API endpoints.

### Acceptance Criteria

- [ ] A small FastAPI/Pydantic app written in `.tpy` builds to ordinary Python.
- [ ] Generated artifacts pass mypy, pyright, and ty without Pydantic-specific checker plugins.
- [ ] Runtime behavior remains delegated to Pydantic/FastAPI, not a TypePython runtime.
- [ ] The implementation path can be reused by at least one non-Pydantic framework fixture.

## P1: Migration and Publication Workflow

### Goal

Reduce the cost of adopting TypePython in existing projects and increase confidence before publishing generated artifacts.

### Migration TODO

- [ ] Improve `typepython migrate --report` into an adoption dashboard.
- [ ] Report public API annotation completeness.
- [ ] Report `dynamic` and `unknown` boundaries.
- [ ] Report untyped imports.
- [ ] Report modules with high downstream dependency impact.
- [ ] Report framework patterns that would benefit from transform declarations.
- [ ] Add JSON output suitable for CI dashboards.

### Baseline TODO

- [ ] Add a diagnostic baseline file format.
- [ ] Support "no new diagnostics" mode.
- [ ] Support per-rule severity configuration.
- [ ] Support inline suppression with diagnostic codes.
- [ ] Add docs for strict migration strategy.

### Publication TODO

- [ ] Extend `typepython verify` with checker presets.
- [ ] Validate `py.typed`, `.pyi`, wheel, and sdist consistency.
- [ ] Add richer public surface drift reports.
- [ ] Add optional runtime import probes with clear trust model.
- [ ] Add output that explains why a package is or is not PEP 561 ready.

### Acceptance Criteria

- [ ] A partially typed project can adopt TypePython without fixing every diagnostic on day one.
- [ ] CI can fail only on new errors.
- [ ] Library authors can run one command before publishing to validate generated artifacts.

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
- [ ] Add editor commands that run `migrate --report`, `compat`, and `type-health` for the current project.

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

- [ ] Extend `typepython migrate --report` into a type coverage budget system.
- [ ] Add baseline files that record current:
  - [ ] diagnostics
  - [ ] public `Any`
  - [ ] public `Unknown`
  - [ ] untyped imports
  - [ ] dynamic framework boundaries
  - [ ] checker portability issues
- [ ] Add "no new public `Any`" mode.
- [ ] Add "no new `Unknown` exported from package" mode.
- [ ] Add per-path budgets for:
  - [ ] library public surface
  - [ ] application code
  - [ ] generated code
  - [ ] tests
  - [ ] migrations/scripts
- [ ] Rank untyped modules by downstream blast radius.
- [ ] Add CI annotations for only the lines changed in a PR.
- [ ] Add JSON and SARIF output for dashboards.
- [ ] Add trend output so teams can track type coverage over time.

### Acceptance Criteria

- [ ] A partially typed project can enforce "no new type debt" in CI.
- [ ] Reports identify the smallest set of files that would unlock the largest downstream precision gain.
- [ ] Teams can ratchet strictness without mass suppressions.

## P1: Framework Adapter SDK Prototype

### Goal

Allow framework authors and advanced users to define TypePython transform adapters without writing checker plugins or modifying the compiler for every framework.

Start with a prototype SDK for first-party fixtures. Do not build a public registry until at least two real adapters have forced the abstraction to prove itself.

### Why This Matters

The framework-shape system will not scale if TypePython hard-codes every behavior for Django, SQLAlchemy, Pydantic, Celery, FastAPI, Click, Typer, and the long tail of internal frameworks.

The ecosystem needs a constrained adapter model: expressive enough for common runtime shape patterns, but declarative and testable enough to avoid recreating arbitrary mypy plugins.

### TODO

- [ ] Design a framework adapter manifest, such as `typepython-framework.toml`.
- [ ] Define which adapter declarations are allowed:
  - [ ] class decorator transforms
  - [ ] base class transforms
  - [ ] metaclass transforms
  - [ ] function-to-object decorator transforms
  - [ ] field collector rules
  - [ ] constructor synthesis rules
  - [ ] descriptor-backed attribute rules
  - [ ] alias/default/frozen metadata mapping
- [ ] Prohibit arbitrary Python execution in adapter definitions.
- [ ] Add an adapter validation command.
- [ ] Require adapter golden tests:
  - [ ] input `.tpy`
  - [ ] emitted `.py`
  - [ ] emitted `.pyi`
  - [ ] downstream checker expectations
  - [ ] runtime smoke expectations where relevant
- [ ] Add minimal adapter compatibility metadata for local validation.
- [ ] Defer a public registry and broad versioning policy until after:
  - [ ] a Pydantic-like adapter works
  - [ ] a non-validation framework adapter works
  - [ ] checker portability reports cover adapter output
- [ ] Add documentation for building first-party and third-party adapters.
- [ ] Add examples for a toy ORM, task queue, and validation model.

### Acceptance Criteria

- [ ] A framework can ship a TypePython adapter without depending on a mypy plugin.
- [ ] Adapter behavior is deterministic, reviewable, and testable.
- [ ] TypePython can reject unsafe or unsupported adapter declarations before compilation.
- [ ] The SDK is not treated as stable until multiple adapters have been implemented.

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

- [ ] Define `dual async def` syntax or decorator form.
- [ ] Define name generation policy, such as `fetch_user` and `afetch_user`.
- [ ] Define sync/async type mapping declarations.
- [ ] Define how `await` is lowered in sync output.
- [ ] Define how errors are reported when no sync mapping exists.
- [ ] Define output ordering and stub generation.

### Implementation TODO

- [ ] Extend parser metadata for dual functions.
- [ ] Extend lowering to emit two function bodies.
- [ ] Extend source maps for dual emitted bodies.
- [ ] Extend stub generation for both functions.
- [ ] Add examples for HTTP client and database access.
- [ ] Add downstream checker fixtures.

### Acceptance Criteria

- [ ] A simple async client API can generate a sync and async pair.
- [ ] Both emitted functions type-check externally.
- [ ] Unsupported async constructs fail with clear diagnostics.

## P2: Result, ADT, and Effect Experiment

### Goal

Explore safer error modeling using TypePython's existing sealed-class and exhaustive-match strengths.

### TODO

- [ ] Define a standard `Result[T, E]` pattern using sealed classes.
- [ ] Add examples for `Ok[T]` and `Err[E]`.
- [ ] Add exhaustiveness examples using `match`.
- [ ] Explore optional `raises` syntax behind an experimental flag.
- [ ] Decide whether `raises` lowers to metadata, `Result`, or ordinary exceptions.
- [ ] Avoid requiring throws annotations for arbitrary Python imports.
- [ ] Avoid Java-style checked exceptions in Core v1.

### Acceptance Criteria

- [ ] Users can opt into a Rust-like error style in `.tpy`.
- [ ] Existing Python exception behavior is preserved by default.
- [ ] The feature remains optional and does not burden ordinary interop.

## P2: Boundary Validator Generation

### Goal

Generate or connect runtime validation only at trust boundaries, while keeping TypePython itself free of a mandatory runtime.

### Why This Matters

Typed Python teams often need both static guarantees and runtime validation for untrusted data. The highest-value boundaries are API requests, CLI inputs, config files, message queues, plugin systems, and serialized payloads.

Full runtime type checking would be too invasive and too slow. Boundary-only generation is narrower, easier to explain, and aligns with existing tools such as Pydantic, msgspec, cattrs, attrs, dataclasses, beartype, and framework validators.

### TODO

- [ ] Define what counts as a validation boundary.
- [ ] Support boundary annotations for:
  - [ ] HTTP request and response payloads
  - [ ] CLI command parameters
  - [ ] config files
  - [ ] message queue payloads
  - [ ] plugin entrypoints
  - [ ] JSON/YAML/TOML serialization boundaries
- [ ] Decide whether TypePython generates validators directly or delegates to existing libraries.
- [ ] Add an adapter interface for Pydantic/msgspec/cattrs-style validators.
- [ ] Generate validation code only when explicitly requested.
- [ ] Keep generated validators out of public `.pyi` unless intentionally exported.
- [ ] Add diagnostics when a type cannot be faithfully validated at runtime.
- [ ] Add examples for API payloads and config loading.

### Acceptance Criteria

- [ ] A project can opt into runtime validation at selected boundaries.
- [ ] The default TypePython workflow still has no mandatory runtime dependency.
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

- [ ] Define opt-in annotations or decorators:
  - [ ] `@must_use`
  - [ ] `@must_await`
  - [ ] `@must_close`
  - [ ] `@must_consume`
- [ ] Define how diagnostics behave for:
  - [ ] ignored return values
  - [ ] assigned-but-never-used resources
  - [ ] context manager usage
  - [ ] async context manager usage
  - [ ] early returns
  - [ ] exceptions
- [ ] Add conservative intra-procedural analysis first.
- [ ] Add clear false-positive escape hatches.
- [ ] Add framework adapters for common resources:
  - [ ] files
  - [ ] HTTP responses
  - [ ] database sessions
  - [ ] transactions
  - [ ] async tasks
- [ ] Keep this separate from full typestate until the simpler diagnostics prove useful.

### Acceptance Criteria

- [ ] TypePython can catch an ignored coroutine/task-like result in `.tpy`.
- [ ] TypePython can warn when an annotated resource is created but not closed or consumed.
- [ ] The analysis is conservative enough to avoid blocking normal Python resource patterns.

## Deferred: Notebook-to-Package Typing Workflow

### Why Deferred

Notebook migration is valuable for data and ML teams, but it introduces `.ipynb` parsing, cell dependency graphs, side-effect analysis, and DataFrame schema modeling. Those are adjacent to the north star rather than necessary for the first framework compiler loop.

Do not start until TypePython has a credible story for ordinary package/module migration.

Possible later work:

- [ ] Add a notebook ingestion prototype for `.ipynb`.
- [ ] Extract code cells into an ordered module graph.
- [ ] Detect cross-cell variable dependencies.
- [ ] Identify symbols that should become module-level public API.
- [ ] Generate a migration report for:
  - [ ] implicit globals
  - [ ] untyped function boundaries
  - [ ] dict-like records
  - [ ] DataFrame-like schema boundaries
  - [ ] side-effectful cells
- [ ] Generate candidate `.tpy` modules from selected cells.
- [ ] Generate `.pyi` previews for extracted modules.
- [ ] Add lightweight schema annotations for pandas/polars-like tabular boundaries.
- [ ] Avoid committing to full tensor shape algebra in this workflow.

## Deferred: Typestate

### Why Deferred

Typestate is useful for protocols, resources, and drivers, but it requires a state-transition model that could become its own language.

Do not start until the framework-shape path is stable.

Possible later work:

- [ ] Model finite states with sealed classes.
- [ ] Model method transitions.
- [ ] Emit runtime assertions optionally.
- [ ] Explore whether external `.pyi` can expose useful receiver-state changes.

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

- [ ] Draft framework transform RFC.
- [ ] Draft phase-1 internal shape IR RFC.
- [ ] Draft checker portability audit design for mypy, pyright, and ty.
- [ ] Draft Python 3.14+ annotation runtime compatibility notes for generated `.py`.
- [ ] Add typeshed pin and stdlib refresh design.
- [ ] Add coverage tool choice and local command docs.

### Weeks 3-5: Minimal Transform Prototype

- [ ] Implement function-to-object decorator transform prototype.
- [ ] Emit transformed `.pyi` for a toy `Task[P, R]`.
- [ ] Add mypy/pyright/ty downstream fixture for the toy task framework.
- [ ] Add strict-mode diagnostic for unsupported non-callable decorator transforms.
- [ ] Add a first `typepython compat` prototype over the toy generated artifacts.

### Weeks 6-8: Shape Model Prototype

- [ ] Introduce internal `Shape` representation.
- [ ] Bridge existing `TypedDictShape` to the new model.
- [ ] Keep current TypedDict transform tests green.
- [ ] Implement a class-shape rewriter for a toy model framework.
- [ ] Emit a generated constructor and generated members in `.pyi`.
- [ ] Validate emitted stubs with mypy, pyright, and ty.

### Weeks 9-10: Pydantic/FastAPI Spike

- [ ] Build a tiny Pydantic-like fixture using the transform system.
- [ ] Generate `__init__` stub with aliases and defaults.
- [ ] Validate generated artifacts with mypy/pyright/ty.
- [ ] Document known unsupported Pydantic features.
- [ ] Add annotation runtime probes for Python 3.14-style deferred annotations.
- [ ] Sketch the framework adapter manifest only from the toy and Pydantic-like fixtures.

### Weeks 11-12: Harden and Publish Direction

- [ ] Add conformance mapping skeleton.
- [ ] Add coverage CI artifact.
- [ ] Add first parser or lowering fuzz smoke CI.
- [ ] Add checker portability report to CI artifacts.
- [ ] Write user-facing roadmap in `docs/`.
- [ ] Add a showcase example for framework transforms.
- [ ] Decide whether type-health, API diff, or migration baseline is the next P1 productization step.

## Definition of Done for the North Star

- [ ] A framework author can describe a runtime transform without writing a mypy plugin.
- [ ] A TypePython user can write framework-heavy `.tpy` and emit ordinary Python.
- [ ] Generated `.pyi` works in mypy, pyright, ty, and IDEs.
- [ ] pyrefly and other emerging checkers are reported as optional compatibility signals until stable enough for default gating.
- [ ] TypePython can explain checker portability risks before users publish artifacts.
- [ ] Generated runtime annotations behave predictably across supported Python targets, including Python 3.14+.
- [ ] Runtime behavior remains framework-owned.
- [ ] Unsupported dynamic behavior is diagnosed, not guessed.
- [ ] The implementation has coverage, fuzz, downstream checker, and conformance evidence.

## Definition of Done for Productization

- [ ] TypePython can report dependency and stub health for a project.
- [ ] Library authors can diff public type surfaces across releases.
- [ ] Teams can adopt TypePython incrementally with baselines and type coverage budgets.
- [ ] IDE and CLI workflows explain `Any`, `Unknown`, generated stubs, and checker portability without requiring users to read compiler internals.

## Things Not To Do Yet

- [ ] Do not add broad dependent typing.
- [ ] Do not add TypeScript-style full conditional or mapped types before the shape model.
- [ ] Do not make TypePython require a runtime.
- [ ] Do not make mypy plugins part of the success path.
- [ ] Do not build framework-specific hacks that cannot be expressed through a general transform/shape model.
- [ ] Do not execute arbitrary framework adapter code during compilation.
- [ ] Do not stabilize a public framework adapter registry before multiple adapters prove the abstraction.
- [ ] Do not start sync/async dual emit before the framework-shape path has real users.
- [ ] Do not turn boundary validation into whole-program runtime type checking.
- [ ] Do not make notebook support depend on executing untrusted notebook cells.
- [ ] Do not make AI-generated annotations authoritative without compiler and checker validation.
- [ ] Do not expand into typestate, tensor shape types, or taint tracking before the framework path has real users.
