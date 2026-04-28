# RFC: Result, ADT, and Effect Experiment

**Status:** experimental pattern, no new syntax in Core v1  
**Strategic source:** `docs/strategic-todo.md` P2 Result, ADT, and Effect Experiment  
**Scope:** opt-in Rust-like error modeling with existing sealed classes and exhaustive `match`

## Summary

TypePython already has the two primitives needed for an opt-in `Result[T, E]` style: generic
classes and sealed-class exhaustiveness. The standard pattern should be a small sealed hierarchy
that authors define or import when they want explicit error values, while ordinary Python exceptions
remain the default interop model.

No Core v1 checker rule requires throws annotations for imports, functions, or framework callbacks.
No Java-style checked-exception model is planned.

## Standard pattern

```python
sealed class Result[T, E]:
    pass

class Ok[T, E](Result[T, E]):
    value: T

class Err[T, E](Result[T, E]):
    error: E
```

Functions opt into this style by returning the sealed root:

```python
def parse_user(raw: str) -> Result[User, ParseError]:
    user = try_parse(raw)
    if user is None:
        return Err(ParseError("invalid user"))
    return Ok(user)
```

Consumers use normal pattern matching. With sealed exhaustiveness enabled, omitting either branch is
diagnosed by the existing finite-domain `match` analysis.

```python
def display(result: Result[User, ParseError]) -> str:
    match result:
        case Ok(value=user):
            return user.name
        case Err(error=err):
            return err.message
```

The pattern lowers to ordinary Python classes and ordinary `.pyi` class surfaces. The stronger
exhaustiveness guarantee is authoring-time TypePython behavior; downstream checkers still see normal
nominal classes.

## `raises` decision

Optional `raises` syntax remains deferred behind a future experimental flag. If it is explored, it
should lower to metadata used by TypePython diagnostics and documentation, not to mandatory checked
exceptions. The syntax must not change ordinary Python exception behavior and must not require
throws annotations for arbitrary imports.

Potential future sketch:

```python
def load_config(path: str) raises ConfigError -> Config:
    ...
```

Open questions for that future experiment:

- whether `raises E` is documentation-only metadata or can drive quick fixes to `Result[T, E]`
- whether raised exception metadata is emitted into comments, sidecar metadata, or neither
- how to avoid false confidence around dynamic exceptions raised by imported Python code

## Acceptance boundary

- Users can opt into Rust-like error style today by writing a sealed `Result` hierarchy.
- Existing exception behavior is preserved by default.
- The feature remains optional and does not burden ordinary Python interop.
- Any future `raises` syntax must be explicitly experimental and must not become a prerequisite for
  calling Python libraries.
