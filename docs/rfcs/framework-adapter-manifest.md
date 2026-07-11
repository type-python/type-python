# RFC: Framework Adapter Manifest Prototype

**Status:** prototype design only  
**Strategic source:** `docs/strategic-todo.md` P1 Framework Adapter SDK Prototype  
**Scope:** a constrained `typepython-framework.toml` manifest format for first-party adapter fixtures

## Summary

Framework adapters should describe static shape transforms declaratively, without executing
framework code during compilation. The prototype manifest is a TOML representation of the same
metadata already exercised by the toy task fixture and the Pydantic-like `BaseModel` fixture.

The manifest is intentionally not a public registry format. It is a local validation contract for
first-party fixtures until at least one validation-model adapter and one non-validation adapter have
proven the abstraction.

Because manifests lower into ordinary `.py`/`.pyi` artifacts, a framework can ship TypePython adapter
metadata without also maintaining a mypy plugin or checker-specific extension. The prototype contract
is deliberately narrower: adapters may be distributed with a framework package or companion metadata
package, but TypePython treats them as local, reviewable data until several independent adapters prove
the same manifest path.

## Manifest shape

```toml
[adapter]
name = "toy-pydantic"
version = "0.1.0"
framework = "toy.pydantic"
typepython_min = "0.3.0"
python_targets = ["3.12"]
stability = "prototype"

[[transforms]]
provider = "toy.pydantic.BaseModel"
kind = "base_class"
target = "class"
capabilities = [
  "field_collection",
  "constructor_generation",
  "alias_handling",
  "required_optional_fields",
  "readonly_fields",
]
field_collector = "annotated_class_fields"
constructor = "fields"
alias = { source = "field_specifier", keyword = "alias", literal_only = true }
default = { keyword = "default" }
default_factory = { keyword = "default_factory" }
frozen = { model_keyword = "frozen_default", field_keyword = "frozen" }
fallback = "strict_diagnostic"

[[transforms]]
provider = "toy.tasks.task"
kind = "function_to_object_decorator"
target = "function"
capabilities = ["function_to_object_replacement", "generic_preservation"]
replacement_type = "toy.tasks.Task[P, R]"
preserve_paramspec = true
preserve_return_type = true
fallback = "strict_diagnostic"

[[transforms]]
provider = "toy.http.remote_call"
kind = "function_decorator"
target = "function"
capabilities = ["effect_io_net", "effect_time"]
fallback = "strict_diagnostic"

[[golden_tests]]
name = "pydantic-like-package"
input = "test-fixtures/downstream-checkers/pydantic-like-package/src/app/__init__.tpy"
expected_py = "test-fixtures/downstream-checkers/pydantic-like-package/expected/app/__init__.py"
expected_pyi = "test-fixtures/downstream-checkers/pydantic-like-package/expected/app/__init__.pyi"
checkers = ["mypy", "pyright", "ty"]

[[golden_tests]]
name = "toy-task-package"
input = "test-fixtures/downstream-checkers/toy-task-package/src/app/__init__.tpy"
expected_py = "test-fixtures/downstream-checkers/toy-task-package/expected/app/__init__.py"
expected_pyi = "test-fixtures/downstream-checkers/toy-task-package/expected/app/__init__.pyi"
checkers = ["mypy", "pyright", "ty"]
```

## Allowed declarations

Prototype manifests may declare only these transform families:

- class decorator transforms
- base class transforms
- metaclass transforms
- function-to-object decorator transforms
- field collector rules over annotated class fields
- constructor synthesis from collected fields
- descriptor-backed attribute exclusion rules
- alias, default, default factory, and frozen metadata mapping from statically evaluable field
  specifier keywords

Every declared transform must lower to the existing framework transform metadata model. If the
compiler cannot express a manifest feature through that model, the adapter is invalid rather than
partially executed.

### Strict schema contract

The prototype schema is closed at every level. Unknown keys in the manifest root, `[adapter]`, a
`[[transforms]]` or `[[golden_tests]]` table, or an inline mapping are validation errors; keys are
never retained or silently ignored. This includes misspellings and proposed extension fields that
have not been added to this RFC.

Class transforms use these optional static keyword mappings:

- `alias = { source, keyword, literal_only }` requires `alias_handling`, supports only
  `source = "field_specifier"`, and requires `literal_only = true`.
- `default = { keyword }` and `default_factory = { keyword }` require
  `required_optional_fields`.
- `frozen = { model_keyword, field_keyword }` requires `readonly_fields`.

Mapping keywords may be framework-specific, but each must be a non-empty Python identifier. These
mapping fields are valid only for class transforms. Class transforms must declare
`field_collection` and `constructor_generation`, with
`field_collector = "annotated_class_fields"` and `constructor = "fields"`.

A `function_to_object_decorator` must declare `function_to_object_replacement` and a parseable
`replacement_type`. When it declares `generic_preservation`, both `preserve_paramspec` and
`preserve_return_type` must be present and true; those flags are invalid without that capability.
Every transform must explicitly select `strict_diagnostic` or `non_strict_degrade` as its fallback.

The compatibility discussion below calls for a future framework-version range, but this prototype
does not yet define a manifest key for one. Authors must not invent such a key: the strict schema
rejects it until its name and semantics are specified here.

## Safety rules

- Adapter manifests are data, not code.
- The compiler must not import, evaluate, or execute adapter Python during manifest validation.
- Alias/default/frozen metadata must come from literals or explicitly supported static values.
- Runtime-computed metadata must produce deterministic diagnostics or an explicit dynamic boundary.
- Fallback behavior must be one of `strict_diagnostic` or `non_strict_degrade`.

## Validation command sketch

A future command should validate manifests before compilation:

```bash
typepython adapter validate typepython-framework.toml
```

The command should check schema validity, provider kinds, allowed capability combinations, local
golden-test paths, downstream checker expectations, and compatibility metadata. It should reject
unsafe declarations before a project build can consume them.

Malformed TOML, missing required schema fields, type mismatches, and unknown fields are ordinary
`TPY7003` validation failures. Text and JSON modes return exit status `1` with the normal diagnostic
report instead of treating author-controlled schema errors as internal failures.

## Compatibility metadata

Local validation needs enough metadata to explain whether an adapter can be used in a project:

- adapter name and version
- framework package name and supported version range
- minimum TypePython version
- supported Python targets
- required downstream checker coverage
- stability marker (`prototype`, `experimental`, or future `stable`)

For the prototype SDK, `stability = "prototype"` is the only supported marker. Future `experimental`
or `stable` values require multiple adapters, downstream checker fixtures, runtime-smoke coverage,
and a versioning policy; accepting those values before then would imply a compatibility promise the
manifest format has not earned.

## Non-goals

- No public registry format yet.
- No arbitrary Python execution.
- No checker-specific plugin hooks.
- No broad versioning policy until the Pydantic-like and toy task adapters both validate through
  the same manifest path.
