# Checker Benchmark Baseline

This file records the current checked-in baseline evidence for the
`typepython_checking` Criterion suite.

## Command

```sh
cargo bench -p typepython-checking --bench checker
```

## Context

- Bench harness: `crates/typepython_checking/benches/checker.rs`
- Backend: Criterion with plotters fallback (`gnuplot` unavailable locally)
- Graph shape:
  - imported generic direct calls via imported symbols
  - TypeVarTuple direct-call inference
  - generic overload specificity selection
- Inputs:
  - `check_solver_direct_calls_small`: 8 repetitions of the solver/direct-call mix
  - `check_solver_direct_calls_medium`: 64 repetitions of the same mix
  - `check_semantic_incremental_summary_medium`: semantic summary snapshot generation over the 64-repetition graph

## Measured Results

Recorded from a successful local run on Tue Apr 07 2026.

| Benchmark                                   | Measured time range      |
| ------------------------------------------- | ------------------------ |
| `check_solver_direct_calls_small`           | `388.77 µs .. 390.61 µs` |
| `check_solver_direct_calls_medium`          | `3.1233 ms .. 3.1894 ms` |
| `check_semantic_incremental_summary_medium` | `15.552 µs .. 15.805 µs` |

Criterion also reported these runs as regressions relative to the currently
saved local Criterion baseline, but the purpose of this checked-in file is to
capture a repository-tracked reference point for the current semantic checker
architecture rather than a local `target/criterion` comparison snapshot.
