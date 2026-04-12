# Diagnostics Reference

TypePython emits structured diagnostics with unique error codes organized by category. Each diagnostic includes a severity level, source location, and optionally machine-readable suggestions for fixes.

## Severity Levels

| Severity  | Meaning              | Effect                                                                                                      |
| --------- | -------------------- | ----------------------------------------------------------------------------------------------------------- |
| `error`   | Build-blocking issue | Sets `has_errors()`; `no_emit_on_error = false` still allows best-effort emit for non-fatal pipeline stages |
| `warning` | Non-fatal issue      | Does not block build                                                                                        |
| `note`    | Informational        | Additional context                                                                                          |

## Diagnostic Structure

Each diagnostic contains:

| Field         | Description                                                               |
| ------------- | ------------------------------------------------------------------------- |
| `code`        | Unique identifier (e.g., `TPY4001`)                                       |
| `severity`    | `error`, `warning`, or `note`                                             |
| `message`     | Human-readable description                                                |
| `notes`       | Additional context lines                                                  |
| `suggestions` | Machine-readable fix proposals with replacement spans                     |
| `span`        | Source location (path, line, column, end_line, end_column) -- all 1-based |

### Suggestion structure

```json
{
  "message": "Add '| None' to the return type",
  "span": { "line": 5, "column": 20, "end_line": 5, "end_column": 23 },
  "replacement": "str | None"
}
```

Suggestions are designed for editor quick-fix actions via the LSP `textDocument/codeAction` method.

## Error Code Reference

### TPY1xxx -- Configuration and Project

| Code      | Severity | Description                                                                    |
| --------- | -------- | ------------------------------------------------------------------------------ |
| `TPY1001` | error    | Configuration I/O error (file not found, permission denied, parse error)       |
| `TPY1002` | error    | Invalid configuration value (unsupported target_python, invalid profile, etc.) |

**TPY1001 example:**

```
typepython.toml:1:1  TPY1001  error  Failed to read configuration: typepython.toml not found
```

**TPY1002 example:**

```
typepython.toml:8:18  TPY1002  error  Invalid target_python "3.8": supported values are "3.10", "3.11", "3.12", "3.13", "3.14"
```

---

### TPY2xxx -- Parsing and Lowering

| Code      | Severity | Description                                                           |
| --------- | -------- | --------------------------------------------------------------------- |
| `TPY2001` | error    | Parse error in `.tpy` file (syntax error in source)                   |
| `TPY2002` | error    | TypePython-only syntax recognized by the parser but not lowerable yet |

**TPY2001 example:**

```
src/app/main.tpy:10:5  TPY2001  error  Expected ':', found '='
```

---

### TPY3xxx -- Import and Module Resolution

| Code      | Severity | Description                                                                |
| --------- | -------- | -------------------------------------------------------------------------- |
| `TPY3001` | error    | Module not found (unresolved import)                                       |
| `TPY3002` | error    | Conflicting module path (e.g., `foo.tpy` and `foo.py` in same source root) |

**TPY3001 example:**

```
src/app/main.tpy:1:1  TPY3001  error  Module 'app.utils' not found
```

**TPY3002 example:**

```
src/app/foo.tpy:1:1  TPY3002  error  Conflicting module path: 'app.foo' defined by both foo.tpy and foo.py
```

---

### TPY4xxx -- Type Checking and Flow Analysis

This is the largest category, covering all type checking rules.

