# TypePython v1 Artifact and Tooling Specification

**Status:** draft, normative for the first shippable v1 implementation  
**Scope:** project model, artifact authority, lowering, diagnostics, cache, CLI, and LSP  
**Numbering note:** original section numbering is preserved for stable reference.

This document defines the parts of TypePython v1 that govern project discovery, artifact authority, lowering and emission, structured diagnostics, incremental behavior, and external tooling protocols.

Sections intentionally included here:

- Sections 5-6
- Section 13
- Sections 16-19

---

## 5. Project Model and Configuration

### 5.1 Project Definition

A project is defined by one TypePython configuration source and one or more source roots.

The configuration source is either:

- a standalone `typepython.toml` file, or
- a `[tool.typepython]` table inside `pyproject.toml`

### 5.2 Source Roots and File Classification

Within a source root:

| File Kind | Treatment                                           |
| --------- | --------------------------------------------------- |
| `.tpy`    | TypePython compilation unit, parsed by the frontend |
| `.py`     | Pass-through Python module, copied unchanged        |
| `.pyi`    | Stub input for typing, MAY also be copied to output |

### 5.3 Package Model

Core v1 uses Python's package model with deterministic source-graph rules:

- A directory containing `__init__.py`, `__init__.tpy`, or `__init__.pyi` is a regular package.
- A directory without an `__init__` file MAY still contribute an implicit namespace package when it contains descendant modules or packages in the source graph.
- If two source files map to the same logical module, the compiler MUST issue an error.

### 5.4 Module Path Collisions (Compile Errors)

The following MUST be diagnosed as compile errors:

- `pkg/foo.tpy` and `pkg/foo.py` in the same source graph
- `pkg/foo.tpy` and `pkg/foo.pyi` in the same source graph
- `pkg/__init__.tpy` and `pkg/__init__.py` in the same source graph
- Two source roots contributing the same logical module path

`pkg/foo.py` and `pkg/foo.pyi` MAY coexist. In that case the `.py` file is the runtime implementation and the `.pyi` file is its companion stub.

### 5.5 Configuration File

#### 5.5.1 File Name and Location

The project configuration source MAY be either `typepython.toml` or a `[tool.typepython]` table inside `pyproject.toml`.

If no explicit project path is provided on the CLI, the implementation MUST search upward from the working directory until reaching the filesystem root. At each directory level, the search order is:

1. `typepython.toml`
2. `pyproject.toml` containing `[tool.typepython]`

If both exist in the same directory, `typepython.toml` is authoritative for TypePython configuration in that directory.

#### 5.5.2 CLI Override

CLI flags MUST override configuration file values. Unknown config keys MUST be diagnosed. Invalid config values MUST be diagnosed before build graph creation.

#### 5.5.3 v1 Schema

```toml
[project]
src = ["src"]                                    # List of source roots
include = ["src/**/*.tpy", "src/**/*.py", "src/**/*.pyi"]  # Glob patterns
exclude = [".typepython/**", "dist/**", ".venv/**", "venv/**"]  # Exclude patterns
root_dir = "src"                                 # Logical source root
out_dir = ".typepython/build"                   # Output directory
cache_dir = ".typepython/cache"                  # Cache directory
target_python = "3.10"                           # "3.10", "3.11", "3.12", "3.13", or "3.14"

[resolution]
base_url = "."                                    # Reserved; only project-root default is supported today
type_roots = []                                   # Extra stub directories
python_executable = null                          # Interpreter used for installed-package resolution outside safe structural verify mode
analysis_python = "3.13"                          # Optional support-surface analysis interpreter version

[resolution.paths]
# "@app/*" = ["src/app/*"]                       # Reserved; non-empty tables are rejected today

[emit]
emit_pyi = true                                  # Emit .pyi stub files
emit_pyc = false                                 # Emit .pyc files
write_py_typed = true                            # Emit py.typed marker
preserve_comments = true                         # Current implementations always preserve comments when available
no_emit_on_error = true                          # Block best-effort emit on semantic errors
runtime_validators = false                       # Experimental: emit runtime validators for data class
emit_style = "compat"                            # "compat" or "native"; defaults by target version

[typing]
profile = null                                    # "library", "application", or "migration"
strict = true                                    # Master strictness
strict_nulls = true                              # None excluded from T
imports = "unknown"                              # "unknown" or "dynamic"
no_implicit_dynamic = true                      # Disallow implicit dynamic
warn_unsafe = true                               # Warn on unsafe boundaries
enable_sealed_exhaustiveness = true              # Check sealed exhaustiveness
report_deprecated = "warning"                    # "ignore", "warning", or "error"
require_explicit_overrides = false               # Require @override on overriding members
require_known_public_types = false               # Disallow exported dynamic/unknown types
infer_passthrough = false                        # Experimental: best-effort inference for .py files

[watch]
debounce_ms = 80                                 # Debounce delay in ms
```

When this schema is embedded in `pyproject.toml`, the same tables appear under `[tool.typepython]`, for example `[tool.typepython.project]` and `[tool.typepython.typing]`.

#### 5.5.4 Field Semantics

**`[project]` fields:**

| Field           | Type         | Semantics                                 |
| --------------- | ------------ | ----------------------------------------- |
| `src`           | list[string] | Source roots. MUST default to `["src"]`   |
| `include`       | list[string] | Glob patterns relative to config file     |
| `exclude`       | list[string] | Glob patterns applied after include       |
| `root_dir`      | string       | Logical root for relative output paths    |
| `out_dir`       | string       | Root for emitted `.py` and `.pyi`         |
| `cache_dir`     | string       | Root for incremental state                |
| `target_python` | string       | Target version: `3.10`, `3.11`, `3.12`, `3.13`, or `3.14` |

