# Framework Adapter Authoring

Framework adapters describe static framework behavior as reviewable data. They are for cases where
runtime Python frameworks synthesize constructor parameters, generated members, replacement objects,
or field metadata that ordinary downstream checkers cannot infer without plugins.

The prototype adapter format is `typepython-framework.toml`. It is intentionally local and
experimental: adapters are validated before use, but TypePython does not yet load a public registry
or execute adapter Python code.

## First-party adapters

Use a first-party adapter when the framework or application code lives in the same repository as the
TypePython project. This is the safest path for early adoption because the adapter, fixtures, and
runtime framework behavior can be reviewed together.

Recommended layout:

```text
my-project/
  typepython.toml
  typepython-framework.toml
  src/
    app/
      __init__.tpy
  tests/
    adapter-golden/
      input.tpy
      expected.py
      expected.pyi
```

Validate the manifest before relying on it in a build:

```bash
typepython adapter validate typepython-framework.toml
```

For first-party fixtures, keep the adapter narrow:

- map class decorators, base classes, metaclasses, or function-to-object decorators only when they
  lower into TypePython's existing framework transform model
- collect fields from annotated class fields
- synthesize constructors from statically known fields
- map aliases, defaults, default factories, and frozen metadata from literal field-specifier
  keywords
- keep runtime behavior owned by the framework; the adapter should affect emitted static surfaces,
  not replace the framework runtime

## Third-party adapters

Use a third-party adapter when a framework package ships TypePython metadata separately from user
source. Third-party adapters are more sensitive because they need version and compatibility metadata
that lets projects decide whether the adapter applies locally.

A third-party adapter should include:

- adapter name and version
- framework package name and supported version range
- minimum TypePython version
- supported Python targets
- downstream checker coverage expectations
- golden tests for input `.tpy`, emitted `.py`, emitted `.pyi`, and checker outcomes; add
  runtime smoke expectations when the framework exposes runtime-visible generated behavior

Third-party adapters must still be declarative. They must not import the framework package, call
framework decorators, or run adapter Python during compilation or validation. If framework behavior
depends on runtime-computed metadata, the adapter must either reject it with a deterministic
diagnostic or mark the affected static surface as dynamic.

## Minimal manifest example

```toml
[adapter]
name = "toy-validation"
version = "0.1.0"
framework = "toy.validation"
typepython_min = "0.3.0"
python_targets = ["3.12"]
stability = "prototype"

[[transforms]]
provider = "toy.validation.BaseModel"
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
fallback = "strict_diagnostic"

[[golden_tests]]
name = "pydantic-like-package"
input = "test-fixtures/downstream-checkers/pydantic-like-package/src/app/__init__.tpy"
expected_py = "expected/app/__init__.py"
expected_pyi = "expected/app/__init__.pyi"
checkers = ["mypy", "pyright", "ty"]
```

Run validation in text or JSON mode:

```bash
typepython adapter validate typepython-framework.toml
typepython adapter validate typepython-framework.toml --format json
```

See [framework adapter examples](examples/framework-adapters.md) for validation-model, task queue,
and toy ORM manifest sketches that use the same constrained metadata model.

## Safety checklist

- The manifest is TOML data only.
- Provider kinds and capabilities are from the allowed prototype set.
- Golden-test inputs exist locally.
- Checker expectations name only supported downstream checkers.
- Dynamic aliases and other runtime-only metadata are diagnosed rather than guessed.
- Public registry distribution is deferred until multiple adapters prove the abstraction.
