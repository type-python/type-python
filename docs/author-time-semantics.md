# Author-Time Semantics

TypePython's research slices are implemented as stronger author-time semantics with a portable output boundary. The source language can track effects, shape projections, taint, and validator witnesses, while emitted `.py` and `.pyi` stay standard Python artifacts.

## The Boundary

TypePython checks richer facts while you author `.tpy`:

- effect and capability rows
- field-bearing shape projections
- restricted type-level aliases
- taint qualifiers and source/sink/sanitizer facts
- trusted validator witnesses for `unknown` narrowing

Those facts are either reduced, erased, or written to TypePython-owned metadata before publication. Consumers still see ordinary Python typing through generated `.py`, `.pyi`, and `py.typed`.

## Implemented Slices

| Slice | What users can try now | Output behavior |
| --- | --- | --- |
| P0 Effect / capability rows | Mark callables with `@effect("io.net")`, `@effect_pure`, lifecycle decorators, or framework effect capabilities; uncovered effect calls report `TPY4026` and LSP hover/code actions explain the row. | Effect facts are author-time diagnostics and `.typepython/cache/effects.json` metadata; emitted Python remains standard. |
| P1 Shape substrate | Use `Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, and supported `MapValues` over `TypedDict`; opt into `[experimental].shape_transforms = true` for TypePython `data class` projection materialization. | Shape aliases materialize as standard `TypedDict` output or fail closed. |
| P2 Restricted evaluator | Use `TypeIf[IsSubtype[...], A, B]`, `KeyOf`, `RequiredKeys`, `OptionalKeys`, `Pick`, `Omit`, and supported `MapValues`. Unsupported forms report `TPY4027`. | Reducible aliases are emitted as standard types; unreduced TypePython-only forms are not emitted. |
| P3 Taint qualifier | Use `Tainted[T, Context]`, `@source`, `@sink`, and `@sanitizer`; direct unsanitized source-to-sink flows report `TPY4028`. | `Tainted[...]` is checker-only and erased to the underlying runtime type during lowering. |
| P4 Validator witness | Use `ValidatorWitness[T, Literal["trusted"]]`, `ValidatorWitness[T, Literal["generated"]]`, or trusted validator decorator metadata to narrow `unknown` in a true branch. Assignment, deletion, and detected mutation invalidate the witness. | Witness types are checker-only and erased before emit. |

## Current Demo

The runnable example is `examples/research-roadmap-demo`:

```bash
cargo run -p typepython-cli -- check --project examples/research-roadmap-demo
cargo run -p typepython-cli -- build --project examples/research-roadmap-demo
```

It demonstrates all five slices in one small project and intentionally keeps the public output portable.

## Non-Goals

The current implementation does not claim:

- whole-program taint analysis
- full runtime soundness for arbitrary validators
- public `Shape[...]` literal syntax
- full TypeScript-style mapped or conditional type semantics
- CPython bytecode specialization metadata
- tensor dependent typing or SMT-backed symbolic algebra

Those remain future research directions or explicitly deferred tracks. The stable product value is the middle ground: stronger author-time facts, standard consumer-time Python.
