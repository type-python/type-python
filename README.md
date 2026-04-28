<p align="center">
  <img src="logo.png" alt="TypePython" width="128" />
</p>

<h1 align="center">TypePython</h1>

<p align="center">
  <strong>Compile framework-shaped Python into checker-portable <code>.py</code> + <code>.pyi</code>.</strong>
</p>

<p align="center">
  <a href="https://pypi.org/project/type-python/"><img src="https://img.shields.io/pypi/v/type-python" alt="PyPI" /></a>
  <a href="https://github.com/type-python/type-python/actions/workflows/rust.yml"><img src="https://github.com/type-python/type-python/actions/workflows/rust.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/python-3.9%2B-blue.svg" alt="Python 3.9+" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-msrv%201.94.0-orange.svg" alt="Rust" /></a>
</p>

<p align="center">
  TypePython is a static shape compiler for typed Python projects.<br/>
  It lets framework-heavy code describe generated static surfaces once, then emits ordinary Python,
  authoritative stubs, and publication-ready typing metadata for mypy, pyright, ty, IDEs, and PyPI.<br/>
  No mandatory runtime. No checker-specific plugin path.
</p>

---

## Why TypePython

Python already has type checkers. TypePython sits one step earlier.

```
you write .tpy + framework shape metadata
        |
        v
TypePython compiles standard artifacts
        |
        v
.py + .pyi + py.typed
        |
        v
mypy / pyright / ty / IDEs / package consumers
```

The goal is not to replace mypy, pyright, ty, or Pyrefly. The goal is to make the typed surface those tools consume more explicit, portable, and release-ready.

This matters most when normal Python annotations cannot express the static shape that a runtime framework creates:

- Pydantic-style model constructors and field aliases
- ORM mapped attributes and generated class members
- task decorators that replace functions with task objects
- route, dependency, CLI, and command decorators
- dataclass-like frameworks that go beyond the standardized `dataclass_transform` lane

TypePython preserves framework runtime behavior in emitted `.py` and emits the transformed static surface in `.pyi`, so downstream tools do not need TypePython-specific support.

## Who It Helps

| If you are...               | TypePython helps you...                                                                                                           |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| **Framework author**        | Ship checker-neutral static shape metadata instead of maintaining one plugin per checker.                                         |
| **Library maintainer**      | Generate and verify `.pyi`, `py.typed`, wheel/sdist contents, and public typing API diffs before release.                         |
| **Large application team**  | Migrate gradually with `unknown`, strict nulls, diagnostic baselines, type budgets, and checker portability reports.              |
| **Platform / typing owner** | Centralize framework transforms, dependency type-health checks, and multi-checker CI policy.                                      |
| **IDE integrator**          | Use the built-in LSP for diagnostics, hover, definitions, references, rename, completions, formatting, and emitted-stub previews. |

## Framework Shape Demo

Frameworks often create a useful runtime API that ordinary type checkers cannot infer without custom plugins. TypePython models that static shape declaratively and lowers it into standard artifacts.

```python
# framework metadata, sidecar stubs, or a local framework module

def framework_transform(*args, **kwargs):
    def wrap(obj):
        return obj
    return wrap

def Field(*, default=None, default_factory=None, alias=None, frozen=False):
    return default

@framework_transform(
    kind="base_class",
    capabilities=(
        "field_collection",
        "constructor_generation",
        "alias_handling",
        "required_optional_fields",
        "readonly_fields",
    ),
)
class BaseModel:
    pass
```

```python
# app/models.tpy

class User(BaseModel):
    id: int = Field(alias="user_id", frozen=True)
    name: str = Field(default="Ada")
    tags: object = Field(default_factory=list)

user: User = User(user_id=1)
```

TypePython can expose the checker-facing constructor and generated shape in `.pyi` while leaving the runtime framework code alone:

```python
class User(BaseModel):
    id: int
    name: str
    tags: object

    def __init__(
        self,
        user_id: int,
        name: str = ...,
        tags: object = ...,
    ) -> None: ...
```

The runtime framework still owns validation and behavior. TypePython owns the static artifact that mypy, pyright, ty, and IDEs can read.

See [Framework Adapters](docs/framework-adapters.md) and the downstream checker fixtures under [`test-fixtures/downstream-checkers/`](test-fixtures/downstream-checkers/).

## Language Ergonomics

TypePython also gives `.tpy` authors a stricter and more expressive type authoring layer:

| TypePython source                 | Standard emitted surface                       |
| --------------------------------- | ---------------------------------------------- |
| `interface Drawable:`             | `class Drawable(Protocol):`                    |
| `data class User:`                | `@dataclass` plus ordinary `class User:`       |
| `sealed class Expr:`              | ordinary class plus TypePython sealed metadata |
| `overload def parse(...):`        | `@overload def parse(...):`                    |
| `typealias Pair[T] = tuple[T, T]` | `TypeVar` + `TypeAlias` or native `type` alias |
| `def first[T](xs: list[T]) -> T:` | compat or native generic function output       |
| `unsafe: eval(expr)`              | valid Python block preserving runtime behavior |

Safety-focused additions include:

- `unknown` for safe dynamic boundaries that must be narrowed before use
- explicit `dynamic` for intentional opt-out behavior
- strict null checks with `T | None`
- sealed class and enum exhaustiveness checks
- `Partial`, `Pick`, `Omit`, `Readonly`, `Mutable`, and `Required_` transforms for `TypedDict` and shape-backed experiments
- `ParamSpec`, `TypeVarTuple`, generic defaults, recursive aliases, `Self`, `NewType`, and standard decorator typing

See [Syntax Guide](docs/syntax-guide.md) and [Type System](docs/type-system.md).

## Publishing Workflow

TypePython is designed for projects that publish or depend on typed Python packages.

```bash
typepython build --project .
typepython verify --project . --checker-preset all
typepython compat --project . --profile library-portable
typepython api-diff dist/previous.whl .typepython/build
typepython type-health --project . --fail-under 85
```

These commands help catch:

- missing or stale `.py`, `.pyi`, and `py.typed` artifacts
- wheel/sdist contents that diverge from the local build tree
- checker portability problems across mypy, pyright, and ty
- public typing API drift between releases
- untyped or low-precision dependency surfaces
- runtime annotation compatibility risks for annotation-inspecting frameworks

See [Interoperability](docs/interop.md), [CLI Reference](docs/cli-reference.md), and [Migration Guide](docs/migration-guide.md).

## Install

```bash
pip install type-python
typepython --help
```

The Python package bridge supports **Python 3.9+**. Generated TypePython projects can target **Python 3.10 through 3.14**.

Published wheels are platform-specific because they bundle the Rust CLI binary. Prebuilt wheels are available for Windows AMD64, macOS x86_64, macOS arm64, and Linux x86_64. Other platforms fall back to the source distribution and require Rust + `cargo`.

The workspace MSRV is Rust 1.94.0. `./scripts/bootstrap-rust.sh` installs the same Rust 1.94.0 toolchain used by CI.

Build from source:

```bash
git clone https://github.com/type-python/type-python.git
cd type-python
./scripts/bootstrap-rust.sh
cargo build --release -p typepython-cli
```

## Quick Start

```bash
typepython init --dir my-project
cd my-project
typepython check --project .
typepython build --project .
```

The starter project writes:

```text
my-project/
  typepython.toml
  src/
    app/
      __init__.tpy
```

Project configuration can live in `typepython.toml` or `[tool.typepython]` in `pyproject.toml`:

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

See [Configuration](docs/configuration.md).

## CLI

```bash
typepython init    --dir my-project
typepython check   --project .
typepython build   --project .
typepython watch   --project .
typepython clean   --project .
typepython lsp     --project .

typepython verify  --project .
typepython verify  --project . --unsafe-runtime-imports
typepython verify  --project . --checker-preset all
typepython compat  --project . --profile library-portable
typepython api-diff old-stubs new-stubs
typepython type-health --project . --fail-under 85
typepython migrate --project . --report
typepython adapter validate typepython-framework.toml
```

Project-oriented commands support `--format text|json` where applicable. `typepython lsp` speaks JSON-RPC over stdio. See [CLI Reference](docs/cli-reference.md).

## Migration

You can adopt TypePython incrementally:

- start new modules as `.tpy` while existing `.py` files remain in the graph
- use `typing.profile = "migration"` for a lenient first pass
- establish a diagnostic baseline and fail CI only on new debt
- track public `Any` / `Unknown` exports with type budget gates
- generate starter `.pyi` stubs from existing `.py` sources

See [Migration Guide](docs/migration-guide.md).

## Editor Support

`typepython lsp --project .` provides a stdio Language Server Protocol server with:

- real-time diagnostics
- hover, go to definition, references, rename
- completions and signature help
- formatting through `ruff format`, `black`, or a configured formatter
- code actions for common migration and portability fixes
- project commands for `migrate`, `compat`, `type-health`, emitted-output preview, and `Any` / `Unknown` source lookup