**`[resolution]` fields:**

| Field               | Type           | Semantics                                                                                    |
| ------------------- | -------------- | -------------------------------------------------------------------------------------------- |
| `base_url`          | string         | Reserved for non-relative path resolution; current implementations only support the default project root (`.`) |
| `type_roots`        | list[string]   | Directories searched for stub packages before installed packages                             |
| `python_executable` | string \| null | Interpreter used to locate installed packages and, when supported, verification subprocesses; safe structural verify MAY ignore project-controlled interpreters |
| `analysis_python`  | string \| null | Python version used to select support-surface inputs; defaults to `project.target_python` when omitted |
| `paths`             | table          | Reserved for alias mapping from module patterns to filesystem patterns; current implementations reject non-empty tables |

Path mappings are reserved for future static resolution support. Current implementations MUST reject non-empty tables rather than silently ignoring them.

If `python_executable` is configured and its resolved Python major/minor version is incompatible with the effective analysis Python version, ordinary configuration loading MUST diagnose `TPY1002`. The effective analysis Python version is `resolution.analysis_python` when configured, otherwise `project.target_python`. A safe structural verification mode MAY bypass execution-based validation of a project-controlled interpreter, but it MUST document that behavior and surface when the configured interpreter was ignored.

**`[emit]` fields:**

| Field                | Type | Semantics                                                       |
| -------------------- | ---- | --------------------------------------------------------------- |
| `emit_pyi`           | bool | Emit `.pyi` declaration stubs for each compiled `.tpy` module   |
| `emit_pyc`           | bool | Compile `.py` to `.pyc` using target interpreter                |
| `write_py_typed`     | bool | Emit `py.typed` marker for typed packages                       |
| `preserve_comments`  | bool | Reserved toggle for future comment-stripping control; current implementations always preserve comments when available |
| `no_emit_on_error`   | bool | Block best-effort output after semantic/public-surface errors; discovery/parse/lowering remain hard blockers |
| `runtime_validators` | bool | Experimental: emit `validate` classmethod on `data class` types |
| `emit_style`         | string | Lowering strategy: `compat` preserves broad legacy compatibility, `native` preserves target-native typing syntax when supported |

**`[typing]` fields:**

| Field                          | Type           | Semantics                                                                             |
| ------------------------------ | -------------- | ------------------------------------------------------------------------------------- |
| `profile`                      | string \| null | Optional named adoption profile; explicit keys still override profile defaults        |
| `strict`                       | bool           | Master strictness switch                                                              |
| `strict_nulls`                 | bool           | `None` excluded from `T` unless explicitly included                                   |
| `imports`                      | string         | Default type for untyped imports: `unknown` or `dynamic`                              |
| `no_implicit_dynamic`          | bool           | Diagnose silent fallback to `dynamic`                                                 |
| `warn_unsafe`                  | bool           | Controls unsafe-boundary severity                                                     |
| `enable_sealed_exhaustiveness` | bool           | Enable exhaustiveness checks for sealed match targets                                 |
| `report_deprecated`            | string         | Severity for deprecated-symbol use: `ignore`, `warning`, or `error`                   |
| `require_explicit_overrides`   | bool           | Require `@override` on overriding methods and properties                              |
| `require_known_public_types`   | bool           | Diagnose exported surfaces containing `dynamic` or unresolved `unknown`               |
| `infer_passthrough`            | bool           | Experimental best-effort type inference for pass-through `.py` files (Section 18.8.2) |

**`[watch]` fields:**

| Field         | Type | Semantics                                      |
| ------------- | ---- | ---------------------------------------------- |
| `debounce_ms` | int  | Minimum delay between invalidation and rebuild |

#### 5.5.5 Schema Validity Rules

The v1 configuration schema is closed unless this document explicitly states otherwise.

At minimum:

- unknown top-level tables MUST be diagnosed
- unknown keys inside a recognized table MUST be diagnosed
- implementations MUST reject values whose types do not match the declared schema
- implementations MUST apply CLI overrides only after configuration-file parsing and schema validation complete
- implementations MUST NOT silently reinterpret an invalid key as a profile expansion, alias, or deprecated spelling

#### 5.5.6 Profile Expansion

`typing.profile`, when present, expands to a deterministic starter configuration. Explicit sibling keys in `[typing]` override the profile defaults.

The following v1 profiles are reserved:

- `library`: `strict = true`, `strict_nulls = true`, `imports = "unknown"`, `no_implicit_dynamic = true`, `warn_unsafe = true`, `require_known_public_types = true`
- `application`: `strict = true`, `strict_nulls = true`, `imports = "unknown"`, `no_implicit_dynamic = true`, `warn_unsafe = true`, `require_known_public_types = false`
- `migration`: `strict = false`, `strict_nulls = true`, `imports = "dynamic"`, `no_implicit_dynamic = false`, `warn_unsafe = true`, `report_deprecated = "ignore"`, `require_known_public_types = false`

Profiles leave `require_explicit_overrides = false` unless explicitly overridden by the user. Profiles other than `migration` leave `report_deprecated = "warning"` unless explicitly overridden by the user.

Support for the `library` profile is incomplete unless the implementation also provides the Core v1 `typepython verify` command and the typed-publication checks from Section 13.6.4.

Implementations MUST NOT silently invent additional profile names in v1.

### 5.6 Output Layout

The output tree MUST mirror the logical module structure of the source tree:

