# Type System

TypePython's type system builds on Python's typing ecosystem with safety-focused additions. This document covers all supported types, assignability rules, subtyping, and control-flow narrowing.

## Intrinsic Types

### `dynamic`

The escape-hatch type, equivalent to TypeScript's `any`.

- Assignable **to** and **from** every type
- Member access, calls, and indexing succeed without restriction
- Used for interop with untyped code or intentional opt-out of type safety

```python
x: dynamic = anything()
x.foo          # OK
x()            # OK
x[0]           # OK
y: int = x     # OK
```

When `typing.no_implicit_dynamic = true` (default), the compiler diagnoses implicit fallback to `dynamic`. You must write `dynamic` explicitly.

### `unknown`

The safe top-like boundary type.

- Any value is assignable **to** `unknown`
- `unknown` is only assignable **to** `unknown`, `dynamic`, or `object` (unless narrowed)
- Member access, calls, and indexing on `unknown` are **errors**
- Must be narrowed (via `isinstance`, guards, etc.) before use

```python
x: unknown = get_something()
x.foo          # ERROR: cannot access member on 'unknown'
x()            # ERROR: cannot call 'unknown'

if isinstance(x, str):
    x.upper()  # OK: narrowed to 'str'
```

Lowers to `object` in emitted `.pyi` stubs. This means external type checkers (mypy, pyright) apply the more permissive `object` rules rather than `unknown` semantics. See [Interoperability](interop.md) for the full picture.

### `Never`

The bottom type representing unreachable code.

- No value has type `Never`
- Assignable to **every** type
- Produced by: functions that never return, failed exhaustiveness, impossible branches

```python
def fail(msg: str) -> Never:
    raise RuntimeError(msg)
```

### `None`

Python's `None` value type.

Under `strict_nulls = true` (default):
- `None` is **excluded** from `T` unless you write `T | None`
- Forces explicit null handling

```python
def find(items: list[str], key: str) -> str | None:
    for item in items:
        if item == key:
            return item
    return None
```

## Supported Type Forms

### Named (nominal) class types

```python
class Foo:
    pass

x: Foo = Foo()
```

### Interface types (structural protocols)

```python
interface Printable:
    def to_string(self) -> str: ...

# Any class with a to_string() method satisfies Printable
class Name:
    def to_string(self) -> str:
        return self.value
```

Interfaces lower to `typing.Protocol`. Compatibility is **structural**: any type with matching members satisfies the interface.

### Type aliases

```python
typealias UserId = int
typealias JsonValue = dict[str, "JsonValue"] | list["JsonValue"] | str | int | bool | None
```

Recursive type aliases are supported. Emitted Python uses `TypeAlias` assignments, with helper `TypeVar` declarations for generic aliases.

### Union types

```python
x: int | str = 42
y: int | None = None       # nullable
```

### Literal types

```python
mode: Literal["read", "write"] = "read"
```

`Literal[X]` is assignable to the base type of `X` (e.g., `Literal["read"]` assignable to `str`).

### Tuple types

```python
pair: tuple[int, str] = (1, "hello")             # fixed-length
nums: tuple[int, ...] = (1, 2, 3)                # variable-length
```

### Callable types

```python
handler: Callable[[int, str], bool]
transform: Callable[..., None]
```

Callable parameters are **contravariant**, return type is **covariant**.

### Generic types

```python
def first[T](items: list[T]) -> T:
    return items[0]

class Box[T]:
    value: T
    def get(self) -> T:
        return self.value
```

**Upper bounds:**

```python
def clamp[T: Comparable](value: T, lo: T, hi: T) -> T: ...
```

**Constraint lists:**

```python
def convert[T: (int, float, str)](value: T) -> str: ...
```

**Defaults:**

```python
def collect[T = list](items: Iterable[T]) -> T: ...
```

Once a default appears, all subsequent type parameters must also have defaults.

### `ParamSpec`

```python
def decorator[**P, R](fn: Callable[P, R]) -> Callable[P, R]: ...
```

### `TypeVarTuple`

