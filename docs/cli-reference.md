# CLI Reference

The `typepython` command-line tool provides all TypePython compiler and tooling operations.

## Global Options

```
typepython [COMMAND] [OPTIONS]
```

Project-oriented commands use these shared options:

- `check`, `build`, `watch`, `verify`, `compat`, and `migrate` accept `--project PATH` and `--format text|json`
- `api-diff` accepts two artifact paths and `--format text|json`
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

**Pipeline steps:** discover sources -> parse -> bind -> build graph -> dependency-driven selective type check -> update semantic cache

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

`typepython check` persists the current semantic snapshot and module-diagnostic cache under `cache_dir`, but it does not materialize runtime artifacts.

### `typepython build`

Full compilation: type-check, lower to Python, emit `.py` and `.pyi` files, update incremental cache.

```bash
typepython build [OPTIONS]
```

| Flag              | Description                     |
| ----------------- | ------------------------------- |
| `--project PATH`  | Project directory               |
| `--format FORMAT` | Output format: `text` or `json` |

**Pipeline steps:** discover -> parse -> bind -> graph -> selective check -> selective lower -> selective emit planning -> snapshot/cache update -> write affected outputs

When previous cache state is available, the CLI reuses unchanged semantic summaries and unaffected module diagnostics, rechecks only changed modules plus dependents whose public summaries changed, and lowers/rematerializes only the project modules that need refreshed outputs. A materialized-build manifest keeps output reuse and stale-artifact cleanup tied to the last emitted build tree instead of only the semantic snapshot.

**Output artifacts:**

- `.py` files -- lowered Python in `out_dir`
- `.pyi` files -- generated type stubs (if `emit.emit_pyi = true`)
- `py.typed` -- PEP 561 marker files (if `emit.write_py_typed = true`)
- `snapshot.json` -- semantic incremental state in `cache_dir`
- `analysis-cache.json` -- cached per-module CLI diagnostics in `cache_dir`
- `build-manifest.json` -- last materialized build-output manifest in `cache_dir`
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
| `--checker-preset PRESET` | Run a named checker preset; `all` expands to `mypy`, `pyright`, and `ty` |
| `--checker-allowlist PATH` | TOML allowlist of known checker disagreements that should remain visible but non-blocking |
| `--unsafe-runtime-imports` | Import emitted runtime modules to compare runtime-visible public names; this executes project-controlled Python code |

**Checks performed:**

- Public API completeness: all exported names have known types (when `typing.require_known_public_types = true`)
- Runtime/type surface consistency: names in `.py` match names in `.pyi`
- Wheel/sdist structure validation
- `py.typed` marker presence
- Packaging metadata consistency: `Requires-Python` and `typing_extensions` declarations keep pace with emitted native/backport requirements

By default, `verify` stays on structural checks and does not import emitted runtime modules. In that safe mode, TypePython may ignore a project-controlled `resolution.python_executable` and fall back to the host default interpreter for structural helper probes and interpreter-backed package discovery. Pass `--unsafe-runtime-imports` if you also want runtime-visible public-name parity checks for cases like dynamically computed `__all__` and verification against the configured interpreter environment.

**Example:**

```bash
# Verify the project build
typepython verify --project .

# Verify a built wheel
typepython verify --project . --wheel dist/my_package-1.0.0-py3-none-any.whl

# Also import emitted runtime modules for public-name parity checks
typepython verify --project . --unsafe-runtime-imports

# Run the stable downstream checker matrix during publication verification
typepython verify --project . --checker-preset all
```

Checker allowlists use TOML and are intended for temporary, reviewable checker disagreements:

```toml
[[disagreements]]
checker = "pyright"
contains = "message substring emitted by the checker"
reason = "known checker limitation around generated shape metadata"
issue = "https://github.com/example/project/issues/123"
expires = "2026-12-31"
```

---

### `typepython compat`

Validate emitted artifacts across downstream Python type checkers. This command builds the project, runs the same structural artifact checks as `verify`, then invokes the configured checker matrix against the generated build tree.

```bash
typepython compat [OPTIONS]
```

| Flag                     | Description                                                                                 |
| ------------------------ | ------------------------------------------------------------------------------------------- |
| `--project PATH`         | Project directory                                                                           |
| `--format FORMAT`        | Output format: `text` or `json`                                                             |
| `--checkers LIST`        | Comma-separated checker list; default `all` expands to `mypy,pyright,ty`                    |
| `--profile NAME`         | Named checker profile: `library-portable`, `app-strict`, `pyright-first`, `mypy-compatible`, or `experimental-checkers` |
| `--strict-portability`   | Keep portability failures build-blocking for every configured checker rejection              |
| `--checker-allowlist PATH` | TOML allowlist of known checker disagreements that should remain visible but non-blocking   |

