# Architecture

TypePython is implemented as a virtual Cargo workspace containing 11 Rust crates. Each crate owns a single phase of the compilation pipeline, with clear boundaries enforced by crate-level dependencies.

## Compilation Pipeline

```
                    SOURCE FILES
              (.tpy / .py / .pyi)
                       |
                       v
              +------------------+
              | typepython_syntax|     Parse source into SyntaxTree
              |  (ruff-python)   |     using ruff_python_parser
              +------------------+
                       |
                       v
              +------------------+
              |typepython_binding|     Extract declarations, calls,
              |                  |     guards, returns, yields
              +------------------+
                       |
                       v
              +------------------+
              | typepython_graph |     Build module dependency graph
              |                  |     Inject typing/collections prelude
              +------------------+
                       |
                       v
              +------------------+
              |typepython_checking|    Run multiple type checking rule sets
              |                  |     Produce DiagnosticReport
              +------------------+
                       |
                       v
              +------------------+
              |typepython_lowering|    Convert .tpy to valid Python
              |                  |     Generate source maps
              +------------------+
                       |
                       v
              +------------------+
              | typepython_emit  |     Plan output paths
              |                  |     Generate .pyi stubs
              +------------------+
                       |
                       v
              +--------------------+
              |typepython_incremental|  Compute module fingerprints
              |                    |    Detect changed modules
              +--------------------+
                       |
              +--------+--------+
              |                 |
              v                 v
     +----------------+  +---------------+
     | typepython_cli |  | typepython_lsp|
     | (binary)       |  | (LSP server)  |
     +----------------+  +---------------+
```

## Crate Dependency Graph

```
typepython_diagnostics          (no internal deps -- foundation crate)
       ^
       |
       +--- typepython_config   (diagnostics)
       |
       +--- typepython_syntax   (diagnostics)
       |         ^
       |         |
       |    typepython_binding  (diagnostics, syntax)
       |         ^
       |         |
       |    typepython_graph    (diagnostics, binding)
       |         ^
       |         |
       |    typepython_checking (diagnostics, syntax, binding, graph)
       |
       +--- typepython_lowering (diagnostics, syntax)
       |
       +--- typepython_emit     (diagnostics, config, lowering)
       |
       +--- typepython_incremental (diagnostics, graph)
       |
       +--- typepython_cli      (all crates)
       |
       +--- typepython_lsp      (all crates)
```

## Crate Details

### typepython_diagnostics

The foundation crate shared by every other crate. Defines the diagnostic model used across all compilation phases.

**Key types:**

| Type                   | Description                                                             |
| ---------------------- | ----------------------------------------------------------------------- |
| `Severity`             | `Error` (build-blocking), `Warning`, `Note`                             |
| `Span`                 | Source location: path, line, column, end_line, end_column (all 1-based) |
| `Diagnostic`           | Code, severity, message, notes, suggestions, span                       |
| `DiagnosticSuggestion` | Machine-readable fix with replacement span and text                     |
| `DiagnosticReport`     | Collection with `has_errors()`, `as_text()`, JSON serialization         |

### typepython_config

Project discovery and configuration loading.

**Discovery order:**

1. Walk up from the project directory looking for `typepython.toml`
2. Fall back to `[tool.typepython]` in `pyproject.toml`

**Configuration sections:**

| Section        | Key options                                                                                   |
| -------------- | --------------------------------------------------------------------------------------------- |
| `[project]`    | `src`, `include`, `exclude`, `out_dir`, `cache_dir`, `target_python`                          |
| `[resolution]` | `base_url`, `type_roots`, `python_executable`, `paths`                                        |
| `[format]`     | `command`, `line_length`                                                                      |
| `[emit]`       | `emit_pyi`, `emit_pyc`, `write_py_typed`, `no_emit_on_error`, `runtime_validators`            |
| `[typing]`     | `profile`, `strict`, `strict_nulls`, `imports`, `warn_unsafe`, `enable_sealed_exhaustiveness` |
| `[watch]`      | `debounce_ms`                                                                                 |

**Typing profiles:**

| Profile       | Description                                                  |
| ------------- | ------------------------------------------------------------ |
| `library`     | Strict + `require_known_public_types` for published packages |
| `application` | Strict, relaxed public API requirements                      |
| `migration`   | Lenient, `imports = "dynamic"`, no implicit dynamic warnings |

### typepython_syntax

Parser boundary that wraps `ruff_python_parser` to produce a `SyntaxTree`.

**Source kinds:**