```
src/pkg/core.tpy   -> .typepython/build/pkg/core.py
src/pkg/core.tpy   -> .typepython/build/pkg/core.pyi  (if emit_pyi = true)
src/pkg/util.py    -> .typepython/build/pkg/util.py   (copied verbatim)
src/pkg/types.pyi  -> .typepython/build/pkg/types.pyi (copied if included)
```

---

## 6. File Kinds and Authority

### 6.1 Input File Kinds

| Suffix | Description         | Parsed By                                        |
| ------ | ------------------- | ------------------------------------------------ |
| `.tpy` | TypePython source   | TypePython frontend                              |
| `.py`  | Pass-through Python | Resolved for packaging; optionally for type info |
| `.pyi` | Stub input          | Type information only                            |

### 6.2 Output File Kinds

| Suffix | Description       | Authority                                                 |
| ------ | ----------------- | --------------------------------------------------------- |
| `.py`  | Emitted code      | **Runtime authority**: definitive for execution           |
| `.pyi` | Declaration stub  | **Type authority**: definitive for external type checkers |
| `.pyc` | Compiled bytecode | Optional cache artifact                                   |

### 6.3 Authority Boundaries

#### 6.3.1 Runtime Authority (`.py`)

The emitted `.py` file is authoritative for runtime behavior.

- It MUST be syntactically valid Python for the configured `target_python`.
- It MUST preserve import order, execution order, and observable side effects.
- It MAY contain shallow or stringified annotations that do not encode the full TypePython model.

#### 6.3.2 Type Authority (`.pyi`)

The emitted `.pyi` file is authoritative for external type checking.

- It MUST contain the complete public API surface.
- It MUST materialize generic parameters, overloads, protocols, and aliases in standard Python typing form.
- It MUST omit implementation details that are not part of the public contract.

#### 6.3.3 Summary Authority (Public Summary)

The compiler's public summary is authoritative for incremental invalidation inside TypePython itself.

- It MUST contain enough information to determine whether dependents need rechecking.
- It is internal to the compiler and does not replace `.pyi` as the external declaration artifact.

#### 6.3.4 Lowering Metadata Authority

Lowering metadata is authoritative for editor tooling and diagnostic remapping.

- It MUST map original `.tpy` spans to emitted spans.
- It MAY remain an internal artifact in v1.

### 6.4 Ambient Declarations (Stub Semantics)

A `.pyi` file is an **ambient declaration surface** — a source of type information that does not produce runtime code.

#### 6.4.1 Stub Declaration Properties

All declarations in a `.pyi` file have the following properties:

1. **Declaration-only**: Stub declarations are not executable. They contribute type information but do not generate Python code.

2. **No implementation**: Stub declarations MUST NOT contain executable bodies. Use `...` for function bodies:

   ```python
   def process(items: list[int]) -> None: ...
   ```

3. **Nominal identity preserved**: Stub class declarations create nominal types, matching Python `.pyi` semantics.

4. **Generic support**: Stub files support generic declarations using standard Python typing syntax:

   ```python
   from typing import TypeVar, Generic

   T = TypeVar("T")

   class Container(Generic[T]):
       value: T
       def get(self) -> T: ...
   ```

#### 6.4.2 Stub-Only Constructs

The following declaration patterns are relevant when consuming `.pyi` files in Core v1:

| Construct                 | Example                      | Purpose                        |
| ------------------------- | ---------------------------- | ------------------------------ |
| Pure interface stubs      | `class Proto(Protocol): ...` | Structural protocol definition |
| Re-export with type alias | `MyList = list[int]`         | Type alias re-export           |

Core v1 does not introduce a separate `declare` syntax. Ambient declaration behavior comes from `.pyi` files themselves.

#### 6.4.3 Stub Resolution Priority

When resolving types from external modules, the following priority applies:

1. **Local `.tpy`** (if same module path exists)
2. **Local `.pyi`** (companion stub for local `.py`)
3. **Installed stubs** (from `type_roots` or `py.typed` packages)
4. **Runtime `.py`** with inline annotations

#### 6.4.4 Stub Emission vs. Stub Input

- **Emitted stubs** (from `.tpy` → `.pyi`): Contain full public API surface with type information
- **Input stubs** (consumed for typing): May be partial; unknown types fall back to `unknown` or `dynamic` per config

#### 6.4.5 Stub Type Authority

A `.pyi` file is authoritative for:

- Public API surface shape
- Generic parameterization
- Type aliases exported from the module
- Base class and interface relationships

A `.pyi` file is NOT authoritative for:

- Private/internal member details (unless explicitly exported)
- Implementation details
- Runtime behavior

---

## 13. Semantic Elaboration, Lowering, and Code Generation

### 13.1 General Rule

TypePython v1 distinguishes **semantic elaboration** from purely syntactic lowering.

The required phase order is:

1. parse source into concrete or abstract syntax
2. bind names, imports, and declaration ownership
3. type declaration surfaces (signatures, aliases, class headers, inheritance, and exported annotations)
4. semantically elaborate features that require bound declarations or declaration typing
5. type-check bodies
6. emit `.py`, `.pyi`, summaries, and cache artifacts

Syntactic lowering is the subset of step 4 and step 6 that rewrites TypePython-only syntax into legal Python text. Features such as `TypedDict` utility transforms and sealed-hierarchy closure are not parse-only rewrites; they require semantic elaboration after binding.

Lowering MUST produce:

- Emitted Python source text
- A lowering map from original spans to emitted spans
- Module-level metadata for the checker and emitter
- A required-import set

### 13.2 Determinism

Given identical source text, target version, and compiler options, lowering MUST be deterministic.

### 13.3 Import Injection

The lowerer MAY inject imports only when required by the lowered form.

**Allowed injected imports in Core v1:**