Supported checker names use checker-specific CLI conventions:

- `mypy` -> `mypy --python-version <target> <build-dir>`
- `pyright` -> `pyright --pythonversion <target> <build-dir>`
- `ty` -> `ty check --no-progress --python-version <target> <build-dir>`
- optional local tools: `pyrefly`, `basedpyright`, and `zuban`

Unknown checker values are treated as custom command paths and receive the generated build directory as their only argument.

When external checkers run, the summary includes a `type portability score` note. The score is the percentage of configured checker invocations that did not produce build-blocking checker diagnostics; allowlisted disagreements remain visible as warnings but do not reduce the score.

Before invoking external checkers, `verify` and `compat` also scan emitted `.pyi` files for known portability risks such as native `type` statements, inline generic headers, defaulted type parameters, and `typing.ReadOnly` / `typing.TypeIs` forms used before their target Python version supports them. These diagnostics include suggested rewrites, usually switching to compat emit output or `typing_extensions` imports.

**Example:**

```bash
# Run the default stable portability matrix
typepython compat --project .

# Run one checker while iterating locally
typepython compat --project . --checkers pyright

# Use a named checker profile
typepython compat --project . --profile library-portable
```

---

### `typepython api-diff`

Compare two public typing surfaces and report conservative API drift. The first prototype accepts `.pyi` files or directories containing `.pyi` files; wheel, sdist, and source-directory normalization are reserved for the next implementation step.

```bash
typepython api-diff <old> <new> [OPTIONS]
```

| Flag              | Description                     |
| ----------------- | ------------------------------- |
| `--format FORMAT` | Output format: `text` or `json` |

The report classifies:

- removed public symbols as `likely type-breaking`
- changed public signatures as `unknown risk`
- added public symbols as `source-compatible`

**Example:**

```bash
typepython api-diff dist/old-stubs dist/new-stubs --format json
```

---

### `typepython type-health`

Inspect configured dependency/type roots for PEP 561 typing metadata and produce a typing-supply-chain score. The first prototype scans `resolution.type_roots`, detects `py.typed`, `*-stubs` packages, partial stub markers, and can write `.typepython/type-lock.toml` for review.

```bash
typepython type-health [OPTIONS]
```

| Flag                 | Description                                           |
| -------------------- | ----------------------------------------------------- |
| `--project PATH`     | Project directory                                     |
| `--format FORMAT`    | Output format: `text` or `json`                       |
| `--fail-under SCORE` | Fail if the type-health score is below `SCORE`        |
| `--write-lock`       | Write `.typepython/type-lock.toml` with observed data |

**Example:**

```bash
typepython type-health --project . --fail-under 80 --write-lock
```

---

### `typepython migrate`

Analyze an existing Python project for migration to TypePython.

```bash
typepython migrate [OPTIONS]
```

| Flag                     | Description                                           |
| ------------------------ | ----------------------------------------------------- |
| `--format FORMAT`        | Output format: `text` or `json`                       |
| `--project PATH`         | Project directory                                     |
| `--report`               | Print a typing coverage summary                       |
| `--baseline PATH`        | Compare diagnostics against a saved migration baseline |
| `--write-baseline PATH`  | Write the current diagnostic baseline as JSON          |
| `--no-new-diagnostics`   | Fail when diagnostics appear outside the baseline      |
| `--emit-stubs PATH`      | Generate `.pyi` stubs from inferred `.py` types       |
| `--stub-out-dir PATH`    | Output directory for generated stubs                  |

**Report mode** (`--report`):

- Reports declaration coverage and dynamic/unknown boundary counts
- Includes per-file and per-directory coverage entries
- Identifies high-impact files with many untyped declarations
- Flags framework-heavy files, such as Pydantic/FastAPI/Django/SQLAlchemy/Celery/Click/Typer patterns, that may benefit from transform declarations
- Can compare current diagnostics against a JSON baseline so CI can enforce no new type debt

**Diagnostic baselines** (`--baseline`, `--write-baseline`, `--no-new-diagnostics`):

- `--write-baseline .typepython/migration-baseline.json` records the current diagnostic set
- `--baseline .typepython/migration-baseline.json` reports new and resolved diagnostics
- `--baseline ... --no-new-diagnostics` exits with a diagnostic error if new diagnostics appear

**Stub emission** (`--emit-stubs`):

- Generates `.pyi` files with inferred types from `.py` sources
- Includes `TODO` markers for types that could not be inferred
- Useful as a starting point for gradual typing

**Example:**

```bash
# Get a migration report
typepython migrate --project . --report

# Establish and enforce a diagnostic baseline
typepython migrate --project . --write-baseline .typepython/migration-baseline.json
typepython migrate --project . --baseline .typepython/migration-baseline.json --no-new-diagnostics

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
