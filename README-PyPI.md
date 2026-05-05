<p align="center">
  <img src="https://raw.githubusercontent.com/type-python/type-python/main/logo.png" alt="TypePython" width="160" />
</p>

<h1 align="center">TypePython</h1>

<p align="center">
  <strong>Write richer types in <code>.tpy</code>. Ship plain <code>.py</code> + <code>.pyi</code>.</strong>
</p>

---

TypePython is a typed dialect of Python that compiles to standard `.py` + `.pyi`.
It brings TypeScript-class ergonomics — `sealed` classes, exhaustive `match`,
strict null checks, `unknown`, `interface`, `data class` — to a language whose
output runs anywhere CPython runs.

**No custom runtime. No per-checker plugin. No vendor lock-in.**

> Status: **Core v1 Beta** (v0.4.0). Core syntax, config, `init`/`check`/`build`/
> `clean`/`verify`, Core checker semantics, diagnostic code identity, and emitted
> `.py`/`.pyi` compatibility are the stable Beta surfaces. Supported DX,
> Experimental opt-in, and Roadmap / prototype features remain outside the Beta
> compatibility promise.
> Bug reports and contributions are very welcome.

## Install

```bash
pip install type-python
typepython init --dir hello && cd hello
typepython check --project .
typepython build --project .
```

You now have `.typepython/build/` with `.py` + `.pyi` + `py.typed` ready for
any Python interpreter, IDE, or downstream type checker.

- **Wheels** are prebuilt for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64.
  Other platforms fall back to source and need Rust + `cargo`.
- **Python**: the package bridge supports 3.9+; generated projects target Python 3.10 through 3.14.

## See it in 15 lines

```python
# src/app/expr.tpy

sealed class Expr:
    pass

class Num(Expr):  value: int
class Add(Expr):  left: Expr; right: Expr
class Neg(Expr):  operand: Expr

def evaluate(expr: Expr) -> int:
    match expr:
        case Num(value=v):           return v
        case Add(left=l, right=r):   return evaluate(l) + evaluate(r)
        case Neg(operand=o):         return -evaluate(o)
    # No default branch needed — the compiler proves the match is exhaustive.
    # Add a fourth subclass and TypePython tells you exactly where to update.
```

`typepython build` lowers that to ordinary Python and writes a matching `.pyi`
that any modern type checker can consume — no TypePython runtime required.

## What you write vs. what ships

| You write (`.tpy`)                    | TypePython emits (`.py` / `.pyi`)                         |
| ------------------------------------- | --------------------------------------------------------- |
| `interface Drawable:`                 | `class Drawable(Protocol):`                               |
| `data class User:`                    | `@dataclass class User:`                                  |
| `sealed class Expr:`                  | ordinary class + `tpy:sealed` marker                      |
| `overload def parse(...)`             | `@overload def parse(...)`                                |
| `typealias Pair[T] = tuple[T, T]`     | `T = TypeVar("T")` + `Pair: TypeAlias = tuple[T, T]`      |
| `def first[T](xs: list[T]) -> T:`     | inline generic on 3.13+, `TypeVar`-based on 3.10 – 3.12   |
| `unsafe: eval(expr)`                  | valid Python, marker erased — runtime behavior preserved  |
| `unknown` (must narrow before use)    | `object` in `.pyi`                                        |
| `Partial[Config]` / `Pick[Config, …]` | expanded `TypedDict` shapes (PEP 655 / 705)               |

## Why not just mypy / pyright / PEP 695?

