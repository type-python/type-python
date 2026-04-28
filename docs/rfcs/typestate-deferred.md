# RFC: Deferred Typestate Model

**Status:** research note; implementation deferred  
**Strategic source:** `docs/strategic-todo.md` Deferred Typestate  
**Scope:** possible future modeling pattern for finite states and method transitions, not a Core v1 language feature

## Summary

Typestate can help model protocols such as unopened/open/closed resources, initialized drivers, and
transaction lifecycles. TypePython should not introduce a general typestate system until the lighter
lifecycle diagnostics and framework-shape work have real users. If this area is revisited, the first
shape should be a small sealed-state-token model rather than arbitrary dependent receiver types.

## Finite states with sealed classes

Finite state sets can already be represented with TypePython's sealed classes:

```python
sealed class ConnectionState:
    pass


data class Disconnected(ConnectionState):
    pass


data class Connected(ConnectionState):
    handle: int


data class Closed(ConnectionState):
    pass
```

The useful future extension is not the state representation itself; it is connecting a value's allowed
operations to one of those state tokens while preserving exhaustiveness checking for state transitions.

## Method transition model

A conservative design starts with explicit transition functions or methods whose signatures name both
the input and output state:

```python
class Socket[S: ConnectionState]:
    state: S


def connect(sock: Socket[Disconnected]) -> Socket[Connected]: ...


def close(sock: Socket[Connected]) -> Socket[Closed]: ...
```

This keeps transitions visible in ordinary type signatures. It avoids hidden mutation of a receiver's
type and makes it possible for external checkers to understand at least the function-result part of the
protocol.

Receiver-state mutation is harder:

```python
sock.connect()
# Is `sock` now `Socket[Connected]`?
```

TypePython should not promise this until it has a clear assignment/narrowing story for mutable aliases.
The first implementation, if any, should prefer returned-state values over in-place receiver rewrites.

## Optional runtime assertions

Runtime assertions should be opt-in and reviewable, similar to boundary validator generation. A future
configuration could request guards at selected transition boundaries:

```python
def send(sock: Socket[Connected], payload: bytes) -> None:
    ...
```

When enabled, emitted Python may assert the runtime state tag before executing the body. When disabled,
TypePython should emit standard Python and rely on static checking. The default should be no runtime
assertions so ordinary library output remains side-effect-compatible with authored code.

## External `.pyi` exposure

Generated stubs can expose useful state transitions when the model is explicit:

```python
def connect(sock: Socket[Disconnected]) -> Socket[Connected]: ...
def close(sock: Socket[Connected]) -> Socket[Closed]: ...
```

This is checker-neutral because state is just a type parameter. Stubs cannot portably express "this
method changes the receiver variable's type after the call" across mypy, pyright, and ty. Any future
receiver-state feature therefore needs TypePython-specific diagnostics plus a graceful `.pyi` fallback
that shows the callable shape without overpromising mutation tracking.

## Non-goals

- No arbitrary dependent types in Core.
- No path-sensitive alias analysis for mutable receiver state in the first version.
- No implicit runtime checks unless a project explicitly opts in.
- No framework-specific resource protocol before lifecycle diagnostics prove the demand.