| Code      | Severity      | Description                                                                                                        |
| --------- | ------------- | ------------------------------------------------------------------------------------------------------------------ |
| `TPY4001` | error         | Type mismatch (value not assignable to target type)                                                                |
| `TPY4002` | error         | Invalid member access (attribute not found on type)                                                                |
| `TPY4003` | error         | Unsupported operation on `unknown` (must narrow first)                                                             |
| `TPY4004` | error         | Duplicate declaration (same name declared twice in scope)                                                          |
| `TPY4005` | error/warning | Missing or invalid `@override` annotation                                                                          |
| `TPY4006` | error         | Reassignment of `Final` variable                                                                                   |
| `TPY4007` | error         | Direct instantiation of abstract class                                                                             |
| `TPY4008` | error         | Unimplemented abstract methods in concrete subclass                                                                |
| `TPY4009` | error         | Non-exhaustive `match` on sealed class or enum                                                                     |
| `TPY4010` | error         | Deferred-beyond-v1 syntax in source                                                                                |
| `TPY4011` | error         | Invalid assignment or deletion target                                                                              |
| `TPY4012` | error         | Ambiguous overload resolution (multiple overloads match)                                                           |
| `TPY4013` | error         | Invalid TypedDict literal or keyword expansion                                                                     |
| `TPY4014` | error         | Generic parameter list could not be resolved from a call, including unresolved `ParamSpec` or `TypeVarTuple` packs |
| `TPY4015` | warning       | Incomplete exported type surface (`dynamic`/`unknown` in public API)                                               |
| `TPY4016` | error         | Mutation of read-only TypedDict field                                                                              |
| `TPY4017` | error         | Invalid TypedDict transform target or key selection                                                                |
| `TPY4018` | error         | Conditional return type does not cover all cases                                                                   |
| `TPY4019` | warning       | Unsafe boundary operation used outside `unsafe:`                                                                   |
| `TPY4101` | warning/error | Use of deprecated declaration                                                                                      |

#### TPY4001 -- Type mismatch

Raised when a value's type is not assignable to the target type.

```python
x: int = "hello"   # TPY4001: Type 'str' is not assignable to 'int'

def greet() -> str:
    return 42       # TPY4001: Type 'int' is not assignable to return type 'str'
```

**Suggestions:** The compiler may suggest adding `| None`, using `isinstance`, or correcting a type annotation.

#### TPY4002 -- Invalid member access

```python
x: int = 42
x.foo              # TPY4002: 'int' has no attribute 'foo'
```

#### TPY4003 -- Operation on unknown

```python
x: unknown = get()
x.method()         # TPY4003: Cannot access member 'method' on type 'unknown'
x()                # TPY4003: Cannot call type 'unknown'
x[0]               # TPY4003: Cannot index type 'unknown'
```

**Fix:** Narrow with `isinstance` or a type guard before use.

#### TPY4004 -- Duplicate declaration

```python
class Foo:
    x: int
    x: str          # TPY4004: Duplicate declaration 'x'
```

#### TPY4005 -- Override issues

When `typing.require_explicit_overrides = true`:

```python
class Base:
    def run(self) -> None: ...

class Child(Base):
    def run(self) -> None: ...   # TPY4005: Method 'run' overrides 'Base.run' without @override
```

**Fix:** Add `@override` decorator.

Also raised when `@override` is used but no parent method exists:

```python
class Child(Base):
    @override
    def foo(self) -> None: ...   # TPY4005: Method 'foo' has @override but does not override any base method
```

#### TPY4006 -- Final reassignment

```python
x: Final[int] = 42
x = 100            # TPY4006: Cannot reassign Final variable 'x'
```

#### TPY4007 -- Abstract instantiation

```python
class Shape(ABC):
    @abstractmethod
    def area(self) -> float: ...

Shape()            # TPY4007: Cannot instantiate abstract class 'Shape'
```

#### TPY4008 -- Unimplemented abstract methods

```python
class Circle(Shape):
    radius: float
    # Missing area() implementation
    # TPY4008: Class 'Circle' does not implement abstract method 'area' from 'Shape'
```

#### TPY4009 -- Non-exhaustive match

```python
sealed class Expr: ...
class Num(Expr): ...
class Add(Expr): ...

def eval(e: Expr) -> int:
    match e:
        case Num():
            return 0
    # TPY4009: Non-exhaustive match: missing case 'Add'
```

**Suggestions:** Lists the missing cases.

#### TPY4011 -- Invalid target

```python
1 = x              # TPY4011: Invalid assignment target
del 42             # TPY4011: Invalid deletion target
```

#### TPY4012 -- Ambiguous overload

```python
overload def f(x: int) -> int: ...
overload def f(x: int, y: int = 0) -> int: ...

f(1)               # TPY4012: Ambiguous overload resolution for 'f': multiple overloads match
```

#### TPY4013 -- TypedDict literal issues

```python
class Config(TypedDict):
    debug: bool

c: Config = {"debug": True, "extra": 1}   # TPY4013: Extra key 'extra' in TypedDict literal
c: Config = {"other": True}               # TPY4013: Missing required key 'debug'
c: Config = {"debug": "yes"}              # TPY4013: Type 'str' not assignable to 'bool' for key 'debug'
```

#### TPY4015 -- Incomplete public API types

When `typing.require_known_public_types = true`:

