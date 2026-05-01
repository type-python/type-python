# RFC: Must-Use, Must-Await, and Must-Close Diagnostics

**Status:** implemented local checker slice
**Strategic source:** `docs/strategic-todo.md` P2 Must-Use, Must-Await, and Must-Close Diagnostics  
**Scope:** lightweight intra-procedural lifecycle checks, not full typestate

## Summary

TypePython catches common lifecycle mistakes such as ignored task handles, un-awaited
coroutine-like objects, resources opened but never closed, and response streams that are never
consumed. The first implemented version is opt-in, intra-procedural, and conservative.

This feature is intentionally separate from full typestate. It does not model arbitrary state
machines or cross-function ownership transfer until simple local diagnostics prove useful.

## Opt-in annotations

The implemented surface uses decorators or adapter metadata with these meanings:

| Marker | Applies to | Obligation |
| --- | --- | --- |
| `@must_use` | function or class | the returned value must be used, assigned, returned, or passed onward |
| `@must_await` | function or awaitable-like class | the returned value must be awaited or returned from an async function |
| `@must_close` | resource class or factory | the value must be closed, used in `with`/`async with`, or intentionally escaped |
| `@must_consume` | stream/response class | the value must be iterated, read, closed, or intentionally escaped |

Framework adapters can attach related obligations to known resources without requiring source-level
decorators on third-party classes; lifecycle markers also feed `resource.lifecycle` effect facts so
imports, summaries, and LSP hover can explain the obligation.

## Diagnostic behavior

The first analysis pass stays intra-procedural and local-variable based:

- ignored return value: report when a marked call is used only as an expression statement
- assigned-but-never-used resource: report when a marked value is bound locally and neither consumed
  nor escaped before scope exit
- `with` / `async with`: treat successful context-manager entry as satisfying close/consume
  obligations for that block
- early returns: report obligations still owned by the function unless the value is returned or
  passed to a known transfer function
- exceptions: avoid path-sensitive exception guarantees in v1; only report obligations that are
  definitely unsatisfied on ordinary fallthrough paths

Diagnostics should be warnings at first and include a concrete escape hatch.

## Escape hatches

False-positive control must be explicit and reviewable:

```python
_ = intentionally_detach(task)      # transfer helper marks task as escaped
resource.close()                    # satisfies @must_close
stream.consume()                    # satisfies @must_consume
with open_resource() as resource:   # satisfies @must_close for the block
    ...
```

Future suppression syntax can use ordinary diagnostic-code suppression once a dedicated diagnostic
code exists. Until then, examples should prefer explicit transfer/consume helper calls over comments.

## Adapter targets

Common framework/resource adapters should start with:

- files and file-like objects
- HTTP responses and streaming responses
- database sessions
- transactions
- async tasks and task handles

Adapters should declare obligations as metadata. They must not execute framework code to discover
resource behavior.

## Non-goals

- No general typestate system in the first version.
- No inter-procedural ownership inference.
- No guarantee that exceptions close resources unless represented by a local `with`/`async with` or
  explicit cleanup call.
- No hard errors until warning precision is proven.

## Acceptance boundary

The implemented slice is complete for local marker meanings, diagnostics, escape hatches, adapter
targets, and non-goals. Future work remains inter-procedural ownership, richer transfer helpers,
and framework-specific resource adapters beyond the shared effect/capability vocabulary.
