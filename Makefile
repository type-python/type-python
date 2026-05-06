CARGO ?= cargo
MSRV ?= 1.94.0
PYTHON ?= python3
RUSTDOCFLAGS ?= -D warnings
FUZZ_TARGETS ?= parser type_expr lowering_stub
FUZZ_SMOKE_SECONDS ?= 30
FUZZ_LONG_SECONDS ?= 300
COVERAGE_MIN_LINES ?= 20

.PHONY: bootstrap fmt fmt-check check msrv-check lint test test-fast test-cli-verification test-downstream-checkers roadmap-demo-smoke coverage fuzz-smoke fuzz-long stdlib-baseline-check conformance-check diagnostic-coverage-check repo-contracts bench bench-check bench-baseline bench-compare perf-smoke package-check beta-release-gate snapshot-review docs ci bump-version

bootstrap:
	./scripts/bootstrap-rust.sh

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all --check

check:
	$(CARGO) check --workspace

msrv-check:
	rustup toolchain install $(MSRV) --profile minimal
	$(CARGO) +$(MSRV) check --workspace

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

test-fast:
	$(CARGO) test --workspace -- --skip tests::verification::

test-cli-verification:
	$(CARGO) test -p typepython-cli tests::verification::

test-downstream-checkers:
	$(PYTHON) scripts/downstream_checker_smoke.py

roadmap-demo-smoke:
	$(PYTHON) scripts/research_roadmap_demo_smoke.py

coverage:
	mkdir -p coverage
	$(CARGO) llvm-cov clean --workspace
	$(CARGO) llvm-cov --workspace --all-features --no-report
	$(CARGO) llvm-cov report --lcov --output-path coverage/lcov.info
	$(CARGO) llvm-cov report --text --output-path coverage/coverage.txt --fail-under-lines $(COVERAGE_MIN_LINES)
	$(CARGO) llvm-cov report --html

fuzz-smoke:
	for target in $(FUZZ_TARGETS); do $(CARGO) +nightly fuzz run $$target -- -max_total_time=$(FUZZ_SMOKE_SECONDS); done

fuzz-long:
	for target in $(FUZZ_TARGETS); do $(CARGO) +nightly fuzz run $$target -- -max_total_time=$(FUZZ_LONG_SECONDS); done

stdlib-baseline-check:
	$(PYTHON) scripts/refresh_stdlib_stubs.py --check

conformance-check:
	$(PYTHON) scripts/conformance_report.py --check

diagnostic-coverage-check:
	$(PYTHON) scripts/diagnostic_test_coverage.py --check

repo-contracts:
	$(PYTHON) -m unittest scripts/test_repo_contracts.py scripts/test_downstream_checker_matrix.py scripts/test_research_roadmap_demo_smoke.py scripts/test_industrial_perf_smoke.py scripts/test_editor_integrations.py scripts/test_packaging_contracts.py

bench:
	$(CARGO) bench --workspace --bench parse --bench lower --bench graph --bench checker

bench-check:
	$(CARGO) bench --workspace --no-run

package-check:
	rm -rf dist
	$(PYTHON) -m build --sdist --wheel
	$(PYTHON) -m twine check dist/*

beta-release-gate: fmt-check lint test test-cli-verification test-downstream-checkers roadmap-demo-smoke perf-smoke fuzz-smoke package-check stdlib-baseline-check conformance-check diagnostic-coverage-check repo-contracts

bump-version:
	@test -n "$(VERSION)" || (echo "Usage: make bump-version VERSION=0.0.8" && exit 1)
	$(PYTHON) scripts/bump_version.py $(VERSION)

bench-baseline:
	$(CARGO) bench --workspace --bench parse --bench lower --bench graph --bench checker -- --save-baseline v0.1.0

bench-compare:
	$(CARGO) bench --workspace --bench parse --bench lower --bench graph --bench checker -- --baseline v0.1.0

perf-smoke:
	$(PYTHON) scripts/industrial_perf_smoke.py

snapshot-review:
	$(CARGO) insta review

docs:
	RUSTDOCFLAGS="$(RUSTDOCFLAGS)" $(CARGO) doc --workspace --no-deps

ci: fmt-check lint test-fast test-cli-verification test-downstream-checkers roadmap-demo-smoke stdlib-baseline-check conformance-check diagnostic-coverage-check repo-contracts bench-check package-check