- `TypePython` (`.tpy`) -- full TypePython syntax
- `Python` (`.py`) -- standard Python, pass-through
- `Stub` (`.pyi`) -- type stubs

**Key output:** `SyntaxTree` containing:

- Parsed statements (type aliases, classes, functions, imports, control flow, etc.)
- Type-ignore directives
- Rich metadata: `DirectExprMetadata`, `TypedDictLiteralSite`, `UnsafeOperationSite`

**Unsafe operation tracking:**
`EvalCall`, `ExecCall`, `GlobalsWrite`, `LocalsWrite`, `DictWrite`, `SetAttrNonLiteral`, `DelAttrNonLiteral`

### typepython_binding

Symbol extraction phase that transforms a `SyntaxTree` into a `BindingTable`.

**BindingTable contents:**

| Field                           | Description                                               |
| ------------------------------- | --------------------------------------------------------- |
| `declarations`                  | Top-level and member symbols with kinds, types, modifiers |
| `calls`                         | Function call sites with argument metadata                |
| `method_calls`                  | Method invocations on known receivers                     |
| `member_accesses`               | Attribute access tracking                                 |
| `returns` / `yields`            | Return and yield value tracking                           |
| `if_guards` / `asserts`         | Type narrowing guard conditions                           |
| `matches`                       | Match statement sites for exhaustiveness                  |
| `for_loops` / `with_statements` | Iteration and context manager sites                       |
| `assignments`                   | Annotated and destructuring assignments                   |

**Declaration kinds:** `TypeAlias`, `Class`, `Function`, `Overload`, `Value`, `Import`

**Guard conditions:** `IsNone`, `IsInstance`, `PredicateCall`, `TruthyName`, `Not`, `And`, `Or`

### typepython_graph

Builds the module dependency graph from all binding tables.

**Key behaviors:**

- Injects synthetic `__init__` modules for implicit namespace packages
- Injects **prelude modules**: `typing`, `typing_extensions`, `collections.abc` with standard type declarations
- Computes `summary_fingerprint` (u64) per module for incremental tracking

**Prelude declarations include:** `Any`, `List`, `Dict`, `Tuple`, `Set`, `Callable`, `Literal`, `TypedDict`, `Protocol`, `Awaitable`, `AsyncIterable`, `Iterable`, `Iterator`, `Generator`, and more.

### typepython_checking

The core type-checking engine. Runs multiple diagnostic rule categories against the module graph. The table below is representative, not exhaustive.

**Current checker architecture:**

- Bound `Declaration.detail` strings are normalized once into checker-owned `SemanticDeclarationFacts` and cached by declaration content in the semantic helper layer itself. Callable signatures now cache both semantic parameter data and semantic returns, while type-alias bodies, import targets, and value annotations enter the checker through that same cache-backed semantic surface rather than repeated ad hoc string splitting.
- Generic inference is organized as an explicit solver: the checker collects TypeVar, ParamSpec, and TypeVarTuple constraints, solves them in a separate phase, and preserves structured failure reasons (`GenericSolveFailure`) for diagnostic use.
- Direct overload and callable resolution flow through solver-backed resolved candidates (`ResolvedDirectCallCandidate`) that carry instantiated params, return type, and substitutions. Applicability and specificity use this instantiated information instead of comparing raw declaration text first.
- The active call-diagnostic path keeps structured failure information long enough to explain `TPY4014` and related unresolved generic-call failures with reason notes instead of collapsing directly to `None`.
- Touched semantic/call/assignment diagnostics render semantic types through a shared exit path (`diagnostic_type_text`) so the checker does not maintain multiple independent formatting behaviors in its direct-call path.
- Alias expansion now stays on semantic alias bodies: imported typing rewrites and generic substitution operate on `SemanticType` values directly instead of rendering alias bodies back to text for reparsing.
- Contextual and flow-sensitive owner lookup no longer bridge through rendered owner signature text in the active path; scope-local parameter lookup uses owner declarations and semantic signature sites directly.

**TypeStore decision (implemented narrowly in the main path):**

`TypeStore` is part of the checker declaration-semantic hot path. The shared declaration semantic cache interns declaration-derived semantic types (callable parameter annotations, callable returns, value annotations, alias bodies) into `TypeStore` and materializes semantic facts from those stored IDs. Solver state and final diagnostics continue to use `SemanticType` values as their human-readable working surface, while declaration-driven lookup and reuse rely on the interned store.

The `typepython_checking` Criterion suite remains the baseline for deciding whether to thread Type IDs deeper into solver/candidate/diagnostic boundaries, but the current architecture already treats `TypeStore` as live checker infrastructure rather than a deferred side utility.

