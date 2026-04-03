# Syntax Guide

TypePython extends Python syntax with soft keywords and ergonomic type-level constructs. All extensions are designed to lower cleanly to standard Python.

## File Extension

TypePython source files use the `.tpy` extension. They support all standard Python 3.10+ syntax plus the extensions described below.

## Soft Keywords

TypePython introduces context-sensitive keywords that are only special in specific syntactic positions. They remain valid identifiers elsewhere.

| Keyword     | Position          | Purpose                         |
| ----------- | ----------------- | ------------------------------- |
| `typealias` | Statement start   | Type alias declaration          |
| `interface` | Statement start   | Structural protocol declaration |
| `data`      | Prefix to `class` | Dataclass declaration           |
| `sealed`    | Prefix to `class` | Sealed class declaration        |
| `overload`  | Prefix to `def`   | Overloaded function             |
| `unsafe`    | Block start       | Unsafe operation block          |

## Type Aliases

Declare type aliases with the `typealias` keyword:

```python
typealias UserId = int
typealias Pair[T] = tuple[T, T]
typealias JsonValue = dict[str, "JsonValue"] | list["JsonValue"] | str | int | bool | None
```

**Generics in aliases:**

```python
typealias Mapper[A, B] = Callable[[A], B]
typealias Result[T, E = Exception] = T | E
```

**Lowering:**

| TypePython                 | Emitted Python                               |
| -------------------------- | -------------------------------------------- |
| `typealias X = int`        | `X: TypeAlias = int`                         |
| `typealias P[T] = list[T]` | `T = TypeVar("T")`; `P: TypeAlias = list[T]` |

Recursive type aliases are supported:

```python
typealias Tree[T] = T | list["Tree[T]"]
```

## Interfaces

Declare structural protocols (duck typing contracts) with the `interface` keyword:

```python
interface Drawable:
    def draw(self, canvas: Canvas) -> None: ...

interface Serializable:
    def to_json(self) -> str: ...
    def from_json(cls, data: str) -> Self: ...

interface Comparable[T]:
    def compare(self, other: T) -> int: ...
```

**Key properties:**

- Interfaces are **structural**: any type with matching members satisfies the interface
- No explicit inheritance required
- Can have generic type parameters
- Can declare methods, properties, and class-level attributes

**Lowering:**

```python
# TypePython
interface Drawable:
    def draw(self, canvas: Canvas) -> None: ...

# Python output
from typing import Protocol

class Drawable(Protocol):
    def draw(self, canvas: Canvas) -> None: ...
```

**Generic interface lowering:**

```python
# TypePython
interface Container[T]:
    def get(self) -> T: ...
    def set(self, value: T) -> None: ...

# Python output
from typing import Protocol, TypeVar, Generic

T = TypeVar("T")

class Container(Protocol, Generic[T]):
    def get(self) -> T: ...
    def set(self, value: T) -> None: ...
```

## Data Classes

Declare data classes with the `data class` keyword:

```python
data class Point:
    x: float
    y: float

data class User:
    name: str
    email: str
    age: int = 0
    active: bool = True
```

**Lowering:**

```python
from dataclasses import dataclass

@dataclass
class Point:
    x: float
    y: float

@dataclass
class User:
    name: str
    email: str
    age: int = 0
    active: bool = True
```

**Frozen data classes:**

```python
data class Config:
    host: str
    port: int
```

The `frozen` attribute is controlled by the standard `@dataclass(frozen=True)` pattern. TypePython validates that frozen fields are not mutated.

**Runtime validators (experimental):**

When `emit.runtime_validators = true`, data classes gain a `__tpy_validate__()` method:

```python
# Generated (experimental)
@dataclass
class User:
    name: str
    age: int

    def __tpy_validate__(self) -> None:
        if not isinstance(self.name, str):
            raise TypeError(...)
        if not isinstance(self.age, int):
            raise TypeError(...)
```

## Sealed Classes

Declare sealed class hierarchies for exhaustive pattern matching:

```python
sealed class Expr:
    pass

class Num(Expr):
    value: int

class Add(Expr):
    left: Expr
    right: Expr

class Neg(Expr):
    operand: Expr
```

**Key properties:**

- All direct subclasses must be in the **same module**
- `match` statements on sealed types are checked for exhaustiveness
- Compiler proves all cases are covered -- no `default` needed

```python
def evaluate(expr: Expr) -> int:
    match expr:
        case Num(value=v):
            return v
        case Add(left=l, right=r):
            return evaluate(l) + evaluate(r)
        case Neg(operand=o):
            return -evaluate(o)
```

If you add a new subclass and forget to handle it, the compiler reports a diagnostic.

Note: sealed exhaustiveness checking is enforced by the TypePython checker only. External type checkers (mypy, pyright) see a plain class and will not enforce exhaustiveness or subclassing restrictions. See [Interoperability](interop.md) for details.

