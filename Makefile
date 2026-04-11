CARGO ?= cargo
PYTHON ?= python3
RUSTDOCFLAGS ?= -D warnings

.PHONY: bootstrap fmt fmt-check check lint test bench bench-check bench-baseline bench-compare package-check snapshot-review docs ci bump-version

bootstrap:
	./scripts/bootstrap-rust.sh

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all --check

check:
	$(CARGO) check --workspace

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

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

ci: fmt-check lint test bench-check package-check