- `from __future__ import annotations`
- `from dataclasses import dataclass`
- imports from `typing` required to express the lowered runtime or public type surface for the configured `target_python`, including `Protocol`, `TypeAlias`, `TypeVar`, `Generic`, `TypedDict`, `Required`, `NotRequired`, and `overload`
- `from warnings import deprecated` when required to express a target-version-compatible public surface
- imports from `typing_extensions` when required by Section 12.6.1 or Section 12.6.2 to express a target-version-compatible public surface

The lowerer MUST avoid duplicate imports and preserve module docstrings and leading comments where possible.

### 13.4 Lowering Transformations

#### 13.4.1 `data class`

```python
# Input
data class Point:
    x: float
    y: float

# Output
from dataclasses import dataclass

@dataclass
class Point:
    x: float
    y: float
```

Checker metadata MUST record that `Point` originated from `data class`.

#### 13.4.2 `interface`

```python
# Input
interface SupportsClose:
    def close(self) -> None: ...

# Output
from typing import Protocol

class SupportsClose(Protocol):
    def close(self) -> None: ...
```

An interface body MAY contain:

- Annotated attributes
- Method signatures
- Overload declarations
- Docstrings

An interface body MUST NOT contain executable statements other than `...` or `pass`.
Interface method signatures follow ordinary Python instance-method conventions; when lowered to `Protocol`, instance methods include `self` exactly as normal methods do.

#### 13.4.3 `typealias`

```python
# Input
typealias UserId = int

# Output
from typing import TypeAlias

UserId: TypeAlias = int
```

Generic aliases follow the same rule in `.pyi`. In `.py`, generic alias parameters MAY remain represented in metadata rather than as explicit runtime `TypeVar` declarations.

#### 13.4.4 `sealed class`

```python
# Input
sealed class Expr:
    ...

# Output
class Expr: ...  # tpy:sealed
```

The emitted comment is informative. The normative sealed relationship lives in compiler metadata and public summaries, including the defining module that closes the hierarchy for Core v1 exhaustiveness.

#### 13.4.5 `overload def`

```python
# Input
overload def parse(x: str) -> int: ...
overload def parse(x: bytes) -> int: ...

def parse(x):
    return 0

# Output
from typing import overload

@overload
def parse(x: str) -> int: ...

@overload
def parse(x: bytes) -> int: ...

def parse(x):
    return 0
```

Each overload declaration MUST be followed in the same module by exactly one concrete implementation.

#### 13.4.6 `unsafe`

```python
# Input
unsafe:
    obj.__dict__[name] = value

# Output
if True:  # tpy:unsafe
    obj.__dict__[name] = value
```

The emitted form has zero semantic effect beyond preserving block structure.

#### 13.4.7 Experimental Conditional Return Types

```python
# Input
def decode(x: str | bytes | None) -> match x:
    case str: str
    case bytes: str
    case None: None

# Output in .py and .pyi
from typing import overload

@overload
def decode(x: str) -> str: ...
@overload
def decode(x: bytes) -> str: ...
@overload
def decode(x: None) -> None: ...

def decode(x: str | bytes | None) -> str | None:
    ...
```

This transformation is Experimental v1 only. When enabled, the lowerer MUST generate one `@overload` declaration per `case` arm, narrowing the matched parameter type and using the arm's return type. The implementation signature uses the original union parameter type and the union of all arm return types.

#### 13.4.8 Type Transforms

Built-in `TypedDict` transforms (Section 8.14) are expanded during semantic elaboration and emitted during lowering.

```python
# Input
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
    email: str

typealias UserCreate = Omit[User, "id"]

# Output in .pyi
from typing import TypedDict

class UserCreate(TypedDict):
    name: str
    email: str
```

The lowerer MUST:

- Resolve the source `TypedDict`'s item set after binding and alias substitution
- Apply the transform operation
- Emit a standalone type declaration with the transformed fields
- Preserve field types, requiredness, and read-only markers through the transform

If the expanded form requires imports (e.g., `NotRequired`, `ReadOnly`), those imports MUST be included in the emitted output.

#### 13.4.9 Type Parameters

```python
# Input
def first[T](xs: Sequence[T]) -> T:
    return xs[0]

# Output in .py
from __future__ import annotations

def first(xs: "Sequence[T]") -> "T":
    return xs[0]  # tpy:typeparams[T]

# Output in .pyi
from typing import Sequence, TypeVar

T = TypeVar("T")

def first(xs: Sequence[T]) -> T: ...
```

**Normative rule:**

- `.py` emit MAY preserve type parameters only in metadata and string annotations
- `.pyi` emit MUST materialize generic parameters in standard Python typing form

If `from __future__ import annotations` is injected, runtime `__annotations__` values become strings. TypePython does not guarantee runtime annotation introspection preserves the full model; `.pyi` output and compiler summaries remain authoritative.

When `.py` emit preserves type parameters only in metadata, the lowering metadata for each generic declaration MUST record at least:

- declaration kind (`function`, `class`, `interface`, or `typealias`)
- declaration qualified name within the module
- ordered parameter list
- per-parameter upper bound if present

If persisted to disk, this metadata MUST use a deterministic serialization format.

An equivalent persisted record MAY take a form such as:

```json
{
  "kind": "function",
  "name": "first",
  "typeParams": [{ "name": "T", "bound": null }]
}
```

### 13.5 Lowering Map

The compiler MUST maintain a lowering map for every compiled module.

Each mapping segment MUST contain:

- Source file path
- Emitted file path
- Source start line and column
- Source end line and column
- Emitted start line and column
- Emitted end line and column
- Segment kind: `copied`, `inserted`, `rewritten`, or `synthetic`

