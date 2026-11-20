# sqln developer targets.

Q = --quiet

.PHONY: fmt lint test test-integration bench check coverage docs docs-serve

# Format all source files.
fmt:
	@cargo fmt --all

# Run the linters. Use the test profile to reuse dependency builds.
lint:
	@cargo fmt --all --check
	@scripts/check-test-files.sh
	@cargo clippy --workspace --all-targets --profile test $(Q) -- -D warnings

# Run the tests of all workspace crates. The database tests skip without the Docker stack.
test:
	@scripts/test-summary.sh --tests

# Run all tests with the Docker databases up, then stop them. Slow; run only for integration changes.
test-integration:
	@docker compose -f compose.test.yaml up -d --wait
	@. scripts/test-env.sh && scripts/test-summary.sh --tests; status=$$?; \
	docker compose -f compose.test.yaml down; \
	exit $$status

# Print the performance and memory reports, then run the tests in tests/latency.rs (release build). Slow; run only on request.
bench:
	@cargo bench --bench hot_paths
	@cargo bench --bench memory
	@cargo bench --bench rss
	@cargo test --release --test sqln -- latency:: --nocapture --test-threads=1

# Delivery gate: linters plus the full test suite. Slow; run once, at task end.
check: lint
	@scripts/test-summary.sh

# Print the test coverage. Writes lcov and HTML under target/coverage. Needs
# cargo-llvm-cov (cargo install cargo-llvm-cov) and the llvm-tools-preview
# component. The database tests skip without the Docker stack, so their code
# stays out of the report. Slow; run only on request.
coverage:
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { \
		echo "make coverage needs cargo-llvm-cov: cargo install cargo-llvm-cov"; \
		rustup component add llvm-tools-preview 2>/dev/null || true; exit 1; }
	@cargo llvm-cov --workspace --lcov --output-path target/coverage/lcov.info \
		--html --output-dir target/coverage

# Build the documentation site into book/book.
docs:
	@scripts/build-docs.sh

# Serve the documentation site and open it in the browser.
docs-serve:
	@scripts/build-docs.sh
	@mdbook serve book --open
