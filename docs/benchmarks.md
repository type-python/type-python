# Benchmarks

TypePython uses [Criterion.rs](https://github.com/bheisler/criterion.rs) for
micro-benchmarks. Results are stored under `target/criterion/`.

## Suites

### parse (`typepython_syntax`)

Measures end-to-end parsing of `.tpy` source text into the syntax tree.

| Benchmark                     | Input                                                                                               |
| ----------------------------- | --------------------------------------------------------------------------------------------------- |
| `parse_small_module`          | A short module with one function and one class                                                      |
| `parse_medium_module`         | 50 functions and 10 classes                                                                         |
| `parse_typepython_extensions` | TypePython-specific syntax: interfaces, data/sealed classes, type aliases, overloads, unsafe blocks |

### lower (`typepython_lowering`)

Measures lowering of a parsed syntax tree into the intermediate representation.

| Benchmark                  | Input                                                         |
| -------------------------- | ------------------------------------------------------------- |
| `lower_small_module`       | Small module with a type alias and a function                 |
| `lower_medium_module`      | Interfaces, data classes, sealed classes, overloads, generics |
| `lower_python_passthrough` | 30 plain-Python functions (no TypePython extensions)          |

### graph (`typepython_graph`)

Measures construction of the module dependency graph from binding tables.

| Benchmark                    | Input                                 |
| ---------------------------- | ------------------------------------- |
| `build_10_module_graph`      | 10 modules in a single package        |
| `build_50_module_graph`      | 50 modules in a single package        |
| `build_nested_package_graph` | 20 modules across nested sub-packages |

### checker (`typepython_checking`)

Measures the semantic checker's cache-backed declaration semantics plus
solver-backed direct-call path using an in-memory module graph with imported
generic calls, TypeVarTuple expansion, and generic overload specificity.

Checked-in baseline evidence for the current checker suite lives in
[`docs/benchmarks-checker-baseline.md`](./benchmarks-checker-baseline.md).

| Benchmark                                   | Input                                                                                              |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `check_solver_direct_calls_small`           | 8 repetitions of imported generic calls, variadic tuple collection, and generic overload selection |
| `check_solver_direct_calls_medium`          | 64 repetitions of the same semantic-solver/direct-call mix                                         |
| `check_semantic_incremental_summary_medium` | Semantic summary snapshot generation over the 64-repetition checker graph                          |

### incremental (`typepython_lsp`)

Measures end-to-end LSP edit sessions over 48-module and 512-module workspaces
using the stdio JSON-RPC server path.

| Benchmark                                        | Input                                                   |
| ------------------------------------------------ | ------------------------------------------------------- |
| `lsp_incremental_impl_edit_session_48_modules`   | An implementation-only edit followed by a hover request |
| `lsp_incremental_public_edit_session_48_modules` | A public-signature edit followed by a hover request     |
| `lsp_incremental_impl_edit_session_512_modules`  | Same session shape over a larger workspace              |
| `lsp_incremental_public_edit_session_512_modules` | Same session shape over a larger workspace             |

The checked-in end-to-end incremental benchmark currently targets the longest-lived
incremental session path (`typepython_lsp`). One-shot CLI commands now use the same
affected-module invalidation rules for selective check/lower/emit, and that behavior
is covered by the CLI pipeline test suite rather than a separate Criterion target.

### industrial smoke (`scripts/industrial_perf_smoke.py`)

Generates a synthetic large workspace and measures full CLI pipeline behavior
outside Criterion. The fixture includes a chain of `.tpy` modules, a configured
external `typestubs` root, implicit namespace-package stubs, a partial stub
package, and a deterministic Python probe so local `site-packages` does not
pollute the run.

The smoke records:

- cold check time
- warm check time
- single-file implementation edit recheck time
- public surface edit recheck time
- peak RSS when `/usr/bin/time` is available

The script writes machine-readable JSON when `--json-out` is provided. Use this
for large-workspace release evidence; do not treat the 48-module LSP session or
the checker micro-benchmark as an industrial-scale proof by itself.

## Running benchmarks

Run the core benchmark suites tracked by the Makefile:

```sh
cargo bench --workspace --bench parse --bench lower --bench graph --bench checker
```

Run the LSP incremental suite separately:

```sh
cargo bench -p typepython-lsp --bench incremental
```

Run the industrial CLI smoke with the default 512-module, 128-external-stub
fixture:

```sh
make perf-smoke
```

Record a larger v1 release-candidate sample:

```sh
python scripts/industrial_perf_smoke.py \
  --modules 1000 \
  --external-stubs 500 \
  --target-python 3.13 \
  --json-out perf/industrial-1000-3.13.json
```

For LSP latency claims, record the 512-module implementation-edit and
public-surface-edit Criterion runs and calculate p95/p99 hover-session latency
evidence from Criterion's per-iteration samples:

```sh
make lsp-latency-evidence
```

The target requires at least 20 one-session Flat samples for both 512-module
session shapes and writes `perf/lsp-latency.json`. CI uploads that JSON together
with both Criterion HTML reports; a missing, malformed, batched, or undersized
sample set fails the release gate.

Compile-check benchmarks without running them:

```sh
cargo bench --workspace --no-run
```

`cargo bench --workspace --no-run` compiles every benchmark target in the
workspace. CI additionally runs the 512-module LSP benchmarks through
`make lsp-latency-evidence`; compile success alone is not performance evidence.

## Baselines

### Compare against the saved baseline

```sh
cargo bench --workspace --bench parse --bench lower --bench graph --bench checker -- --baseline v0.1.0
```

Criterion will print a comparison showing whether each benchmark regressed,
improved, or stayed within noise.

### Save a new baseline

After intentional performance changes, update the stored baseline:

```sh
cargo bench --workspace --bench parse --bench lower --bench graph --bench checker -- --save-baseline v0.1.0
```

The checked-in baseline flow currently applies to the core parse/lower/graph/checker
suites. The LSP incremental benchmark is intentionally documented separately.

### Makefile targets

| Target                | Description                                           |
| --------------------- | ----------------------------------------------------- |
| `make bench`          | Run the core parse/lower/graph/checker suites         |
| `make bench-check`    | Compile all workspace benchmarks without running them |
| `make bench-baseline` | Save the v0.1.0 baseline for the core suites          |
| `make bench-compare`  | Compare the core suites against the v0.1.0 baseline   |
| `make perf-smoke`     | Run the industrial CLI cold/warm/edit smoke           |
| `make lsp-latency-evidence` | Run 512-module LSP sessions and emit p95/p99 JSON |
