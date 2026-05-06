# RFC: Boundary Validator Generation

**Status:** experimental opt-in design  
**Strategic source:** `docs/strategic-todo.md` P2 Boundary Validator Generation  
**Scope:** runtime validation only at explicit trust boundaries, with no mandatory TypePython runtime

## Summary

TypePython should not become a whole-program runtime type checker. Runtime validation is useful only
where untrusted data crosses into typed code: HTTP payloads, CLI arguments, configuration files,
message queues, plugin entrypoints, and serialized JSON/YAML/TOML blobs.

Boundary validation is opt-in. By default, emitted Python has no mandatory runtime dependency and no
generated validators are part of public `.pyi` surfaces.

## Boundary annotations

The prototype boundary model uses declarative metadata rather than executing validators during
compilation. Emitted classes may mark a selected boundary with `__tpy_validation_boundary__` or a
`# tpy:validate-boundary:<kind>` comment, and adapter manifests may identify a boundary with these
dimensions:

| Boundary kind | Example input | Preferred runtime owner |
| --- | --- | --- |
| `http_request` | request body or query object | FastAPI/Pydantic/msgspec adapter |
| `http_response` | response payload | FastAPI/Pydantic/msgspec adapter |
| `cli_param` | command-line argument | Click/Typer/argparse adapter |
| `config_file` | JSON/YAML/TOML config | cattrs/msgspec/Pydantic adapter |
| `message_payload` | queue/event payload | framework adapter |
| `plugin_entrypoint` | dynamically loaded object | project-local adapter |
| `serialized_payload` | JSON/YAML/TOML blob | explicit serializer adapter |

The built-in `emit.runtime_validators = true` option remains opt-in and intentionally narrow by
default: the project must also list `"runtime_validators"` in
`[experimental].accepted_features`. When both gates are present, it emits data-class runtime
validators for project-owned generated code. Selected
boundaries can now opt into adapter delegation by setting `__tpy_validate_boundary__ = True` and
`__tpy_validation_adapter__ = "builtin" | "pydantic" | "msgspec" | "cattrs"` on the generated
runtime class. Non-built-in adapters delegate to the framework/library-owned validation entrypoint
instead of adding a TypePython-specific runtime dependency.

## Adapter interface

Boundary-capable adapters declare that a framework or library owns validation for a specific boundary:

```toml
[[boundaries]]
name = "create_user_request"
kind = "http_request"
provider = "toy.fastapi.Body"
schema = "app.UserCreate"
validator = "delegate"
failure = "diagnostic"
```

Rules:

- `validator = "delegate"` means TypePython emits static surfaces and leaves runtime checks to the
  framework/library.
- `validator = "generate"` is allowed only for explicitly opted-in project-owned types and must be
  reviewable in emitted `.py`.
- Generated validators must stay out of public `.pyi` unless an adapter explicitly exports a public
  validation API.
- Unsupported type forms produce deterministic diagnostics instead of guessed runtime checks.

## Examples

HTTP payload boundary delegated to a framework:

```python
class UserCreate(BaseModel):
    name: str

@app.post("/users", response_model=UserOut)
def create_user(payload: UserCreate) -> UserOut:
    return service.create(payload)
```

Config boundary with explicit project-owned validation:

```python
data class Config:
    host: str
    port: int

def load_config(raw: dict[str, object]) -> Config:
    return Config.__tpy_validate__(raw)
```

## Diagnostics

Boundary validation must diagnose when a runtime validator cannot faithfully represent the static
type. Examples include recursive aliases without a supported runtime strategy, callable values,
protocols with behavioral requirements, dynamically computed field aliases, and framework metadata
that is not statically evaluable.

The diagnostic should identify the boundary name, unsupported type form, and whether the user can
switch to delegation (`validator = "delegate"`) or make the type static enough to generate.

## Acceptance boundary

- Projects opt in per boundary or through `emit.runtime_validators`; there is no global mandatory
  runtime validation mode.
- Generated behavior is visible in emitted `.py` and reviewable in adapter golden tests.
- Public `.pyi` files remain checker-neutral and do not expose generated validators unless an adapter
  intentionally exports them.
- The default TypePython workflow remains static-only and has no mandatory runtime dependency.
