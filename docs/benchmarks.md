# Benchmarks

TypePython uses [Criterion.rs](https://github.com/bheisler/criterion.rs) for
micro-benchmarks. Results are stored under `target/criterion/`.

## Suites

### parse (`typepython_syntax`)

Measures end-to-end parsing of `.tpy` source text into the syntax tree.

| Benchmark | Input |
| ----------------------------- | --------------------------------------------------------------------------------------------------- |
| `parse_small_module` | A short module with one function and one class |
| `parse_medium_module` | 50 functions and 10 classes |
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

| Benchmark                          | Input                                                                                              |
| ---------------------------------- | -------------------------------------------------------------------------------------------------- |
| `check_solver_direct_calls_small`  | 8 repetitions of imported generic calls, variadic tuple collection, and generic overload selection |
| `check_solver_direct_calls_medium` | 64 repetitions of the same semantic-solver/direct-call mix                                         |
| `check_semantic_incremental_summary_medium` | Semantic summary snapshot generation over the 64-repetition checker graph                         |

### incremental (`typepython_lsp`)

Measures end-to-end LSP edit sessions over a 48-module workspace using the
stdio JSON-RPC server path.

| Benchmark                                 | Input                                                                |
| ----------------------------------------- | -------------------------------------------------------------------- |
| `lsp_incremental_impl_edit_session_48_modules` | An implementation-only edit followed by a hover request            |
| `lsp_incremental_public_edit_session_48_modules` | A public-signature edit followed by a hover request               |

## Running benchmarks

Run the core benchmark suites tracked by the Makefile:

```sh
cargo bench --workspace --bench parse --bench lower --bench graph --bench checker
```

Run the LSP incremental suite separately:

```sh
cargo bench -p typepython-lsp --bench incremental
```

Compile-check benchmarks without running them (used in CI):

```sh
cargo bench --workspace --no-run
```

`cargo bench --workspace --no-run` compiles every benchmark target in the
workspace, including the LSP incremental bench.

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

| Target                | Description                                                     |
| --------------------- | --------------------------------------------------------------- |
| `make bench`          | Run the core parse/lower/graph/checker suites                   |
| `make bench-check`    | Compile all workspace benchmarks without running them           |
| `make bench-baseline` | Save the v0.1.0 baseline for the core suites                    |
| `make bench-compare`  | Compare the core suites against the v0.1.0 baseline             |
