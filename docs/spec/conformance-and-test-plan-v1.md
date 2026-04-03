# TypePython v1 Conformance and Test Plan

**Status:** draft, normative for conformance claims  
**Scope:** feature tiers, conformance claims, feature matrix, and test obligations  
**Numbering note:** original section numbering is preserved for stable reference.

This document defines what it means to claim Core v1, DX v1, or Experimental v1 support, and what testing evidence should back those claims.

Sections intentionally included here:

- Section 4
- Section 20
- Appendix C

---

## 4. v1 Conformance and Feature Tiers

A v1-conformant TypePython implementation MUST satisfy every Core v1 requirement in this section.

Features designated **DX v1** are recommended product features. They are not required for base conformance, but any implementation that claims them MUST implement their documented semantics.

Features designated **Experimental v1** are outside conformance. Implementations MAY provide them only behind an explicit opt-in mechanism and MUST NOT silently enable them for ordinary Core v1 builds.

### 4.1 Core v1 Compiler Requirements

1. **Scanner and parser** for the Core v1 syntax defined in Section 7 and Appendix A
2. **Binding and declaration typing** for modules, classes, interfaces, aliases, and imports
3. **Semantic elaboration** for v1 features that require bound declarations before expansion or checking
4. **Body type checking** for the supported feature set
5. **Deterministic emission** of `.py` and `.pyi`
6. **Source span mapping** from original `.tpy` to emitted `.py`
7. **Standards-based typing interop** for installed typed packages, stub packages, and the imported typing constructs required by Section 12.6
8. **Framework transform support** for `dataclass_transform`-driven dataclass-like libraries per Section 10.5
9. **Public-summary computation and deterministic invalidation**
10. **`typepython verify`** per Section 18.7

### 4.2 Core v1 Project and CLI Requirements

1. **Config loader** for `typepython.toml` and `[tool.typepython]` in `pyproject.toml`
2. **CLI commands**: `init`, `check`, `build`, `clean`, `verify`
3. **Diagnostics** with stable error codes per Section 16
4. **Incremental cache** based on source and public summary hashes
5. **Deterministic Python package model** with both regular packages and implicit namespace packages participating in module discovery

### 4.3 DX v1 Requirements

The following are DX v1 features:

- `watch`
- `lsp`
- enhanced diagnostics from Section 16.5
- `typepython migrate --report`
- migration-oriented stub generation workflows that do not affect authoritative public surfaces

### 4.4 Experimental v1 Surface

The following features are explicitly experimental in v1:

- source-authored conditional return syntax
- pass-through `.py` inference and shadow-stub generation
- runtime validator generation

If an implementation supports an experimental feature, it MUST:

- gate it behind an explicit opt-in flag or config mechanism
- document that it is outside Core v1 conformance
- avoid changing the acceptance or rejection of Core v1 programs when the feature is disabled

### 4.5 Product Claims and First-Shippable Scope

An implementation MAY ship as a first public v1 release while claiming only **Core v1** conformance.

If an implementation advertises **DX v1** or **Experimental v1** features, it MUST:

- identify those tiers explicitly in user-facing documentation, release notes, or capability output
- avoid presenting DX v1 or Experimental v1 features as required for ordinary Core v1 library publication
- preserve the acceptance, rejection, emit, and diagnostic behavior of Core v1 programs when optional tiers are disabled

A product claim such as "TypePython v1 compatible" is incomplete unless it states which of the three tiers are implemented.

---

## 20. Testing Requirements

A Core v1 implementation SHOULD include at least four test classes:

| Test Class            | Purpose                                    |
| --------------------- | ------------------------------------------ |
| Golden lowering tests | Verify `.tpy` → `.py` transformations      |
| Stub tests            | Verify `.tpy` → `.pyi` output              |
| Diagnostic tests      | Verify error codes and spans               |
| Incremental tests     | Verify rebuild triggers on summary changes |

For Core v1 conformance, the test suite MUST additionally cover:

- expansion of built-in type transforms to standard `.pyi` output and their required failure cases
- typed-publication consistency and `typepython verify` failure cases for missing or divergent artifact surfaces

### 20.1 Rule-to-Test Traceability

For a serious conformance claim, implementations SHOULD maintain a traceable mapping from normative requirements to concrete tests.

At minimum:

- every externally visible `MUST` rule in the normative documents SHOULD map to at least one test case or test family
- every stable diagnostic code reserved by Section 16 SHOULD map to at least one positive or negative test
- conformance reports SHOULD identify any unimplemented or intentionally skipped normative requirements rather than silently omitting them

**Recommended additional suites:**

- Runtime smoke tests on emitted `.py`
- LSP scenario tests for hover, definitions, and rename
- Sealed exhaustiveness tests
- Overload resolution tests
- `TypedDict` literal and `**kwargs` expansion tests
- `Annotated`, `ClassVar`, `Required` / `NotRequired`, and `ReadOnly` wrapper tests
- `NewType` assignability and construction tests
- Enum / flag exhaustiveness distinction tests
- `Final` and abstract-class diagnostic tests
- `@override` and `require_explicit_overrides` tests
- `@deprecated` use-site diagnostic tests
- `dataclass_transform` synthesis tests for decorator, base-class, and metaclass cases
- PEP 561 partial-stub resolution tests
- `typing` / `typing_extensions` equivalence and target-version emission tests
- wheel/sdist typed-artifact consistency tests for `py.typed` and emitted `.pyi`
- Public API completeness tests under `require_known_public_types = true`
- Parser-accepts / checker-rejects tests for still-deferred `.tpy` constructs
- `ParamSpec` / `Concatenate` consumption and source-authored forwarding tests
- `verify` command tests for runtime/type-surface public-name mismatch
- Built-in type transform tests (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, composition, generic input, error cases)
- Experimental conditional return lowering tests (coverage checking, generic conditional return, mutual exclusion with `overload def`)
- Migration report output tests (coverage percentages, high-impact file ranking)
- Experimental pass-through inference shadow stub tests (`infer_passthrough = true`)
- Experimental runtime validator generation tests for `data class` only (supported types, unsupported type fallback, nested types)
- Enhanced diagnostic quality tests (mismatch path, inference trace, suggested fix presence)

---

## 21. Appendices

The following appendix remains normative for conformance and capability claims.

### Appendix C: v1 Feature Matrix