| Capability                                              | mypy strict | pyright strict | PEP 695 `.py` | **TypePython `.tpy`** |
| ------------------------------------------------------- | :---------: | :------------: | :-----------: | :-------------------: |
| `sealed class` + compiler-proved exhaustiveness         |      —      |       —        |       —       |          ✅           |
| `unknown` — safe dynamic boundary, must narrow          |      —      |       —        |       —       |          ✅           |
| `unsafe:` audit fence for `eval` / `exec` / `setattr`   |      —      |       —        |       —       |          ✅           |
| First-class `interface` / `data class` / `typealias`    |    via      |     via        |    via        |     keyword           |
| Inline generics `def f[T]` on **any** target ≥ 3.10     |     3.12+   |     3.12+      |     3.12+     |          ✅           |
| `TypedDict` transforms (`Partial`, `Pick`, `Readonly`…) |      —      |       —        |       —       |          ✅           |
| Output consumed by mypy / pyright / ty unmodified       |     N/A     |     N/A        |       ✅       |          ✅           |

TypePython doesn't replace those checkers. It sits **one step earlier**: you
author in `.tpy`, the compiler emits standard typed Python that those tools
then consume normally.

The strongest TypePython guarantees are author-time checks. Emitted artifacts
remain standard Python: sealed exhaustiveness, `unknown` strictness, and
`unsafe:` fences are enforced by TypePython during authoring, but external
checkers see the portable `.py` / `.pyi` boundary.

## What you also get

- **Rust-native, incremental compiler** — public-API fingerprints skip
  downstream rechecks when only a body changes.
- **Full toolchain** — `init`, `check`, `build`, `watch`, `clean`, `verify`,
  `compat`, `api-diff`, `type-health`, `migrate`, `lsp`.
- **LSP server** — hover, go to definition, references, rename, completions,
  signature help, real-time diagnostics, and code-action quick fixes.
  Bring your own LSP-capable editor (VS Code, Neovim, Helix, Sublime, Emacs).
- **Standard, portable output** — emitted `.py` + `.pyi` work with mypy,
  pyright, and ty out of the box. PEP 561 `py.typed` is written automatically.
- **Mixed projects** — `.tpy`, `.py`, and `.pyi` live in the same source tree.
  `.py` is pass-through; `.pyi` is stub-authoritative.

## For library authors

Five commands aimed at people who *publish* typed Python:

```bash
typepython build       --project .
typepython verify      --project . --checker-preset all
typepython compat      --project . --profile library-portable
typepython api-diff    dist/previous.whl .typepython/build
typepython type-health --project . --fail-under 85
```

These catch missing/stale `py.typed`, wheel ↔ build-tree drift, multi-checker
portability gaps, public-API drift between releases, and runtime-annotation
hazards for frameworks that introspect annotations.

## Documentation

- [Getting Started](https://github.com/type-python/type-python/blob/main/docs/getting-started.md)
- [Syntax Guide](https://github.com/type-python/type-python/blob/main/docs/syntax-guide.md)
- [Type System](https://github.com/type-python/type-python/blob/main/docs/type-system.md)
- [Configuration](https://github.com/type-python/type-python/blob/main/docs/configuration.md)
- [CLI Reference](https://github.com/type-python/type-python/blob/main/docs/cli-reference.md)
- [Diagnostics (TPYxxxx codes)](https://github.com/type-python/type-python/blob/main/docs/diagnostics.md)
- [LSP Integration](https://github.com/type-python/type-python/blob/main/docs/lsp.md)
- [Interoperability](https://github.com/type-python/type-python/blob/main/docs/interop.md)
- [Migration Guide](https://github.com/type-python/type-python/blob/main/docs/migration-guide.md)
- [Framework Adapters](https://github.com/type-python/type-python/blob/main/docs/framework-adapters.md)
- [Beta Readiness](https://github.com/type-python/type-python/blob/main/docs/beta-readiness.md)
- [Feature Status](https://github.com/type-python/type-python/blob/main/docs/feature-status.md)
- [Language Spec v1](https://github.com/type-python/type-python/blob/main/docs/spec/language-spec-v1.md)

## Links

- [Repository](https://github.com/type-python/type-python)
- [Issues / bug reports](https://github.com/type-python/type-python/issues)
- [Contributing](https://github.com/type-python/type-python/blob/main/docs/contributing.md)
- [License (MIT)](https://github.com/type-python/type-python/blob/main/LICENSE)
