# CLI Reference

The `typepython` command-line tool provides all TypePython compiler and tooling operations.

## Global Options

```
typepython [COMMAND] [OPTIONS]
```

Project-oriented commands use these shared options:

- `check`, `build`, `watch`, `verify`, and `migrate` accept `--project PATH` and `--format text|json`
- `clean` accepts `--project PATH`
- `lsp` accepts `--project PATH` and speaks JSON-RPC over stdio instead of CLI JSON output
- `init` has its own command-specific flags

## Commands

### `typepython init`

Create a new TypePython project with starter configuration and source files.

```bash
typepython init [OPTIONS]
```

| Flag                | Description                                                                                     |
| ------------------- | ----------------------------------------------------------------------------------------------- |
| `--dir PATH`        | Target directory (default: current directory)                                                   |
| `--force`           | Overwrite existing files                                                                        |
| `--embed-pyproject` | Append `[tool.typepython]` to an existing `pyproject.toml` instead of writing `typepython.toml` |

**Generated files:**

```
<dir>/
  typepython.toml          # or [tool.typepython] in pyproject.toml
  src/
    app/
      __init__.tpy          # Starter source: def greet(name: str) -> str
```

`--embed-pyproject` keeps the same `src/app/__init__.tpy` starter file, but it requires an existing `pyproject.toml` and fails if `typepython.toml` already exists or `pyproject.toml` already defines `[tool.typepython]`.

**Example:**

```bash
typepython init --dir my-project
```

---

### `typepython check`

Type-check the project without emitting output files. Use this for fast feedback during development.

```bash
typepython check [OPTIONS]
```

| Flag              | Description                     |
| ----------------- | ------------------------------- |
| `--project PATH`  | Project directory               |
| `--format FORMAT` | Output format: `text` or `json` |

**Pipeline steps:** discover sources -> parse -> bind -> build graph -> type check

**Text output:**

```
check:
  config: examples/hello-world/typepython.toml (typepython.toml)
  discovered sources: 1
  lowered modules: 0
  planned artifacts: 1
  tracked modules: 4
  note: compiler pipeline, artifact planning, and verification completed for the loaded project
```

**JSON output:**

```json
{
  "diagnostics": {
    "diagnostics": []
  },
  "summary": {
    "command": "check",
    "config_path": "examples/hello-world/typepython.toml",
    "config_source": "type_python_toml",
    "discovered_sources": 1,
    "lowered_modules": 0,
    "notes": [
      "compiler pipeline, artifact planning, and verification completed for the loaded project"
    ],
    "planned_artifacts": 1,
    "tracked_modules": 4
  }
}
```

**Example:**

```bash
typepython check --project . --format json
```

---

### `typepython build`

Full compilation: type-check, lower to Python, emit `.py` and `.pyi` files, update incremental cache.

```bash
typepython build [OPTIONS]
```

| Flag              | Description                     |
| ----------------- | ------------------------------- |
| `--project PATH`  | Project directory               |
| `--format FORMAT` | Output format: `text` or `json` |

**Pipeline steps:** discover -> parse -> bind -> graph -> check -> lower -> plan emits -> snapshot -> write outputs

When a previous snapshot is available, the CLI reuses unchanged semantic summaries based on persisted source hashes before deciding whether full output reuse is safe.

**Output artifacts:**

- `.py` files -- lowered Python in `out_dir`
- `.pyi` files -- generated type stubs (if `emit.emit_pyi = true`)
- `py.typed` -- PEP 561 marker files (if `emit.write_py_typed = true`)
- `snapshot.json` -- incremental state in `cache_dir`
- `.pyc` files -- compiled bytecode (if `emit.emit_pyc = true`)

**Text output:**

```
build:
  config: /path/to/project/typepython.toml (typepython.toml)
  discovered sources: 8
  lowered modules: 8
  planned artifacts: 8
  tracked modules: 12
  note: wrote 8 runtime artifact(s), 8 stub artifact(s), 1 `py.typed` marker(s)
  note: cached 12 module fingerprint(s) at /path/to/project/.typepython/cache/snapshot.json
```

**Blocked by errors:** When `emit.no_emit_on_error = true` (default), type-checking and public-surface errors suppress writing and add `TPY5002`. Discovery, parse, and lowering errors block emission regardless of this setting.

**Example:**

```bash
typepython build --project .
```

---

### `typepython watch`

File-watching mode: rebuild automatically when source files change.

```bash
typepython watch [OPTIONS]
```

| Flag              | Description                     |
| ----------------- | ------------------------------- |
| `--project PATH`  | Project directory               |
| `--format FORMAT` | Output format: `text` or `json` |

**Behavior:**

- Performs an initial full build
- Watches all `src` directories for changes
- Debounces filesystem events (configurable via `watch.debounce_ms`, default: 80ms)
- Rebuilds the full pipeline on each change
- Reports diagnostics after each rebuild

**Example:**

```bash
typepython watch --project .
```

