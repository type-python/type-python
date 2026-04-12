# Interoperability with External Type Checkers

TypePython compiles `.tpy` source to standard `.py` and `.pyi` files. This document explains how the emitted artifacts interact with external type checkers such as mypy and pyright, what compatibility guarantees hold, and where semantic differences exist.

## Design Principle

TypePython follows a single rule for its output:

> Emitted `.py` and `.pyi` files contain **only standard Python typing constructs**. No TypePython-specific syntax, no custom runtime, no proprietary type forms.

This means tools that understand the emitted standard typing constructs can consume TypePython's output without modification. In practice, TypePython output relies on a broader set of standard typing features than just PEP 484 / PEP 561, including constructs such as `Protocol`, `ParamSpec`, `TypeAlias`, `Required` / `NotRequired`, and `ReadOnly`.

## Lowering Map for `.pyi` Stubs

Every TypePython-specific construct is lowered to an equivalent standard Python form before it appears in a `.pyi` file:

| TypePython construct         | Representation in `.pyi`                                      | Standard basis |
| ---------------------------- | ------------------------------------------------------------- | -------------- |
| `unknown`                    | `object`                                                      | PEP 484        |
| `dynamic`                    | `Any`                                                         | PEP 484        |
| `interface Foo:`             | `class Foo(Protocol):`                                        | PEP 544        |
| `data class Bar:`            | `@dataclass class Bar:`                                       | PEP 557        |
| `sealed class Expr:`         | `class Expr:` (plain class)                                   | --             |
| `overload def f():`          | `@overload def f():`                                          | PEP 484        |
| `typealias X = T`            | `X: TypeAlias = T`                                            | PEP 613        |
| Inline generics `def f[T]()` | `T = TypeVar("T")` + `def f():`                               | PEP 484        |
| `ParamSpec` `**P`            | `P = ParamSpec("P")`                                          | PEP 612        |
| `Partial[Config]`            | Expanded `TypedDict` with all keys `NotRequired`              | PEP 655        |
| `Pick[Config, "a", "b"]`     | Expanded `TypedDict` with selected keys only                  | PEP 589        |
| `Omit[Config, "a"]`          | Expanded `TypedDict` without excluded keys                    | PEP 589        |
| `Readonly[Config]`           | Expanded `TypedDict` with `ReadOnly` on each field            | PEP 705        |
| `Required_[Config]`          | Expanded `TypedDict` with `NotRequired[...]` wrappers removed | PEP 655        |
| `Mutable[Config]`            | Expanded `TypedDict` without `ReadOnly` wrappers              | PEP 705        |
| `unsafe: ...`                | Not represented (safety boundary is erased)                   | --             |
| `sealed` marker              | Not represented (comment only in `.py`)                       | --             |

### Dual lowering strategy

TypePython now supports two lowering modes:

- `emit.emit_style = "compat"` preserves broad checker/runtime compatibility by lowering inline generics to `TypeVar` / `ParamSpec` / `TypeVarTuple` declarations and `TypeAlias`.
- `emit.emit_style = "native"` preserves native typing syntax when the configured target runtime supports it, including `type Alias = ...`, `def f[T]`, and `class C[T]`.

Targets 3.10-3.12 default to `compat`. Targets 3.13+ default to `native`, though every target can override the mode explicitly.

## `typing` / `typing_extensions` Equivalence

TypePython treats the following import sources as semantically identical:

```python
from typing import Protocol
from typing_extensions import Protocol   # same type
```

When emitting `.pyi` stubs, the compiler selects the import source based on `target_python`:

| Construct    | Python 3.10         | Python 3.11         | Python 3.12         | Python 3.13+ |
| ------------ | ------------------- | ------------------- | ------------------- | ------------ |
| `TypeGuard`  | `typing_extensions` | `typing`            | `typing`            | `typing`     |
| `TypeIs`     | `typing_extensions` | `typing_extensions` | `typing_extensions` | `typing`     |
| `ReadOnly`   | `typing_extensions` | `typing_extensions` | `typing_extensions` | `typing`     |
| `override`   | `typing_extensions` | `typing_extensions` | `typing`            | `typing`     |
| `deprecated` | `typing_extensions` | `typing_extensions` | `typing_extensions` | `warnings`   |

This ensures that mypy and pyright can resolve all imports for the given target version without needing manual `typing_extensions` fallbacks.

## PEP 561 Compliance

