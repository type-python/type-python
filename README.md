<p align="center">
  <img src="logo.png" alt="TypePython" width="160" />
</p>

<h1 align="center">TypePython</h1>

<p align="center">
  <strong>Write richer types in <code>.tpy</code>. Ship plain <code>.py</code> + <code>.pyi</code>.</strong>
</p>

<p align="center">
  <a href="https://pypi.org/project/type-python/"><img src="https://img.shields.io/pypi/v/type-python" alt="PyPI" /></a>
  <a href="https://github.com/type-python/type-python/actions/workflows/rust.yml"><img src="https://github.com/type-python/type-python/actions/workflows/rust.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/python-3.9%2B-blue.svg" alt="Python 3.9+" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/built%20with-rust-orange.svg" alt="Built with Rust" /></a>
  <a href="https://github.com/type-python/type-python/issues"><img src="https://img.shields.io/badge/status-Core%20v1%20Beta-blue.svg" alt="Core v1 Beta" /></a>
</p>

<p align="center">
  TypePython is a typed dialect of Python that compiles to standard <code>.py</code> + <code>.pyi</code>.<br/>
  It brings TypeScript-class ergonomics — <code>sealed</code> classes, exhaustive <code>match</code>,
  strict null checks, <code>unknown</code>, <code>interface</code>, <code>data class</code> —
  to a language whose output runs anywhere CPython runs.<br/>
  <em>No custom runtime. No per-checker plugin. No vendor lock-in.</em>
</p>

---

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

`typepython build` lowers that to ordinary Python and writes a matching `.pyi` that any modern type checker can consume:

```python
# .typepython/build/app/expr.py        # runs on stock CPython, no TypePython runtime
class Expr:
    pass  # tpy:sealed
class Num(Expr): ...
class Add(Expr): ...
class Neg(Expr): ...
def evaluate(expr: Expr) -> int: ...
```

## Install & first project (60 seconds)

```bash
pip install type-python
typepython init --dir hello && cd hello
typepython check --project .
typepython build --project .
```

You now have `.typepython/build/` with `.py` + `.pyi` + `py.typed` ready for any Python interpreter, IDE, or downstream type checker.

> **Wheels** are prebuilt for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64. Other platforms fall back to source and need Rust + `cargo`.
> **Python**: the package bridge supports 3.9+; generated projects target Python 3.10 through 3.14.

## What you actually write vs. what ships

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

Full lowering map: [`docs/interop.md`](docs/interop.md) · syntax tour: [`docs/syntax-guide.md`](docs/syntax-guide.md).

## Why not just mypy / pyright / PEP 695?

Python's type story has gotten genuinely good. TypePython exists for the gaps that source-only annotations still can't close.

| Capability                                              | mypy strict | pyright strict | PEP 695 `.py` | **TypePython `.tpy`** |
| ------------------------------------------------------- | :---------: | :------------: | :-----------: | :-------------------: |
| `sealed class` + compiler-proved exhaustiveness         |      —      |       —        |       —       |          ✅           |
| `unknown` — safe dynamic boundary, must narrow          |      —      |       —        |       —       |          ✅           |
| `unsafe:` audit fence for `eval` / `exec` / `setattr`   |      —      |       —        |       —       |          ✅           |
| First-class `interface` / `data class` / `typealias`    |    via      |     via        |    via        |     keyword           |
| Inline generics `def f[T]` on **any** target ≥ 3.10     |     3.12+   |     3.12+      |     3.12+     |          ✅           |
| `TypedDict` transforms (`Partial`, `Pick`, `Readonly`…) |      —      |       —        |       —       |          ✅           |
| Output consumed by mypy / pyright / ty unmodified       |     N/A     |     N/A        |       ✅       |          ✅           |

TypePython doesn't replace those checkers. It sits **one step earlier**: you author in `.tpy`, the compiler emits standard typed Python that those tools then consume normally.

## Who this is for