Press `Ctrl+C` to stop.

---

### `typepython clean`

Remove build output and cache directories.

```bash
typepython clean [OPTIONS]
```

| Flag             | Description       |
| ---------------- | ----------------- |
| `--project PATH` | Project directory |

**Removes:**

- `out_dir` (default: `.typepython/build/`)
- `cache_dir` (default: `.typepython/cache/`)

**Example:**

```bash
typepython clean --project .
```

---

### `typepython lsp`

Start the Language Server Protocol server for editor integration.

```bash
typepython lsp [OPTIONS]
```

| Flag             | Description       |
| ---------------- | ----------------- |
| `--project PATH` | Project directory |

**Transport:** stdio-based JSON-RPC 2.0

`typepython lsp` reuses the standard run-args parser, but `--format json` is rejected because the command already speaks JSON-RPC over stdio.

**Supported LSP methods:**

- `textDocument/hover` -- type information at cursor
- `textDocument/definition` -- jump to definition
- `textDocument/references` -- find all usages
- `textDocument/rename` -- rename symbol across project
- `textDocument/codeAction` -- quick fixes
- `textDocument/completion` -- autocomplete (triggered on `.`)
- `textDocument/didOpen|didChange|didClose` -- document synchronization

See [LSP Integration](lsp.md) for editor setup.

**Example:**

```bash
typepython lsp --project .
```

---

### `typepython verify`

Validate build artifacts for publication. Checks consistency between runtime and type surfaces, and inspects wheel/sdist packages.

```bash
typepython verify [OPTIONS]
```

| Flag                | Description                                                                |
| ------------------- | -------------------------------------------------------------------------- |
| `--project PATH`    | Project directory                                                          |
| `--format FORMAT`   | Output format: `text` or `json`                                            |
| `--wheel PATH`      | Path to a `.whl` file to verify (repeatable)                               |
| `--sdist PATH`      | Path to a `.tar.gz` sdist to verify (repeatable)                           |
| `--checker COMMAND` | Run an external type checker against the emitted build output (repeatable) |

**Checks performed:**

- Public API completeness: all exported names have known types (when `typing.require_known_public_types = true`)
- Runtime/type surface consistency: names in `.py` match names in `.pyi`
- Wheel/sdist structure validation
- `py.typed` marker presence

**Example:**

```bash
# Verify the project build
typepython verify --project .

# Verify a built wheel
typepython verify --project . --wheel dist/my_package-1.0.0-py3-none-any.whl
```

---

### `typepython migrate`

Analyze an existing Python project for migration to TypePython.

```bash
typepython migrate [OPTIONS]
```

| Flag                  | Description                                     |
| --------------------- | ----------------------------------------------- |
| `--format FORMAT`     | Output format: `text` or `json`                 |
| `--project PATH`      | Project directory                               |
| `--report`            | Print a typing coverage summary                 |
| `--emit-stubs PATH`   | Generate `.pyi` stubs from inferred `.py` types |
| `--stub-out-dir PATH` | Output directory for generated stubs            |

**Report mode** (`--report`):

- Reports declaration coverage and dynamic/unknown boundary counts
- Includes per-file and per-directory coverage entries
- Identifies high-impact files with many untyped declarations

**Stub emission** (`--emit-stubs`):

- Generates `.pyi` files with inferred types from `.py` sources
- Includes `TODO` markers for types that could not be inferred
- Useful as a starting point for gradual typing

**Example:**

```bash
# Get a migration report
typepython migrate --project . --report

# Generate starter stubs
typepython migrate --project . --emit-stubs src/ --stub-out-dir stubs/
```

## Exit Codes

| Code | Meaning                                           |
| ---- | ------------------------------------------------- |
| `0`  | Success (no errors)                               |
| `1`  | Diagnostic errors or configuration/setup failures |
| `2`  | Other tool/runtime failures                       |

## Output Formats

### Text format (default)

Human-readable output with colored diagnostics (when connected to a terminal):

```
path/to/file.tpy:LINE:COL  CODE  SEVERITY  Message text
```

### JSON format

Machine-readable structured output suitable for CI pipelines and editor integration:

```json
{
  "diagnostics": {
    "diagnostics": [
      {
        "code": "TPY4001",
        "severity": "error",
        "message": "...",
        "notes": [],
        "suggestions": [
          {
            "message": "...",
            "span": { "line": 5, "column": 10, "end_line": 5, "end_column": 13 },
            "replacement": "int | None"
          }
        ],
        "span": {
          "path": "src/app/models.tpy",
          "line": 5,
          "column": 10,
          "end_line": 5,
          "end_column": 20
        }
      }
    ]
  },
  "summary": { ... }
}
```

## Usage with cargo (development)

During development, run commands via cargo without installing the Python package:

```bash
# Equivalent to: typepython check --project examples/hello-world
cargo run -p typepython-cli -- check --project examples/hello-world

# Release build for speed
cargo run --release -p typepython-cli -- build --project .
```