**Checker naming conventions:**

| Prefix / term    | Meaning                                                                                                                                                                |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `direct_*`       | Operates on the directly bound module surface from `typepython_binding` (calls, returns, assignments, member accesses, declaration facts) before secondary synthesis |
| `contextual_*`   | Re-types a local expression using an expected type from the surrounding assignment, call, yield, or return site                                                        |
| `imported_*`     | Consults imported-module information instead of only the current module's direct sites                                                                                 |
| `instantiated_*` | Applies generic substitutions before validating a callable or signature                                                                                                |
| `synthetic_*`    | Uses checker-authored helper surfaces such as built-in signatures or synthesized stub methods                                                                          |

These prefixes are descriptive rather than exhaustive. A single diagnostic pass may combine direct sites with contextual or instantiated helper logic when resolving a final type judgment.

**Check rules:**

| Rule                           | Validates                                      |
| ------------------------------ | ---------------------------------------------- |
| `ambiguous_overload_call`      | Overload resolution is unambiguous             |
| `direct_unknown_operation`     | No operations on `unknown` without narrowing   |
| `unresolved_import`            | All imports resolve to known modules           |
| `direct_member_access`         | Member access on known types                   |
| `unsafe_boundary`              | Unsafe operations confined to `unsafe:` blocks |
| `deprecated_use`               | Deprecated symbol usage                        |
| `direct_method_call`           | Method calls match receiver type               |
| `direct_return_type`           | Return values match declared type              |
| `direct_yield_type`            | Yield values match declared type               |
| `for_loop_target`              | Iteration target is iterable                   |
| `destructuring_assignment`     | Unpacking matches structure                    |
| `with_statement`               | Context manager protocol compliance            |
| `direct_call_arity`            | Correct number of arguments                    |
| `direct_call_type`             | Argument types match parameters                |
| `direct_call_keyword`          | Keyword arguments are valid                    |
| `annotated_assignment_type`    | Annotation matches assigned value              |
| `typed_dict_literal`           | TypedDict shape validation                     |
| `typed_dict_readonly_mutation` | Read-only field immutability                   |
| `frozen_dataclass_mutation`    | Frozen field immutability                      |
| `attribute_assignment_type`    | Attribute assignment type correctness          |
| `duplicate`                    | No duplicate declarations                      |
| `override`                     | `@override` validation and enforcement         |
| `final_decorator`              | `@final` violation detection                   |
| `abstract_member`              | Abstract class instantiation prevention        |

**Built-in signature knowledge:** `len`, `str`, `int`, `float`, `bool`, `bytes`, `list`, `dict`, `tuple`, `set`, `frozenset`, `range`, `input`, `print`, `ord`, `chr`, `hash`, `id`, `cast`, `TypeVar`, `ParamSpec`, `TypeVarTuple`, `NewType`.

### typepython_lowering

Converts TypePython syntax to valid Python with full source maps.

**Lowering transformations:**

| Input                   | Output                                                            |
| ----------------------- | ----------------------------------------------------------------- |
| `data class Foo:`       | `@dataclass` + `class Foo:`                                       |
| `interface Bar:`        | `class Bar(Protocol):`                                            |
| `sealed class Expr:`    | `class Expr:  # tpy:sealed`                                       |
| `overload def f():`     | `@overload def f():`                                              |
| `typealias X = T`       | `X: TypeAlias = T` with helper `TypeVar` declarations when needed |
| Inline `[T]` generics   | `TypeVar` imports + `Generic[T]` bases                            |
| Annotated lambda params | Normalized to Python-legal form                                   |

**Output:** `LoweredModule` with:

- `python_source` -- valid Python text
- `source_map` -- line-to-line mapping (original <-> lowered)
- `span_map` -- column-level mapping with segment kinds: `Copied`, `Inserted`, `Rewritten`, `Synthetic`
- `required_imports` -- synthesized import statements

**Passthrough:** `.py` files are copied with 1:1 source mapping. `.pyi` files pass through as-is.

### typepython_emit

Plans output artifacts and generates type stubs.

**Artifact planning:**

| Source | Runtime output  | Stub output        |
| ------ | --------------- | ------------------ |
| `.tpy` | `.py` (lowered) | `.pyi` (generated) |
| `.py`  | `.py` (copy)    | --                 |
| `.pyi` | --              | `.pyi` (copy)      |

**Features:**

