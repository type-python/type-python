# TypePython Conformance Report

This report maps the feature matrix in `docs/spec/conformance-and-test-plan-v1.md` to test evidence commands. It is intentionally conservative: missing evidence is reported as `missing`, not inferred from nearby tests.

| Feature | Tier | Requirement | Evidence |
| ------- | ---- | ----------- | -------- |
| `.tpy` parsing for Core syntax | Core v1 | MUST | `cargo test -p typepython-syntax` |
| `.py` emission | Core v1 | MUST | `cargo test -p typepython-lowering`<br>`cargo test -p typepython-cli tests::pipeline` |
| `.pyi` emission | Core v1 | MUST | `cargo test -p typepython-emit`<br>`cargo test -p typepython-cli tests::pipeline` |
| `typealias` | Core v1 | MUST | `cargo test -p typepython-lowering typealias` |
| `interface` | Core v1 | MUST | `cargo test -p typepython-lowering interface` |
| `data class` | Core v1 | MUST | `cargo test -p typepython-lowering data_class` |
| `sealed class` with same-module closure | Core v1 | MUST | `cargo test -p typepython-checking sealed` |
| `overload def` | Core v1 | MUST | `cargo test -p typepython-checking overload` |
| `unsafe:` | Core v1 | MUST | `cargo test -p typepython-checking unsafe` |
| Generics with single upper bound | Core v1 | MUST | `cargo test -p typepython-checking generics` |
| Type-parameter defaults and constraint lists | Core v1 | MUST | `cargo test -p typepython-checking advanced_generics` |
| `ParamSpec` authoring including source-authored `P.args` / `P.kwargs` forwarding | Core v1 | MUST | `cargo test -p typepython-checking paramspec` |
| Recursive type aliases | Core v1 | MUST | `cargo test -p typepython-checking recursive` |
| Unions and literals | Core v1 | MUST | `cargo test -p typepython-checking literal` |
| Local inference | Core v1 | MUST | `cargo test -p typepython-checking inference` |
| Widened literal and container inference | Core v1 | MUST | `cargo test -p typepython-checking widened` |
| `Self` and receiver typing | Core v1 | MUST | `cargo test -p typepython-checking receiver` |
| Callable compatibility and overload specificity | Core v1 | MUST | `cargo test -p typepython-checking calls` |
| Typed callable decorator transforms (callable-to-callable) | Core v1 | MUST | `cargo test -p typepython-checking decorator` |
| `TypedDict` literal checking in contextual positions | Core v1 | MUST | `cargo test -p typepython-checking typed_dict` |
| `TypedDict` `closed=` / `extra_items=` semantics | Core v1 | MUST | `cargo test -p typepython-checking typed_dict` |
| `Annotated`, `ClassVar`, `Required`, `NotRequired`, and `ReadOnly` in their supported positions | Core v1 | MUST | `cargo test -p typepython-checking wrappers` |
| `NewType` declarations and nominal compatibility | Core v1 | MUST | `cargo test -p typepython-checking newtype` |
| Narrowing (`is None`, `isinstance`, `TypeGuard`/`TypeIs`, `assert`, `match`, boolean composition) | Core v1 | MUST | `cargo test -p typepython-checking narrowing` |
| Builtin decorator typing (`@property`, `@classmethod`, `@staticmethod`, `@final`, `@override`, `@deprecated`) | Core v1 | MUST | `cargo test -p typepython-checking decorator` |
| `dataclass_transform`-based dataclass-like framework typing | Core v1 | MUST | `cargo test -p typepython-checking dataclass` |
| Lambda parameter annotation sugar | Core v1 | MUST | `cargo test -p typepython-syntax lambda` |
| Authored async semantics in `.tpy` (`async def`, `await`, `async for`, `async with`, `yield`, `yield from`) | Core v1 | MUST | `cargo test -p typepython-checking async` |
| `with` statement typing | Core v1 | MUST | `cargo test -p typepython-checking with` |
| `for` loop and comprehension typing | Core v1 | MUST | `cargo test -p typepython-checking for_loop` |
| `try`/`except` exception variable typing | Core v1 | MUST | `cargo test -p typepython-checking except` |
| Enum type support and enum member typing | Core v1 | MUST | `cargo test -p typepython-checking enum` |
| `Final` binding enforcement | Core v1 | MUST | `cargo test -p typepython-checking final` |
| Abstract class and `@abstractmethod` checking | Core v1 | MUST | `cargo test -p typepython-checking abstract` |
| Implicit namespace packages / PEP 420 project modeling | Core v1 | MUST | `cargo test -p typepython-cli collect_source_paths` |
| PEP 561 typed-package and partial-stub resolution | Core v1 | MUST | `cargo test -p typepython-cli external_resolution` |
| `typing` / `typing_extensions` semantic equivalence for supported constructs | Core v1 | MUST | `cargo test -p typepython-target` |
| Target-version compatibility matrix for emitted typing constructs | Core v1 | MUST | `cargo test -p typepython-cli target` |
| Untyped import fallback (`unknown`/`dynamic`) | Core v1 | MUST | `cargo test -p typepython-checking imports` |
| Deterministic diagnostics | Core v1 | MUST | `cargo test -p typepython-diagnostics` |
| Cache invalidation | Core v1 | MUST | `cargo test -p typepython-incremental` |
| `TypedDict` utility transforms (`Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_`) | Core v1 | MUST | `cargo test -p typepython-checking typed_dict` |
| Public API completeness enforcement when configured | Core v1 | MUST | `cargo test -p typepython-cli public_surface` |
| Packaging artifact consistency rules for typed publication | Core v1 | MUST | `cargo test -p typepython-cli tests::verification` |
| `typepython verify` library publishability checks | Core v1 | MUST | `cargo test -p typepython-cli tests::verification` |
| Sealed exhaustiveness | DX v1 | SHOULD | missing |
| Enum exhaustiveness | DX v1 | SHOULD | missing |
| Enhanced diagnostics (mismatch path, inference trace, suggested fixes) | DX v1 | SHOULD | missing |
| Stable JSON diagnostic output | DX v1 | SHOULD | missing |
| `typepython watch` | DX v1 | SHOULD | missing |
| `typepython lsp` | DX v1 | SHOULD | missing |
| `typepython migrate --report` | DX v1 | SHOULD | missing |
| `typepython migrate` stub-generation workflows that do not affect authoritative public surfaces | DX v1 | SHOULD | missing |
| Optional `.pyc` generation | DX v1 | MAY | missing |
| Conditional return types (overload sugar via `-> match param:`) | Experimental v1 | MAY | missing |
| Pass-through `.py` inference (`infer_passthrough`) | Experimental v1 | MAY | missing |
| Runtime validator emission for `data class` | Experimental v1 | MAY | missing |
