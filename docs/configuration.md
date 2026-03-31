# Configuration Reference

TypePython projects are configured via `typepython.toml` at the project root, or via the `[tool.typepython]` table in `pyproject.toml`.

## Configuration Discovery

The compiler walks up from the project directory (or `--project` path) looking for:

1. `typepython.toml` -- standalone configuration (takes precedence)
2. `pyproject.toml` with a `[tool.typepython]` table -- embedded configuration

If neither is found, the compiler reports an error.

## Full Configuration Reference

```toml
# ============================================================================
# [project] -- Source layout and build output
# ============================================================================
[project]

# Source root directories to scan for .tpy/.py/.pyi files.
# Paths are relative to the config file's directory.
# Default: ["src"]
src = ["src"]

# Glob patterns for files to include.
# Default: ["src/**/*.tpy", "src/**/*.py", "src/**/*.pyi"]
include = ["src/**/*.tpy", "src/**/*.py", "src/**/*.pyi"]

# Glob patterns for files to exclude.
# Default: [".typepython/**", "dist/**", ".venv/**", "venv/**"]
exclude = [".typepython/**", "dist/**", ".venv/**", "venv/**"]

# Logical root directory for computing module paths.
# Default: "src"
root_dir = "src"

# Output directory for emitted .py and .pyi files.
# Default: ".typepython/build"
out_dir = ".typepython/build"

# Cache directory for incremental build state.
# Must be distinct from out_dir; the two paths cannot be equal or nested.
# Default: ".typepython/cache"
cache_dir = ".typepython/cache"

# Target Python version for lowering decisions.
# Supported values: "3.10", "3.11", "3.12"
# Default: "3.10"
target_python = "3.10"


# ============================================================================
# [resolution] -- Module resolution
# ============================================================================
[resolution]

# Base URL for non-relative module resolution.
# Default: "."
base_url = "."

# Additional directories to search for type stubs.
# Default: []
type_roots = []

# Path to the Python executable for resolving installed packages.
# Default: null (auto-detect)
python_executable = null

# Path aliases for import rewriting (similar to tsconfig paths).
# Keys are import patterns; values are lists of directory patterns.
# Default: {}
[resolution.paths]
# Example:
# "@app/*" = ["src/app/*"]
# "@lib/*" = ["src/lib/*"]


# ============================================================================
# [emit] -- Output generation
# ============================================================================
[emit]

# Emit .pyi type stub files alongside .py output.
# Default: true
emit_pyi = true

# Compile .py output to .pyc bytecode.
# Default: false
emit_pyc = false

# Write a py.typed marker file in package roots (PEP 561).
# Default: true
write_py_typed = true

# Preserve comments from source in lowered output.
# Default: true
preserve_comments = true

# Block all output if any diagnostic errors exist.
# Default: true
no_emit_on_error = true

# [Experimental] Emit runtime __tpy_validate__() methods on data classes.
# Default: false
runtime_validators = false


# ============================================================================
# [typing] -- Type checking behavior
# ============================================================================
[typing]

# Typing profile preset. Overrides individual settings when set.
# Values: "library", "application", "migration", or null
# Default: null
profile = null

# Enable strict type checking mode.
# Default: true
strict = true

# Enforce strict null checks: None excluded from T unless T | None.
# Default: true
strict_nulls = true

# How to treat imports of untyped modules.
# "unknown" -- imported symbols typed as `unknown` (safer)
# "dynamic" -- imported symbols typed as `dynamic` (permissive)
# Default: "unknown"
imports = "unknown"

# Disallow implicit dynamic types; require explicit `dynamic` annotation.
# Default: true
no_implicit_dynamic = true

# Warn on unsafe operations outside `unsafe:` blocks.
# Default: true
warn_unsafe = true

# Enable exhaustiveness checking for sealed class hierarchies.
# Default: true
enable_sealed_exhaustiveness = true

# How to report usage of deprecated symbols.
# Values: "error", "warning", "ignore"
# Default: "warning"
report_deprecated = "warning"

# Require @override annotation on overriding methods.
# Default: false
require_explicit_overrides = false

# Require that all public types are known (not dynamic/unknown).
# Useful for library authors.
# Default: false
require_known_public_types = false

# [Experimental] Enable pass-through type inference for .py files.
# Default: false
infer_passthrough = false

# [Experimental] Enable conditional return type narrowing.
# Default: false
conditional_returns = false


# ============================================================================
# [watch] -- File watching behavior
# ============================================================================
[watch]

# Debounce interval in milliseconds for filesystem change events.
# Default: 80
debounce_ms = 80
```

