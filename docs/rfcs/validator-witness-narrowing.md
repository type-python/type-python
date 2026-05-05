# RFC: Validator Witness Narrowing Slice

**Status:** internal P4 vertical slice  
**Scope:** true-branch narrowing for `unknown`; no full runtime soundness claim

## Summary

This slice introduces a conservative checker-only witness form, `ValidatorWitness[T, Trust]`. A trusted generated or adapter-declared validator returning `ValidatorWitness[T, Literal["trusted"]]` or `ValidatorWitness[T, Literal["generated"]]` narrows an `unknown` argument to `T` inside the predicate's true branch. TypePython-generated boundary validators may also expose the same trust through explicit decorator metadata such as `@validator(User, trust="generated")` on a `bool` predicate. The witness is local to flow analysis and is invalidated by reassignment, deletion, detected mutation of the validated name, and mutation through a local alias assigned from that validated name.

## Example

```python
class User:
    name: str

def validate_user(value: unknown) -> ValidatorWitness[User, Literal["trusted"]]: ...

@validator(User, trust="generated")
def generated_validate_user(value: unknown) -> bool: ...

def handle(value: unknown) -> str:
    if validate_user(value):
        return value.name  # value is narrowed to User in this branch
    return ""
```

Adapter-declared or TypePython-generated validators expose trust in explicit metadata rather than through the callable name. The checker accepts `Literal["trusted"]` and `Literal["generated"]` witness markers, generated-validator decorators only when they carry `trust="trusted"` or `trust="generated"`, and adapter decorators only when their provider advertises `validator_witness`. A one-argument `ValidatorWitness[T]`, broad names such as `validate_*`, `*_validator`, and arbitrary `is_*` predicates are not treated as proofs. The checker treats the witness like a flow fact, not like a globally persistent proof.

When a member access or method call still sees `unknown` after a validator predicate, diagnostics include a trust-boundary note explaining whether the witness was untrusted or whether assignment, deletion, direct mutation, or alias mutation invalidated the validated value after the boundary check. Alias tracking is intentionally local and conservative: if `alias = value` occurs after the witness is created, a later rebind or mutation of `alias` invalidates the witness for `value` in that flow window. That note is intentionally attached to the unsafe use site rather than emitted as a global proof failure: it preserves TypePython's author-time-only semantics and keeps emitted Python/stubs standard.

## Non-goals

- No claim that arbitrary validators are faithful to TypePython's full type semantics.
- No cross-function witness persistence.
- No whole-program aliasing, heap, or container witness calculus beyond local name and local alias invalidation in one function body.
- No emitted `.py` or `.pyi` changes.
