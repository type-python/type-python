CARGO ?= cargo
RUSTDOCFLAGS ?= -D warnings

.PHONY: bootstrap fmt fmt-check check lint test bench bench-check snapshot-review docs ci

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
	$(CARGO) bench --workspace

bench-check:
	$(CARGO) bench --workspace --no-run

snapshot-review:
	$(CARGO) insta review

docs:
	RUSTDOCFLAGS="$(RUSTDOCFLAGS)" $(CARGO) doc --workspace --no-deps

ci: fmt-check lint test bench-check