- **Python developers who envy TypeScript.** You want sealed unions, exhaustiveness, `unknown`, strict nulls, and `interface` without inventing five mypy plugins to get there.
- **Library authors.** You publish a typed package and need `py.typed`, `.pyi`, wheel/sdist contents, and public-API drift to all stay in sync. `verify`, `compat`, and `api-diff` are built for that.
- **Application teams adopting types gradually.** Mix `.tpy`, `.py`, and `.pyi` in the same source tree; baseline existing debt; gate new debt with type-budgets.
- **Framework authors** (advanced, currently prototype). Describe your runtime-generated shape **once**, declaratively, and stop maintaining one plugin per checker.
- **Platform / typing owners.** Centralize multi-checker policy, dependency type-health checks, and framework adapters in CI.

## Tour of the toolchain

```bash
typepython init      --dir my-project        # scaffold
typepython check     --project .             # type check only
typepython build     --project .             # emit .py + .pyi + py.typed
typepython watch     --project .             # rebuild on save (~80 ms debounce)
typepython lsp       --project .             # JSON-RPC LSP over stdio

typepython verify    --project .             # structural .py / .pyi parity
typepython compat    --project . --profile library-portable
typepython api-diff  old-stubs new-stubs     # public typing API drift
typepython type-health --project . --fail-under 85
typepython migrate   --project . --report    # adoption baseline
```

- All project-oriented commands accept `--format text|json` for CI.
- The Rust core is incremental: a public-API fingerprint per module skips downstream rechecks when only a body changes; persistent state lives under `.typepython/cache/`.
- Diagnostics ship as a documented catalog of `TPYxxxx` codes ([`docs/diagnostics.md`](docs/diagnostics.md)), with machine-readable suggestions for code-action fixes.

Full reference: [`docs/cli-reference.md`](docs/cli-reference.md).

## Editor support

`typepython lsp --project .` is a stdio LSP server with:

- real-time diagnostics, hover, go to definition, references, rename
- completions, signature help
- formatting via `ruff format`, `black`, or a configured formatter
- code actions for migration and portability fixes
- project commands (`migrate`, `compat`, `type-health`, emitted-output preview, `Any` / `Unknown` source lookup)

There is **no official editor extension yet** — any LSP-capable editor (VS Code, Neovim, Helix, Sublime Text, Emacs) can launch the server. Setup snippets are in [`docs/lsp.md`](docs/lsp.md).

## Standard, portable output — by design

TypePython makes one strong promise about its output:

> Emitted `.py` and `.pyi` contain **only standard Python typing constructs**. Nothing TypePython-specific leaves the build directory.

A few stronger guarantees are author-time only — they live in your `.tpy` source and intentionally
degrade to standard typing surfaces in consumer-facing artifacts:

| TypePython author-time fact         | Stability status                | At the boundary                                |
| ----------------------------------- | ------------------------------- | ---------------------------------------------- |
| `unknown` requires narrowing        | Stable Core v1                  | lowers to `object` in `.pyi`                   |
| `sealed class` exhaustiveness       | Stable Core v1                  | external checkers see a normal class           |
| `unsafe:` audit fence               | Stable Core v1                  | erased; lowered to valid Python                |
| `TypedDict` transforms              | Stable Core v1                  | expanded to standard `TypedDict` shapes        |
| effect, taint, and witness facts    | Roadmap / prototype             | checked at author-time; erased or sidecar-only |

This trade is intentional: **you get stronger checks while authoring; consumers get clean, portable Python they can read with mypy, pyright, ty, IDEs, and any PEP 561 tool**. See [`docs/interop.md`](docs/interop.md), [`docs/feature-status.md`](docs/feature-status.md), and [`docs/author-time-semantics.md`](docs/author-time-semantics.md).

## For library and framework authors

Two pieces of the toolchain are aimed squarely at people who *publish* typed Python.

**Publishing workflow** — keep your typed surface release-ready:

```bash
typepython build       --project .
typepython verify      --project . --checker-preset all
typepython compat      --project . --profile library-portable
typepython api-diff    dist/previous.whl .typepython/build
typepython type-health --project . --fail-under 85
```

