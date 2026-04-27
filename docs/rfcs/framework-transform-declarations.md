# RFC: Framework Transform Declarations

**Status:** accepted for implementation planning  
**Strategic source:** `docs/strategic-todo.md` P0 Framework Shape and Decorator Transform System  
**Scope:** declarative static-shape metadata for framework behavior that ordinary Python checkers cannot infer without plugins

## Summary

TypePython should let a framework describe how a runtime declaration changes the static public surface, then emit ordinary `.py` and authoritative `.pyi` artifacts that downstream tools can consume without checker-specific plugins.

The first supported transform families are:

- **function-to-object decorator transforms**, such as a task decorator that leaves the runtime decorator in `.py` but exposes a typed `Task[P, R]` object in `.pyi`
- **class shape rewriters**, such as model-like decorators, bases, or metaclasses that synthesize constructor parameters, fields, generated members, and readonly behavior

Transform declarations are intentionally declarative. They may name static provider metadata, field collection rules, generated members, and fallback behavior, but they must not execute arbitrary framework code during compilation.

## Goals

1. Represent framework magic once in TypePython-authored metadata.
2. Preserve runtime framework behavior in emitted `.py`.
3. Emit checker-neutral `.pyi` that reflects the transformed static surface rather than the raw source declaration.
4. Make unsupported dynamic behavior deterministic: strict mode diagnoses it, non-strict mode degrades at explicit dynamic boundaries.
5. Keep the mechanism general enough for Pydantic-like models, task queues, ORMs, CLIs, and route decorators without hard-coding every framework in the checker.

## Non-goals

- Recreating mypy's arbitrary plugin API.
- Executing framework adapter Python code during normal compilation.
- Guaranteeing support for runtime-computed aliases, validators, or generated members that cannot be described statically.
- Making external checker plugins part of TypePython's success path.

## Terminology

- **Runtime declaration:** the source declaration and decorators/bases/metaclass that Python executes at runtime.
- **Static shape:** the public type surface TypePython exposes to checkers and IDEs after applying known transforms.
- **Transform provider:** the declaration, imported symbol, sidecar entry, or config entry that declares transform metadata.
- **Transform target:** the function, method, class, or member declaration to which a transform provider applies.
- **Generated member:** a field, method, class attribute, descriptor-backed attribute, manager, validator, metadata object, or protocol member synthesized by the transform.
- **Generated constructor:** an explicit synthetic `__init__` signature built from field and transform metadata.
- **Replacement callable/object:** the static object replacing the raw function surface after a decorator, such as `Task[P, R]` instead of `Callable[P, R]`.
- **Emitted stub authority:** the rule that generated `.pyi` is the downstream contract; external checkers do not need to understand TypePython transform syntax.

## Declaration locations

Transform declarations may appear in three places, resolved in this order:

1. **Inline `.tpy` provider declarations** for first-party framework definitions and examples.
2. **Sidecar `.tpyi` declarations** for typed framework packages where runtime source should stay unchanged.
3. **Project or adapter configuration** for third-party frameworks that ship declarative metadata independently of TypePython source.

All three forms must lower into the same internal metadata model. A provider may be imported across modules; binding summaries must preserve enough provider identity and metadata for downstream modules to resolve transform applications without reparsing the provider source.

## Proposed source forms

The inline syntax is descriptive and intentionally close to normal Python typing:

```python
from typing import Callable

transform decorator def celery_task[**P, R](fn: Callable[P, R]) -> Task[P, R]:
    replacement = object(type="Task[P, R]")
    preserve_runtime_decorator = True
```

For class shape rewrites:

```python
transform class_decorator def model(cls: type[T]) -> type[T]:
    collect_fields = "annotated_instance_fields"
    constructor = "fields"
    alias_source = "field_specifier"
    frozen_source = "field_or_model"
```

Sidecar or manifest metadata may express the same facts in TOML-like form:

```toml
[[transforms]]
provider = "toyframework.task"
kind = "function_to_object_decorator"
replacement_type = "toyframework.Task[P, R]"
preserve_paramspec = true
preserve_return_type = true

[[transforms]]
provider = "toyframework.model"
kind = "class_shape_rewriter"
field_collection = "annotated_instance_fields"
constructor = "fields"
generated_members = ["objects: toyframework.Manager[Self]"]
```

The exact parser syntax may evolve, but the metadata model below is the compatibility target.