**Use cases:**

- Diagnostics reported against `.tpy`
- Go-to-definition and references in LSP
- Code actions and quick fixes
- Incremental invalidation at statement granularity

The map MAY remain an internal cache artifact in v1; no user-facing sourcemap format is required.

### 13.6 Emission Rules

#### 13.6.1 `.py` Emission

- MUST preserve execution order
- SHOULD preserve comments where feasible
- MUST preserve import semantics
- MAY use `# tpy:*` markers for internal metadata (no runtime effect)
- Discovery, parse, and lowering failures MUST block `.py` emission
- If `no_emit_on_error = true`, semantic and public-surface diagnostics MUST block `.py` emission

#### 13.6.2 `.pyi` Emission

The stub emitter MUST output the public API surface with:

- Fully typed function signatures
- Overload declarations
- Generic parameters using standard Python typing
- Protocol definitions for interfaces
- Aliases as valid Python stubs
- `NewType` declarations preserved as `NewType`
- Preserved `Annotated[...]`, `ClassVar[...]`, `Required[...]`, `NotRequired[...]`, and `ReadOnly[...]` wrappers when they are part of the authoritative public source annotation

The stub emitter SHOULD preserve informative sealed metadata in comments when useful to TypePython tooling, for example:

```python
# tpy:sealed Expr -> {Num, Add}
```

The stub emitter MUST generate `TypeVar` declarations and `Generic[...]` bases as needed.

If `write_py_typed = true` and output represents a package, the emitter MUST write `py.typed` at the package root.

#### 13.6.3 Public API Completeness and Library Publishability

If `typing.require_known_public_types = true`, the emitter MUST reject any module whose public surface is not type-complete per Section 11.5.

An implementation MAY still emit private implementation details containing `dynamic` or unresolved `unknown` so long as they do not leak into the authoritative exported surface.

Implementations SHOULD make it easy to tell library authors which exported declaration caused incompleteness, and SHOULD point to the leaking symbol rather than only the downstream use site.

#### 13.6.4 Packaging Artifact Consistency

TypePython does not require the compiler itself to build wheels or sdists, but it DOES define the normative expectations for typed artifact publication.

When an emitted package is published as a typed distribution:

- the wheel and sdist MUST contain the same emitted `.py` and `.pyi` module tree for the published package surface
- if `write_py_typed = true`, the `py.typed` marker MUST be included in every published package artifact that is intended to advertise inline type support
- omission of emitted `.pyi` files or `py.typed` from a selected published artifact MUST cause `typepython verify` to fail when that omission changes the authoritative published type surface

Package backends MAY add non-TypePython files such as tests, documentation, license files, or backend metadata, but they MUST NOT change the authoritative runtime/type surface between artifact kinds.

These requirements are about publishability and downstream-tool consumption, not about mandating a specific packaging backend.

#### 13.6.5 Runtime Validator Emission

Runtime validator emission is Experimental v1. When `emit.runtime_validators = true`, the emitter MUST generate a `validate` classmethod on each `data class` type in the emitted `.py`:

```python
# Input (.tpy)
data class UserInput:
    name: str
    age: int
    email: str | None = None

# Emitted .py (with runtime_validators = true)
from dataclasses import dataclass

@dataclass
class UserInput:
    name: str
    age: int
    email: str | None = None

    @classmethod
    def __tpy_validate__(cls, __data: dict) -> "UserInput":
        # Auto-generated runtime type validator
        ...
```

**Validator rules:**

- The generated validator MUST check each field's runtime type using `isinstance` or equivalent checks for the supported type forms.
- Supported runtime-checkable types in Experimental v1: `int`, `float`, `str`, `bytes`, `bool`, `None`, `list`, `dict`, `set`, `tuple`, and nominal class types. For generic types (`list[int]`), the container type is checked but element types MAY be checked only shallowly (first element) or skipped with a documented limitation.
- Union types are checked by attempting each branch.
- `TypedDict` fields are checked key-by-key.
- Types that cannot be runtime-checked (e.g., `Protocol`, unbounded `TypeVar`, `Callable`) MUST be skipped with no runtime check for that field rather than generating incorrect code.
- The validator method name is `__tpy_validate__` to avoid collision with user-defined methods. An implementation MAY additionally generate a `validate` alias.
- The validator MUST raise `TypeError` or `ValueError` with a message identifying the failing field, expected type, and actual type.
- Validator generation is purely additive — it MUST NOT change the behavior of the emitted code when the validator is not called.
- The generated validator MUST NOT appear in emitted `.pyi` stubs unless the implementation explicitly documents it as part of the public runtime API.

### 13.7 Pass-Through Python Files

If a `.py` file is part of the project inputs and does not collide with a `.tpy` module:

- the build MUST copy it unchanged to `out_dir`
- the checker MAY read inline annotations from it for dependency typing
- the compiler MUST NOT rewrite its runtime semantics in Core v1

If a pass-through `.py` module has no usable annotations and no stub, its exported surface is treated according to `typing.imports`.

---

## 16. Diagnostics

### 16.1 Required Diagnostic Shape

Every diagnostic MUST include:

- Stable error code
- Severity (error, warning, note)
- Primary message
- Source span in the original `.tpy` if one exists
- Optional notes or suggested fixes

### 16.2 Error Code Bands

| Range     | Category                             |
| --------- | ------------------------------------ |
| `TPY1xxx` | Configuration and project graph      |
| `TPY2xxx` | Scanning, parsing, and lowering      |
| `TPY3xxx` | Import and module resolution         |
| `TPY4xxx` | Type checking and flow analysis      |
| `TPY5xxx` | Emit and stub generation             |
| `TPY6xxx` | Cache, watch, and LSP infrastructure |