- Stub generation with value/callable overrides and synthetic methods
- Runtime validator injection for TypedDict classes (experimental)
- `py.typed` marker file writing for PEP 561 compliance
- Optional `.pyc` bytecode compilation

### typepython_incremental

Fingerprint-based incremental build tracking.

**IncrementalState** contains:

- `fingerprints` -- `BTreeMap<module_key, u64>` using FNV-1a hash
- `summaries` -- public API summary per module (exports, imports, sealed roots)
- `stdlib_snapshot` -- optional stdlib version tag

**Rebuild rules:**

- Implementation-only change (fingerprint same): dependents NOT rechecked
- Public summary change (fingerprint differs): direct and transitive dependents rechecked

**Persistence:** JSON-encoded with schema versioning, stored in `.typepython/cache/snapshot.json`.

### typepython_cli

User-facing binary implementing all commands.

**Full pipeline steps:**

1. `discover_sources()` -- glob-based source discovery
2. `load_syntax_trees()` -- parse all files
3. `apply_type_ignore_directives()` -- process `# type: ignore`
4. `bind()` -- extract symbols
5. `build()` -- assemble module graph
6. `check_with_options()` -- run type checker
7. `lower_with_options()` -- convert to Python
8. `plan_emits()` -- plan output paths
9. `snapshot()` -- capture fingerprints
10. `write_runtime_outputs()` -- write files to disk

**Embedded resources:** Project init templates from `templates/`.

### typepython_lsp

Language Server Protocol implementation using stdio-based JSON-RPC.

**Supported methods:**

| Method                        | Feature                                                 |
| ----------------------------- | ------------------------------------------------------- |
| `textDocument/didOpen`        | Open document overlay                                   |
| `textDocument/didChange`      | Update document overlay (incremental or full-text sync) |
| `textDocument/didClose`       | Close document                                          |
| `textDocument/hover`          | Type information at cursor                              |
| `textDocument/definition`     | Jump to definition                                      |
| `textDocument/references`     | Find all usages                                         |
| `textDocument/formatting`     | Format current document                                 |
| `textDocument/signatureHelp`  | Show active call signature                              |
| `textDocument/documentSymbol` | List symbols in current document                        |
| `workspace/symbol`            | Search declarations across workspace                    |
| `textDocument/rename`         | Rename symbol across project                            |
| `textDocument/codeAction`     | Quick fixes from diagnostics                            |
| `textDocument/completion`     | Autocomplete (triggered on `.`)                         |

**Internal architecture:**

- In-memory document overlays for unsaved changes
- Persistent incremental workspace cache for project/support syntax trees and bindings
- Snapshot-diff invalidation via `typepython_incremental`
- Subset checker reruns for directly changed modules and dependent modules whose public summaries changed
- Incrementally refreshed query indexes for document, occurrence, and module-node lookups
- Diagnostics pushed to editor after each incremental update

## Workspace Configuration

**Rust edition:** 2024
**Pinned development toolchain:** 1.94.0
**Minimum supported Rust version:** 1.85
**Resolver:** v3

**Lint policy:**

- `unsafe_code` -- forbidden
- `unwrap_used`, `todo`, `dbg_macro` -- denied
- All clippy lints -- warned

**Release profile:**

- Single codegen unit
- Thin LTO

## External Dependencies

| Dependency                      | Purpose                                      |
| ------------------------------- | -------------------------------------------- |
| `ruff-python`                   | Python AST parsing                           |
| `clap`                          | CLI argument parsing with derive macros      |
| `serde` / `serde_json` / `toml` | Serialization for config, diagnostics, cache |
| `notify`                        | Filesystem watching for `watch` command      |
| `tracing`                       | Structured logging                           |
| `anyhow` / `thiserror`          | Error handling                               |
| `flate2` / `tar` / `zip`        | Archive support for `verify` command         |
| `glob`                          | File pattern matching                        |
| `url`                           | URL parsing for resolution config            |

## Design Decisions

1. **Virtual workspace** -- profiles, shared dependency versions, and lint policy live at the root
2. **Crate-per-phase** -- each compilation phase is isolated; phases can evolve independently
3. **No unsafe code** -- `forbid(unsafe_code)` workspace-wide
4. **Deterministic output** -- sorted keys, stable fingerprints, reproducible builds
5. **Source maps everywhere** -- line-and-column mapping from `.tpy` to `.py` for accurate error reporting
6. **Prelude injection** -- typing/collections.abc always available without explicit import
7. **Guard combinators** -- type narrowing supports `And`/`Or`/`Not` for compound conditions
8. **Profile-based defaults** -- library/application/migration presets for consistent configuration
