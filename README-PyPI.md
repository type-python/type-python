<p align="center">
  <img src="https://raw.githubusercontent.com/type-python/type-python/main/logo.png" alt="TypePython" width="128" />
</p>

<h1 align="center">TypePython</h1>

<p align="center">
  <strong>Write richer types. Emit standard Python.</strong>
</p>

---

A statically-typed authoring language that compiles to standard Python.

Write `.tpy` source files with `interface`, `data class`, `sealed class`, inline generics, and strict null safety. The compiler emits standard `.py` + `.pyi` files for Python 3.10-3.14. No custom runtime, no vendor lock-in.

## Install

```bash
pip install type-python
typepython --help
```

The Python package bridge supports **Python 3.9+**. Generated projects can target **Python 3.10 through 3.14**.

Published wheels bundle the Rust CLI binary. Prebuilt wheels are available for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64. Other platforms fall back to the source distribution and require Rust + `cargo`.

## Quick Start

```bash
typepython init --dir my-project
cd my-project
typepython check --project .
typepython build --project .
```

Example source:

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

## What You Get

| Your code (`.tpy`)                | Emitted output (`.py` / `.pyi`)                    |
| --------------------------------- | -------------------------------------------------- |
| `sealed class Expr:`              | `class Expr:  # tpy:sealed`                        |
| `data class Num(Expr):`           | `@dataclass` plus ordinary `class Num(Expr):`      |
| `interface Drawable:`             | `class Drawable(Protocol):`                        |
| `overload def f(x: int) -> int:`  | `@overload` plus ordinary `def f(x: int) -> int:`  |
| `typealias Pair[T] = tuple[T, T]` | `T = TypeVar("T"); Pair: TypeAlias = tuple[T, T]`  |

- **Rich type system** -- `unknown`, `dynamic`, `Never`, strict nulls, sealed exhaustiveness, generic defaults, TypeVarTuple
- **TypedDict utilities** -- `Partial`, `Required_`, `Readonly`, `Mutable`, `Pick`, `Omit`
- **Full toolchain** -- `init`, `check`, `build`, `watch`, `clean`, `verify`, `lsp`, `migrate`
- **LSP server** -- hover, go-to-definition, references, rename, completions, signature help, diagnostics
- **Standard output** -- emitted `.py` + `.pyi` work with mypy, pyright, and ty out of the box

## Documentation

- [Getting Started](https://github.com/type-python/type-python/blob/main/docs/getting-started.md)
- [Syntax Guide](https://github.com/type-python/type-python/blob/main/docs/syntax-guide.md)
- [Type System](https://github.com/type-python/type-python/blob/main/docs/type-system.md)
- [Configuration](https://github.com/type-python/type-python/blob/main/docs/configuration.md)
- [CLI Reference](https://github.com/type-python/type-python/blob/main/docs/cli-reference.md)
- [Interoperability](https://github.com/type-python/type-python/blob/main/docs/interop.md)
- [Language Spec](https://github.com/type-python/type-python/blob/main/docs/spec/language-spec-v1.md)

## Links

- [Repository](https://github.com/type-python/type-python)
- [Issues](https://github.com/type-python/type-python/issues)
- [License (MIT)](https://github.com/type-python/type-python/blob/main/LICENSE)