## Typing Profiles

Profiles provide curated defaults for common use cases. When a profile is set, its defaults override individual settings (but explicitly set values still take precedence).

### `library`

For published packages that need strict type safety and complete public API typing.

| Setting | Value |
|---|---|
| `strict` | `true` |
| `strict_nulls` | `true` |
| `imports` | `"unknown"` |
| `no_implicit_dynamic` | `true` |
| `require_known_public_types` | `true` |

### `application`

For applications where strict typing is desired but public API completeness is not critical.

| Setting | Value |
|---|---|
| `strict` | `true` |
| `strict_nulls` | `true` |
| `imports` | `"unknown"` |
| `no_implicit_dynamic` | `true` |
| `require_known_public_types` | `false` |

### `migration`

For gradual adoption in existing Python projects. It relaxes strictness, but it does not disable every safety-oriented check.

| Setting | Value |
|---|---|
| `strict` | `false` |
| `strict_nulls` | `true` |
| `imports` | `"dynamic"` |
| `no_implicit_dynamic` | `false` |
| `warn_unsafe` | `true` |
| `enable_sealed_exhaustiveness` | `true` |
| `report_deprecated` | `"ignore"` |
| `require_explicit_overrides` | `false` |
| `require_known_public_types` | `false` |
| `infer_passthrough` | `false` |
| `conditional_returns` | `false` |

## pyproject.toml Embedding

Instead of a standalone `typepython.toml`, you can embed configuration in `pyproject.toml`:

```toml
[tool.typepython.project]
src = ["src"]
target_python = "3.12"

[tool.typepython.typing]
profile = "application"
strict = true

[tool.typepython.emit]
emit_pyi = true
```

Use `typepython init --embed-pyproject` to append this layout to an existing `pyproject.toml` automatically.

## File Kinds and Authority

| Extension | Role | Treatment |
|---|---|---|
| `.tpy` | TypePython source | Parsed, type-checked, lowered to `.py`, stubs generated |
| `.py` | Python runtime authority | Copied to output; included in module graph for imports |
| `.pyi` | Type stub authority | Used for type checking; copied to output as-is |

### Module Path Collisions

These combinations within the same source roots cause a compile error:

- `pkg/foo.tpy` + `pkg/foo.py` (same module from two source kinds)
- `pkg/foo.tpy` + `pkg/foo.pyi` (source and stub for same module)
- `pkg/__init__.tpy` + `pkg/__init__.py`

This combination is **allowed** (standard Python stub companion pattern):

- `pkg/foo.py` + `pkg/foo.pyi`

## Output Structure

Given a project with `out_dir = ".typepython/build"` and `cache_dir = ".typepython/cache"`:

```
.typepython/
  build/
    app/
      __init__.py            # Lowered from __init__.tpy
      __init__.pyi           # Generated stub
      models.py              # Lowered from models.tpy
      models.pyi             # Generated stub
      utils.py               # Copied from utils.py
      py.typed               # PEP 561 marker
  cache/
    snapshot.json            # Incremental build state
```

## Environment Variables

| Variable | Purpose |
|---|---|
| `TYPEPYTHON_BIN` | Override the path to the TypePython CLI binary |

## Minimal Configuration Examples

### Library

```toml
[project]
src = ["src"]
target_python = "3.10"

[typing]
profile = "library"
```

### Application

```toml
[project]
src = ["src"]
target_python = "3.12"

[typing]
profile = "application"
```

### Migration (gradual adoption)

```toml
[project]
src = ["src"]
target_python = "3.10"

[typing]
profile = "migration"

[emit]
no_emit_on_error = false
```