At minimum, a Core v1 implementation MUST reserve the following concrete codes with these meanings:

| Code      | Meaning                                                                 |
| --------- | ----------------------------------------------------------------------- |
| `TPY1001` | Invalid or unreadable TypePython configuration source                   |
| `TPY1002` | Unsupported or invalid configuration value                              |
| `TPY2001` | Parse error in `.tpy` source                                            |
| `TPY2002` | Invalid TypePython-only syntax lowering precondition                    |
| `TPY3001` | Module not found                                                        |
| `TPY3002` | Conflicting module path in source graph                                 |
| `TPY4001` | Type mismatch in assignment or return                                   |
| `TPY4002` | Member access on incompatible type                                      |
| `TPY4003` | Unsupported operation on `unknown`                                      |
| `TPY4004` | Duplicate declaration in a declaration space                            |
| `TPY4005` | Incompatible override of base class member                              |
| `TPY4006` | Reassignment of `Final` binding                                         |
| `TPY4007` | Direct instantiation of abstract class                                  |
| `TPY4008` | Concrete class does not implement all abstract methods                  |
| `TPY4009` | Non-exhaustive `match` over finite domain                               |
| `TPY4010` | Deferred-beyond-v1 construct in `.tpy` source                           |
| `TPY4011` | Invalid assignment or deletion target                                   |
| `TPY4012` | Ambiguous overload resolution                                           |
| `TPY4013` | Invalid `TypedDict` literal or keyword expansion                        |
| `TPY4014` | Unresolved `ParamSpec`/`Concatenate` call shape                         |
| `TPY4015` | Incomplete exported type surface                                        |
| `TPY4016` | Mutation of read-only `TypedDict` item                                  |
| `TPY4017` | Invalid type transform argument (unknown field, unsupported input type) |
| `TPY4018` | Incomplete conditional return type coverage                             |
| `TPY4101` | Use of deprecated declaration                                           |
| `TPY5001` | `.pyi` generation failure                                               |
| `TPY5002` | Best-effort emit blocked by `no_emit_on_error` after semantic errors     |
| `TPY5003` | Public API verification failure                                         |
| `TPY6001` | Incremental cache incompatibility or corruption                         |
| `TPY6002` | LSP overlay/state synchronization failure                               |

### 16.2.1 Recommended Code Mapping for Core v1 Rules

The following mappings are normative where the corresponding rule is triggered:

- Invalid assignment target or invalid `del` target: `TPY4011`
- Ambiguous overload resolution after applicability filtering: `TPY4012`
- Missing required `TypedDict` key, unknown contextual `TypedDict` key, incompatible contextual `TypedDict` value, or invalid `**TypedDict` keyword expansion: `TPY4013`
- Direct call of a callable whose accepted arguments still depend on an unresolved `ParamSpec` or unresolved `Concatenate` tail: `TPY4014`
- Invalid `@override` usage or missing required `@override` when `typing.require_explicit_overrides = true`: `TPY4005`
- Exported declaration surface containing `dynamic` or unresolved `unknown` while `typing.require_known_public_types = true`: `TPY4015`
- Assignment to, augmented assignment through, or deletion of a known read-only `TypedDict` item: `TPY4016`
- Incomplete experimental conditional-return coverage: `TPY4018`
- Use of a deprecated declaration when `typing.report_deprecated` is not `ignore`: `TPY4101`
- `typepython verify` detecting a structural or runtime-assisted public-surface mismatch against the authoritative type surface: `TPY5003`

General assignability failures that do not fall into one of the more specific categories above continue to use `TPY4001`.

### 16.3 Severity Levels

- **Error**: Blocks compilation and emission
- **Warning**: Indicates potential issues but allows emission
- **Note**: Informational context

`TPY4101` MUST use the severity selected by `typing.report_deprecated`: no diagnostic when set to `ignore`, warning when set to `warning`, and error when set to `error`.

### 16.4 Output Formats

The CLI MUST support:

- Human-readable text output
- Structured JSON output

#### 16.4.1 Structured JSON Diagnostic Schema

If a CLI mode emits structured JSON diagnostics, the payload MUST be a JSON object with at least the following fields:

```json
{
  "diagnostics": [
    {
      "code": "TPY4001",
      "severity": "error",
      "message": "Type mismatch in assignment",
      "file": "src/pkg/mod.tpy",
      "span": {
        "start": { "line": 12, "column": 5 },
        "end": { "line": 12, "column": 16 }
      },
      "notes": [],
      "fixes": []
    }
  ],
  "summary": {
    "errors": 1,
    "warnings": 0,
    "notes": 0
  }
}
```

Within this schema:

- `diagnostics` MUST be present and MUST be an array
- each diagnostic object MUST contain `code`, `severity`, and `message`
- `file` and `span` SHOULD be present when the diagnostic corresponds to a source location
- `notes` and `fixes` MAY be omitted, but when present they MUST be arrays
- `summary` MAY be omitted only for streaming or per-file modes that do not produce a whole-project aggregate result

### 16.5 Enhanced Diagnostic Quality

TypePython diagnostics SHOULD go beyond pointing at the error site. The following diagnostic enhancements are SHOULD-level requirements for DX v1:

#### 16.5.1 Type Mismatch Path

When a type mismatch (`TPY4001`) involves nested generic, union, or structural types, the diagnostic SHOULD include a **mismatch path** showing the specific nested component that failed:

