# SQLN — Results of the technical tests

## State of this document

This file records the results of the technical tests that came before the product implementation.
The product implementation was authorized on 2026-10-30.
The spike programs stayed out of Git, and this file is their record.
The capture log of the key test stays in `spikes/keytest-results.log`; the `spikes/` folder is in `.gitignore`.
The checks of the product phase are not here.
They live in the [plan](INITIAL_PLAN.md) ("Check of the complete implementation" and "Validation of the real vaults") and in [PAGINATION-VALIDATION](PAGINATION-VALIDATION.md).

## 1. Driver comparison — 2026-10-30

The tests ran against PostgreSQL 18.6 and MySQL 8.4.13 in Docker, with test certificates.
The candidates were SQLx and the dedicated drivers `tokio-postgres`, `mysql_async`, and `rusqlite`.

| # | Result | Consequence |
|---|---|---|
| 1 | On SQLx, `timeout` did not interrupt the consumption of a result. On the dedicated drivers, it worked | Own runner and own pool, without a library pool |
| 2 | The SQLx pool with a long deadline did not return an error for a refused connection | Own pool returns the error to the request |
| 3 | SQLx does not expose the cancellation data nor the MySQL session reset | Own `Canceller` and own session cleaning |

The spike suite ended with 109 tests passed on real databases.

Decision: the dedicated drivers with an own pool.
The accepted cost is three driver APIs, the own pool, and the TLS setup on each driver.
`rusqlite` is synchronous and runs on its own thread.
The rules are in "Driver decision" of the [plan](INITIAL_PLAN.md) and "Libraries" of [DECISIONS](DECISIONS.md).

## 2. Driver behavior — 2026-10-30

| Subject | Result | Rule in the code |
|---|---|---|
| TLS of PostgreSQL | The default `Prefer` accepts a connection without TLS | Set `SslMode::Require` on remote hosts |
| TLS of MySQL | The default does not use TLS | Always set `ssl_opts` on remote hosts |
| Verification | With `rustls`, a wrong CA and a wrong name refused the connection, without an insecure alternative | Verification by default, off only on local addresses |
| Client certificate | Worked on both databases | Fields for CA and client certificate |
| Client key on MySQL | `mysql_async` 0.36 accepted only RSA in PKCS#1. Version 0.37 also accepts PKCS#8 and SEC1 | The driver reports a bad key; `docs/DECISIONS.md` holds the change |
| Connection deadline | `connect_timeout` did not cover a silent server | Wrap the whole connection in `timeout` of 30 s |
| Connection loss | Both network drivers reported the loss on an idle connection, in a transaction, and during an execution | Replace idle connections; fail the statement without repeating SQL |
| Types on PostgreSQL | The simple protocol does not inform the types | `prepare` before the statement, on the same connection |
| Transaction state on PostgreSQL | `tokio-postgres` does not expose it | Clock probe `SELECT transaction_timestamp() <> statement_timestamp()` through the simple protocol; about 0.3 ms on `localhost`. A `SAVEPOINT` probe was rejected: it generates log errors |
| Transaction state on MySQL | A `SET TRANSACTION` probe erased the user `SET TRANSACTION READ ONLY`, and `performance_schema` needs privileges | `SERVER_STATUS_IN_TRANS` of the last response, with `DO 0` after an error. The probe hit 1,000 of 1,000 reads without log errors; `25P02` marks the transaction with an error |
| Close on MySQL | The `drop` of `Conn` reads the result to the end with the socket open | Drop the connection outside the runtime |
| Values in templates | SQLite writes `NULL` into a `?` without a value | The SQL templates use `<column>`, never `?` |

Not tested in this phase: keepalive with a real network failure, and the operating system CA.
The plan keeps those items in "Checks without a result".

## 3. Keys in iTerm (`keytest`) — 2026-10-30

The record `keytest-results.log` sits in `../spikes/`, beside the capture of that test.
The conditions were: terminal iTerm.app, `TERM=xterm-256color`, keyboard enhancement on.
The log holds `KeyEvent` and `MouseEvent` lines in the order of capture.