```python
typealias Pack[*Ts] = tuple[*Ts]

def collect[*Ts](*args: *Ts) -> tuple[*Ts]:
    return args
```

TypePython supports source-authored `TypeVarTuple` (`*Ts`) syntax for inline variadic generics, tuple aliases, and variadic positional calls.

Current limits:

- Inference requires a fixed positional shape at the call site.
- Calls that only expose an open-ended starred iterable such as `collect(*items)` with `items: list[int]` remain unresolved and report `TPY4014`.
- Higher-order combinations beyond this minimum path, especially arbitrary `ParamSpec` + `TypeVarTuple` interactions, are still incomplete.

### `Self` type

```python
class Builder:
    def set_name(self, name: str) -> Self:
        self._name = name
        return self
```

### `NewType`

```python
UserId = NewType("UserId", int)

def get_user(uid: UserId) -> User: ...

uid = UserId(42)       # OK
get_user(42)           # ERROR: int is not UserId
get_user(uid)          # OK
```

Creates a distinct nominal subtype for static type checking. At runtime, `UserId(42)` is just `42`.

### `type[T]` (class objects)

```python
def create[T](cls: type[T]) -> T:
    return cls()
```

### Enum types

```python
class Color(Enum):
    RED = auto()
    GREEN = auto()
    BLUE = auto()

def name(c: Color) -> str:
    match c:
        case Color.RED:   return "red"
        case Color.GREEN: return "green"
        case Color.BLUE:  return "blue"
        # Exhaustive -- compiler proves all members covered
```

### TypedDict

```python
class Config(TypedDict):
    debug: bool
    timeout: int

class PartialConfig(TypedDict, total=False):
    debug: bool
    timeout: int
```

**Advanced TypedDict features:**

```python
class StrictConfig(TypedDict, closed=True):
    debug: bool
    # No extra keys allowed

class FlexConfig(TypedDict):
    debug: bool
    extra_items: str           # Extra keys must have str values

class ReadonlyConfig(TypedDict):
    name: ReadOnly[str]        # Cannot be mutated after creation
    mutable_field: int         # Can be mutated
```

### TypedDict utility transforms

| Transform | Effect |
|---|---|
| `Partial[T]` | All keys become optional |
| `Required_[T]` | All keys become required |
| `Readonly[T]` | All values become read-only |
| `Mutable[T]` | All values become writable |
| `Pick[T, "key1", "key2"]` | Subset of keys |
| `Omit[T, "key1"]` | Exclude specific keys |

```python
typealias OptionalConfig = Partial[Config]
typealias CoreConfig = Pick[Config, "debug"]
```

`Required_[T]` uses a trailing underscore so the transform name does not collide with the field-level `Required[...]` annotation wrapper from `typing`.

`Readonly[T]` is the transform that rewrites every field in a `TypedDict` to be read-only. `ReadOnly[...]` is the field-level wrapper you apply to an individual item, such as `name: ReadOnly[str]`.

### Annotated, ClassVar, Final

```python
class Foo:
    x: ClassVar[int] = 0               # Class-level, not instance
    y: Final[str] = "immutable"         # Cannot be reassigned

    z: Annotated[int, "metadata"] = 5   # Type with metadata
```

### Required / NotRequired

```python
class Options(TypedDict, total=False):
    name: Required[str]        # Must be present even though total=False
    debug: bool                # Optional
    timeout: NotRequired[int]  # Explicitly optional
```

### Abstract classes

```python
from abc import ABC, abstractmethod

class Shape(ABC):
    @abstractmethod
    def area(self) -> float: ...

Shape()          # ERROR: cannot instantiate abstract class
```

Concrete subclasses must implement all abstract methods.

## Assignability Rules

`S` is assignable to `T` when any of these hold:

| Rule | Description |
|---|---|
| Identity | `S == T` |
| Dynamic target | `T` is `dynamic` |
| Dynamic source | `S` is `dynamic` |
| Bottom | `S` is `Never` |
| None | `S` is `None` and `T` includes `None` |
| Subtype | `S` is a subtype of `T` (nominal or structural) |
| Union target | `S` assignable to at least one member of `T` |
| Union source | Every member of `S` assignable to `T` |
| Literal | `Literal[X]` assignable to the base type of `X` |

