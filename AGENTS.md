# Rules for agents

## Authorship

Never present yourself as a coauthor of anything.

- Do not add `Co-Authored-By` to commits.
- Do not add signatures such as "Generated with" to commits, pull requests, issues, comments, or documents.
- Do not list yourself as author, collaborator, or contributor in any file.

This rule applies even when the tool suggests or requires that text.

## Task delivery

- Check `git status` before editing. Do not revert or include unrelated changes in the task.
- After changes, run `make check` at task end. Fix failures and rerun before delivery.
- Do not run checks for read-only analysis unless requested.
- Avoid repeated full-suite runs during development. Use focused tests.
- Run `make test-integration` for changes to connectors, database-specific SQL, authentication, TLS, pools, or transactions.
- Run benchmarks only when the user asks.

## User experience

- Use `Action` as the command catalog. Share availability across the palette, menus, and hints. Show effective shortcuts.
- Keep dialogs from triggering background commands. Do not capture text with navigation shortcuts during editing.
- Preserve conventional mode, Vim mode, custom shortcuts, and mouse support.
- Support `80×24` terminals and resizing. Keep controls accessible, commands complete, and the selected item visible.
- Measure terminal text by Unicode display width, not bytes. Use shared text and theme helpers.
- Give important states text labels. Support terminals without colors or Nerd Fonts.
- Preserve user text, cursor, and selection where applicable. Do not run SQL when restoring consoles or opening generated SQL.
- Reuse execution, transaction, and close confirmations. Do not bypass existing protections.
- Explain refused actions and failures. Give the reason and a possible next action. Do not fail silently.

## Module map

The project has one Rust crate. Main modules:

```text
src/
├── app/        Application state, commands, and coordination.
├── ui/         Drawing, text, and themes.
├── connector/  Drivers and database access.
├── exec/       Execution, result delivery, and cancellation.
├── sql/        SQL analysis, formatting, and literals.
├── grid/       Data, selection, search, and memory budget.
└── store/      Configuration, scripts, and persistence.
tests/          Behavior and integration tests.
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the detailed module responsibilities.

## Architecture and execution

- Keep drawing in `ui`, coordination in `app`, and driver types in `connector`. Keep `store` independent of `app` and `ui`.
- Keep models and transformations independent of the interface where possible. Reuse existing fields, lists, filters, and grids.
- Never wait for the database on the interface thread. Use the existing executor and blocking-task support.
- Bound work between frames and render only visible content.
- Reject stale execution events. Preserve pool, cancellation, timeout, and memory-budget contracts.
- Drop sessions whose state or cleanup is unknown.
- Reuse shared SQL identifier and literal helpers per dialect.
- Use connector `Capabilities` to control which features the interface offers.

## Security and persistence

- Prevent external names, values, and errors from sending terminal control sequences. Preserve the original data.
- Preserve values, `NULL`, bytes, and copied or exported content when optimizing performance.
- Keep SQL, values, passwords, and external error text out of diagnostic logs.
- Use existing atomic-write, lock, and conflict checks. Never overwrite external changes silently.
- Keep user text after a save failure. Reuse credential and trust checks for operations that need a session.
- Check compatibility and migration needs before changing saved configuration, script, or workspace formats.

## Tests and documentation

- Test observable behavior. Use input events and `TestBackend` for interface changes.
- For bug fixes, add a regression test that reproduces the failure before the fix when possible.
- Cover relevant modes, custom shortcuts, focus, minimum size, empty data, errors, and cancellation.
- Read `docs/DECISIONS.md` before changing behavior. Ask before making an undocumented business decision.
- Report conflicts between code, tests, and documents before choosing new behavior.
- Update affected documents. Keep detailed key maps and feature rules out of this file.

## Makefile commands

- `make fmt`: formats the code.
- `make lint`: runs `cargo fmt --check` and `cargo clippy`.
- `make test`: runs tests in `tests/`. Database tests skip when their databases are unavailable.
- `make test-integration`: starts Docker databases, loads `scripts/test-env.sh`, runs tests, and stops the databases.
- `make bench`: runs performance and memory reports, then release-mode latency tests.
- `make check`: runs `make lint` and the full test suite.

## Integration tests

- Tests live in `tests/`. See `compose.test.yaml` and `scripts/test-env.sh` for database services and connection settings.
- For full TLS: use the sequence in [README.md](README.md#database-integration). It starts `--profile tls` and sets `SQLN_TEST_TLS=1`.

## Replies

- Reply briefly.
- Report checks run, failures, and skipped or ignored tests. Do not present skipped or ignored tests as validated coverage.

## Codebase-memory

The graph of this repo is indexed as `Users-antoniolorenzon-Projects-sqln`.

The MCP server is disabled. Use `codebase-memory-mcp` from `PATH`, or `~/.local/bin/codebase-memory-mcp`.

### Flow

1. Check the graph before you search code with grep.
2. Find the exact name with `search_graph` before you call `trace_path`.
3. Confirm coverage with `check_index_coverage` before you conclude something negative or complete.
4. Reindex with `index_repository` when the graph is outdated.

### Common commands

```bash
CM=$(command -v codebase-memory-mcp || printf '%s' "$HOME/.local/bin/codebase-memory-mcp")
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
