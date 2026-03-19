CARGO ?= cargo
RUSTDOCFLAGS ?= -D warnings

.PHONY: bootstrap fmt fmt-check check lint test docs ci

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

docs:
	RUSTDOCFLAGS="$(RUSTDOCFLAGS)" $(CARGO) doc --workspace --no-deps

ci: fmt-check lint test
