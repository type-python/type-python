# RFC: Deferred Taint and Provenance Types

**Status:** research note; implementation deferred  
**Strategic source:** `docs/strategic-todo.md` Deferred Taint and Provenance Types  
**Scope:** future static metadata for untrusted data flow, not a replacement for SAST tools

## Summary

Taint tracking is useful for web and plugin boundaries, but broad security data-flow analysis competes
with dedicated SAST tools. TypePython should not attempt whole-program taint analysis in Core. A future
feature should start with explicit provenance wrappers, sanitizer declarations, and framework adapter
metadata for source/sink boundaries.

## Type wrappers

The minimal type vocabulary is:

```python
typealias Untrusted[T] = T
typealias Tainted[T] = T
```

These aliases are intentionally metadata-bearing at the TypePython layer. Emitted `.pyi` should expose
ordinary `T` unless a downstream checker gains a portable way to consume provenance metadata. The two
names have different intent:

- `Untrusted[T]`: data came from outside the trust boundary and needs validation before privileged use.
- `Tainted[T]`: data may carry a specific unsafe provenance, such as SQL, shell, path, HTML, or URL
  context.

## Sanitizer declarations

Sanitizers should be explicit and reviewable:

```toml
[[taint.sanitizers]]
name = "escape_html"
from = "Tainted[str]"
to = "str"
context = "html"
```

TypePython should treat sanitizer declarations as metadata and diagnostics input. It should not execute
sanitizers or infer sanitizer behavior from function names.

## Source and sink configuration

Framework adapters can declare sources and sinks:

```toml
[[taint.sources]]
kind = "http_request"
provider = "toy.fastapi.Request"
type = "Untrusted[dict[str, object]]"

[[taint.sinks]]
kind = "sql_query"
function = "db.execute"
expects = "str"
rejects = "Tainted[str]"
```

Diagnostics should report direct flows from declared sources to declared sinks when no sanitizer or
validator boundary is visible. The first version should stay intra-procedural or boundary-local rather
than attempting global data-flow analysis.

## Web framework integration

Route and request-model adapters are the natural first source of untrusted values. A FastAPI-like
adapter can mark raw request bodies, query parameters, headers, cookies, and path parameters as
`Untrusted[...]` until a Pydantic/msgspec/cattrs-style validator or declared sanitizer converts them to
trusted application types.

This should compose with boundary validator generation: validation can remove `Untrusted[...]`, while
context-specific sanitizers remove `Tainted[...]` for a particular sink context.

## Non-goals

- No replacement for SAST tools.
- No implicit sanitizer inference by name.
- No whole-program inter-procedural taint engine in the first version.
- No mandatory runtime dependency or runtime instrumentation.
