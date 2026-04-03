# TypePython

[![PyPI](https://img.shields.io/pypi/v/type-python)](https://pypi.org/project/type-python/)
[![CI](https://github.com/type-python/type-python/actions/workflows/rust.yml/badge.svg)](https://github.com/type-python/type-python/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Python 3.9+](https://img.shields.io/badge/python-3.9%2B-blue.svg)](https://www.python.org/)
[![Rust](https://img.shields.io/badge/rust-1.94.0-orange.svg)](https://www.rust-lang.org/)

**A statically-typed authoring language that compiles to standard Python.**

Write `.tpy` files with `interface`, `data class`, `sealed class`, inline generics, and strict null safety. The compiler emits clean `.py` + `.pyi` files that run on any Python 3.10+ interpreter and type-check with mypy or pyright -- no custom runtime, no lock-in.

---

## Install

```bash
pip install type-python
typepython --help
```

The Python package bridge supports Python 3.9+. Generated TypePython projects currently target Python 3.10, 3.11, or 3.12.
Published wheels are platform-specific because they bundle the Rust CLI binary. If PyPI does not have a wheel for your platform, `pip` falls back to the source distribution and requires Rust + `cargo`.

Or build from source:

```bash
git clone https://github.com/type-python/type-python.git && cd type-python
./scripts/bootstrap-rust.sh
cargo build --release -p typepython-cli
```

## 30-Second Demo

```python
# src/app/__init__.tpy

sealed class Expr:
    pass

data class Num(Expr):
    value: int

data class Add(Expr):
    left: Expr
    right: Expr

def evaluate(expr: Expr) -> int:
    match expr:
        case Num(value=v):
            return v
        case Add(left=l, right=r):
            return evaluate(l) + evaluate(r)
    # Compiler proves all cases are covered -- no default needed.
```

```bash
typepython build --project .
```

The compiler outputs:

| Your code (`.tpy`)                | Compiled output (`.py`)                           |
| --------------------------------- | ------------------------------------------------- |
| `sealed class Expr:`              | `class Expr:  # tpy:sealed`                       |
| `data class Num(Expr):`           | `@dataclass class Num(Expr):`                     |
| `interface Drawable:`             | `class Drawable(Protocol):`                       |
| `overload def f(x: int) -> int:`  | `@overload def f(x: int) -> int:`                 |
| `typealias Pair[T] = tuple[T, T]` | `T = TypeVar("T"); Pair: TypeAlias = tuple[T, T]` |
| `def first[T](xs: list[T]) -> T:` | `T = TypeVar("T"); def first(xs: list[T]) -> T:`  |
| `unsafe: eval(expr)`              | `if True: eval(expr)`                             |

Emitted `.py` and `.pyi` contain only standard Python typing constructs (PEP 484/544/561/612/613/655). Downstream consumers never need TypePython.

## Why TypePython

**mypy and pyright check your types. TypePython gives you better types to check.**

|                           | mypy / pyright                    | TypePython                                                  |
| ------------------------- | --------------------------------- | ----------------------------------------------------------- |
| **What it does**          | Checks annotations on `.py` files | Compiles `.tpy` to `.py` + `.pyi` with richer type features |
| **Sealed exhaustiveness** | Limited (via `assert_never`)      | Compiler-proven in `match` statements                       |
| **`unknown` type**        | Not available                     | Forces narrowing before use (safer than `Any`)              |
| **`interface` keyword**   | Manual `Protocol` classes         | First-class syntax, same output                             |
| **`data class` keyword**  | Manual `@dataclass` decorator     | First-class syntax, same output                             |
| **TypedDict utilities**   | Not available                     | `Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`            |
| **`unsafe` blocks**       | Not available                     | Fences `eval`/`exec`/`setattr` for auditing                 |
| **Interop**               | Native                            | Full -- output is consumed by mypy/pyright unchanged        |

TypePython is not a replacement for mypy or pyright. It is a **source language** whose output is checked by those tools.

## Features

- **Compiles to Python** -- `.tpy` emits standard `.py` + `.pyi`; runs on CPython 3.10+ ([syntax guide](docs/syntax-guide.md))
- **Rich type system** -- `unknown`, `dynamic`, `Never`, strict nulls, sealed exhaustiveness, generic defaults, TypeVarTuple ([type system](docs/type-system.md))
- **Syntax extensions** -- `interface`, `data class`, `sealed class`, `overload def`, `typealias`, `unsafe:`, inline type parameters ([syntax guide](docs/syntax-guide.md))
- **TypedDict utilities** -- `Partial`, `Required_`, `Readonly`, `Mutable`, `Pick`, `Omit` ([type system](docs/type-system.md))
- **Incremental builds** -- fingerprint-based caching; only rechecks modules whose public API changed ([architecture](docs/architecture.md))
- **Full toolchain** -- `init`, `check`, `build`, `watch`, `clean`, `verify`, `migrate` ([CLI reference](docs/cli-reference.md))
- **LSP server** -- hover, go-to-definition, references, rename, completions, signature help, document symbols, workspace symbols, formatting, code actions, real-time diagnostics ([LSP](docs/lsp.md))
- **Publication-ready** -- `typepython verify` validates wheel/sdist consistency and public API completeness ([interop](docs/interop.md))
- **Bundled stdlib stubs** -- typing data for Python 3.10-3.12 standard library, no external dependencies

## Examples

| Example                                     | Features                                                          |
| ------------------------------------------- | ----------------------------------------------------------------- |
| [`hello-world/`](examples/hello-world/)     | Minimal starter project                                           |
| [`todo-app/`](examples/todo-app/)           | `data class`, `TypedDict`, `overload`, enum, null narrowing       |
| [`shapes/`](examples/shapes/)               | `sealed class`, exhaustive `match`, `interface`, generic function |
| [`http-client/`](examples/http-client/)     | `interface`, generic class with bound, `overload`, `TypedDict`    |
| [`config-loader/`](examples/config-loader/) | `unknown` type, `isinstance` narrowing, `unsafe` blocks           |
| [`event-system/`](examples/event-system/)   | `sealed` + `data class` + `interface` + generics + `match`        |
| [`showcase/`](examples/showcase/)           | All features combined in a multi-file project                     |

## CLI

```bash
typepython init    --dir my-project     # Scaffold a new project
typepython check   --project .          # Type-check only
typepython build   --project .          # Emit .py + .pyi
typepython watch   --project .          # Rebuild on changes
typepython clean   --project .          # Remove build artifacts
typepython lsp     --project .          # Start language server
typepython verify  --project .          # Validate for publication
typepython migrate --project . --report # Migration coverage report
```

All commands support `--format text|json`. See [CLI reference](docs/cli-reference.md).

## Configuration

Projects are configured via `typepython.toml` or `[tool.typepython]` in `pyproject.toml`:

```toml
[project]
src = ["src"]
target_python = "3.10"

[typing]
profile = "application"    # "library" | "application" | "migration"
strict = true
strict_nulls = true

[emit]
emit_pyi = true
no_emit_on_error = true
```

See [configuration reference](docs/configuration.md).

## Documentation

|                                                |                                          |
| ---------------------------------------------- | ---------------------------------------- |
| [Getting Started](docs/getting-started.md)     | Installation and first project           |
| [Syntax Guide](docs/syntax-guide.md)           | TypePython syntax extensions             |
| [Type System](docs/type-system.md)             | Types, assignability, narrowing          |
| [Configuration](docs/configuration.md)         | Full `typepython.toml` reference         |
| [CLI Reference](docs/cli-reference.md)         | Commands, flags, output formats          |
| [Diagnostics](docs/diagnostics.md)             | All TPYxxxx error codes                  |
| [LSP Integration](docs/lsp.md)                 | Editor setup and capabilities            |
| [Interoperability](docs/interop.md)            | mypy/pyright compatibility               |
| [Migration Guide](docs/migration-guide.md)     | Adopting TypePython in existing projects |
| [Architecture](docs/architecture.md)           | Crate map, pipeline, dependency graph    |
| [Contributing](docs/contributing.md)           | Development setup and PR workflow        |
| [FAQ](docs/faq.md)                             | Frequently asked questions               |
| [Language Spec](docs/spec/language-spec-v1.md) | Normative language semantics             |

## Contributing

```bash
make ci              # format + lint + test + bench-check + package-check
make test            # run all 833 tests
make bench           # run performance benchmarks
make snapshot-review # review insta snapshot changes
```

See [contributing guide](docs/contributing.md) for the full development workflow.

## License

[MIT](LICENSE)
