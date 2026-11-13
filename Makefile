# sqln developer targets.

Q = --quiet

.PHONY: fmt lint test test-integration bench check docs docs-serve

# Format all source files.
fmt:
	@cargo fmt

# Run the linters. Use the test profile to reuse dependency builds.
lint:
	@cargo fmt --check
	@scripts/check-test-files.sh
	@cargo clippy --all-targets --profile test $(Q) -- -D warnings

# Run the tests in tests/. The database tests skip without the Docker stack.
test:
	@scripts/test-summary.sh --tests

# Run all tests with the Docker databases up, then stop them. Slow; run only for integration changes.
test-integration:
	@docker compose -f compose.test.yaml up -d --wait
	@. scripts/test-env.sh && scripts/test-summary.sh --tests; status=$$?; \
	docker compose -f compose.test.yaml down; \
	exit $$status

# Print the performance and memory reports, then run the targets in tests/latency.rs (release build). Slow; run only on request.
bench:
	@cargo bench --bench hot_paths
	@cargo bench --bench memory
	@cargo test --release --test latency -- --nocapture --test-threads=1

# Delivery gate: linters plus the full test suite. Slow; run once, at task end.
check: lint
	@scripts/test-summary.sh

# Build the documentation site into book/book.
docs:
	@scripts/build-docs.sh

# Serve the documentation site and open it in the browser.
docs-serve:
	@scripts/build-docs.sh
	@mdbook serve book --open
