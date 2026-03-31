# FAQ

## General

### What is TypePython?

TypePython is a statically-typed authoring language that compiles to standard Python. You write `.tpy` files with enhanced type syntax -- the compiler emits clean `.py` and `.pyi` files that run on any standard Python 3.10+ interpreter.

### Is TypePython a new Python interpreter?

No. TypePython is a **compiler** and **type checker**. The output is standard Python code. There is no custom runtime or interpreter -- your emitted `.py` files run on CPython, PyPy, or any other Python implementation.

### How does TypePython relate to mypy / pyright / pytype?

TypePython is a **source language** with its own syntax extensions (`.tpy` files), not just a type checker for existing Python. It:

- Adds new syntax: `interface`, `data class`, `sealed class`, `overload def`, `typealias`, `unsafe:`
- Compiles to standard Python (`.py` + `.pyi` output)
- Includes its own type checker, LSP, and build system
- Ships with bundled stdlib typing data

You can use mypy or pyright on the **emitted** `.py`/`.pyi` files, since those are standard Python. See [Interoperability](interop.md) for the full lowering map, semantic boundary details, and PEP 561 compliance.

### What Python versions does TypePython target?

TypePython can target Python 3.10, 3.11, or 3.12 via the `project.target_python` configuration. The target version affects compatibility-oriented lowering decisions, such as whether emitted helpers come from `typing` or `typing_extensions`.

### Is TypePython production-ready?

TypePython aims to track the current Core v1 draft closely, and the repository implements the major Core v1 language and checking features described in this documentation. The DX v1 tier (watch, LSP, migration tools) and Experimental tier (conditional returns, pass-through inference, runtime validators) are also present. Evaluate it against your project's needs and tolerance for draft-spec evolution.

## Language

### What's the difference between `unknown` and `dynamic`?

| | `unknown` | `dynamic` |
|---|---|---|
| Safety | Safe -- must narrow before use | Unsafe -- escape hatch |
| Member access | Error until narrowed | Always allowed |
| Assignability | Only to `unknown`/`dynamic`/`object` | To and from everything |
| Equivalent to | A strict `object` | TypeScript `any` / Python `Any` |

Use `unknown` at safety boundaries (external input, untyped packages). Use `dynamic` only when you explicitly want to opt out of type checking.

### When should I use `interface` vs `class`?

- Use **`interface`** for structural contracts (duck typing). Any class that has the right methods/attributes satisfies the interface without inheriting from it. Lowers to `Protocol`.
- Use **`class`** for nominal types. Subtyping requires explicit inheritance.

```python
# Structural: any object with .name satisfies this
interface Named:
    name: str

# Nominal: must explicitly inherit from Animal
class Animal:
    name: str
```

### What is a sealed class?

A `sealed class` restricts direct subclassing to the same module. This enables the compiler to perform **exhaustiveness checking** in `match` statements -- if you handle all subclasses, no `default` case is needed.

```python
sealed class Shape: ...
class Circle(Shape): ...
class Rect(Shape): ...

match shape:
    case Circle(): ...
    case Rect(): ...
    # Exhaustive -- compiler guarantees all cases covered
```

### Can I use TypePython features in `.py` files?

No. TypePython syntax extensions (`interface`, `data class`, `sealed class`, etc.) are only available in `.tpy` files. Plain `.py` files are pass-through -- they are included in the module graph for import resolution but are not type-checked with TypePython-specific rules.

### Does TypePython support async/await?

Yes. `async def`, `await`, `async for`, `async with`, `yield`, and `yield from` are all fully supported. The lowered output preserves async semantics exactly.

### Are generics supported?

Yes. TypePython supports inline type parameters on functions, classes, and type aliases:

```python
def first[T](items: list[T]) -> T: ...
class Box[T]: ...
typealias Pair[T] = tuple[T, T]
```

Including upper bounds (`T: Base`), constraint lists (`T: (A, B, C)`), defaults (`T = int`), and `ParamSpec` (`**P`). Source-authored `TypeVarTuple` (`*Ts`) syntax is currently deferred.

## Build and Tooling

### How do I install TypePython?

The fastest way:

```bash
git clone https://github.com/type-python/type-python.git
cd type-python
./scripts/bootstrap-rust.sh
cargo build --release -p typepython-cli
```

Or via pip: `pip install -e .`

### What does `typepython build` output?

Given a `.tpy` source file, the build produces:
- A `.py` file (lowered standard Python)
- A `.pyi` file (type stub for external tools)
- A `py.typed` marker (PEP 561 compliance)
- A `snapshot.json` (incremental build state)

### Can I use TypePython alongside existing Python tools?

Yes. The emitted `.py` and `.pyi` files are standard Python. You can:
- Run them with any Python interpreter
- Use mypy/pyright on the output
- Package them with setuptools/flit/poetry
- Import them from regular Python code

Some TypePython-specific guarantees (sealed exhaustiveness, `unknown` strictness, `unsafe` boundaries) do not transfer to external tools because the standard typing system has no equivalents. See [Interoperability](interop.md) for details.

### How does incremental building work?

TypePython computes a fingerprint (FNV-1a hash) for each module's public API. On subsequent builds:
- If a module's source changed but its public API fingerprint is the same: dependents are NOT rechecked
- If the public API changed: direct and transitive dependents are rechecked

State is persisted in `.typepython/cache/snapshot.json`.

### How do I set up editor support?

Run `typepython lsp --project .` and configure your editor to use it as a language server. See [LSP Integration](lsp.md) for specific editor instructions (VS Code, Neovim, Helix, Sublime Text, Emacs).

## Configuration

### Where does TypePython look for configuration?

1. `typepython.toml` in the project directory (or any parent directory)
2. `[tool.typepython]` in `pyproject.toml`

The first one found wins.

### What's the difference between the typing profiles?

| Profile | Use case | Strictness |
|---|---|---|
| `library` | Published packages | Maximum: strict + require known public types |
| `application` | Applications | High: strict, relaxed public API |
| `migration` | Gradual adoption | Minimum: lenient, dynamic imports |

### How do I handle imports from untyped packages?

Three options:
1. Set `typing.imports = "dynamic"` -- treat as `dynamic` (no checking)
2. Set `typing.imports = "unknown"` (default) -- treat as `unknown` (must narrow)
3. Write `.pyi` stubs and add their directory to `resolution.type_roots`

### How do I suppress a specific diagnostic?

```python
x: int = "hello"  # type: ignore[TPY4001]
```

Or suppress all diagnostics on a line:

```python
x: int = "hello"  # type: ignore
```

## Troubleshooting

### `typepython check` reports "typepython.toml not found"

Ensure there's a `typepython.toml` (or `pyproject.toml` with `[tool.typepython]`) in your project directory, or pass `--project /path/to/project`.

### "Unable to locate the TypePython Rust CLI"

The Python bridge can't find the binary. Either:
- Set `TYPEPYTHON_BIN=/path/to/typepython`
- Install the bundled binary: `pip install -e .`
- Run from the repository checkout with `cargo` available

### Build produces no output files

Check if `emit.no_emit_on_error = true` (default) and there are diagnostic errors. Fix the errors or set `emit.no_emit_on_error = false`.

### "Conflicting module path" error

You have both `foo.tpy` and `foo.py` (or `foo.tpy` and `foo.pyi`) in the same source root. Each module must have exactly one source kind. Rename or remove one of the conflicting files.
