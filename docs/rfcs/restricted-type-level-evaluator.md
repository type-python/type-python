# RFC: Restricted Type-Level Evaluator

**Status:** internal P2 substrate slice  
**Strategic source:** `docs/research-roadmap-assessment.zh.md` P2 Restricted Conditional + Mapped Types  
**Scope:** checker-internal evaluator with guarded lowering and LSP explanation for reduced standard typing

## Summary

TypePython needs a small, terminating type-level evaluator before conditional or mapped types can become a user-facing feature. The first slice adds checker infrastructure for finite evaluation of existing generic-looking type forms while preserving the project rule that emitted `.py` and `.pyi` remain standard Python typing.

## Supported internal forms

- `IsSubtype[T, U]` evaluates to a boolean using the existing semantic assignability relation.
- `TypeIf[Cond, A, B]` evaluates one branch when `Cond` is a decidable boolean form.
- `KeyOf[ShapeLike]` can read keys from the existing internal `Shape` resolver.
- `Pick[ShapeLike, Literal[...]]` and `Omit[ShapeLike, Literal[...]]` project a known Shape's key set through the shared shape operations.
- `RequiredKeys[ShapeLike]` and `OptionalKeys[ShapeLike]` partition a known Shape by requiredness, including `TypedDict(total=False)` defaults and explicit `Required[...]` / `NotRequired[...]` wrappers.
- `MapValues[ShapeLike, F]` accepts the initial built-in wrapper set (`Optional`, `Readonly`) and fails closed for unsupported wrappers.
- `Pick` / `Omit` accept both legacy quoted-key arguments and `Literal[...]` key sets, so checker examples such as `Pick[User, Literal["id"]]` lower through the same standard TypedDict materialization path.

Unsupported or failed reductions now surface as `TPY4027` instead of being silently ignored. This keeps the evaluator fail-closed: a type-level alias must reduce to standard Python typing, or the checker reports the exact unsupported/ill-formed form.

Lowering performs the same reduction for decidable `TypeIf[IsSubtype[...], A, B]` aliases before writing `.py` / `.pyi`, so emitted stubs contain the selected standard Python type rather than TypePython-only type functions. LSP hover shows both the authored alias and the reduced type-level alias result.

## Guardrails

- Evaluation has a fixed step budget.
- Unsupported or ill-formed evaluator inputs return structured errors instead of producing non-standard output.
- The evaluator is internal infrastructure; author-facing syntax and lowering behavior are unchanged in this slice.
