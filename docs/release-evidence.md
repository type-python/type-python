# Release Evidence

This page records concrete release-gate runs that can be audited after the fact.
The GitHub `rust` workflow remains the authoritative cross-platform publish gate;
local evidence is a maintainer preflight, not a replacement for the CI matrix.

## 2026-05-08 macOS Local RC Preflight

Environment:

- Host: macOS 15.7.5 (24G624), Apple Silicon
- Rust stable: `cargo 1.94.0 (85eff7c80 2026-01-15)`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`
- Rust nightly for fuzzing: `cargo 1.97.0-nightly (eb9b60f1f 2026-04-24)`
- `cargo-fuzz 0.13.1`
- Host Python: `Python 3.9.6`
- Native-target smoke interpreters: `CPython 3.13.7` and `CPython 3.14.0rc2` from `uv`
- Downstream checkers: `mypy 1.19.1`, `pyright 1.1.409`, `basedpyright 1.39.3`, `ty 0.0.32`
- Packaging tools: `build 1.4.4`, `twine 6.2.0`

Passed commands:

| Gate | Result | Notes |
| --- | --- | --- |
| `cargo fmt --all --check` | PASS | Full workspace format check. |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | Full workspace lint gate. |
| `cargo test --workspace` | PASS | Full Rust workspace test suite. |
| `cargo test -p typepython-cli tests::verification::` | PASS | 89 verification tests. |
| `python3 scripts/conformance_report.py --check` | PASS | Conformance report is current. |
| `python3 scripts/diagnostic_test_coverage.py --check` | PASS | Diagnostic coverage report is current. |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | PASS | 74 Python repo-script tests. |
| `PATH="/tmp/typepython-py313-smoke/bin:/Users/Michael.Lin/Library/Python/3.9/bin:/Users/Michael.Lin/.local/bin:$PATH" python3 scripts/downstream_checker_smoke.py` | PASS | Full fixture matrix with mypy strict, pyright strict, basedpyright strict, and ty strict. `mypy` ran under Python 3.13 so 3.13+ syntax fixtures were parsed by a compatible interpreter. |
| `python3 scripts/research_roadmap_demo_smoke.py` | PASS | P0-P4 roadmap example checked and built with portable output. |
| `make quickstart-smoke` | PASS | Rebuilt sdist/wheel, ran `twine check dist/*`, installed wheel into a clean venv, then ran `typepython --help`, `init`, `check`, `build`, and `verify`. |
| `TYPEPYTHON_BIN="$PWD/target/release/typepython" uv run --no-project --python 3.13 python scripts/native_target_smoke.py --target-python 3.13` | PASS | Native Python 3.13 target smoke. |
| `PYTHONPATH="$PWD" TYPEPYTHON_BIN="$PWD/target/release/typepython" uv run --no-project --python 3.14 python scripts/native_target_smoke.py --target-python 3.14` | PASS | Native Python 3.14 target smoke using `CPython 3.14.0rc2`, the only 3.14 runtime listed by the installed `uv`. |
| `python3 scripts/refresh_stdlib_stubs.py --check` | PASS | `stdlib_sha256=927023866400ef3cc991521fa89f827fc16132cfef3d3f48530e5780cdb4587c`, `typeshed_commit=68517355a3269be407bde20fea8fd66af2dc4241`. |
| `python3 scripts/industrial_perf_smoke.py --json-out /tmp/typepython-industrial-perf.json` | PASS | 512 modules, 128 external stub packages, Python 3.12 target. |
| `make fuzz-smoke` | PASS | `parser`, `type_expr`, and `lowering_stub`, 30 seconds each. Generated corpus byproducts were discarded. |
| `cargo bench --workspace --no-run` | PASS | Bench binaries compile for the workspace. |

Industrial performance smoke:

| Step | Time | Peak RSS |
| --- | ---: | ---: |
| cold check | 1.413s | 69.2 MiB |
| warm check | 0.773s | 84.2 MiB |
| single-file implementation edit | 0.776s | 82.1 MiB |
| public surface edit | 1.153s | 88.2 MiB |

CI-only follow-up for a publishable release:

- Run the GitHub `rust` workflow on the release commit and retain the successful
  `beta-release-gate` job URL or artifact.
- Confirm Linux, macOS, and Windows platform smoke in CI.
- Confirm GitHub-hosted Python 3.13 and final Python 3.14 target smoke, because
  this local preflight used `uv`'s available `CPython 3.14.0rc2` runtime.
- Retain CI-uploaded industrial performance and bench artifacts with the release notes.
