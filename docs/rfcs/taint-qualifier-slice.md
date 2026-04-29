# RFC: Taint Qualifier Slice

**Status:** internal P3 vertical slice  
**Strategic source:** `docs/research-roadmap-assessment.zh.md` P3 Taint as Real Type  
**Scope:** checker-only `Tainted[T, Context]` assignability semantics plus explicit source/sink/sanitizer facts

## Summary

This slice models taint as a real TypePython author-time qualifier without changing emitted `.py` or `.pyi`. `Tainted[T, Context]` is intentionally not assignable to plain `T`; code must pass through an explicit sanitizer boundary whose return type is untainted.

## Rules

- `Tainted[T, C]` is assignable to `Tainted[U, C]` only when `T` is assignable to `U` and the context matches.
- `Tainted[T, C]` is not assignable to plain `T`.
- Plain `T` is not assignable to `Tainted[T, C]`.
- `@source`, `@sink`, and `@sanitizer` decorators are recognized as TypePython effect facts for the vertical slice.
- Passing a direct `@source` result to a direct `@sink` call is rejected even when both signatures use plain runtime types; local assignments carry that taint forward inside the module slice, and assigning through a `@sanitizer` callable clears the source-to-sink diagnostic.
- Top-like checker escape hatches (`Any`, `unknown`, `dynamic`) keep their existing behavior.

## Example

```python
@source
def body() -> Tainted[str, "html"]: ...

@sanitizer
def escape_html(value: Tainted[str, "html"]) -> str: ...
@sink
def render_html(value: str) -> None: ...

raw: Tainted[str, "html"] = body()
safe: str = raw                 # TPY4001
safe = escape_html(body())      # accepted
render_html(body())             # TPY4001
render_html(raw)                # TPY4001
render_html(escape_html(body())) # accepted
```

Framework adapters can use the same `@source`, `@sink`, and `@sanitizer` vocabulary for FastAPI-style request bodies, template/HTML sinks, and escaping helpers without introducing runtime TypePython artifacts.
