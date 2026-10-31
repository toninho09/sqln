# sqln developer targets.

Q = --quiet

.PHONY: fmt lint test test-integration bench check

# Format all source files.
fmt:
	@cargo fmt

# Run the linters: rustfmt check and clippy.
lint:
	@cargo fmt --check
	@cargo clippy --all-targets $(Q) -- -D warnings

# Run the tests in tests/. The database tests skip without the Docker stack.
test:
	@scripts/test-summary.sh --tests

# Run all tests with the Docker databases up, then stop them. Slow; run only for integration changes.
test-integration:
	@docker compose -f compose.test.yaml up -d --wait
	@. scripts/test-env.sh && scripts/test-summary.sh --tests; status=$$?; \
	docker compose -f compose.test.yaml down; \
	exit $$status

# Run the performance targets in tests/latency.rs (release build). Slow; run only on request.
bench:
	@cargo test --release --test latency

# Delivery gate: linters plus the full test suite. Slow; run once, at task end.
check: lint
	@scripts/test-summary.sh