TypePython does not currently ship an official editor extension. Any editor with generic LSP support can launch the server. See [LSP Integration](docs/lsp.md).

## Examples

| Example                                     | Shows                                                         |
| ------------------------------------------- | ------------------------------------------------------------- |
| [`hello-world/`](examples/hello-world/)     | Minimal starter project                                       |
| [`todo-app/`](examples/todo-app/)           | `data class`, `TypedDict`, overloads, enum, null narrowing    |
| [`shapes/`](examples/shapes/)               | sealed classes, exhaustive `match`, interface, generics       |
| [`http-client/`](examples/http-client/)     | interface bounds, generic classes, overloads, TypedDict       |
| [`config-loader/`](examples/config-loader/) | `unknown`, unsafe boundaries, trust-boundary parsing patterns |
| [`event-system/`](examples/event-system/)   | sealed events, interfaces, data classes, generics             |
| [`showcase/`](examples/showcase/)           | multi-file feature showcase                                   |

Framework and downstream checker fixtures live in [`test-fixtures/downstream-checkers/`](test-fixtures/downstream-checkers/).

## Status

Stable core:

- `.tpy`, `.py`, and `.pyi` parsing and project discovery
- checker diagnostics for the documented Core v1 feature set
- `.py` lowering and `.pyi` generation
- incremental CLI/LSP analysis cache
- `build`, `check`, `verify`, `compat`, `api-diff`, `type-health`, and `migrate`
- LSP diagnostics, navigation, completion, hover, formatting, and code actions

Prototype:

- framework transform metadata and `typepython-framework.toml` adapter validation
- framework shape synthesis for representative fixture families
- boundary validator generation and delegated validator adapters
- checker portability profiles and allowlists

Experimental:

- shape projection beyond `TypedDict` and dataclass-backed shapes
- conditional return syntax
- sync/async dual emit paths
- notebook ingestion and other deferred research tracks

The conformance and diagnostic coverage reports are generated under [`docs/conformance-report.md`](docs/conformance-report.md) and [`docs/diagnostic-test-coverage.md`](docs/diagnostic-test-coverage.md).

## Boundary Semantics

Emitted artifacts are intentionally standard Python. Some stronger TypePython guarantees are author-time guarantees:

| TypePython guarantee                    | Emitted boundary                                     |
| --------------------------------------- | ---------------------------------------------------- |
| `unknown` requires narrowing before use | lowers to `object` in `.pyi`                         |
| `sealed class` exhaustiveness           | external checkers see a normal class                 |
| `unsafe:` auditing fence                | erased from public stubs and lowered to valid Python |
| TypedDict transform provenance          | emitted as expanded standard `TypedDict` shapes      |

This trade-off keeps generated packages portable. Use TypePython to enforce stronger checks while authoring `.tpy`; downstream consumers receive standard, well-typed Python artifacts. See [Interoperability](docs/interop.md).

## Documentation

|                                                  |                                          |
| ------------------------------------------------ | ---------------------------------------- |
| [Getting Started](docs/getting-started.md)       | Installation and first project           |
| [Syntax Guide](docs/syntax-guide.md)             | TypePython syntax extensions             |
| [Type System](docs/type-system.md)               | Types, assignability, narrowing          |
| [Configuration](docs/configuration.md)           | Full `typepython.toml` reference         |
| [CLI Reference](docs/cli-reference.md)           | Commands, flags, output formats          |
| [Diagnostics](docs/diagnostics.md)               | All TPYxxxx error codes                  |
| [LSP Integration](docs/lsp.md)                   | Editor setup and capabilities            |
| [Interoperability](docs/interop.md)              | mypy/pyright/ty compatibility            |
| [Migration Guide](docs/migration-guide.md)       | Adopting TypePython in existing projects |
| [Framework Adapters](docs/framework-adapters.md) | Declarative framework transform adapters |
| [Architecture](docs/architecture.md)             | Crate map, pipeline, dependency graph    |
| [Contributing](docs/contributing.md)             | Development setup and PR workflow        |
| [FAQ](docs/faq.md)                               | Frequently asked questions               |
| [Language Spec](docs/spec/language-spec-v1.md)   | Normative language semantics             |

## Contributing

```bash
make ci
make test
make bench
make bump-version VERSION=0.0.8
make snapshot-review
```

See [Contributing](docs/contributing.md) for the full development workflow.

## License

[MIT](LICENSE)
