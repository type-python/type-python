# TypePython

**A statically-typed authoring language that compiles to standard Python.**

TypePython lets you write `.tpy` source files with features such as `interface`, `data class`, `sealed class`, inline generics, and strict null safety. The compiler emits standard `.py` and `.pyi` files that run on ordinary Python interpreters and type-check with mypy or pyright.

## Install

```bash
pip install type-python
typepython --help
```

Published wheels are platform-specific because they bundle the Rust CLI binary. Supported releases publish prebuilt wheels for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64, so those platforms can install and run TypePython without Rust. Other platforms fall back to the source distribution and require a Rust toolchain with `cargo`.

The Python package bridge supports Python 3.9+. Generated TypePython projects currently target Python 3.10, 3.11, or 3.12.

## Quick Start

Create a project:

```bash
typepython init --dir my-project
cd my-project
typepython check --project .
typepython build --project .
```

Example source:

```python
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
```

## What You Get

- Standard `.py` and `.pyi` output with no custom runtime
- Type system features such as `unknown`, strict nulls, sealed exhaustiveness, `TypedDict` utilities, and generic defaults
- CLI commands for `init`, `check`, `build`, `watch`, `clean`, `verify`, `lsp`, and `migrate`
- Interoperability with standard Python typing tools and package publishing workflows

## Documentation

- Getting started: <https://github.com/type-python/type-python/blob/main/docs/getting-started.md>
- Syntax guide: <https://github.com/type-python/type-python/blob/main/docs/syntax-guide.md>
- Type system: <https://github.com/type-python/type-python/blob/main/docs/type-system.md>
- Configuration: <https://github.com/type-python/type-python/blob/main/docs/configuration.md>
- CLI reference: <https://github.com/type-python/type-python/blob/main/docs/cli-reference.md>
- Interoperability: <https://github.com/type-python/type-python/blob/main/docs/interop.md>
- Language spec: <https://github.com/type-python/type-python/blob/main/docs/spec/language-spec-v1.md>

## Project Links

- Repository: <https://github.com/type-python/type-python>
- Issues: <https://github.com/type-python/type-python/issues>
- License: <https://github.com/type-python/type-python/blob/main/LICENSE>
