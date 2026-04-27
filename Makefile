CARGO ?= cargo
MSRV ?= 1.94.0
PYTHON ?= python3
RUSTDOCFLAGS ?= -D warnings

.PHONY: bootstrap fmt fmt-check check msrv-check lint test test-fast test-cli-verification test-downstream-checkers coverage stdlib-baseline-check conformance-check repo-contracts bench bench-check bench-baseline bench-compare package-check snapshot-review docs ci bump-version

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

coverage:
	mkdir -p coverage
	$(CARGO) llvm-cov clean --workspace
	$(CARGO) llvm-cov --workspace --all-features --no-report
	$(CARGO) llvm-cov report --workspace --all-features --lcov --output-path coverage/lcov.info
	$(CARGO) llvm-cov report --workspace --all-features --text --output-path coverage/coverage.txt
	$(CARGO) llvm-cov report --workspace --all-features --html

stdlib-baseline-check:
	$(PYTHON) scripts/refresh_stdlib_stubs.py --check

conformance-check:
	$(PYTHON) scripts/conformance_report.py --check

repo-contracts:
	$(PYTHON) -m unittest scripts/test_repo_contracts.py

bench:
	$(CARGO) bench --workspace --bench parse --bench lower --bench graph --bench checker

bench-check:
	$(CARGO) bench --workspace --no-run

package-check:
	rm -rf dist
	$(PYTHON) -m build --sdist --wheel
	$(PYTHON) -m twine check dist/*

bump-version:
	@test -n "$(VERSION)" || (echo "Usage: make bump-version VERSION=0.0.8" && exit 1)
	$(PYTHON) scripts/bump_version.py $(VERSION)

bench-baseline:
	$(CARGO) bench --workspace --bench parse --bench lower --bench graph --bench checker -- --save-baseline v0.1.0

bench-compare:
	$(CARGO) bench --workspace --bench parse --bench lower --bench graph --bench checker -- --baseline v0.1.0

snapshot-review:
	$(CARGO) insta review

docs:
	RUSTDOCFLAGS="$(RUSTDOCFLAGS)" $(CARGO) doc --workspace --no-deps

ci: fmt-check lint test-fast test-cli-verification test-downstream-checkers stdlib-baseline-check conformance-check repo-contracts bench-check package-check
