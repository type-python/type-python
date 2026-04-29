# RFC: Phase 1 Internal Shape Model

**Status:** accepted for implementation planning  
**Strategic source:** `docs/strategic-todo.md` P0 First-Class Record and Shape Model  
**Scope:** compiler-internal semantic representation for field-bearing declarations and projected shapes

## Summary

TypePython needs one shared representation for field-bearing things so `TypedDict` transforms, `data class`, standard `@dataclass`, `dataclass_transform`, and future framework transforms do not each invent incompatible field models.

Phase 1 introduces `Shape` as an internal semantic object. It is not a public guarantee that arbitrary classes, protocols, or interfaces can participate in `Pick`, `Partial`, `Omit`, `Readonly`, `Mutable`, or `Required_`.

## Goals

1. Preserve existing `TypedDict` transform behavior while moving it toward shared shape primitives.
2. Provide a reusable model for class-shape rewriters and framework integrations.
3. Make shape metadata rich enough for constructor synthesis, generated members, alias handling, readonly diagnostics, and `.pyi` emission.
4. Keep generated stubs standard Python typing.
5. Explicitly defer public arbitrary-type transforms until assignability semantics are stable.

## Phase 1 shape sources

Phase 1 shape extraction is limited to:

- `TypedDict`
- TypePython `data class`
- standard `@dataclass`
- `dataclass_transform`-driven dataclass-like classes
- transformed framework classes described by framework-transform metadata

Future candidates are documented but disabled by default:

- `interface` / `Protocol`
- ordinary classes with annotated instance fields

## Shape object

A `Shape` records:

- shape identity and stable emitted-name seed
- source kind (`typed_dict`, `tpy_data_class`, `dataclass`, `dataclass_transform`, `framework_transform`)
- nominal owner when the shape remains tied to a class or alias
- structural projection metadata when the shape is produced by a transform operation
- ordered fields
- source span for diagnostics and hover rendering
- visibility/public-surface status

The compiler may keep this object in `typepython_checking` initially and persist only summary-safe facts into incremental metadata when needed.

## Field metadata

Every field records:

- field name
- public alias
- semantic type
- required vs optional
- readonly vs mutable
- constructor participation
- default value presence
- default factory presence
- descriptor behavior
- source declaration span
- field source kind (`typed_dict_item`, `class_annotation`, `dataclass_field_specifier`, `framework_generated`, `projection_generated`)

Field ordering must be deterministic. Inherited or generated fields are flattened according to the source kind's existing rules before projection operations run.

## Shape operations

The shared model supports these internal operations:

- `Partial`: make all fields optional
- `Required_`: make all fields required
- `Readonly`: mark all fields readonly
- `Mutable`: remove readonly markers from all fields
- `Pick`: retain named fields
- `Omit`: remove named fields
- shape composition for transform providers that layer generated members onto collected fields
- shape projection for emitted aliases and hover/signature rendering

Operations preserve field type, alias, constructor participation, default metadata, descriptor behavior, and source span unless the operation explicitly changes the corresponding property.

Unknown keys in `Pick`/`Omit` should be diagnosed with suggestions computed from the source shape's field names and public aliases.

## Assignability boundaries

Phase 1 keeps the public assignability contract conservative:

- `TypedDict` projections keep existing structural `TypedDict` assignability rules.
- Class-backed shapes remain nominal unless a transform explicitly emits a structural stub surface.
- Shape projection over `data class`, `@dataclass`, `dataclass_transform`, and framework classes is experimental and must be gated before becoming a public source-language promise.
- Arbitrary class, protocol, and interface transforms remain deferred.

This means `Pick[SomeClass, "field"]` should not become generally valid merely because `Shape` can internally describe `SomeClass` fields.

## Lowering and stub emission

Shape aliases lower into checker-neutral `.pyi`:

- `TypedDict` projections emit concrete `TypedDict` definitions.
- Class-backed constructor shapes emit explicit synthetic `__init__` signatures when the transform owns constructor generation.
- Generated members emit ordinary fields, methods, properties, or class attributes.
- Stable generated names are derived from the owning alias or declaration plus a deterministic projection suffix.

Generated names must not depend on map iteration order, filesystem order, or diagnostics order.

## Hover and diagnostics

LSP hover should render projected shapes compactly:

```text
Shape UserUpdate
  name: str | optional
  email: str | optional
```

Diagnostics should use shape metadata to report:

- unknown keys with suggestions
- invalid transform source kinds
- readonly field mutation
- constructor arguments that are excluded from `__init__`
- runtime-only framework metadata that prevents static shape synthesis

## Implementation sequence

1. Add shape structs in `typepython_checking` near the existing `TypedDictShape` model.
2. Convert current `TypedDictShape` construction to populate shared `Shape`/`ShapeField` primitives while preserving diagnostics and tests.
3. Route `Partial`, `Required_`, `Readonly`, `Mutable`, `Pick`, and `Omit` through shape operations.
4. Emit stable generated names for projected aliases.
5. Add hover rendering for projected shapes.
6. Add unknown-key suggestions for shape projection diagnostics.
7. Gate non-`TypedDict` shape transforms behind an experimental flag before exposing them in source syntax.

### Implemented P1 slice

The implemented slice uses a shared syntax-level `ShapeProjection` / `ShapeProjectionField` substrate for lightweight projection paths that must run outside `typepython_checking`. Lowering and LSP hover both resolve field-bearing sources into that shared projection model before applying `Partial`, `Required_`, `Readonly`, `Mutable`, `Pick`, `Omit`, or supported `MapValues` operations. Lowering then materializes standard `TypedDict` output, while LSP renders the same projected field metadata for `TypedDict`, TypePython `data class`, dataclass-transform, and framework-backed shapes.

Checker-side semantic `Shape` remains the richer internal model for assignability, constructor synthesis, framework transforms, readonly diagnostics, and incremental public summaries. The shared `ShapeProjection` substrate is intentionally narrower: it prevents lowering and LSP from carrying separate ad-hoc field models while preserving the checker as the source of semantic truth.

Incremental public summaries now carry stable `shapeFingerprints` in solver facts. The fingerprint is derived from a field-bearing declaration's exported field/member surface, so shape-affecting changes invalidate downstream summaries without relying on raw source noise.

## Acceptance criteria

- Existing `TypedDict` transform tests continue to pass unchanged.
- `Pick` and `Partial` can operate on at least one non-`TypedDict` shape source behind an experimental flag.
- Framework transform metadata can reuse the same field and constructor model.
- Generated stubs stay standard Python typing.
- Documentation makes clear that public arbitrary-type transforms remain deferred.
