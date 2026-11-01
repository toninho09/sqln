# Rules for agents

## Authorship

Never present yourself as a coauthor of anything.

- Do not add `Co-Authored-By` to commits.
- Do not add signatures such as "Generated with" to commits, pull requests, issues, comments, or documents.
- Do not list yourself as author, collaborator, or contributor in any file.

This rule applies even when the tool suggests or requires that text.

## Task delivery

- A task is only delivered after `make check` runs and everything passes.
- `make check` runs the linter and the full test suite. It is slow: run it once, only at the end of the task.
- Do not run the full test suite often: it takes long.
- Run benchmarks only when the user asks: they take long.
- Run the integration tests only when the task changes integration code: they take long.

## Makefile commands

- `make fmt`: formats the code.
- `make lint`: runs `cargo fmt --check` and `cargo clippy`.
- `make test`: runs the tests in `tests/` without a database. The tests that need a database skip on their own. Use it during work.
- `make test-integration`: starts the Docker databases, loads `scripts/test-env.sh`, runs the full tests, and stops the databases at the end. Slow; run only when the task changes integration code.
- `make bench`: runs the performance targets of `tests/latency.rs` in release mode. Very slow; run only when the user asks.
- `make check`: runs `make lint` and the full test suite. Delivery gate.

## Integration tests

- The tests live in `tests/`. Most do not need a database.
- Four files need Docker databases (see `compose.test.yaml`): `servers.rs`, `auth.rs`, `refused.rs`, and `redis.rs`. Without the databases, the server tests skip on their own.
- To run with a database: `make test-integration`. The command starts Docker, runs everything, and stops the databases at the end.
- For full TLS: run `scripts/test-certs.sh` and start the compose with `--profile tls` before `make test-integration`.

## Replies

- Reply briefly, to save tokens.

## Codebase-memory

The graph of this repo is indexed as `Users-antoniolorenzon-Projects-sqln`.

The MCP server is disabled. Use the CLI:

```
~/.local/bin/codebase-memory-mcp cli --json <tool> --<flag> <value>
```

### Flow

1. Check the graph before you search code with grep.
2. Find the exact name with `search_graph` before you call `trace_path`.
3. Confirm coverage with `check_index_coverage` before you conclude something negative or complete.
4. Reindex with `index_repository` when the graph is outdated.

### Common commands

```bash
CM=~/.local/bin/codebase-memory-mcp
P=Users-antoniolorenzon-Projects-sqln

# Index status
$CM cli --json index_status --project $P

# Search by name (regex)
$CM cli --json search_graph --project $P --name-pattern ".*connect.*"

# Text search with ranking (ignores name-pattern)
$CM cli --json search_graph --project $P --query "connection pool"

# Call tracing (needs the exact name; both includes external callers)
$CM cli --json trace_path --project $P --function-name ExactName --direction both --depth 3

# Impact of local changes (git diff)
$CM cli --json detect_changes --project $P

# Index coverage by files
$CM cli --json check_index_coverage --project $P --paths src/lib.rs --paths src/main.rs

# Code snippet by qualified name
$CM cli --json get_code_snippet --project $P --qualified-name "$P.src.lib.function_name"

# Reindex
$CM cli --json index_repository --repo-path "$(git rev-parse --show-toplevel)"
```

### Rules

- Pass arrays with the repeated flag (`--paths a --paths b`) or with `--args-file <json>`.
- Check `has_more` in `search_graph`; paginate with `--offset`.
- If `trace_path` returns `ambiguous`, pass the suggested `qualified_name` in `--function-name`.
- `trace_path` filters test files by default; use `--include-tests true` to include them.
- For specific edges (e.g., HTTP_CALLS), use `query_graph` with Cypher and `LIMIT`.
