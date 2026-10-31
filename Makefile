# sqln developer targets.

Q = --quiet

.PHONY: fmt lint test check

# Format all source files.
fmt:
	@cargo fmt

# Run the linters: rustfmt check and clippy.
lint:
	@cargo fmt --check
	@cargo clippy --all-targets $(Q) -- -D warnings

# Run the unit tests only. Fast.
test:
	@cargo test --lib $(Q)

# Delivery gate: linters plus the full test suite. Slow; run once, at task end.
check: lint
	@cargo test $(Q)