The choice criteria of [KEYMAP](KEYMAP.md) use only keys in this record.
The record has gaps:

- `Ctrl+P` is not in the record. A PTY test delivered its byte.
- `Ctrl+/` is not in the record. Use `Ctrl+W /` when the terminal does not deliver the key.
- A drag gesture is not in the record.
- Terminal.app and Linux terminals were not tested.
- `Shift+F5` and `Ctrl+Enter` were tested with simulated sequences in a PTY, not in iTerm.

## 4. Measured costs

| Subject | Before | After | Where |
|---|---|---|---|
| Autocomplete on a 50,000-line script | 55 ms per key | 33 ms per key | Reuse the highlight tokens in the autocomplete |
| Detail window with a 40 MB value | 2.3 s per frame | 0.4 ms | Wrap the lines on demand (`ui/wrap`) |
| Transaction probe on PostgreSQL | One round trip | About 0.3 ms on `localhost` | Clock probe of section 2 |

The product targets run with `make bench`; the goal is under 50 ms per key and frame.
The latest numbers are in [PAGINATION-VALIDATION](PAGINATION-VALIDATION.md).

## 5. Vault probes — 2026-10-31

Scope:

- **macOS:** the login Keychain, and a throwaway Keychain in a temporary `HOME` folder. The locked scenarios used `SecKeychainSetUserInteractionAllowed`, not a real session without windows.
- **Linux:** a `rust:1.98-slim-bookworm` container, `arm64`, with D-Bus and `gnome-keyring`. It does not represent a desktop session.
- **Values:** one test credential, without real secrets.

Results:

- Unlocked stores: read, write, and delete passed on both systems.
- Locked macOS store, or item of another application: `Failed` or `NoAccess`, with `-25293`, `-128`, `-25244`, `-25299`, `-25307`, or `-25308`, per scenario.
- Linux without a session bus, or without the Secret Service on the bus: `Absent`, three responses in total.
- Locked Linux collection: `NoAccess`.
- Before this validation, `Absent` never happened on a real service. `is_absent` accepts only the three confirmed responses.

The full table per scenario is in "Validation of the real vaults" of the [plan](INITIAL_PLAN.md).
The Linux desktop session, with a real dialog, stays without a result.

## 6. How to repeat

The original spike programs are gone.
The product tests in `tests/` now cover the same rules against real servers: `tests/servers.rs`, `tests/redis.rs`, `tests/refused.rs`, and `tests/auth.rs`.
`scripts/test-env.sh` keeps `SQLN_PG_URL_SQLX` and `SQLN_PG_URL_DRIVERS` from the driver phase.

1. Start the databases: `docker compose -f compose.test.yaml up -d --wait`.
2. Load the environment: `. scripts/test-env.sh`.
3. Run `make test` against that stack, then stop it with `docker compose -f compose.test.yaml down`.
4. For the full TLS sequence, use [Database integration](../README.md#database-integration). It sets `SQLN_TEST_TLS=1`.

Alternatively, `make test-integration` starts the plain stack, loads its environment, runs tests, and stops it.

Additional checks:

1. Run the vault probe on macOS: `sh scripts/test-vault-macos.sh`. Add `--dialogs` for the steps that need a person.
2. Run the vault probe on Linux: `sh scripts/test-vault-linux.sh`. It uses a throwaway container.
3. Run the performance targets: `make bench`, in release mode.

## References

- [DECISIONS](DECISIONS.md) — choices and rejected alternatives, with the test reasons.
- [INITIAL_PLAN](INITIAL_PLAN.md) — driver decision, validations, and checks without a result.
- [KEYMAP](KEYMAP.md) — key criteria and the gaps of the `keytest` record.
- [PAGINATION-VALIDATION](PAGINATION-VALIDATION.md) — verification report of the page buttons.
- [TOOLS](TOOLS.md) — how other tools solve the same points; it does not replace these tests.
