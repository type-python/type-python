# Getting Started

This guide walks you through installing TypePython, creating your first project, and running the type checker.

## Prerequisites

- **Rust 1.94.0** -- pinned development/CI toolchain for compiling TypePython (workspace MSRV: 1.85)
- **Python 3.9+** -- required for the Python package bridge (optional; you can use the Rust binary directly)
- **Git** -- to clone the repository

TypePython projects themselves currently target Python 3.10, 3.11, or 3.12 via `project.target_python`.

## Installation

### Option 1: From source (recommended for development)

```bash
# Clone the repository
git clone https://github.com/type-python/type-python.git
cd type-python

# Bootstrap the Rust toolchain (installs Rust 1.94.0 with clippy + rustfmt)
./scripts/bootstrap-rust.sh

# Verify everything works
make ci
```

The compiled binary will be at `target/release/typepython` after a release build:

```bash
cargo build --release -p typepython-cli
./target/release/typepython --help
```

### Option 2: Python package (pip install)

```bash
pip install -e .
```

This compiles the Rust binary during installation and bundles it into the Python package. After installation:

```bash
# All equivalent:
typepython --help
python -m typepython --help
```

### Option 3: Custom binary location

If you have a pre-built binary, point to it with the `TYPEPYTHON_BIN` environment variable:

```bash
export TYPEPYTHON_BIN=/path/to/typepython
typepython check --project .
```

## Creating a Project

### Using `typepython init`

```bash
# Create a new project in the current directory
typepython init

# Or specify a target directory
typepython init --dir my-project

# Force overwrite existing files
typepython init --dir my-project --force

# If you already have a pyproject.toml, embed config there instead
typepython init --embed-pyproject
```

This generates:

```
my-project/
  typepython.toml           # Project configuration
  src/
    app/
      __init__.tpy           # Starter source file
```

`typepython init --embed-pyproject` still writes the same starter source tree, but it appends `[tool.typepython]` to an existing `pyproject.toml` instead of creating `typepython.toml`.

### Manual setup

Create a `typepython.toml` at your project root:

```toml
[project]
src = ["src"]
target_python = "3.10"

[typing]
profile = "application"
strict = true
```

Create your source directory:

```bash
mkdir -p src/app
```

Create `src/app/__init__.tpy`:

```python
def greet(name: str) -> str:
    return f"hello, {name}"
```

## Writing TypePython

TypePython files use the `.tpy` extension. They support all standard Python syntax plus TypePython extensions.

### Basic types

```python
# src/app/models.tpy

name: str = "Alice"
age: int = 30
active: bool = True
score: float = 9.5
data: bytes = b"hello"
nothing: None = None
```

### Functions with type annotations

```python
def add(x: int, y: int) -> int:
    return x + y

async def fetch_data(url: str) -> dict[str, str]:
    ...
```

### Data classes

```python
data class Point:
    x: float
    y: float

data class User:
    name: str
    email: str
    age: int = 0
```

### Interfaces (structural protocols)

```python
interface Serializable:
    def to_json(self) -> str: ...

interface Comparable[T]:
    def compare(self, other: T) -> int: ...
```

### Generics with inline type parameters

```python
def first[T](items: list[T]) -> T:
    return items[0]

class Stack[T]:
    def push(self, item: T) -> None: ...
    def pop(self) -> T: ...
```

### Sealed classes and exhaustive matching

```python
sealed class Shape:
    pass

class Circle(Shape):
    radius: float

class Rect(Shape):
    width: float
    height: float

def area(s: Shape) -> float:
    match s:
        case Circle(radius=r):
            return 3.14 * r * r
        case Rect(width=w, height=h):
            return w * h
        # No default needed -- compiler proves exhaustiveness
```

### Overloaded functions

```python
overload def parse(value: str) -> int: ...
overload def parse(value: bytes) -> int: ...

def parse(value: str | bytes) -> int:
    if isinstance(value, str):
        return int(value)
    return int(value.decode())
```

### Type aliases

```python
typealias JsonPrimitive = str | int | float | bool | None
typealias JsonValue = dict[str, "JsonValue"] | list["JsonValue"] | JsonPrimitive
```

### Unsafe blocks

```python
# Operations like eval() require an unsafe block
unsafe:
    result = eval(user_input)
```

## Running the Type Checker

### Check (type-check only)

```bash
typepython check --project .
```

Output:

```
check:
  config: /path/to/project/typepython.toml (typepython.toml)
  discovered sources: 5
  lowered modules: 4
  planned artifacts: 5
  tracked modules: 8
  note: compiler pipeline, artifact planning, and verification completed for the loaded project
src/app/models.tpy:12:5  TPY4001  error  Type 'str' is not assignable to 'int'
```

### Build (emit .py + .pyi)

```bash
typepython build --project .
```

Output files go to `.typepython/build/` by default:

```
.typepython/
  build/
    app/
      __init__.py          # Lowered Python
      __init__.pyi         # Generated type stub
      models.py
      models.pyi
      py.typed             # PEP 561 marker
  cache/
    snapshot.json          # Incremental state
```

### Watch (rebuild on changes)

```bash
typepython watch --project .
```

Watches for file changes and rebuilds automatically with configurable debounce (default: 80ms).

### JSON output

All commands support JSON output for CI/tooling integration:

```bash
typepython check --project . --format json
```

```json
{
  "diagnostics": {
    "diagnostics": [
      {
        "code": "TPY4001",
        "severity": "error",
        "message": "Type 'str' is not assignable to 'int'",
        "span": {
          "path": "src/app/models.tpy",
          "line": 12,
          "column": 5,
          "end_line": 12,
          "end_column": 20
        },
        "suggestions": []
      }
    ]
  },
  "summary": {
    "command": "check",
    "config_path": "/path/to/project/typepython.toml",
    "config_source": "type_python_toml",
    "discovered_sources": 5,
    "lowered_modules": 4,
    "notes": [
      "compiler pipeline, artifact planning, and verification completed for the loaded project"
    ],
    "planned_artifacts": 5,
    "tracked_modules": 8
  }
}
```

## Mixed Projects (.tpy + .py + .pyi)

TypePython handles mixed-language projects:

| File type | Treatment                                                      |
| --------- | -------------------------------------------------------------- |
| `.tpy`    | Compiled: type-checked, lowered to `.py`, stubs generated      |
| `.py`     | Pass-through: copied as-is to output, included in module graph |
| `.pyi`    | Stub authority: used for type checking, copied to output       |

```
src/
  app/
    __init__.tpy         # TypePython source (compiled)
    utils.py             # Plain Python (copied)
    external.pyi         # Type stub for external lib (used for checking)
```

## Next Steps

- [Configuration Reference](configuration.md) -- customize your project settings
- [CLI Reference](cli-reference.md) -- all commands and flags
- [Syntax Guide](syntax-guide.md) -- full TypePython syntax reference
- [Type System](type-system.md) -- deep dive into the type system
- [Migration Guide](migration-guide.md) -- adopt TypePython in existing projects
