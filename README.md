# TypePython

[![PyPI](https://img.shields.io/pypi/v/type-python)](https://pypi.org/project/type-python/)
[![CI](https://github.com/type-python/type-python/actions/workflows/rust.yml/badge.svg)](https://github.com/type-python/type-python/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Python 3.9+](https://img.shields.io/badge/python-3.9%2B-blue.svg)](https://www.python.org/)
[![Rust](https://img.shields.io/badge/rust-msrv%201.85-orange.svg)](https://www.rust-lang.org/)

**A statically-typed authoring language that compiles to standard Python.**

Write `.tpy` files with `interface`, `data class`, `sealed class`, inline generics, and strict null safety. The compiler emits standard `.py` + `.pyi` artifacts for Python 3.10-3.12, using standard typing constructs and `typing_extensions` backports where needed, so the result can be checked by tools like mypy, pyright, or ty. No custom runtime, no proprietary type forms.

---

## Install

```bash
pip install type-python
typepython --help
```

The Python package bridge supports Python 3.9+. Generated TypePython projects currently target Python 3.10, 3.11, or 3.12.
Published wheels are platform-specific because they bundle the Rust CLI binary. Supported releases publish prebuilt wheels for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64, so those platforms can install and run TypePython without Rust. Other platforms fall back to the source distribution and require Rust + `cargo`.
The workspace MSRV is Rust 1.85. `./scripts/bootstrap-rust.sh` installs the pinned Rust 1.94.0 development toolchain used by CI.

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

| Your code (`.tpy`)                | Lowered shape (`.py` / `.pyi`)                     |
| --------------------------------- | -------------------------------------------------- |
| `sealed class Expr:`              | `class Expr:  # tpy:sealed`                        |
| `data class Num(Expr):`           | `@dataclass` plus ordinary `class Num(Expr):`      |
| `interface Drawable:`             | `class Drawable(Protocol):`                        |
| `overload def f(x: int) -> int:`  | `@overload` plus ordinary `def f(x: int) -> int:`  |
| `typealias Pair[T] = tuple[T, T]` | `T = TypeVar("T"); Pair: TypeAlias = tuple[T, T]`  |
| `def first[T](xs: list[T]) -> T:` | Materialized `TypeVar` plus ordinary generic `def` |
| `unsafe: eval(expr)`              | `if True: eval(expr)`                              |

Emitted `.py` and `.pyi` use standard Python typing constructs plus `typing_extensions` compatibility imports when needed for the configured target version. Downstream consumers never need the TypePython compiler or a TypePython-specific runtime.

## Why TypePython

**Type checkers like mypy, pyright, and ty verify your annotations. TypePython gives you a better language to write them in.**

Tools like [ty](https://github.com/astral-sh/ty), pyright, and mypy are _checkers_ -- they analyze standard `.py` files and report type errors. TypePython is a _source language_ -- you write `.tpy` files with richer syntax, and the compiler emits the `.py` + `.pyi` that those checkers consume. They are complementary:

```
you write .tpy  -->  TypePython compiles  -->  .py + .pyi  -->  ty / pyright / mypy checks
```

### What TypePython adds at the source-language layer

External checkers work on standard Python syntax. TypePython adds an authoring layer on top and lowers it away before those tools run:

| Capability                       | Standard Python + checker                                                                                             | TypePython authoring layer                                                            |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| **What it is**                   | Type checker for `.py` / `.pyi`                                                                                       | Source language that compiles `.tpy` to `.py` + `.pyi`                                |
| **Sealed hierarchies**           | No `sealed class` syntax; exhaustiveness depends on checker rules for plain Python types                              | `sealed class` syntax plus TypePython-enforced same-module sealing and exhaustiveness |
| **`unknown` as source syntax**   | Some checkers model unknown or partially-known states internally, but there is no portable `unknown` annotation       | First-class `unknown`; must narrow before use                                         |
| **`interface` keyword**          | Write `Protocol` manually                                                                                             | `interface Foo:` lowers to `Protocol`                                                 |
| **`data class` keyword**         | Write `@dataclass` manually                                                                                           | `data class Foo:` lowers to ordinary dataclass code                                   |
| **TypedDict utility transforms** | Field-level features like `ReadOnly` exist, but `Partial` / `Pick` / `Omit` style transforms must be expanded by hand | `Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, `Required_` compile away            |
| **`unsafe` auditing fence**      | No source-level `unsafe:` block                                                                                       | Explicit boundary around `eval` / `exec` / dynamic mutation                           |
| **Stub generation**              | Separate tools or hand-maintained `.pyi`                                                                              | Auto-generates authoritative `.pyi` from `.tpy` source                                |
| **Interop with checkers**        | Consumes standard Python surfaces                                                                                     | Emits standard Python surfaces intended for external checkers                         |

### How TypePython relates to ty

[ty](https://github.com/astral-sh/ty) is Astral's Rust-based type checker with fine-grained incremental analysis, language-server support, and modern type-system features such as intersection types. TypePython's compiled output is designed to be checked by ty, pyright, or mypy without TypePython-specific support.

TypePython is an authoring layer, not a replacement for external checkers. One improves the source language you write; the other validates the emitted standard Python.

## Features

- **Compiles to Python** -- `.tpy` emits standard `.py` + `.pyi` for target Python 3.10-3.12, using `typing_extensions` compatibility imports when needed ([syntax guide](docs/syntax-guide.md))
- **Rich type system** -- `unknown`, `dynamic`, `Never`, strict nulls, sealed exhaustiveness, generic defaults, TypeVarTuple ([type system](docs/type-system.md))
- **Syntax extensions** -- `interface`, `data class`, `sealed class`, `overload def`, `typealias`, `unsafe:`, inline type parameters ([syntax guide](docs/syntax-guide.md))
- **TypedDict utilities** -- `Partial`, `Required_`, `Readonly`, `Mutable`, `Pick`, `Omit` ([type system](docs/type-system.md))
- **Incremental state and caching** -- fingerprint snapshots, cached artifacts, and LSP rechecks for changed modules plus affected dependents ([architecture](docs/architecture.md))
- **Full toolchain** -- `init`, `check`, `build`, `watch`, `clean`, `verify`, `migrate` ([CLI reference](docs/cli-reference.md))
- **LSP server** -- hover, go-to-definition, references, rename, completions, signature help, document symbols, workspace symbols, formatting, code actions, real-time diagnostics ([LSP](docs/lsp.md))
- **Publication-ready** -- `typepython verify` validates runtime/stub parity, packaged wheel/sdist contents, and optional public API completeness checks ([interop](docs/interop.md))
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

Project-oriented commands `check`, `build`, `watch`, `verify`, and `migrate` support `--format text|json`. `clean` does not. `typepython lsp` speaks JSON-RPC over stdio rather than CLI JSON output. See [CLI reference](docs/cli-reference.md).

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
make ci              # format + lint + split test suites + bench-check + package-check
make test            # run the full workspace test suite
make bench           # run performance benchmarks
make bump-version VERSION=0.0.8  # sync Rust + Python package versions
make snapshot-review # review insta snapshot changes
```

See [contributing guide](docs/contributing.md) for the full development workflow.

## License

[MIT](LICENSE)
