# Migration Guide

This guide covers strategies for adopting TypePython in existing Python projects.

## Migration Strategies

### Strategy 1: Greenfield modules

Start writing new modules in `.tpy` while keeping existing `.py` files unchanged.

```
src/
  legacy/
    __init__.py          # Existing Python (untouched)
    models.py
    utils.py
  new_feature/
    __init__.tpy         # New TypePython module
    handlers.tpy
```

TypePython handles mixed `.tpy` + `.py` projects natively. Python files are included in the module graph and copied to output as-is.

### Strategy 2: Gradual conversion

Convert files one at a time from `.py` to `.tpy`, starting with leaf modules that have few dependents.

1. Rename `module.py` to `module.tpy`
2. Add type annotations where missing
3. Run `typepython check` to find issues
4. Fix diagnostics
5. Repeat for the next module

### Strategy 3: Migration profile

Use the `migration` profile for maximum leniency during initial adoption:

```toml
[typing]
profile = "migration"
```

This sets:
- `strict = false`
- `strict_nulls = true`
- `imports = "dynamic"` (untyped imports are `dynamic`, not `unknown`)
- `no_implicit_dynamic = false` (no warnings for implicit dynamic)
- `warn_unsafe = true`
- `enable_sealed_exhaustiveness = true`
- `report_deprecated = "ignore"`
- `require_explicit_overrides = false`
- `require_known_public_types = false`
- `infer_passthrough = false`
- `conditional_returns = false`

The migration profile is lenient about implicit dynamic flow and public API completeness, but it still keeps nullability checks and unsafe-operation warnings enabled. Gradually tighten settings as you add types.

## Using the Migration Tool

TypePython includes a built-in migration assistant.

### Coverage report

Analyze your project's current typing coverage:

```bash
typepython migrate --project . --report
```

Output includes:
- Count of known vs. unknown declarations
- Dynamic and unknown boundary counts
- Per-file typing coverage percentage
- Per-directory typing coverage percentage
- High-impact files ranked by import frequency

Current migration reports also include bundled stdlib coverage, because the migration analysis loads the same stdlib typing data used during checking.

### Stub generation

Generate starter `.pyi` stubs with inferred types:

```bash
typepython migrate --project . --emit-stubs src/ --stub-out-dir stubs/
```

Generated stubs include:
- Inferred types from default values and return statements
- `TODO` markers for types that could not be inferred
- Standard typing imports

Use these stubs as a starting point; review and refine manually.

## Step-by-Step Migration

### Step 1: Add TypePython configuration

If your project already has a `pyproject.toml`, you can append `[tool.typepython]` with:

```bash
typepython init --embed-pyproject
```

Or create `typepython.toml` manually with the migration profile:

```toml
[project]
src = ["src"]
target_python = "3.10"

[typing]
profile = "migration"

[emit]
no_emit_on_error = false    # Allow output even with type errors
```

### Step 2: Run baseline check

```bash
typepython check --project . --format json > baseline.json
```

This gives you a baseline count of diagnostics to track progress.

### Step 3: Convert leaf modules first

Identify modules with no internal dependents (leaf modules) and convert them first. These are the safest to change because no other code depends on their exact types.

```bash
# Get a migration report to find good candidates
typepython migrate --project . --report
```

### Step 4: Add type annotations

For each converted `.tpy` file, add type annotations:

```python
# Before (untyped .py)
def process(data):
    result = transform(data)
    return result

# After (typed .tpy)
def process(data: dict[str, str]) -> list[str]:
    result: list[str] = transform(data)
    return result
```

### Step 5: Fix diagnostics iteratively

```bash
typepython check --project .
```

Common issues during migration:

| Diagnostic | Fix |
|---|---|
| `TPY3001` (unresolved import) | Add `.pyi` stubs for external packages, or set `imports = "dynamic"` |
| `TPY4001` (type mismatch) | Add or correct type annotations |
| `TPY4003` (operation on unknown) | Add `isinstance` checks or explicit type annotations |
| `TPY4002` (missing member) | Ensure types are correctly annotated |