| Feature                                                                                                       | Tier            | Status |
| ------------------------------------------------------------------------------------------------------------- | --------------- | ------ |
| `.tpy` parsing for Core syntax                                                                                | Core v1         | MUST   |
| `.py` emission                                                                                                | Core v1         | MUST   |
| `.pyi` emission                                                                                               | Core v1         | MUST   |
| `typealias`                                                                                                   | Core v1         | MUST   |
| `interface`                                                                                                   | Core v1         | MUST   |
| `data class`                                                                                                  | Core v1         | MUST   |
| `sealed class` with same-module closure                                                                       | Core v1         | MUST   |
| `overload def`                                                                                                | Core v1         | MUST   |
| `unsafe:`                                                                                                     | Core v1         | MUST   |
| Generics with single upper bound                                                                              | Core v1         | MUST   |
| Type-parameter defaults and constraint lists                                                                  | Core v1         | MUST   |
| `ParamSpec` authoring including source-authored `P.args` / `P.kwargs` forwarding                              | Core v1         | MUST   |
| Recursive type aliases                                                                                        | Core v1         | MUST   |
| Unions and literals                                                                                           | Core v1         | MUST   |
| Local inference                                                                                               | Core v1         | MUST   |
| Widened literal and container inference                                                                       | Core v1         | MUST   |
| `Self` and receiver typing                                                                                    | Core v1         | MUST   |
| Callable compatibility and overload specificity                                                               | Core v1         | MUST   |
| Typed callable decorator transforms (callable-to-callable)                                                    | Core v1         | MUST   |
| `TypedDict` literal checking in contextual positions                                                          | Core v1         | MUST   |
| `TypedDict` `closed=` / `extra_items=` semantics                                                              | Core v1         | MUST   |
| `Annotated`, `ClassVar`, `Required`, `NotRequired`, and `ReadOnly` in their supported positions               | Core v1         | MUST   |
| `NewType` declarations and nominal compatibility                                                              | Core v1         | MUST   |
| Narrowing (`is None`, `isinstance`, `TypeGuard`/`TypeIs`, `assert`, `match`, boolean composition)             | Core v1         | MUST   |
| Builtin decorator typing (`@property`, `@classmethod`, `@staticmethod`, `@final`, `@override`, `@deprecated`) | Core v1         | MUST   |
| `dataclass_transform`-based dataclass-like framework typing                                                   | Core v1         | MUST   |
| Lambda parameter annotation sugar                                                                             | Core v1         | MUST   |
| Authored async semantics in `.tpy` (`async def`, `await`, `async for`, `async with`, `yield`, `yield from`)   | Core v1         | MUST   |
| `with` statement typing                                                                                       | Core v1         | MUST   |
| `for` loop and comprehension typing                                                                           | Core v1         | MUST   |
| `try`/`except` exception variable typing                                                                      | Core v1         | MUST   |
| Enum type support and enum member typing                                                                      | Core v1         | MUST   |
| `Final` binding enforcement                                                                                   | Core v1         | MUST   |
| Abstract class and `@abstractmethod` checking                                                                 | Core v1         | MUST   |
| Implicit namespace packages / PEP 420 project modeling                                                        | Core v1         | MUST   |
| PEP 561 typed-package and partial-stub resolution                                                             | Core v1         | MUST   |
| `typing` / `typing_extensions` semantic equivalence for supported constructs                                  | Core v1         | MUST   |
| Target-version compatibility matrix for emitted typing constructs                                             | Core v1         | MUST   |
| Untyped import fallback (`unknown`/`dynamic`)                                                                 | Core v1         | MUST   |
| Deterministic diagnostics                                                                                     | Core v1         | MUST   |
| Cache invalidation                                                                                            | Core v1         | MUST   |
| `TypedDict` utility transforms (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_`)                | Core v1         | MUST   |
| Public API completeness enforcement when configured                                                           | Core v1         | MUST   |
| Packaging artifact consistency rules for typed publication                                                    | Core v1         | MUST   |
| `typepython verify` library publishability checks                                                             | Core v1         | MUST   |
| Sealed exhaustiveness                                                                                         | DX v1           | SHOULD |
| Enum exhaustiveness                                                                                           | DX v1           | SHOULD |
| Enhanced diagnostics (mismatch path, inference trace, suggested fixes)                                        | DX v1           | SHOULD |
| Stable JSON diagnostic output                                                                                 | DX v1           | SHOULD |
| `typepython watch`                                                                                            | DX v1           | SHOULD |
| `typepython lsp`                                                                                              | DX v1           | SHOULD |
| `typepython migrate --report`                                                                                 | DX v1           | SHOULD |
| `typepython migrate` stub-generation workflows that do not affect authoritative public surfaces               | DX v1           | SHOULD |
| Optional `.pyc` generation                                                                                    | DX v1           | MAY    |
| Conditional return types (overload sugar via `-> match param:`)                                               | Experimental v1 | MAY    |
| Pass-through `.py` inference (`infer_passthrough`)                                                            | Experimental v1 | MAY    |
| Runtime validator emission for `data class`                                                                   | Experimental v1 | MAY    |

**Deferred beyond v1:**

- Intersection types
- Full conditional types (TypeScript-style `T extends U ? X : Y`)
- Full mapped types (TypeScript-style `{[K in keyof T]: ...}`)
- Template literal types
- Non-callable decorator replacement semantics
- Declarative decorator effect annotations
- Direct utility transforms over `data class`, class, interface, or protocol declarations before a first-class record/shape model exists