```
TPY4001: Type mismatch in argument 1 to `process`

  Source: dict[str, list[tuple[int, ...]]]
  Target: Mapping[str, Sequence[tuple[int, int]]]

  Mismatch at: → list[tuple[int, ...]] → tuple[int, ...]
    tuple[int, ...] (variable-length) is not assignable to
    tuple[int, int] (fixed-length 2-tuple)
```

The mismatch path SHOULD name the outermost type, then drill into the first nested position where assignability fails.

#### 16.5.2 Inference Chain Trace

When a generic inference failure or unexpected inferred type occurs, the diagnostic SHOULD include a summary of how the type was inferred:

```
TPY4001: Type mismatch in return

  Inferred return type: str | None
  Declared return type: str

  Inference trace:
    line 5: return x.name     → str     (from Person.name: str)
    line 7: return None        → None    (bare return)
    join: str | None
```

#### 16.5.3 Suggested Fixes

Where the fix is unambiguous, the diagnostic SHOULD include a machine-applicable suggestion:

- Missing `| None` in return type when a `None` path exists
- Missing `isinstance` guard before member access on a union
- Missing `@override` when `require_explicit_overrides = true`
- Missing `case` arms in an incomplete `match` (naming the missing types)
- Incorrect field name in `Pick`/`Omit` transform (with "did you mean?" candidates)

---

## 17. Incremental Build and Caching

### 17.1 Public Summary

The compiler MUST compute a public summary for every source module.

**Minimum summary contents:**

- Module identity
- Exported names and their types
- Imported module identities
- Generic declarations relevant to exports
- Sealed-root relationships relevant to exports
- Public package-entry status when relevant

If persisted to disk, the public summary MUST use a deterministic serialization format. Core v1 implementations SHOULD use canonical JSON. At minimum, the persisted schema MUST be equivalent to:

```json
{
  "module": "pkg.sub.mod",
  "isPackageEntry": false,
  "exports": [
    {
      "name": "Foo",
      "kind": "class",
      "type": "Foo",
      "typeParams": [],
      "public": true
    }
  ],
  "imports": ["pkg.base"],
  "sealedRoots": [
    {
      "root": "Expr",
      "members": ["Num", "Add"]
    }
  ]
}
```

An implementation MAY add fields, but it MUST preserve deterministic ordering and equivalent semantic content for hashing.

#### 17.1.1 Public Summary Serialization Schema

If a public summary is persisted as JSON, the persisted object MUST satisfy the following closed-shape expectations for the standardized fields shown in this section:

- `module` MUST be a string containing the logical module identity
- `isPackageEntry` MUST be a boolean
- `exports` MUST be an array of objects sorted by exported name
- each standardized export object MUST contain `name`, `kind`, `type`, and `public`
- `imports` MUST be an array of strings sorted lexicographically
- `sealedRoots`, when present, MUST be an array sorted by root name; each `members` array MUST be sorted lexicographically

Implementations MAY add versioned extension fields, but they MUST NOT change the meaning of the standardized fields above.

For deterministic hashing in v1:

- `imports` MUST be serialized in lexicographic order by module identity
- `exports` MUST be serialized in lexicographic order by exported name
- `sealedRoots` MUST be serialized in lexicographic order by root name, and each `members` array MUST be lexicographically ordered

### 17.2 Cache Keys

The cache SHOULD track:

- Source hash
- Lowered-text hash
- Public-summary hash
- Dependency summary hashes

### 17.3 Rebuild Rules

- If only implementation details change and public summary is unchanged, dependents SHOULD NOT be rechecked
- If public summary changes, direct and transitive dependents MUST be invalidated

#### 17.3.1 Invalidation Inputs

For v1, an implementation MUST classify incremental invalidation inputs into at least the following buckets:

- **Direct source changes**: a project module was added, removed, renamed, or its authoritative source text changed
- **Public-summary changes**: the persisted public summary for a module differs from the previous build
- **Bundled support-surface changes**: the bundled stdlib snapshot identity differs from the previous build state

An implementation MAY track additional invalidation inputs such as changes in external type roots, partial-stub package composition, or resolver configuration, but it MUST treat any such input conservatively enough that stale public summaries are never reused.

#### 17.3.2 Affected-Module Set

Given:

- `direct_changes`: the set of project modules directly edited, added, removed, or otherwise invalidated before summary comparison
- `summary_changed`: the set of modules whose public summaries were added, removed, or changed relative to the previous snapshot
- `reverse_importers(m)`: the transitive reverse-import closure for module `m`

the affected-module set for semantic rechecking MUST satisfy:

- every module in `direct_changes` MUST be rechecked
- every module in `summary_changed` MUST be treated as invalidated
- every direct or transitive importer of a module in `summary_changed` MUST be rechecked
- modules outside that affected set MAY reuse prior semantic summaries if their own direct input surface is unchanged

An implementation MAY conservatively recheck additional modules, but it MUST NOT omit any module required by the rules above.

#### 17.3.3 Output-Reuse Rules

If a frontend reuses previously materialized build outputs rather than only semantic summaries, it MUST additionally verify all of the following before declaring the build up to date:

- the previous and current public-summary sets are semantically equal
- the previous and current bundled-stdlib snapshot identities are equal
- the persisted build artifacts still exist and are structurally valid for the current build tree

If any of those checks fail, the frontend MUST rebuild or re-emit the affected outputs even if the project source files themselves are unchanged.

#### 17.3.4 Non-Source Support Inputs

Changes in bundled or discovered support surfaces can change type-checking results even when project source text is unchanged. Therefore:

