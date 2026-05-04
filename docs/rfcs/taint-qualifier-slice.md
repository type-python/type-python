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
- `@source`, `@sink`, and `@sanitizer` decorators are recognized as TypePython effect facts for the vertical slice, and those facts survive TypePython module imports through the public effect summary channel.
- Decorator calls may carry a string taint context: `@source("html")`, `@sink(context="html")`, and `@sanitizer(from_taint="html")`.
- Passing a direct `@source` result to a matching direct `@sink` call is rejected with `TPY4028` even when both signatures use plain runtime types; local assignments carry that taint forward inside the module slice, and assigning through a matching `@sanitizer` callable clears the source-to-sink diagnostic.
- Bare `@source`, `@sink`, and `@sanitizer` facts remain wildcard facts for compatibility. A context-specific source only flows to the same context or a bare sink; a bare source flows to any sink. A bare sanitizer clears every sink context, while a context-specific sanitizer clears only that context.
- Top-like checker escape hatches (`Any`, `unknown`, `dynamic`) keep their existing behavior.

## Example

```python
@source("html")
def body() -> Tainted[str, "html"]: ...

@sanitizer("html")
def escape_html(value: Tainted[str, "html"]) -> str: ...
@sanitizer("sql")
def escape_sql(value: str) -> str: ...

@sink("html")
def render_html(value: str) -> None: ...

@sink("sql")
def query_sql(value: str) -> None: ...

raw: Tainted[str, "html"] = body()
safe: str = raw                 # TPY4001
safe = escape_html(body())      # accepted
render_html(body())             # TPY4001 or TPY4028, depending on whether the flow is type-qualified or decorator-only
render_html(raw)                # TPY4001 or TPY4028, depending on whether the flow is type-qualified or decorator-only
render_html(escape_html(body())) # accepted
render_html(escape_sql(body()))  # TPY4028: wrong sanitizer context
query_sql(body())                # accepted by decorator-only context matching
```

Framework adapters can use the same `@source`, `@sink`, and `@sanitizer` vocabulary for FastAPI-style request bodies, template/HTML sinks, and escaping helpers without introducing runtime TypePython artifacts. Source, sink, and sanitizer declarations may live in a framework shim module while application code imports and composes them; the checker consumes the imported taint facts from effect summaries.

LSP hover renders contextual taint facts in effect summaries, for example `taint.source[html]`, `taint.sink[html]`, and `taint.sanitize[html]`. Inferred hover summaries preserve those context labels when a local function or imported callable forwards a contextual taint source.
