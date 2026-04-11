# Getting Started

This guide walks you through installing TypePython, creating your first project, and running the type checker.

## Prerequisites

- **Rust 1.94.0** -- required when building TypePython from source or installing from a local checkout/source distribution (workspace MSRV: 1.85)
- **Python 3.9+** -- required for the Python package interface and packaging checks (optional if you only use a prebuilt Rust binary directly)
- **Git** -- to clone the repository

TypePython projects themselves currently target Python 3.10, 3.11, or 3.12 via `project.target_python`.

## Installation

Choose the path that matches how you want to use TypePython.

### Option 1: Published package (recommended for most users)

```bash
python -m pip install type-python
typepython --help
python -m typepython --help
```

Published wheels are platform-specific because they bundle the Rust CLI binary. Supported releases publish prebuilt wheels for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64, so those platforms can install and run TypePython without Rust. Other platforms fall back to the source distribution and require Rust + `cargo`.

### Option 2: Local checkout installed as a package

```bash
git clone https://github.com/type-python/type-python.git
cd type-python

# Install the pinned Rust toolchain (installs Rust 1.94.0 with clippy + rustfmt)
./scripts/bootstrap-rust.sh

# Build a local wheel and install the package entry points
python -m pip install .
typepython --help
```

This path installs the same Python package interface as the published package, but builds it from your local checkout.

If you specifically want an editable install while developing the repository itself:

```bash
python -m pip install -e .
```

Editable installs are checkout-oriented. The Python wrapper looks for the CLI in this order: `TYPEPYTHON_BIN`, a bundled package binary, then `cargo run` from the repository checkout. If you want an installed `typepython` command that works independently of the checkout, prefer `python -m pip install .`.

### Option 3: Directly from a source checkout (recommended for compiler development)

```bash
git clone https://github.com/type-python/type-python.git
cd type-python
./scripts/bootstrap-rust.sh

# Run the CLI without installing a Python package
cargo run -p typepython-cli -- --help
```

The compiled binary will be at `target/release/typepython` after a release build:

```bash
cargo build --release -p typepython-cli
./target/release/typepython --help
```

If you want to run the maintainer/CI validation suite from a source checkout, install the packaging tools first:

```bash
python -m pip install build twine
make ci
```

`make ci` is a repository validation target, not a prerequisite for using the compiler interactively.

Unless noted otherwise, the rest of this guide shows commands as `typepython ...`. If you are using a source checkout without installing the package, substitute `cargo run -p typepython-cli -- ...` or `./target/release/typepython ...`.

### Option 4: Custom binary location

If you have a pre-built binary, point the Python wrapper at it with the `TYPEPYTHON_BIN` environment variable:

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

`--embed-pyproject` requires an existing `pyproject.toml`. It fails if `typepython.toml` already exists, or if `pyproject.toml` already contains `[tool.typepython]`.

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
  discovered sources: 1
  lowered modules: 1
  planned artifacts: 1
  tracked modules: 4
  note: compiler pipeline, artifact planning, and verification completed for the loaded project
error[TPY4001]: function `bad` in module `/path/to/project/src/app/__init__.tpy` returns `str` where `bad` expects `int`
  --> /path/to/project/src/app/__init__.tpy:2:1-2:1
  = note: reason: `str` is not assignable to `int` under semantic type checking
```

The summary counts depend on your project size and incremental state. Current diagnostics also include additional notes such as inferred and declared types where relevant.

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

The project-oriented commands `check`, `build`, `watch`, `verify`, and `migrate` support JSON output for CI/tooling integration.

`typepython init` and `typepython clean` do not support `--format`. `typepython lsp` always speaks JSON-RPC over stdio instead of CLI JSON output.

```bash
typepython check --project . --format json
```

```json
{
  "diagnostics": {
    "diagnostics": [
      {
        "code": "TPY4001",
        "message": "function `bad` in module `/path/to/project/src/app/__init__.tpy` returns `str` where `bad` expects `int`",
        "notes": [
          "reason: `str` is not assignable to `int` under semantic type checking",
          "inferred return type: `str`",
          "declared return type: `int`"
        ],
        "severity": "error",
        "span": {
          "path": "/path/to/project/src/app/__init__.tpy",
          "line": 2,
          "column": 1,
          "end_line": 2,
          "end_column": 1
        },
        "suggestions": []
      }
    ]
  },
  "summary": {
    "command": "check",
    "config_path": "/path/to/project/typepython.toml",
    "config_source": "type_python_toml",
    "discovered_sources": 1,
    "lowered_modules": 1,
    "notes": [
      "compiler pipeline, artifact planning, and verification completed for the loaded project"
    ],
    "planned_artifacts": 1,
    "tracked_modules": 4
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