These catch missing/stale `py.typed`, wheel ↔ build-tree drift, multi-checker portability gaps, public-API drift between releases, and runtime-annotation hazards for frameworks that introspect annotations.

**Framework shape adapters** *(prototype tier)* — describe a runtime-generated API once, lower it into `.pyi` declaratively, and skip per-checker plugins:

```python
# A framework-side declaration (Pydantic-style, abridged):
@framework_transform(kind="base_class",
                     capabilities=("field_collection",
                                   "constructor_generation",
                                   "alias_handling"))
class BaseModel: ...
```

`.pyi` then exposes the generated `__init__(...)`, alias-aware fields, and shape transforms automatically. See [`docs/framework-adapters.md`](docs/framework-adapters.md) and the downstream-checker fixtures under [`test-fixtures/downstream-checkers/`](test-fixtures/downstream-checkers/).

## Project status

TypePython is **Core v1 Beta** (v0.4.0). The Beta claim is deliberately scoped: Core syntax and
checker semantics, configuration, `init`/`check`/`build`/`clean`/`verify`, diagnostic code identity,
and emitted `.py`/`.pyi` compatibility are the stable surfaces. Supported DX, Experimental opt-in,
and Roadmap / prototype features ship for feedback but are not compatibility-stable. See
[`docs/beta-readiness.md`](docs/beta-readiness.md) and [`docs/feature-status.md`](docs/feature-status.md).

The breakdown:

| Tier | What's there |
| ---- | ------------ |
| **Stable Core v1** | `.tpy` Core syntax; Core checker semantics such as sealed exhaustiveness, `unknown` narrowing, `unsafe:` fences, and supported `TypedDict` transforms; project discovery and `typepython.toml` Core config; `init`, `check`, `build`, `clean`, `verify`; diagnostic code identity; `.py` lowering and `.pyi` generation with no mandatory TypePython runtime. |
| **Supported DX, non-stable** | `watch`, LSP UX details, `compat`, `api-diff`, `type-health`, `migrate`, checker portability profiles, type budgets, and migration dashboards. |
| **Experimental opt-in** | runtime validators, shape projection beyond `TypedDict`, conditional return syntax, pass-through `.py` inference, sync/async dual emit paths, and other opt-in research slices. |
| **Roadmap / prototype** | framework adapter manifests and SDK details, effect/capability rows beyond `unsafe:`, taint facts, validator witnesses, notebook ingestion, and other deferred research tracks. |

Conformance and diagnostic-coverage reports are checked into the repo: [`docs/conformance-report.md`](docs/conformance-report.md), [`docs/diagnostic-test-coverage.md`](docs/diagnostic-test-coverage.md).

> **Beta does not mean all roadmap features are stable.** It means the Core v1 authoring and emit
> contract is ready for serious trial use while DX and Experimental surfaces continue to evolve.

## Configuration

Project config lives in `typepython.toml`, or in `[tool.typepython]` inside `pyproject.toml`:

```toml
[project]
src = ["src"]
target_python = "3.10"            # 3.10 – 3.14 supported

[typing]
profile = "application"           # "library" | "application" | "migration"
strict = true
strict_nulls = true

[emit]
emit_pyi = true
no_emit_on_error = true
```

Full reference: [`docs/configuration.md`](docs/configuration.md).

## Examples

| Example                                     | Demonstrates                                                  |
| ------------------------------------------- | ------------------------------------------------------------- |
| [`hello-world/`](examples/hello-world/)     | Minimal starter project                                       |
| [`todo-app/`](examples/todo-app/)           | `data class`, `TypedDict`, overloads, enum, null narrowing    |
| [`shapes/`](examples/shapes/)               | sealed classes, exhaustive `match`, interface, generics       |
| [`http-client/`](examples/http-client/)     | interface bounds, generic classes, overloads, `TypedDict`     |
| [`config-loader/`](examples/config-loader/) | `unknown`, unsafe boundaries, trust-boundary parsing patterns |
| [`event-system/`](examples/event-system/)   | sealed events, interfaces, data classes, generics             |
| [`showcase/`](examples/showcase/)           | multi-file feature showcase                                   |
| [`research-roadmap-demo/`](examples/research-roadmap-demo/) | effect rows, Shape projection, restricted evaluator, taint, validator witnesses |