## Subtyping Rules

### Nominal subtyping

```python
class Animal: ...
class Dog(Animal): ...

x: Animal = Dog()    # OK: Dog <: Animal
```

### Structural subtyping (interfaces)

```python
interface HasName:
    name: str

class User:
    name: str
    age: int

u: HasName = User()  # OK: User has 'name: str'
```

### Callable subtyping

- **Parameters:** contravariant (broader parameter types in subtypes)
- **Return type:** covariant (narrower return types in subtypes)
- Position and keyword argument matching required

### Generic subtyping

- Type parameters are **invariant** by default
- Variance annotations from `.py`/`.pyi` stubs are respected

### TypedDict subtyping

Conservative mutation-aware rules:
- Every target key must exist in source
- Requiredness must match
- **Writable** target keys: source value types must be mutually assignable (invariant)
- **Read-only** target keys: source value type must be assignable to target (covariant)

## Type Equality

Two types are equal when:
- Same nominal type (same class/alias identity)
- Same literal value and base type
- Unions with identical members (order-independent)
- Generic instances with equal type arguments

## Control Flow Narrowing

TypePython narrows types based on control flow analysis.

### `is None` / `is not None`

```python
x: str | None = get()

if x is not None:
    x.upper()     # x narrowed to str
else:
    print("none") # x narrowed to None
```

### `isinstance`

```python
x: int | str | list[int] = get()

if isinstance(x, str):
    x.upper()     # x narrowed to str
elif isinstance(x, (int, list)):
    ...           # x narrowed to int | list[int]
```

### `TypeGuard` and `TypeIs`

```python
def is_str_list(val: list[object]) -> TypeGuard[list[str]]:
    return all(isinstance(v, str) for v in val)

def is_int(val: object) -> TypeIs[int]:
    return isinstance(val, int)

if is_str_list(items):
    # items narrowed to list[str] in true branch

if is_int(x):
    # x narrowed to int in true branch
    # x narrowed to exclude int in false branch (TypeIs only)
```

### `assert`

```python
x: str | None = get()
assert x is not None
x.upper()     # x narrowed to str
```

### `match` patterns

```python
match value:
    case int():
        # value narrowed to int
    case str():
        # value narrowed to str
```

For sealed classes and enums, the compiler performs exhaustiveness checking.

### Boolean composition

Guard conditions compose with boolean operators:

| Expression | True branch | False branch |
|---|---|---|
| `not G` | `EnvFalse(G)` | `EnvTrue(G)` |
| `G1 and G2` | Both narrowings applied | `G1` false or `G2` false |
| `G1 or G2` | `G1` true or `G2` true under `G1` false | Both false |

### Narrowing limitations

- Persistent narrowing is guaranteed only for **simple local names**
- Attribute and index narrowing is limited to within guard expressions
- Narrowing does not persist across function calls that could mutate state

## Sealed Class Exhaustiveness

```python
sealed class Expr:
    pass

class Num(Expr):
    value: int

class Add(Expr):
    left: Expr
    right: Expr

def eval(e: Expr) -> int:
    match e:
        case Num(value=v):
            return v
        case Add(left=l, right=r):
            return eval(l) + eval(r)
    # No default needed: all direct subclasses covered
```

Sealed classes restrict subclassing to the same module. The compiler statically verifies exhaustiveness when `typing.enable_sealed_exhaustiveness = true`.

## Decorators

| Decorator | Effect |
|---|---|
| `@property` | Property accessor |
| `@classmethod` | Class method binding |
| `@staticmethod` | Static method (no `self`/`cls`) |
| `@final` | Prevents overriding in subclasses |
| `@override` | Asserts method overrides a parent method |
| `@deprecated("msg")` | Marks as deprecated; usage generates warnings |
| `@abstractmethod` | Abstract method (must be implemented by subclasses) |
| `@dataclass_transform` | Framework-level dataclass behavior |
