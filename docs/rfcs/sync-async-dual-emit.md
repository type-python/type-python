# RFC: Sync/Async Dual Emit

**Status:** design only; implementation deferred  
**Strategic source:** `docs/strategic-todo.md` P2 Sync/Async Dual Emit  
**Scope:** explicit code-generation sugar for paired sync and async APIs, not a general async-to-sync converter

## Summary

TypePython already preserves ordinary authored `async def`, `await`, `async for`, and `async with`.
Dual emit is a separate future feature for library authors who maintain one source body but need both
sync and async public APIs.

The feature must be explicit. TypePython should never silently generate a sync API from arbitrary
async code.

## Source form

The preferred source form is a TypePython-only modifier:

```python
dual async def fetch_user(client: AsyncClient, user_id: str) -> User:
    response = await client.get(f"/users/{user_id}")
    return decode_user(response)
```

A decorator form remains a fallback only if parser experiments show the modifier creates ambiguity:

```python
@dual_emit(sync_name="fetch_user", async_name="afetch_user")
async def fetch_user_impl(client: AsyncClient, user_id: str) -> User:
    ...
```

## Naming policy

Default generated names:

- authored name without an async prefix becomes the sync name
- async output gains an `a` prefix: `fetch_user` and `afetch_user`
- if the authored name already starts with `a` followed by a lowercase verb, the author must supply
  explicit names to avoid surprising output

Explicit names win over defaults and must be unique in the module.

## Mapping declarations

Dual emit requires static mappings from async-only surfaces to sync equivalents. A future mapping
declaration can be module-local or adapter-provided:

```toml
[[sync_async_mappings]]
async_type = "httpx.AsyncClient"
sync_type = "httpx.Client"

[[sync_async_mappings.methods]]
async = "get"
sync = "get"
await_result = true
```

Mappings must cover:

- receiver type substitutions, such as `AsyncClient` to `Client`
- method/function name substitutions
- context-manager substitutions for `async with`
- return type substitutions when async APIs wrap results in `Awaitable[T]`

## Lowering rules

For the initial supported subset:

- direct `await mapped_call(...)` lowers to the mapped sync call
- `async with mapped_context(...) as x` lowers to `with mapped_context_sync(...) as x`
- `async for` is design-only until iterator mapping semantics are proven
- ordinary statements, control flow, and local variables are duplicated into both emitted bodies in
  source order

Unsupported constructs must produce deterministic diagnostics instead of partial output. Examples:
task groups, cancellation-sensitive logic, streaming async generators, arbitrary event-loop access,
and callbacks without declared sync equivalents.

## Output ordering and stubs

Emitted `.py` should place the sync function first, followed by the async function, preserving source
module order around the pair. Emitted `.pyi` should declare both functions with the mapped signatures:

```python
def fetch_user(client: Client, user_id: str) -> User: ...
async def afetch_user(client: AsyncClient, user_id: str) -> User: ...
```

Source maps must point diagnostics in either emitted body back to the single `dual async def` source
range and, when possible, to the exact mapped expression.

## Examples

### HTTP client wrapper

```python
from httpx import AsyncClient, Client


class User:
    id: str
    name: str


def decode_user(payload: bytes) -> User:
    ...


dual async def fetch_user(client: AsyncClient, user_id: str) -> User:
    response = await client.get(f"/users/{user_id}")
    return decode_user(response.content)
```

Given a mapping from `httpx.AsyncClient` to `httpx.Client`, the pair lowers to a sync wrapper for
ordinary applications plus an async wrapper for event-loop-native callers:

```python
def fetch_user(client: Client, user_id: str) -> User:
    response = client.get(f"/users/{user_id}")
    return decode_user(response.content)


async def afetch_user(client: AsyncClient, user_id: str) -> User:
    response = await client.get(f"/users/{user_id}")
    return decode_user(response.content)
```

### Database access helper

```python
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import Session


class Row:
    id: int


def decode_row(value: object) -> Row:
    ...


dual async def load_row(session: AsyncSession, row_id: int) -> Row:
    result = await session.execute("select * from rows where id = :id", {"id": row_id})
    return decode_row(result.one())
```

The corresponding mapping substitutes `AsyncSession` with `Session` and removes `await` from the
declared sync equivalent of `execute`:

```python
def load_row(session: Session, row_id: int) -> Row:
    result = session.execute("select * from rows where id = :id", {"id": row_id})
    return decode_row(result.one())


async def aload_row(session: AsyncSession, row_id: int) -> Row:
    result = await session.execute("select * from rows where id = :id", {"id": row_id})
    return decode_row(result.one())
```

Both examples intentionally rely on declared mappings. If the HTTP client or database session lacks a
sync equivalent for a called method, TypePython must reject the `dual async def` instead of silently
emitting a partial or event-loop-driving sync body.

## Implementation boundary

Design completion does not imply parser, lowering, source-map, stub, examples, or downstream-checker
support. Those remain separate implementation tasks.