- changing the bundled stdlib snapshot identity MUST invalidate any cached summary state derived from the previous snapshot
- if an implementation consumes external support roots, stub packages, or inferred shadow stubs, it SHOULD treat changes in the authoritative support surface as invalidation inputs
- when precise support-surface dependency tracking is unavailable, implementations MAY conservatively invalidate all project modules

### 17.4 Standard Library

TypePython Core v1 MUST ship with a pinned typeshed snapshot or equivalent bundled stdlib type source, filtered by `target_python`.

The cache SHOULD track the bundled stdlib snapshot identity so that changing the snapshot invalidates dependent summaries deterministically.

---

## 18. CLI Commands

### 18.1 `typepython init`

Creates a starter TypePython configuration and minimal source tree. If `pyproject.toml` already exists, implementations SHOULD offer to place the config under `[tool.typepython]`; otherwise they SHOULD create `typepython.toml`.

### 18.2 `typepython check`

Performs graph construction, lowering, and type checking without writing `.py` or `.pyi`.

### 18.3 `typepython build`

Performs a full build and writes outputs according to config.

### 18.4 `typepython watch`

`typepython watch` is a DX v1 feature.

Runs incremental rebuilds on file changes, using the configured `debounce_ms`.

### 18.5 `typepython clean`

Removes cache and build artifacts under configured directories.

### 18.6 `typepython lsp`

`typepython lsp` is a DX v1 feature.

Starts a language server process using the same project model and summaries as the compiler.

### 18.7 `typepython verify`

`typepython verify` MUST be provided by a v1-conformant implementation and MUST validate a build for library publication and downstream-tool interoperability.

It MUST at minimum:

- load the authoritative public type surface for each selected module (summary and/or emitted `.pyi`)
- enforce `typing.require_known_public_types` on that public surface
- compare structural public-name presence between the emitted runtime module and the authoritative type surface using the rules from Section 11.3, without requiring runtime imports
- when runtime-assisted verification is explicitly enabled and runtime modules are importable through the configured or default interpreter, implementations MAY additionally compare runtime-visible public-name presence between the imported runtime module and the authoritative type surface using the rules from Section 11.3
- when wheel or sdist artifacts are supplied for verification, confirm the typed-publication requirements from Section 13.6.4

Runtime verification in v1 is name- and declaration-surface-oriented. It MUST NOT require byte-for-byte equivalence of default values, docstrings, or implementation internals.

Implementations MUST document whether runtime-assisted verification is enabled by default or gated behind an explicit opt-in, MUST treat any mode that imports emitted project modules as executing project-controlled code, and MUST document whether safe structural verify ignores a configured project interpreter for package discovery or helper probes.

If any required verification check fails, the command MUST return a failing exit status and MUST surface at least one diagnostic explaining the mismatch.

### 18.8 `typepython migrate`

`typepython migrate` is a DX v1 command family for incremental adoption of TypePython in existing Python projects.

#### 18.8.1 Migration Report

`typepython migrate --report` SHOULD produce a summary of typing coverage across the project:

- Per-directory and per-file percentage of declarations with known (non-`dynamic`, non-`unknown`) types
- A ranked list of **high-impact untyped files** — files whose typing would unlock the most downstream type inference, ordered by the number of downstream references that currently resolve to `unknown` or `dynamic` because of the untyped file
- Total count of `dynamic` and `unknown` boundaries in the project

The report MUST be available in both human-readable and structured JSON formats.

#### 18.8.2 Pass-Through Inference

Pass-through inference is Experimental v1. When `typing.infer_passthrough = true` is configured and the implementation supports the experimental feature, the compiler SHOULD perform best-effort type inference on pass-through `.py` files in the project:

- Inferred types are stored as **shadow stubs** in the cache directory, not written to the source `.py` files.
- Shadow stubs are used as the typing surface for the inferred module during compilation of dependent `.tpy` files.
- Where inference fails or is ambiguous, the inferred type falls back to `unknown` (not `dynamic`), preserving safety.
- Shadow stubs are re-computed when the source `.py` file changes.
- Shadow stubs MUST NOT be emitted to `out_dir` or included in published artifacts.

Inference in v1 is best-effort. An implementation MAY start with simple patterns (annotated assignments, return-type inference from literal returns, class attribute inference from `__init__` assignments) and expand coverage in later releases.

#### 18.8.3 Stub Generation from `.py`

Stub generation from inferred `.py` files is Experimental v1. `typepython migrate --emit-stubs <path>` SHOULD generate `.pyi` stub files from inferred types for the specified `.py` files when the implementation supports the experimental inference tier:

- Generated stubs include `# auto-generated by typepython migrate` as a leading comment.
- Positions where inference failed are annotated with `# TODO: add type annotation` comments and use `...` as the type.
- Generated stubs are written to a configurable output directory (default: alongside the source files or in `out_dir`).
- These stubs are intended as a starting point for manual refinement, not as authoritative type surfaces.

### 18.9 Exit Codes

- `0`: Success
- `1`: User-code or config errors
- `2`: Internal compiler failures

---

## 19. LSP Support

LSP support is a DX v1 feature. A Core v1 implementation need not provide it, but any implementation that does MUST satisfy this section.

### 19.1 Required Features

- Diagnostics
- Hover
- Go-to-definition
- Find references
- Rename within supported symbol set
- Completion based on visible members and narrowed types

### 19.2 Overlay Support

The LSP MUST operate on unsaved overlays and MUST NOT require the user to save files to receive diagnostics.

### 19.3 Formatting and Code Actions

v1 does not require a standalone formatter, but language-service edits SHOULD be represented as text changes over source spans rather than opaque AST rewrites.

At minimum, code actions MAY include:

- add missing type annotation
- insert `unsafe:` around required dynamic code
- import missing symbol

---