### Step 6: Tighten configuration

As typing coverage improves, progressively tighten settings:

```toml
# Phase 1: Migration
[typing]
profile = "migration"

# Phase 2: Application-level strictness
[typing]
profile = "application"
strict = true
strict_nulls = true

# Phase 3: Library-level strictness
[typing]
profile = "library"
require_known_public_types = true
require_explicit_overrides = true
```

### Step 7: Integrate into CI

Add TypePython checking to your CI pipeline:

```yaml
# .github/workflows/typecheck.yml
name: typecheck

on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        run: ./scripts/bootstrap-rust.sh
      - name: Type check
        run: cargo run -p typepython-cli -- check --project . --format json
```

## Handling Untyped Dependencies

When importing from packages without type stubs:

### Option A: Set `imports = "dynamic"` (permissive)

```toml
[typing]
imports = "dynamic"
```

All symbols from untyped packages are treated as `dynamic` -- no type errors, but no type safety either.

### Option B: Set `imports = "unknown"` (strict, default)

```toml
[typing]
imports = "unknown"
```

Symbols from untyped packages are `unknown`. You must narrow before use:

```python
from untyped_lib import get_data

result: unknown = get_data()

if isinstance(result, dict):
    # Now safely narrowed to dict
    process(result)
```

### Option C: Write `.pyi` stubs

Create `.pyi` files for external packages in a `type_roots` directory:

```toml
[resolution]
type_roots = ["typestubs"]
```

```
typestubs/
  untyped_lib.pyi          # Your custom stubs
```

```python
# typestubs/untyped_lib.pyi
def get_data() -> dict[str, str]: ...
def process(data: dict[str, str]) -> None: ...
```

## Common Patterns

### Replacing `Any` with `unknown`

```python
# Before: unsafe Any
def handle(data: Any) -> None:
    data.process()     # No type checking

# After: safe unknown with narrowing
def handle(data: unknown) -> None:
    if isinstance(data, Processable):
        data.process()  # Type-checked
```

### Adding null safety

```python
# Before: implicit None
def find(key: str) -> str:
    result = lookup(key)
    return result       # Could be None!

# After: explicit null handling
def find(key: str) -> str | None:
    result: str | None = lookup(key)
    return result
```

### Upgrading to data classes

```python
# Before: manual __init__
class User:
    def __init__(self, name: str, age: int):
        self.name = name
        self.age = age

# After: data class
data class User:
    name: str
    age: int
```

### Upgrading to interfaces

```python
# Before: duck typing without contracts
def render(obj):
    obj.draw()

# After: explicit interface
interface Drawable:
    def draw(self) -> None: ...

def render(obj: Drawable) -> None:
    obj.draw()
```

### Adding sealed exhaustiveness

```python
# Before: open class hierarchy with default case
class Event: ...
class Click(Event): ...
class Scroll(Event): ...

def handle(e: Event) -> None:
    if isinstance(e, Click):
        ...
    elif isinstance(e, Scroll):
        ...
    else:
        raise ValueError("Unknown event")  # Runtime error if new type added

# After: sealed hierarchy with compile-time exhaustiveness
sealed class Event: ...
class Click(Event): ...
class Scroll(Event): ...

def handle(e: Event) -> None:
    match e:
        case Click():
            ...
        case Scroll():
            ...
    # Compiler guarantees all cases covered
```

## Tips

- **Start with `--report`**: understand your current typing coverage before making changes
- **Convert tests last**: test files benefit less from strict typing
- **Use `# type: ignore` sparingly**: prefer fixing types over suppressing diagnostics
- **Keep CI green**: use the migration profile initially so the build passes while you add types
- **Track progress**: periodically compare diagnostic counts against your baseline
- **Use `typepython watch`**: get immediate feedback while adding type annotations