Framework and downstream-checker fixtures: [`test-fixtures/downstream-checkers/`](test-fixtures/downstream-checkers/).

## Migration path

You don't have to convert a codebase to start using TypePython.

- Add new modules as `.tpy`; existing `.py` and `.pyi` stay in the graph (`.py` is pass-through, `.pyi` is stub-authoritative).
- Run `typing.profile = "migration"` for a lenient first pass.
- Establish a diagnostic baseline; CI fails only on **new** debt.
- Track public `Any` / `unknown` exports with `type-health` budgets.
- Generate starter `.pyi` from existing `.py`.

Full guide: [`docs/migration-guide.md`](docs/migration-guide.md).

## Naming, briefly

| Where you see it    | Identifier      |
| ------------------- | --------------- |
| `pip install …`     | `type-python`   |
| Python import root  | `typepython`    |
| CLI command         | `typepython`    |
| Source file suffix  | `.tpy`          |
| Config file         | `typepython.toml` or `[tool.typepython]` in `pyproject.toml` |

## Documentation

| Read this when…                              | Doc                                              |
| -------------------------------------------- | ------------------------------------------------ |
| You want a guided first project              | [Getting Started](docs/getting-started.md)       |
| You're learning the syntax                   | [Syntax Guide](docs/syntax-guide.md)             |
| You want the assignability / narrowing rules | [Type System](docs/type-system.md)               |
| You're tuning `typepython.toml`              | [Configuration](docs/configuration.md)           |
| You're scripting CI                          | [CLI Reference](docs/cli-reference.md)           |
| You hit a `TPYxxxx` error                    | [Diagnostics](docs/diagnostics.md)               |
| You're wiring it into your editor            | [LSP Integration](docs/lsp.md)                   |
| You care how the output behaves in mypy/pyright | [Interoperability](docs/interop.md)           |
| You're adopting it in an existing codebase   | [Migration Guide](docs/migration-guide.md)       |
| You're a framework author                    | [Framework Adapters](docs/framework-adapters.md) |
| You're evaluating Beta stability             | [Beta Readiness](docs/beta-readiness.md)         |
| You're checking feature stability            | [Feature Status](docs/feature-status.md)         |
| You want the crate map / pipeline diagram    | [Architecture](docs/architecture.md)             |
| You're evaluating the P0-P4 research slices  | [Author-Time Semantics](docs/author-time-semantics.md) |
| You're sending a PR                          | [Contributing](docs/contributing.md)             |
| You have a quick question                    | [FAQ](docs/faq.md)                               |
| You need normative semantics                 | [Language Spec v1](docs/spec/language-spec-v1.md) |

## Contributing & community

The workspace MSRV is Rust 1.94.0. The compiler is a focused Rust workspace — parser, binder, graph, checker, lowering, emit, incremental, LSP, CLI. The architecture diagram in [`docs/architecture.md`](docs/architecture.md) is the fastest way to orient yourself.

Local development:

```bash
./scripts/bootstrap-rust.sh           # pinned Rust 1.94.0
cargo build --release -p typepython-cli
make ci                                # full validation suite
make test
make bench
```

Good first contributions: triaging diagnostic-message ergonomics, adding fixtures under `examples/` or `test-fixtures/downstream-checkers/`, expanding the syntax/type-system docs, or filling in TODOs in the experimental tier.

Bug reports and design feedback go in [GitHub Issues](https://github.com/type-python/type-python/issues). Larger discussions can start as an RFC under [`docs/rfcs/`](docs/rfcs).

See [`docs/contributing.md`](docs/contributing.md) for the full PR workflow.

## License

[MIT](LICENSE)