## Implementation status

The compiler now has an empty-by-default framework transform metadata channel alongside the existing `dataclass_transform` and callable-decorator transform metadata. `FrameworkTransformModuleInfo` records provider declarations through `FrameworkTransformProviderSite`, `FrameworkTransformProviderKind`, `FrameworkTransformCapability`, and `FrameworkTransformFallback`; binding summaries preserve that channel so later phases can consume provider metadata without reopening source files. Parser collection and semantic application are intentionally still future work.

## Minimal metadata model

Every transform provider records:

- provider qualified name and module identity
- provider kind (`function_to_object_decorator`, `class_decorator`, `base_class`, `metaclass`, `function_callable_transform`)
- target declaration kind (`function`, `method`, `class`)
- static application constraints, including decorator order and generic parameter preservation
- source span for provider metadata, so diagnostics can point to the declaration that made a claim

Function-to-object transforms additionally record:

- replacement object type template
- `ParamSpec`, `TypeVar`, and `TypeVarTuple` mapping from the original function
- method templates exposed by the replacement object, such as `delay(*P.args, **P.kwargs) -> AsyncResult[R]`
- whether overload items are preserved as replacement overloads
- async function handling (`Awaitable[R]`, preserved coroutine result, or framework-specific replacement)

Class shape transforms additionally record:

- field collection strategy
- constructor generation strategy and explicit synthetic signature
- alias handling and keyword-only behavior
- required vs optional field rules
- readonly/frozen field rules
- descriptor-backed attribute rules
- generated member declarations
- method synthesis rules
- source spans for fields and generated constructor parameters

## Runtime-only metadata fallback

If a transform depends on a value TypePython cannot statically evaluate, the provider must choose one of these outcomes:

1. **Known unsupported:** emit a deterministic diagnostic explaining which metadata is runtime-only.
2. **Partial static surface:** synthesize the known members and mark the unknown portion as a `dynamic` boundary.
3. **Non-strict degradation:** preserve the raw declaration surface where doing so is safer than guessing a transformed surface.

Strict mode must never guess a generated field, alias, constructor parameter, or replacement object method from runtime-only metadata.

## Diagnostics

Strict-mode diagnostics should cover:

- unknown transform provider
- provider metadata shape that does not match the target declaration kind
- unsupported function-to-object replacement when no replacement type is available
- dynamic aliases that prevent safe constructor typing
- field metadata that cannot be statically evaluated
- descriptor-backed fields with unknown get/set surfaces
- generic preservation failures for `ParamSpec`, `TypeVar`, or `TypeVarTuple`
- emitted stub/runtime name mismatches during `verify`

Non-strict mode should report notes or warnings only when degradation changes the exported surface in a way that can surprise downstream checkers.

## Emission rules

- `.py` output preserves the original runtime framework decorators, bases, metaclasses, and declarations unless a future feature explicitly opts into runtime code generation.
- `.pyi` output reflects the transformed static surface.
- Replacement objects appear as values of the replacement type in `.pyi`; the raw function is not emitted as callable unless the replacement type is itself callable.
- Generated constructors and generated members are emitted explicitly in `.pyi`.
- Stub generation must remain checker-neutral: mypy, pyright, ty, and IDEs should see only standard typing constructs.

## Verification requirements

`typepython verify` should eventually validate that:

1. every transformed `.pyi` public name corresponds to a runtime name preserved in `.py`
2. generated constructor/member names do not claim runtime names that the framework cannot provide
3. downstream checker runs can consume the generated `.pyi` without TypePython-specific plugins

## First vertical slice

The implementation sequence should be:

1. Parse and bind provider metadata for a toy task decorator.
2. Preserve original function parameters with `ParamSpec`.
3. Emit `.py` with the framework decorator unchanged.
4. Emit `.pyi` where the decorated function name has type `Task[P, R]` and exposes typed task methods.
5. Add downstream checker fixtures for mypy, pyright, and ty.
6. Add one strict diagnostic for unsupported non-callable decorator replacement.
7. Reuse the same metadata path for a toy model class rewriter.

## Open questions

- Whether inline syntax should use a dedicated `transform` keyword or a standard-decorator-plus-sidecar form for the first shipped prototype.
- Whether adapter manifests should be loaded only from project configuration at first, or from installed framework packages after a trust model is documented.
- How much generated-member runtime parity `verify` can check without executing framework code.