**Lowering:**

```python
# TypePython
sealed class Expr:
    pass

# Python output
class Expr:
    # tpy:sealed
    pass
```

## Overloaded Functions

Declare type-safe function overloads with the `overload` keyword:

```python
overload def parse(value: str) -> int: ...
overload def parse(value: bytes) -> int: ...
overload def parse(value: float) -> int: ...

def parse(value: str | bytes | float) -> int:
    if isinstance(value, str):
        return int(value)
    if isinstance(value, bytes):
        return int(value.decode())
    return int(value)
```

**Rules:**

- Each `overload def` signature specifies one accepted combination
- The implementation signature must be compatible with all overloads
- The compiler checks that calls match exactly one overload (reports ambiguity as `TPY4012`)

**Lowering:**

```python
from typing import overload

@overload
def parse(value: str) -> int: ...
@overload
def parse(value: bytes) -> int: ...
@overload
def parse(value: float) -> int: ...

def parse(value: str | bytes | float) -> int:
    ...
```

## Unsafe Blocks

Mark regions of dynamic/unsafe code explicitly:

```python
# These operations require an unsafe block when typing.warn_unsafe = true
unsafe:
    result = eval(user_expression)
    exec(code_string)
    globals()["key"] = value
    locals()["key"] = value
    setattr(obj, dynamic_name, value)
    delattr(obj, dynamic_name)
```

**Tracked unsafe operations:**

- `eval()` calls
- `exec()` calls
- `globals()` writes
- `locals()` writes
- `dict.__setitem__` with dynamic keys
- `setattr()` with non-literal attribute names
- `delattr()` with non-literal attribute names

Outside an `unsafe` block, these operations produce diagnostics. Inside, they are allowed with a clear visual marker of intent.

The diagnostic for an unsafe operation outside an `unsafe:` block is `TPY4019`.

**Lowering:** The `unsafe:` block is rewritten to an `if True:` wrapper so the lowered Python remains structurally valid while preserving the original indentation and body.

## Inline Type Parameters

TypePython supports PEP 695-style inline type parameter syntax on functions, classes, and type aliases:

### Functions

```python
def first[T](items: list[T]) -> T:
    return items[0]

def merge[K, V](a: dict[K, V], b: dict[K, V]) -> dict[K, V]:
    return {**a, **b}
```

### Classes

```python
class Stack[T]:
    _items: list[T]

    def push(self, item: T) -> None:
        self._items.append(item)

    def pop(self) -> T:
        return self._items.pop()
```

### Upper bounds

```python
def clamp[T: Comparable](value: T, lo: T, hi: T) -> T: ...
```

### Constraint lists

```python
def to_str[T: (int, float, str)](value: T) -> str: ...
```

### Type parameter defaults

```python
def collect[T = list](items: Iterable[T]) -> T: ...
class Container[T = object]: ...
```

Once a default appears, all subsequent type parameters must also have defaults.

### ParamSpec

```python
def decorator[**P, R](fn: Callable[P, R]) -> Callable[P, R]:
    def wrapper(*args: P.args, **kwargs: P.kwargs) -> R:
        return fn(*args, **kwargs)
    return wrapper
```

### TypeVarTuple

TypePython supports source-authored `TypeVarTuple` syntax:

```python
typealias Pack[*Ts] = tuple[*Ts]

def collect[*Ts](*args: *Ts) -> tuple[*Ts]:
    return args
```

Current limits:

- Variadic pack inference works when the call site exposes a fixed positional shape.
- Open-ended starred iterables such as `collect(*items)` with `items: list[int]` still report `TPY4014`.
- More advanced higher-order pack algebra is not complete yet.

## Async Support

TypePython fully supports async/await syntax:

```python
async def fetch(url: str) -> str:
    response = await http_get(url)
    return response.body

async def process_items(items: AsyncIterable[Item]) -> list[Result]:
    results: list[Result] = []
    async for item in items:
        result = await handle(item)
        results.append(result)
    return results

async def managed() -> None:
    async with open_connection() as conn:
        await conn.send(data)
```

## Lambda Annotations

TypePython allows type annotations on lambda parameters:

```python
transform = lambda (x: int, y: int): x + y
```

The parenthesized parameter list is required when source-authored lambda parameter annotations are present. Lowering normalizes this back to standard Python lambda syntax for runtime output.

## Standard Python Features

TypePython supports all standard Python 3.10+ syntax including:

- Classes with inheritance
- Decorators (`@property`, `@classmethod`, `@staticmethod`, `@final`, `@override`, `@deprecated`)
- `match` statements
- Walrus operator (`:=`)
- f-strings
- Comprehensions (list, dict, set, generator)
- `*args` and `**kwargs`
- `yield` and `yield from`
- `try`/`except`/`finally`
- `with` statements (context managers)
- `assert` statements
- All standard operators
- Type comments (`# type: ignore`)
