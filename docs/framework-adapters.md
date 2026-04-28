# Framework Adapter Authoring

Framework adapters describe static framework behavior as reviewable data. They are for cases where
runtime Python frameworks synthesize constructor parameters, generated members, replacement objects,
or field metadata that ordinary downstream checkers cannot infer without plugins.

The prototype adapter format is `typepython-framework.toml`. It is intentionally local and
experimental: adapters are validated before use, but TypePython does not yet load a public registry
or execute adapter Python code.

Adapters are a checker-neutral shipping surface, not a replacement for framework runtime code. A
framework can distribute `typepython-framework.toml` plus golden fixtures alongside its package or in
a companion metadata package, and TypePython validates that data into ordinary `.py`/`.pyi` outputs
for mypy, pyright, ty, and IDEs. No mypy plugin, pyright extension, or TypePython-specific runtime is
part of the contract.

The SDK remains prototype-only. The only accepted stability marker today is `prototype`, and adapter
authors should treat every manifest as a local contract until multiple independently maintained
adapters have validated the same abstraction across checker and runtime-smoke fixtures.

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

### Shipping without checker plugins

A third-party framework should ship these files together:

```text
framework-package/
  py.typed
  typepython-framework.toml
  adapter-golden/
    input.tpy
    expected.py
    expected.pyi
```

The framework keeps its runtime decorators, base classes, and descriptors. The adapter describes only
the static shape TypePython can emit into `.pyi` artifacts. Downstream projects then run standard
checkers against those artifacts; they do not install or configure a checker-specific plugin.

Do not mark an adapter stable because one local fixture passes. Until the Pydantic-like validation
fixture, toy task queue, toy ORM, and at least one FastAPI-like route/dependency fixture all exercise
the same manifest path, adapter authors should document compatibility as prototype-only.

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

## Boundary validators

Frameworks that own runtime validation should expose that through declarative boundary metadata rather
than TypePython-generated whole-program checks. A boundary adapter names the trusted edge
(`http_request`, `http_response`, `cli_param`, `config_file`, `message_payload`, `plugin_entrypoint`,
or `serialized_payload`), the schema type, and whether validation is delegated to the framework or
generated for project-owned code. See [boundary validator generation](rfcs/boundary-validator-generation.md)
for the prototype contract.