TypePython emits `py.typed` marker files in package root directories when `emit.write_py_typed = true` (default). This marker signals to external tools that the package contains inline type information, following [PEP 561](https://peps.python.org/pep-0561/).

A built TypePython package can be published to PyPI and consumed by downstream projects with type checking support from mypy, pyright, or other tools that understand the emitted standard typing constructs.

## Coexistence with typeshed and Third-Party Stubs

TypePython bundles its own stdlib typing data (the `stdlib/` directory) for use during compilation. This data is **not** emitted into build output -- it is only used by the TypePython checker and does not conflict with:

- **typeshed** stubs that mypy/pyright bundle internally
- **Third-party stub packages** (`types-requests`, `pandas-stubs`, etc.) installed in the user's environment

When downstream consumers type-check the emitted `.py`/`.pyi` files, they use their own stdlib stubs (typically typeshed). TypePython's bundled data is invisible to them.

## Verification with `typepython verify`

The `typepython verify` command performs structural consistency checks that help maintain interoperability:

| Check                 | What it catches                                                     |
| --------------------- | ------------------------------------------------------------------- |
| Public name matching  | `.py` exports a name that `.pyi` does not declare (or vice versa)   |
| Stub syntax validity  | `.pyi` contains runtime statements that stub consumers would reject |
| Artifact completeness | Missing `.py`, `.pyi`, or `py.typed` in the build output            |
| Package consistency   | Wheel/sdist contents diverge from the build tree                    |
| Metadata consistency  | `Requires-Python` or `typing_extensions` metadata lags emitted requirements |
| Snapshot integrity    | Incremental cache is corrupt or incompatible                        |

These checks catch the class of bugs where `.pyi` declarations drift from the actual runtime code -- the same problem that plagues hand-maintained `.d.ts` files in the TypeScript ecosystem.

`typepython verify` stays on structural validation by default. In that safe mode, TypePython may ignore a project-controlled `resolution.python_executable` and use a deterministic host interpreter instead of executing the configured project interpreter. If you need runtime-visible public-name parity checks for modules with dynamically computed exports, or you need verification against the configured interpreter environment, pass `--unsafe-runtime-imports`; that mode executes emitted project code in a subprocess and should only be used for trusted builds.

## Semantic Differences at the Boundary

While the emitted `.pyi` files are syntactically and structurally compatible with all major type checkers, some TypePython-specific semantic guarantees **do not transfer** across the boundary. This is by design: the standard Python typing system does not have equivalents for these concepts.

### `unknown` → `object`

| Behavior        | In TypePython              | In mypy/pyright (via `.pyi`)                     |
| --------------- | -------------------------- | ------------------------------------------------ |
| Member access   | Error -- must narrow first | Allowed (limited `object` methods like `__eq__`) |
| Call            | Error                      | Error                                            |
| Assign to `str` | Error -- must narrow first | Error                                            |
| `==` comparison | Error                      | Allowed                                          |

TypePython's `unknown` is stricter than `object`: it forbids **all** operations until narrowed. When lowered to `object` in `.pyi`, downstream tools apply the standard `object` rules, which are slightly more permissive.

### `sealed class` → plain class

| Behavior                | In TypePython                                      | In mypy/pyright (via `.pyi`)                               |
| ----------------------- | -------------------------------------------------- | ---------------------------------------------------------- |
| `match` exhaustiveness  | Enforced -- compiler proves all subclasses covered | Not enforced -- external tools see an open class hierarchy |
| Subclassing restriction | Same-module only                                   | No restriction                                             |

The sealed constraint is enforced only within the TypePython checker. External consumers can subclass the emitted class freely, and their type checkers will not flag non-exhaustive matches.

### `unsafe:` boundary

| Behavior                   | In TypePython       | In mypy/pyright (via `.pyi`) |
| -------------------------- | ------------------- | ---------------------------- |
| `eval()` outside `unsafe:` | Warning (`TPY4019`) | No diagnostic                |
| `exec()` outside `unsafe:` | Warning (`TPY4019`) | No diagnostic                |

The `unsafe:` boundary is a TypePython-only safety annotation. It is erased during lowering and has no representation in `.pyi` stubs.

### TypedDict transform provenance

```python
# TypePython source
typealias PartialConfig = Partial[Config]
```

In the emitted `.pyi`, `PartialConfig` becomes a standalone `TypedDict` with all keys marked `NotRequired`. The relationship "this was derived from `Config` via `Partial`" is lost. External tools see two independent `TypedDict` types with no structural link.

### Decorator metadata

`@deprecated` messages are preserved in `.pyi` stubs, with lowering normalizing the decorator to `typing_extensions.deprecated`. `@final` and `@override` are standard and fully understood. No information loss for these decorators.

## Summary

| Aspect                   | Status                                                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| Syntactic compatibility  | Full -- all `.pyi` output uses standard PEP 484/544/561/612/613/655/705 constructs                                  |
| Structural compatibility | Full -- `typepython verify` enforces `.py`/`.pyi` name consistency                                                  |
| Semantic compatibility   | Partial -- `sealed` exhaustiveness, `unknown` strictness, and `unsafe` boundaries do not transfer to external tools |
| typeshed coexistence     | No conflict -- bundled stdlib data is compile-time only                                                             |
| PEP 561                  | Compliant -- `py.typed` markers emitted by default                                                                  |

The design trade-off is intentional: **maximum interoperability at the cost of reduced guarantees when crossing the TypePython boundary.** The stronger safety properties (sealed exhaustiveness, unknown strictness, unsafe boundaries) are enforced at authoring time by the TypePython checker. External consumers get standard, well-typed Python artifacts.
