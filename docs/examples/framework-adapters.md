# Framework Adapter Examples

These examples show the three adapter shapes the prototype is meant to cover first: a validation
model, a task queue decorator, and an ORM-style model. They are illustrative manifests for the
declarative adapter model; adapters still validate through `typepython adapter validate` before a
project build consumes them.

## Validation model

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
expected_py = "expected/pydantic-like/app/__init__.py"
expected_pyi = "expected/pydantic-like/app/__init__.pyi"
checkers = ["mypy", "pyright", "ty"]
```

This mirrors the Pydantic-like fixture: annotated fields become constructor parameters, string
literal aliases become public constructor keywords, defaults/default factories make parameters
optional, and statically known frozen metadata feeds mutation diagnostics.

## Task queue

```toml
[adapter]
name = "toy-task"
version = "0.1.0"
framework = "toy.tasks"
typepython_min = "0.3.0"
python_targets = ["3.12"]
stability = "prototype"

[[transforms]]
provider = "toy.tasks.task"
kind = "function_to_object_decorator"
target = "function"
capabilities = ["function_to_object_replacement", "generic_preservation"]
replacement_type = "toy.tasks.Task[P, R]"
preserve_paramspec = true
preserve_return_type = true
fallback = "strict_diagnostic"

[[golden_tests]]
name = "toy-task-package"
input = "test-fixtures/downstream-checkers/toy-task-package/src/app/__init__.tpy"
expected_py = "expected/toy-task/app/__init__.py"
expected_pyi = "expected/toy-task/app/__init__.pyi"
checkers = ["mypy", "pyright", "ty"]
```

This mirrors the toy task fixture: the runtime decorator remains in emitted `.py`, while emitted
`.pyi` exposes the decorated function name as a task object that preserves `ParamSpec` arguments and
return type in methods such as `delay(...)`.

## Toy ORM

```toml
[adapter]
name = "toy-orm"
version = "0.1.0"
framework = "toy.orm"
typepython_min = "0.3.0"
python_targets = ["3.12"]
stability = "prototype"

[[transforms]]
provider = "toy.orm.Model"
kind = "base_class"
target = "class"
capabilities = [
  "field_collection",
  "constructor_generation",
  "required_optional_fields",
  "descriptor_backed_attributes",
  "method_synthesis",
]
field_collector = "annotated_class_fields"
constructor = "fields"
fallback = "strict_diagnostic"

[[golden_tests]]
name = "toy-orm-package"
input = "test-fixtures/downstream-checkers/toy-orm-package/src/app/__init__.tpy"
expected_py = "expected/toy-orm/app/__init__.py"
expected_pyi = "expected/toy-orm/app/__init__.pyi"
checkers = ["mypy", "pyright", "ty"]
```

The ORM shape is deliberately constrained: it may collect annotated fields, exclude descriptor-backed
attributes from constructor synthesis, and expose generated members through the framework transform
metadata model. It must not execute database metadata, inspect live models, or infer runtime-only
relationships during compilation.
