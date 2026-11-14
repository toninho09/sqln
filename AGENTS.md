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

The project is a Cargo workspace. The crates form a graph without cycles. The text after `←` lists the workspace crates that a crate depends on:

```text
crates/core/       sqln-core: data models, grid, text, result sinks, and key map.
crates/sql/        sqln-sql: SQL analysis, formatting, and literals. ← core
crates/session/    sqln-session: connector contract, targets, pools, traces, TLS. ← core, sql
crates/vault/      sqln-vault: system vault. ← session
crates/<driver>/   sqln-postgres, -mysql, -sqlite, -duckdb, -redis, -kafka: one driver each. ← core, sql, session (kafka: vault)
crates/connector/  sqln-connector: facade of the contract and drivers, and the connector factory. ← drivers, store
crates/store/      sqln-store: configuration, scripts, credentials, and persistence. ← core, session, vault
crates/exec/       sqln-exec: jobs, completion, models, and execution event types. ← core, sql, session, store
crates/runtime/    sqln-runtime: execution manager, console tasks, and pools in use. ← exec
crates/ui/         sqln-ui: drawing, themes, editor, and screen states. ← exec
src/               sqln: application state, commands, coordination, and the binary. ← all
crates/testkit/    sqln-testkit: shared test helpers (keys, temporary paths, fake connector, editor fixture).
tests/             Tests that need `sqln` (application or binary). Every file is a `[[test]]` target or a `mod` of a root file.
```

The `sqln` crate re-exports every module, so `sqln::<module>` paths stay valid.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the detailed module responsibilities.

## Architecture and execution

- Keep drawing in `ui`, coordination in `app`, and driver types in their driver crate. Keep `store` independent of `app` and `ui`.
- Put new code in the lowest crate that has all its dependencies. Do not add a dependency on a higher crate; move the shared type down instead.
- Keep driver libraries in their driver crate. Only `sqln-connector` and `sqln` depend on driver crates. Put code that needs no driver library in `sqln-session`.
- Keep `Manager` and the console tasks in `sqln-runtime`. `sqln-ui` uses only the types of `sqln-exec`.
- Keep models and transformations independent of the interface where possible. Reuse existing fields, lists, filters, and grids.
- Never wait for the database on the interface thread. Use the existing executor and blocking-task support.
- Bound work between frames and render only visible content.
- Reject stale execution events. Preserve pool, cancellation, timeout, and memory-budget contracts.
- Drop sessions whose state or cleanup is unknown.
- Reuse shared SQL identifier and literal helpers per dialect.
- Put per-dialect behavior in `sqln_sql::sql::dialect` and `sqln_exec::dialect`. Do not compare `Dialect` values outside them, except on the command-line paths of Redis and Kafka. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#dialects).
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
- `make lint`: runs `cargo fmt --all --check`, `cargo clippy --workspace`, and `scripts/check-test-files.sh`.
- `make test`: runs the tests of all workspace crates. Database tests skip when their databases are unavailable.
- `make test-integration`: starts Docker databases, loads `scripts/test-env.sh`, runs tests, and stops the databases.
- `make bench`: runs performance and memory reports, then release-mode latency tests.
- `make docs`: builds the documentation site into `book/book`.
- `make docs-serve`: serves the documentation site locally.
- `make check`: runs `make lint` and the full test suite.

## Integration tests

- Put a test in the lowest crate that has all its dependencies. A test that needs a driver lists it as a dev-dependency. Each crate with integration tests has one test binary, `crates/<crate>/tests/sqln_<crate>/main.rs`, and each file in that folder is a `mod` of it. `scripts/check-test-files.sh` enforces it.
- Put shared helpers in `sqln-testkit`. Helpers that need `App` stay in `tests/common`.
- Database tests are in `tests/` and in the crate tests. See `compose.test.yaml` and `scripts/test-env.sh` for database services and connection settings.
- For full TLS: use the sequence in [README.md](README.md#database-integration). It starts `--profile tls` and sets `SQLN_TEST_TLS=1`.

## Replies

- Reply briefly.
- Report checks run, failures, and skipped or ignored tests. Do not present skipped or ignored tests as validated coverage.