```python
def get_data():              # TPY4015: Public function 'get_data' has no return type annotation
    return {}

x: dynamic = 42             # TPY4015: Public symbol 'x' has type 'dynamic'
```

#### TPY4016 -- ReadOnly mutation

```python
class Config(TypedDict):
    name: ReadOnly[str]

c: Config = {"name": "app"}
c["name"] = "new"          # TPY4016: Cannot assign to read-only TypedDict key 'name'
```

#### TPY4101 -- Deprecated usage

```python
@deprecated("Use new_api() instead")
def old_api() -> None: ...

old_api()                   # TPY4101: 'old_api' is deprecated: Use new_api() instead
```

Severity controlled by `typing.report_deprecated`: `"error"`, `"warning"`, or `"ignore"`.

**Additional TPY4xxx diagnostics** (checked but may not have explicit user-facing codes):

- Call arity errors (too many / too few arguments)
- Call type errors (argument type mismatch)
- Keyword argument errors (unknown keyword, duplicate keyword)
- Return type mismatch
- Yield type mismatch
- For-loop target not iterable
- Destructuring assignment mismatch
- With-statement target not a context manager
- Annotated assignment type mismatch
- Attribute assignment type mismatch
- Frozen dataclass field mutation
- `@final` class subclassed or `@final` method overridden

---

### TPY5xxx -- Emit and Stub Generation

| Code      | Severity | Description                                                                                    |
| --------- | -------- | ---------------------------------------------------------------------------------------------- |
| `TPY5001` | error    | Stub (`.pyi`) generation failure                                                               |
| `TPY5002` | error    | Best-effort emit was disabled by `no_emit_on_error = true` after semantic errors were reported |
| `TPY5003` | error    | Verify failure for missing or mismatched emitted/published artifacts                           |

**TPY5002 example:**

```
TPY5002  error  emit blocked by `emit.no_emit_on_error` for /path/to/project
```

---

### TPY6xxx -- LSP and Infrastructure

| Code      | Severity | Description                                                       |
| --------- | -------- | ----------------------------------------------------------------- |
| `TPY6001` | error    | Incremental snapshot is incompatible or corrupt                   |
| `TPY6002` | error    | LSP document protocol error (invalid request, malformed JSON-RPC) |
| `TPY6003` | error    | LSP formatter backend startup or execution failure                |

---

## DX Enhancements

TypePython diagnostics include enhanced developer experience features:

### Mismatch path drill-down

For complex type mismatches, the diagnostic traces into nested types to show exactly where assignability fails:

```
TPY4001  error  Type 'dict[str, list[int]]' is not assignable to 'dict[str, list[str]]'
  note: Mismatch at value type: 'list[int]' is not assignable to 'list[str]'
  note: Mismatch at element type: 'int' is not assignable to 'str'
```

### Inference chain trace

Shows how a type was inferred through branches:

```
TPY4001  error  Type 'str | None' is not assignable to 'str'
  note: 'None' branch introduced at line 5 (if x is None: return None)
```

### Suggested fixes

Machine-readable fix suggestions:

```
TPY4001  error  Type 'str | None' is not assignable to 'str'
  suggestion: Add '| None' to the return type annotation
  suggestion: Add 'assert result is not None' before return
```

```
TPY4009  error  Non-exhaustive match on 'Shape'
  suggestion: Add missing cases: 'Circle', 'Triangle'
```

```
TPY4005  warning  Method 'run' overrides 'Base.run' without @override
  suggestion: Add '@override' decorator
```

## Type-Ignore Comments

Suppress diagnostics with `# type: ignore`:

```python
x: int = "hello"  # type: ignore          # Suppresses TPY4001
x: int = "hello"  # type: ignore[TPY4001]  # Suppresses only TPY4001
```

## Output Formats

### Text (human-readable)

```
src/app/main.tpy:15:10  TPY4001  error  Type 'str' is not assignable to 'int'
  note: Expected 'int' based on annotation at line 14
```

### JSON (machine-readable)

```json
{
  "code": "TPY4001",
  "severity": "error",
  "message": "Type 'str' is not assignable to 'int'",
  "notes": ["Expected 'int' based on annotation at line 14"],
  "suggestions": [],
  "span": {
    "path": "src/app/main.tpy",
    "line": 15,
    "column": 10,
    "end_line": 15,
    "end_column": 17
  }
}
```
