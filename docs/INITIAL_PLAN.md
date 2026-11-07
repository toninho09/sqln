# SQLN — Consolidated plan

## 1. State of this document

This document records the exploration and current functional rules.
Later approved decisions in [DECISIONS.md](DECISIONS.md) replace earlier rules on the same subject.
Dated test results below remain historical records, not results for the current working tree.

- **Defined:** decision confirmed in the conversation.
- **Proposed:** technical or functional recommendation not yet confirmed.
- **Pending:** decision that needs to happen before the corresponding implementation.

### Current state

- The design, the navigation, the architecture, and the decisions of the project are complete.
- The technical tests ran before the implementation. [docs/RESULTS.md](RESULTS.md) records their results: 109 tests passed with real databases.
- The local Git repository exists.
- The product implementation was authorized on 2026-10-30.
- The application works with SQLite, PostgreSQL, MySQL, Redis, and Valkey, with several consoles and several connections.
- The navigator, the tabs, the dividers, the key map, and the connection form exist.
- The table view exists, with pages, the `WHERE` and `ORDER BY` bar, sorting, and stop.
- The configuration and the saving exist: global and project `config.json`, automatic saving of the consoles, and reopening.
- The credential preparation uses `creds` and `trust` before execution, catalog, pages, and connection test.
- The interface asks for the password and trust and allows saving into the vault by explicit choice.
- Without that choice, the password stays in the session only. `Secret`, text fields, and passwords use `zeroize` when the values are dropped.
- The code of all phases is implemented. "What is missing for the release" lists what depends on third parties or a real environment.
- The user did a manual test on 2026-10-30. The record has neither the terminal nor what the test covered.

### What is implemented

| Area | Content | Rules |
|---|---|---|
| Editor | Conventional and Vim modes, selection, undo, search, replace, ANSI highlight, and `NO_COLOR` | Section 5 |
| Autocomplete | Schemas, tables, views, columns on demand, aliases, CTEs, built-in types, functions with signatures, and keywords | Section 6 |
| SQL analysis | Tokenizer per dialect, statement separation, `DELIMITER`, and statement classes | Sections 9 and 14 |
| Connectors | SQLite, PostgreSQL, MySQL, and Redis/Valkey, with URI, verified TLS, and client certificate; PostgreSQL and MySQL also have keepalive | Sections 3 and 14 |
| Pool | Limit, queue, wait cancellation, cleaning, idle expiry, discard, and lost connection replacement | Section 7 |
| Execution | Statement, selection, script, cap, deadline, stop, indicators, and error position | Sections 7 and 9 |
| Transactions | Autocommit, manual mode, commit, rollback, confirmations, SQL control, and unknown state | Section 8 |
| Grid | Tabs, **Messages**, selection, TSV, detail, budget, search, width, and tab close | Section 10 |
| Terminal | Restore at exit and on panic, mouse, paste, and small terminal warning | Section 5 |
| Interface | Several consoles in tabs, navigator, dividers, bar buttons, database or schema selector, and help per panel | Sections 5, 6, and 7 |
| Exit | Questions in order for each console, with each answer applied at once | Section 8 |
| Key map | Actions per region, multi-key sequences, Settings recorder, and `keymap` rules from `config.json` | `docs/KEYMAP.md` |
| Table view | Table or view tab, **Load more**, `WHERE` and `ORDER BY` bar with autocomplete, header sorting, stop, budget, and pool | Section 10 |
| Connection form | Form and URI, test, edit, removal, names per scope, global or project scope, SQL limit, TLS suggestion, and `foreign_keys` | Sections 3 and 11 |
| Folders and configuration | Global folder and project `.sqln/` folder, `--project`, creation question, `config.json`, precedence, error with file and position, **Reload**, and external change warning | Section 11 |
| Console saving | `.sql` and `.json` files per console, save 500 ms after the last edit, `Ctrl+S`, atomic write, failure with alert, close confirmation, and file lock | Section 12 |
| Saved consoles | List when asking for a console, reopening with cursor and scroll, choice per file on a change outside SQLN, scripts without connection, attach, and delete | Section 12 |
| Local state | `workspace.json` with cursor, scroll, and layout, outside Git | Section 11 |
| Capabilities | Each connector declares transactions, database or schema choice, and SQL filter in the table tab | Section 3 |
| Credentials | First use, trust, masked password, session, vault, authorized plain text, migration, refused password with suspension, and separate deletion | Sections 3 and 13 |
| Consoles | Rename with `Ctrl+W N` and in the list; change the connection with `Ctrl+W a` | Section 12 |
| Settings | `Ctrl+W ,` dialog per scope, with inherited values and removal of a project definition | Section 11 |
| Tabs | Consoles and tables in the opening order; closing the last console with an open table does not create another | Section 10 |
| Operation | `--log`, shutdown signals, panic report, and binary without the example catalog | Sections 5 and 6 |
| Appearance | `auto`, `astrodark`, `ansi`, `tokyo-night`, `catppuccin-mocha`, `catppuccin-latte`, `gruvbox`, and `nord` themes, per-connection theme and icon, Unicode or Nerd Font symbols, status bar, panel titles, scroll bars, and windows with a rounded border | Section 5 and `docs/DECISIONS.md` |
| Discovery | `Ctrl+W` menu, command palette with `Ctrl+P`, and welcome screen | Section 5 and `docs/KEYMAP.md` |
| Command-line target | `sqln file.db` and `sqln postgres://...`, with the connection only in the session | `docs/DECISIONS.md` |
| Run protection | Question before `UPDATE` or `DELETE` without `WHERE`, `PROD` badge, and confirmation in production | `docs/DECISIONS.md` |
| Formatter | `Ctrl+W f` on the selection or the statement under the cursor, with token check | `docs/DECISIONS.md` |
| History | Global `history.jsonl`, window with `Ctrl+W H`, secret filter, and option in Settings | `docs/DECISIONS.md` |
| Row window | `r` key in the grid, with the row vertical and row change through the arrows | `docs/DECISIONS.md` |
| JSON in the detail | Readable shape, `f` key, and **Format JSON** option in Settings | `docs/DECISIONS.md` |
| Comment | `Ctrl+W /` and `Ctrl+/` on the selection or the cursor line | `docs/DECISIONS.md` |
| Row count | `c` key in the table grid, with `COUNT(*)` on demand | `docs/DECISIONS.md` |
| Tree filter | `/` in the navigator, in the in-memory catalogs, without opening connections | `docs/DECISIONS.md` and `docs/KEYMAP.md` |
| Database notices | PostgreSQL `NOTICE` and MySQL notices in **Messages**, with `Messages (n)` | `docs/DECISIONS.md` |
| Production on Redis | Question before a command outside the read list | `docs/DECISIONS.md` |
| Bell at the end | BEL after `bellAfterSeconds`, with a default of 10 s | `docs/DECISIONS.md` |
| Transaction age | Minutes in the status line and error color from 5 min | `docs/DECISIONS.md` |
| SQL generation | `g` lists templates in the navigator, and `y` copies the name | `docs/DECISIONS.md` and `docs/KEYMAP.md` |
| File explorer | `Ctrl+O` or the `...` button in the path fields, with filter, hidden files, and **Use this folder** | `docs/DECISIONS.md` |
| `JOIN` by key | Suggestion after `ON`, with the keys read together with the columns | `docs/DECISIONS.md` |
| Objects and DDL | Groups for routines, sequences, triggers, and types; DDL in a new console without execution | `docs/DECISIONS.md` |
| Key navigation | `F` follows outgoing and incoming foreign keys from a table row | `docs/DECISIONS.md` |
| Query plans and sessions | Plan tree, server sessions, and confirmed query cancellation or session termination | `docs/DECISIONS.md` |
| Compare | Structure and data compare across SQL connections, with a sync script that does not run automatically | `docs/DECISIONS.md` |
| Import | CSV, TSV, JSON, and JSON Lines into existing tables, with mapping, preview, and failure modes | `docs/DECISIONS.md` |
| Export | CSV, TSV, JSON, JSON Lines, and SQL inserts, with explicit file confirmation | `docs/DECISIONS.md` |
| Command history | Per-console Messages history in memory, separate from query history and diagnostic logs | `docs/DECISIONS.md` |
| Visible schemas | Connection field with a searchable server list; the current schema always stays visible | `docs/DECISIONS.md` |
| Home | Closing the last tab shows the home screen; the Quit button uses the normal exit questions | `docs/DECISIONS.md` |

### Implemented credential base

Date: 2026-10-31. The code of the phase is complete. A repeat of the Secret Service on a real Linux desktop remains.

- `src/creds.rs`: identity by scope, ID, engine, address, user, and TLS context; session cache; vault reading, writing, and deletion.
- `SystemVault`: `keyring-core`, Keychain on macOS, and Secret Service on Linux, without changing the default store of the process.
- `Credentials`: a save failure keeps the password in the session; `save_verified` writes and compares the read to support migration.
- `src/trust.rs`: confirmations in the global `trust.json`, with the canonical paths of the projects and a lock between writers.
- An invalid or unreadable trust file returns an error, without authorizing access or overwriting the content.
- `Secret` and the form texts erase their buffers with `zeroize` when the values are dropped.
- The reload does not keep the password when the destination or the resolved TLS context changes.
- Writes of `config.json` and `trust.json` use exclusive temporaries and permission `0600` on Unix.

Second increment:

- `src/app/auth.rs` prepares the saved connections before it sends execution, catalog, columns, page, or test to the connector.
- `Stored::credential` defines the password retrieval. `none` allows passwordless authentication; `session` asks for the password; `vault` reads the vault.
- Imported destinations and new project paths need confirmation before the credential read.
- A local form skips the destination question, but it does not implicitly authorize reuse on a new project path.
- `src/ui/auth.rs` shows the masked password, with save unchecked, conventional keyboard, Vim, paste, and mouse.
- The form offers **Session only** and **System vault**. A test does not write the typed password or replace the password of the saved connection.
- Vault failures offer cancel, explicit retry, and session use, without plain-text storage.
- When saving a requested password, the file receives only the `vault` reference, never the secret.
- The blocking calls use `Manager::spawn_blocking`, with one operation at a time, off the interface thread.
- Cancellation, definition change, or form change invalidates old responses. Jobs cancelled in the queue do not touch the vault.
- The execution keeps the SQL and the revision captured before the wait. An edit does not change the SQL sent and does not receive wrong marks.

Third increment:

- `ConnectError::password_refused` tells the refusal apart: `28P01` on PostgreSQL, `1045` and `1698` on MySQL. Network, TLS, and other errors stay out.
- `ExecEvent::PasswordRefused` arrives once per password. A new password passes `test_connection` before any write.
- After the second refusal, automatic reads stop, and each explicit action opens the password prompt.
- The refusal happens at session open, before the SQL. The first refusal does not repeat the execution; an action requested during the suspension continues after the accepted password.
- `PlainFile` keeps `credentials.json`. The file refuses a link, an open permission, and invalid content, without overwriting.
- `Credentials::lookup` follows the order of section 13. A `session` reference only touches the vault when the file has a password.
- `is_absent` recognizes only the responses confirmed on Linux. Only `Absent` and `Unsupported` offer plain text.
- With a vault available, a password only in `credentials.json` opens the migration: confirmation, write, check, and optional removal.
- The `p` key in the navigator erases the saved password. The removal of a connection offers that deletion separately, with **Keep password** as initial focus.

The real vault test needs explicit authorization; the common tests use an in-memory vault.

### What is missing for the release

All items below depend on third parties, a real environment, or user credentials.

| Item | Situation |
|---|---|
| Secret Service on a real desktop | The container does not cover a desktop session nor the real unlock dialog |
| Publishing | `Cargo.toml` permits publishing. Package availability and name checks need external confirmation |
| Binaries | The workflow builds Linux and macOS archives, and publishes a GitHub Release on `v*` tags. Signing and notarization remain absent |
| Audit | The security and license audit of the dependencies is missing |
| Manual checks | The section "Checks without a result" lists the tests on real terminals and networks |

### Technical debt

The items below do not block the release.

| Item | Situation | Details |
|---|---|---|
| Vim undo memory | EdTUI keeps full-text entries without a memory cap. Conventional undo uses `undoLimitBytes`, 64 MiB by default | `docs/ARCHITECTURE.md`, section "Known editor limit" |

### Checks without a result

These points have code, but no recorded test confirmed the behavior.
The manual test of 2026-10-30 does not close these items, because its details were not recorded.

- Application in iTerm, Terminal.app, and Linux terminals.
- Console lock on a network disk or another file system that is not local.
- Mouse in a real terminal, with clicks, double click on a table, header click, divider drag, and column drag.
- `Ctrl+P` and `Ctrl+W :` in iTerm. The PTY test delivered the byte of `Ctrl+P`, but the `keytest` did not record that key.
- `Ctrl+/` in iTerm and Linux terminals. The PTY test confirmed the byte `0x1F` and the shape `CSI 47;5u`, but not a real keyboard.
- `astrodark` theme and Unicode symbols in iTerm with the Monaco font.
- Nerd Font symbols in a terminal with that font. The test checks only the width of each symbol.
- System clipboard and OSC 52.
- Connection through a Unix socket.
- Operating system CA and certificate with an IP.
- Keepalive with a real network failure.

### Validation of the real vaults

Date: 2026-10-31.

- **macOS:** login Keychain on this machine and a throwaway Keychain in a temporary `HOME` folder.
- **Linux:** `rust:1.98-slim-bookworm` container, `arm64`, with D-Bus and `gnome-keyring`. It does not represent a desktop session.
- **Values:** one test credential only, without real secrets. The login Keychain ended without items of the `sqln` service.
- **Without a window session:** the scenario is simulated with `SecKeychainSetUserInteractionAllowed`, not with a real SSH session.

| System | Scenario | Read | Write | Delete |
|---|---|---|---|---|
| macOS | Login Keychain, `native_vault_round_trip` | Passed | Passed | Passed |
| macOS | Unlocked throwaway Keychain | No item and with item | Passed | With item and without item |
| macOS | Locked, without a window session | `Failed`, `-25293` | `Failed`, `-25293` | Removes the item |
| macOS | Locked, **Cancel** in the dialog | `Failed`, `-128` | Not tested | Not tested |
| macOS | Item of another application, without a window session | `Failed`, `-25293` | `Failed`, `-25299` | `NoAccess`, `-25244` |
| macOS | Item of another application, **Deny** in the dialog | `Failed`, `-128` | Not tested | Not tested |
| macOS | Keychain file removed, without a window session | No item | `Failed`, `-25308` | No item |
| macOS | Without a default Keychain | `Failed`, `-25307` | `Failed`, `-25307` | `Failed`, `-25307` |
| Linux | Without a session bus | `Absent` | `Absent` | `Absent` |
| Linux | Bus without Secret Service | `Absent` | `Absent` | `Absent` |
| Linux | Service without the `login` collection | No item | `NoAccess` | No item |
| Linux | Unlocked collection | No item and with item | Passed | With item and without item |
| Linux | Locked collection, without a screen for the dialog | `NoAccess` | `NoAccess` | `NoAccess` |

Before this validation, `Absent` never happened on a real service.
`is_absent` now recognizes only the responses confirmed on Linux.
No result of this table authorizes plain text on macOS.
The Linux desktop session, with a real dialog, stays without a result.

### Check of the complete implementation

Date: 2026-10-31.

| Check | Result |
|---|---|
| `cargo test --tests` on macOS | 803 passed and 6 ignored, with real PostgreSQL 18 and MySQL 8.4 |
| `cargo test --tests` on Linux | 804 passed and 6 ignored, in an `arm64` container, without databases: the server tests print `skipped` |
| Clippy | `-D warnings`, no warnings on macOS and Linux |
| Refused password | `tests/refused.rs` on real PostgreSQL and MySQL: classification, new password, suspension, and vault protection |
| Binary in a PTY | `SIGTERM`, `SIGHUP`, and `SIGINT` write the text, restore the screen, and exit with code 0; `--log` has no SQL; no example catalog |
| Injected defects | 15 critical rules: 13 detected. The other 2 fall on a second protection: file permission and script hash |
| Key and frame with 50,000 rows, in release | 21 to 24 ms in five rounds; the goal is 50 ms. The previous `HEAD` measures the same |
| Key with 255 MiB loaded, in release | 0.1 ms; the goal is 50 ms |
| Key in the navigator with 20,000 tables | Below the goal in the five rounds |
| Write of a 50,000-line script | 21 to 31 ms in the main loop; the goal is 50 ms |
| Panic report | `write_panic_report` has a test. The hook and the write after a real panic did not run |
| TLS of the real databases | Not enabled; `SQLN_TEST_TLS` was not set |

Under heavy CPU load, `the_row_cap_ends_a_large_read_soon_and_the_session_stays_usable` can pass the 5 s limit.
That test measures wall-clock time. It passes with the machine without that load.

### Check of the second credential increment

Date: 2026-10-31.

| Check | Result |
|---|---|
| `cargo test --tests` | 740 passed and 5 ignored, with real PostgreSQL 18 and MySQL 8.4 |
| Credential integration | 28 tests in `tests/auth.rs`, including a query with the simulated vault password on real PostgreSQL |
| Preparation safety | No TCP before authorization; cancellation, old response, copied project, write failure, and reference failure |
| Interface | Mask, paste, mouse, explicit choice, and edit while the simulated vault waits |
| Clippy | `cargo clippy --all-targets -- -D warnings`, no warnings |
| Real vault and TLS | Not run; the native test stays ignored and `SQLN_TEST_TLS` was not set |
| LSP | Does not start: the `rust-analyzer` component is missing from the local toolchain |

### Check of the first credential increment

Date: 2026-10-31.

| Check | Result |
|---|---|
| `cargo test --tests` | 712 passed and 5 ignored, with real PostgreSQL 18 and MySQL 8.4 |
| New credential tests | 38 passed: 36 in `tests/credentials.rs` and 2 in `src/creds.rs` |
| Real vault | The `native_vault_round_trip` test stays ignored by default; it did not run |
| TLS of the real databases | Not enabled in this increment; `SQLN_TEST_TLS` was not set |
| Clippy | `cargo clippy --all-targets -- -D warnings`, no warnings |
| LSP | Does not start: the `rust-analyzer` component is missing from the local toolchain |
| Terminals and performance | Not repeated in this increment |

### Previous check of configuration and saving

Date: 2026-10-31.

| Check | Result |
|---|---|
| Automated tests | 674 passed, with the real databases |
| Tests with real databases | 64 of the tests above, on PostgreSQL 18 and MySQL 8.4 |
| Simulated real terminal | The 116 checks of an earlier phase did not run again. This phase had one round in a PTY with the binary: project question, saved console, local state, and reopening |
| Lock between processes | In a PTY, a test process saw the console lock while SQLN ran, and saw the lock free after `kill -9` |
| Key with 255 MiB loaded | 0.18 to 0.57 ms; the goal is 50 ms |
| Key in the navigator with 20,000 tables | 0.65 to 1.1 ms; the goal is 50 ms |
| Write of a 50,000-line script | 24 ms in the main loop; the goal is 50 ms |
| Key and frame with 50,000 lines in the editor | 38 to 56 ms in three rounds. This test does not use code of this phase, and the variation comes from the machine load |
| Injected defects in configuration and saving | 47 of 47 detected, after strengthening three tests |
| Clippy | No warnings |

### How to run

```sh
cargo run                                 # uses the global and project configuration
cargo run -- --project ../store           # uses the project of that folder
cargo run -- data.db                      # opens a SQLite file for this session only
cargo run -- postgres://ana@host/store    # opens a URI for this session only
cargo run -- --log sqln.log               # records events, without SQL or values
cargo run -- --dialect sqlite             # development option
cargo test --tests                        # tests
cargo test --release --test latency -- --nocapture
```

The connections and the preferences stay in `config.json`, in the global folder (`~/.config/sqln/` or `$XDG_CONFIG_HOME/sqln/`) and in the `.sqln/` folder of the project.
Without the `.sqln/` folder, SQLN asks whether to create a project in the current directory.
Example of `config.json`, with the connections of the schema of `docs/ARCHITECTURE.md`:

```json
{
  "version": 1,
  "preferences": { "keyboardMode": "vim", "pageSize": 50 },
  "connections": [
    { "id": "6a1d4c12-7c0e-4f0b-9a55-0c2f4b6f7d10", "name": "local", "kind": "sqlite", "file": "data/app.db" }
  ]
}
```

The connection form also accepts a URI, with `sslmode=disable` or `verify-full`, `sslrootcert`, `sslcert`, and `sslkey`.
A remote host needs TLS with verification by default.
`tests/servers.rs` uses real databases; without the variables, those tests print "skipped" and pass.
To run with PostgreSQL 18 and MySQL 8.4:

```sh
docker compose -f compose.test.yaml up -d --wait
. scripts/test-env.sh && cargo test --tests
docker compose -f compose.test.yaml down
```

The real vault test stays ignored by default.
To run it, authorize the access to the vault of this machine with the command below.
It creates and removes a test credential with a random ID in the `sqln` service.

```sh
SQLN_TEST_SYSTEM_VAULT=1 cargo test --test credentials native_vault_round_trip -- --ignored
```

The scripts below repeat the lock, denied access, absence, and error scenarios.
`scripts/test-vault-macos.sh` uses a throwaway Keychain in a temporary `HOME` folder, without touching the login Keychain.
`scripts/test-vault-linux.sh` uses a container with D-Bus and `gnome-keyring`.

```sh
sh scripts/test-vault-macos.sh            # scenarios without dialogs
sh scripts/test-vault-macos.sh --dialogs  # needs a person in the macOS dialogs
sh scripts/test-vault-linux.sh
```

The commands of the technical tests are in [docs/RESULTS.md](RESULTS.md).

`F5` runs, `Shift+F5` runs the script, `Ctrl+W x` stops, `Ctrl+W c` closes the tab, and `Ctrl+W q` quits.
`Ctrl+S` writes the console at once, and `Ctrl+W r` reads the configuration again.
`Ctrl+W h` goes to the navigator, where `a` adds a connection, `n` opens the console list, `s` lists the scripts without connection, and `Enter` on a table opens the table view.
In the table view, `f` and `o` go to the `WHERE` and `ORDER BY` fields, `Enter` applies, `s` sorts, and `m` loads more.
`F1` shows the keys of the focused panel.
`Ctrl+W N` renames the console, `Ctrl+W a` changes the connection, and `Ctrl+W ,` opens the settings.
In the navigator, `p` erases the saved password of the connection.
The development option `--dialect` applies only to a console without a connection.

### Documents

| File | Content |
|---|---|
| [AGENTS.md](../AGENTS.md) | Rules for agents in this repository |
| [INITIAL_PLAN.md](INITIAL_PLAN.md) | Functional rules and verification scenarios |
| [docs/UI.md](UI.md) | Screen sketches |
| [docs/KEYMAP.md](KEYMAP.md) | Navigation, shortcuts, and key priority |
| [docs/ARCHITECTURE.md](ARCHITECTURE.md) | Modules, concurrency, pool, files, and recovery |
| [docs/DECISIONS.md](DECISIONS.md) | Choices and rejected alternatives |
| [docs/TOOLS.md](TOOLS.md) | How other tools solve the same points |
| [docs/FEATURE-MAP.md](FEATURE-MAP.md) | Feature map, with the main files of each feature |
| [docs/PAGINATION-VALIDATION.md](PAGINATION-VALIDATION.md) | Verification report of the table page buttons |
| [docs/RESULTS.md](RESULTS.md) | Results of the technical tests and how to repeat them |
| [compose.test.yaml](../compose.test.yaml) | Throwaway databases for tests |

This document remains the reference for the functional rules.
It does not represent a complete specification to implement the whole application.

**Defined name:** `sqln`. Check the availability of that name before the release.

## 2. Purpose and reference

### Defined

Create a database client with a terminal interface, written in Rust.

The main focus is to make queries easy through several consoles.
The JetBrains IDEs serve as reference for the use model.

Each console has a text editor. It is not just a command prompt.

The consoles work almost independently. They share the resources of the connection definition.

The independence includes the work in each editor and its results.
It does not guarantee session isolation when the consoles share a physical connection.

### Platforms

| Platform | Priority |
|---|---|
| macOS | Main target |
| Linux | Main target |
| Windows | Optional support |

### Privacy

**Defined:** collect no telemetry.
The application only touches the network to talk to the configured databases.

## 3. Data sources

### Defined

Start with:

- PostgreSQL.
- MySQL.
- SQLite in files.
- SQLite in memory, through the `:memory:` text of the file field, with one pooled session.

**Defined — 2026-11-06:** the `:memory:` text opens one database in memory, which lives
in the only session of its pool and ends with a disconnect or the end of the application.
See [DECISIONS](DECISIONS.md), "SQLite in memory".

**Defined:** support the PostgreSQL and MySQL versions still supported by the vendor.
Treat MariaDB as best effort, through the MySQL connector.

Prepare the structure for sources other than relational databases:

- MongoDB.
- Redis.
- Possibly Kafka.

Those future sources do not enter the first implementation automatically.
Redis and Valkey enter by user request on 2026-10-31.

### Connection form

**Defined:** offer a form with fields and the option to enter a connection URI.
The fields change according to the source; SQLite uses a file path.

**Defined default scope:** with an active project, select project for a new connection and allow choosing global.
Without an active project, offer only the global scope.

**Defined names:** require a required and unique name inside each scope, without case distinction.
Apply that validation when registering and renaming connections.
Allow the same name between the global and project scopes.
Use that comparison also to identify collisions between the scopes and show the origin mark.
The identity of the connection stays independent of the visible name.

**Defined:** offer **Test connection** in the form, with the same TLS rules.
Do not integrate an SSH tunnel in the first version; the user creates the tunnel outside.

**Defined:** offer the **Read only** field on each connection, applied by the database.
Reapply that mode on each reservation, because the session cleaning removes it.
That protection prevents accidents, but it is not a security barrier: the user can undo it through SQL.

| Database | Mechanism |
|---|---|
| PostgreSQL | `SET default_transaction_read_only = on` |
| MySQL | `SET SESSION TRANSACTION READ ONLY` |
| SQLite | `PRAGMA query_only = ON` |

Accept a Unix socket as a local connection.

The input form does not change the scope of the connection or the credential rules.
If the URI contains a password, apply the same storage rules of section 13.
Do not persist a URI with a password in the project configuration.

### Change of a connected connection

**Defined:** require a disconnect before changing access data, TLS, or the pool limit of a connected definition.
Do not save those changes as pending for automatic application after a reconnection.
The disconnect needs to respect the execution and transaction confirmations, without discarding work in silence.

### Connection cycle

**Defined:** connect on first use, without requiring an explicit action.
Get the password only at that moment.
Do not open connections at application startup.
Offer the **Disconnect** action on each connected definition.

**Defined for connection loss:** replace idle connections of the pool without interrupting the user.
Fail a connection reserved by script, read, or transaction, without repeating SQL.
Use TCP keepalive to detect drops.

If the drop hits an open transaction, mark the transaction state as unknown.
Require confirmation before the next execution in that console.
Without that confirmation, the next statement could run in autocommit, outside the lost transaction.

### TLS

**Defined for PostgreSQL and MySQL:** require TLS with certificate verification on remote hosts by default.
Allow turning it off only by explicit user configuration.
**Defined for local addresses:** `localhost`, `127.0.0.0/8`, `::1`, and Unix socket may use a connection without TLS.
The form suggests disabled TLS on those addresses and verification on the others.

- Do not ignore verification failures or fall back silently to insecure transport.
- Do not turn a certificate error into an automatic authorization to disable TLS.
- Treat that policy according to the server address, not according to the local or global scope of the configuration.
- Do not apply TLS requirements to SQLite file access.

**Defined:** offer fields for a custom CA and a client certificate.
The specific modes of each driver still need technical detail.

### Trust on the destination

**Defined for PostgreSQL and MySQL:** before the first connection to a destination on this machine, show host, port, user, and TLS state.
Require confirmation before sending a password.
Record those values in the global folder, never in the project files.
Ask again when one of those values changes.
Do not require that question when the user registers or changes the connection on this machine.
Do not depend on detecting clones or copies of the project.

### SQLite

**Defined:**

- If the file does not exist, ask before creating it.
- Offer a field for `foreign_keys` on each connection, with the SQLite default.
- Offer a configurable lock deadline, with a default of 5 seconds.
- Show a visible error when that deadline ends.

### Future sources

**Defined:** share the structure of the consoles without forcing all sources to use SQL.

| Source | Possible input | Possible results |
|---|---|---|
| PostgreSQL, MySQL, and SQLite | SQL of the respective database | Tables, messages, and affected row count |
| MongoDB | Own queries and commands | Documents and tables |
| Redis | Redis commands | Values, lists, and other structures |
| Kafka | Consumption operations | Continuous message flow |

Each connector must declare its capabilities.
Transactions, cancellation, pagination, and continuous consumption do not behave the same on all sources.

**Defined:** do not create a plugin system in this phase.

## 4. Model terms

Use these terms consistently in the architecture design:

| Term | Meaning |
|---|---|
| Connection definition | Access configuration of a source, with identity and scope |
| Physical connection | Active resource used by the driver to reach the database |
| Pool | Manager of the physical connections of a connection definition |
| Console | Working document with editor, execution, and results |
| Execution | Request to run a statement, a selection, or a script |
| Statement | Individual command identified in the editor text |
| Result | Data set or response produced by a statement |
| Global scope | Configuration and documents accessible outside a specific project |
| Local scope | Configuration and documents belonging to the found project |
| Session password | Password kept only in memory during the current run of the application |

Do not confuse a session password with a transaction or a database session.

## 5. Consoles and editor

### Defined

- Allow several open consoles.
- Allow only one pending or active execution per console.
- Do not start or queue another execution in the same console while the previous one stays pending or active.
- Allow parallel execution on different consoles, according to the pool availability.
- Use an integrated text editor.
- Allow work in one console while another runs or waits for a connection.
- Keep the results attached to the corresponding console.
- Show editor and results together in the console view, without requiring a switch between full screens.
- Support the mouse on capable terminals.
- Offer conventional IDE shortcuts and Vim modes.
- Allow configuring the shortcuts and the editing mode.
- Offer autocomplete of schemas, tables, views, and columns from the first version.

### Execution in the editor

**Defined:** the default **Run** action uses the selected snippet.
Without a selection, it runs the statement under the cursor.
Offer a separate action to run the whole script.

Apply the same reservation, row cap, and stop at the first error rules to those executions.
The SQL segmentation and the mapping between statements and editor positions still need technical validation.

**Defined:** if the cursor is outside a statement, do not run and report the reason.

**Defined:** allow editing the console text during its own execution.
The execution uses a copy of the text taken when the request was accepted.

### Editor features

**Defined for the first version:**

- Offer search and replace.
- Require SQL syntax highlight.
- With suggestions open, `Tab` accepts the suggestion and `Enter` breaks the line. The `suggestionKeys` option changes the key that accepts: `tab` (default), `enter`, or `both`. The user defined this option on 2026-10-31.
- The search compares the literal text, case-insensitive.
- `Tab` without suggestions inserts four spaces. With a selection, it has no effect.

**Defined for conventional undo:** keep replaced text in `editor::undo`, with `undoLimitBytes` per console, 64 MiB by default.
There is no step-count limit. The newest undo and redo steps stay even above the byte limit.
Vim uses official EdTUI stacks, with up to 100 full-text entries each and no memory cap.
Section 1 records the Vim memory debt. Neither mode limits the editor text size.

### Technical points to validate

- Selection, undo, redo, copy, and paste.
- Shortcut compatibility with Terminal.app and Linux terminals.
- Separation between editor shortcuts and application shortcuts.
- Autocomplete behavior in the conventional and Vim modes.
- Limits of the chosen widget to integrate suggestions and execution indicators.

The Emacs mode of a library does not automatically equal the conventional shortcuts of an IDE.

### Mouse and panel sizes

Mouse support must complement the keyboard navigation.
Crossterm captures the needed events; the application needs to map them to the interface actions.

**Defined:** allow dragging dividers to resize the panels and collapsing the side navigator.
The editor and the results stay visible together in the console view.
Collapsing the navigator does not replace that division with a full-screen editor or result.

**Defined:** offer these mouse interactions:

- Click to select items and change focus.
- Click on tabs to switch console or result.
- Use the wheel to scroll lists and grids.

The double click on a table belongs to the table view described in section 10.
**Defined:** allow selecting cells by drag in the grid.
**Defined:** allow selecting text by drag in the editor, in both keyboard modes.

### Vim navigation

**Defined:** offer Vim navigation in the whole interface, not only in the editor.

That support covers the navigator of connections and objects, lists, tabs, grids, and dialogs.
The mouse stays available in both keyboard modes.

Treat the shortcuts according to the context and the focus.
During text input, keys such as `j` and `k` write characters, without navigating to another item.

**Defined mappings:**

- `j/k` to walk items and rows outside fields in text input.
- `h/l` for horizontal navigation, according to the panel.
- Focus commands separate from the editing actions.

**Default mode:** conventional.
Allow enabling the Vim mode through configuration.

The shortcuts of each context and their priorities are defined in [docs/KEYMAP.md](KEYMAP.md).

**Defined:** no essential action depends only on `Ctrl`+arrows, `Home`, `End`, `PageUp`, or `PageDown`.
On macOS, part of those combinations does not work, and many keyboards lack those keys.

### Appearance, language, and help

**Defined:**

- Use English in the interface, with the texts isolated for future translation.
- Use the `astrodark` theme, in RGB, when `COLORTERM` declares `truecolor` or `24bit`.
- In the other cases, use the ANSI colors of the terminal.
- Respect `NO_COLOR`, with priority over the environment and the configuration.
- Choose `theme`: `auto`, `astrodark`, `ansi`, `tokyo-night`, `catppuccin-mocha`, `catppuccin-latte`, `gruvbox`, or `nord`.
- Choose the symbols through the `icons` field: `unicode` (default) or `nerd`.
- Show a hint bar and a help screen.
- Show the effective keys of the `keymap` in the bar, the `F1` help, and the `Ctrl+W` prefix options.
- Do not show a shortcut whose mapping the user removed.
- Show only the actions available in the current panel.
- Offer optional logs, enabled by `--log <file>`, without SQL or values.

### Terminal behavior

**Defined:**

- On a small terminal, show a message, without losing the state.
- In the editor, `Ctrl+C` copies a selection. In a grid, it copies a selection or the current cell.
- On a panic, restore the terminal, write the pending texts, and generate a report.
- Do not include SQL, values, or passwords in that report.
- On a shutdown signal, write the pending texts and exit.
- Do not interpret external data, names, and errors as terminal control sequences.

## 6. Autocomplete and catalog

### Defined

**Defined coverage for the first version:** schemas, tables, views, columns, and CTEs (`WITH`).
Suggest the names of the CTEs available in the current statement and their columns when the SQL allows identifying them.
Respect aliases and the scope of the subqueries.
Do not query the database to discover columns of a CTE.
Do not suggest columns when the analysis cannot determine them.
**Expanded coverage:** suggest built-in types in `CREATE TABLE`, `ALTER TABLE`, and `CAST`, according to the connection dialect.
Suggest column options, functions with parameter signatures, and query clauses according to the cursor context.
Accept a function as `COUNT()` and put the cursor inside the parentheses.
Keep existing parentheses and arguments when the function name already has them.
Use local lists for built-in types and functions. Do not query the database for each key.

**Defined:** the navigator shows schemas, tables, views, columns, and groups of supported routines, sequences, triggers, and types.
These extra objects open their DDL. They do not add user-defined function completion.

**Defined:** update the catalog by manual action and after DDL run in the application.

**Defined:** read schemas and tables first and read the columns when each table is used.

### Defined approach

Combine three pieces of information:

1. Text and cursor position.
2. Catalog of the source.
3. Rules of the dialect used by the connection.

Example of desired behavior:

```sql
SELECT u.
FROM users AS u;
```

Suggest the columns of `users` after `u.`.
The exact support of aliases and other constructs will be detailed in the architecture.

The catalog must stay cached to avoid a database query on each key.

Rules to respect the connection model:

- The catalog read uses the same connection limit of the source.
- Do not open a hidden extra connection to load metadata.
- Use the already loaded catalog when a connection is busy with a transaction.
- Allow the first catalog read to wait for a free connection.

### Autocomplete decisions

**Defined:**

- Keep the catalog cache only in memory.
- Use an own tokenizer per dialect, tolerant to incomplete text.
- Without a catalog available, suggest the keywords of the dialect.
- Suggest qualified names, quoted identifiers, and aliases.
- Open the list while typing a name or a dot, as in the JetBrains IDEs; `Ctrl+Space` opens the list without a prefix.
- Keep the list closed in comments, strings, and numbers.
- Follow the typed case in keywords: `sel` suggests `select`, and `SEL` suggests `SELECT`.
- Quote names that need quotes, according to the dialect.

**Defined:** the binary does not use the example catalog.
A console without a connection suggests only the keywords of the dialect.
The `--no-catalog` option stops existing, because it loses its role.
The tests keep the example catalog.

## 7. Pool and concurrent execution

### Defined

A connection definition needs to offer the option to use only one physical connection.

In that mode, the consoles do not open a physical connection of their own.
The requests wait for a free connection in the pool.
The temporary cancellation connection of this section is the only exception to that limit.

Use one pool per connection definition, with a configurable limit.

**Default limit: 5 physical connections per connection definition.**
Allow changing that limit, including to a single connection.
That value does not represent a limit shared between all the definitions of the application.

| Situation | Behavior |
|---|---|
| A free connection exists | The execution can take that connection |
| All the connections are busy | The execution waits in the pool |
| Limit equal to one | The consoles share the only available connection |
| Limit above one | The executions can happen at the same time up to the limit |

The wait for a connection does not block the editor or the interface.

### Reservation rules

- Reserve the same physical connection through the whole script, including in autocommit.
- Do not return the connection to the pool between the statements of the same script.
- A manual transaction reserves its connection for the corresponding console.
- Another console cannot run commands inside that transaction.
- Other consoles use free connections, when they exist.
- Other consoles wait only when no free connection exists.
- A read in progress keeps the connection busy.

The reservation during the script keeps session settings and temporary tables between its statements.
Reserving the connection does not start a transaction and does not turn off autocommit.

Example with three connections:

```text
Connection 1 → Console A: open transaction
Connection 2 → Console B: query running
Connection 3 → Console C: query running

Console D → waits for a free connection
```

The limit must also apply to internal operations, such as asking for tables and columns.

### Cleaning before the return

**Defined:** clean the session state before returning the connection to the pool.
Prevent another console from inheriting `SET` settings, temporary tables, or other transient state of the previous execution.

- Clean only when there is no script in progress, read in progress, or active transaction.
- Use the already reserved connection, without opening an extra connection for that cleaning.
- Do not use the cleaning to change persistent data of the database tables.
- Keep the connection unavailable to other executors during the cleaning.
- Count the connection in cleaning inside the configured pool limit.

**Defined:** drop a connection whose cleaning cannot be confirmed.
Do not try to recover it.

### Wait and cancellation of the wait

**Defined:** wait for a free connection without a maximum queue deadline.
Offer the option to cancel that wait.

- Keep the interface and the editors available during the wait.
- If the cancellation is confirmed before the execution, do not send the SQL to the database.
- Do not cancel or end transactions or queries of other consoles.
- Tell the cancellation of the queue apart from the cancellation of a query already running.

The absence of a deadline applies to the wait for availability in the pool.
Do not apply that policy to the establishment of a network connection or to the duration of a query.
Connection errors or pool shutdown stay as failures, not an indefinite wait for availability.

### Connection establishment deadline

**Defined:** use a configurable default deadline of **30 seconds** to establish a connection with the database.

That deadline does not limit the wait for availability in the pool.
It also does not define a duration limit of the SQL queries.
Tell those phases apart in the implementation and in the interface messages.
The time the user spends on the password prompt does not count as network connection establishment.

### SQL execution deadline

**Defined:** offer a configurable SQL execution timeout, with a default of **30 seconds**.
Allow turning that timeout off to run without a maximum deadline.
That value is independent of the 30 seconds of connection establishment and of the deadline-less pool wait.

On a timeout, report the reason and do not start the next statements of the script.
Do not run commit, rollback, or SQL repetition automatically because of the timeout.
Do not promise immediate interruption at the server or claim that a change command produced no effects without database confirmation.

**Defined counting:** apply the timeout to each statement separately, not to the whole script.
Subtract the pool wait.
Include the row read in the statement deadline.
Each new statement receives its own configured deadline.
In the table view, each page query receives its own deadline.
The interruption and finalization strategy depends on the validation of each driver.

### Interruption of a running statement

**Defined:** cancel first and close the connection only as the last resort.
Apply this sequence to **Stop script** and the SQL timeout:

1. Send the cancellation to the database through a temporary connection.
2. Wait 5 seconds for the statement response.
3. Without an open transaction, close the physical connection.
4. With an open transaction, ask for confirmation before closing, because that ends the transaction and loses the uncommitted changes.

If the user refuses the close, keep waiting for the end of the statement and do not send the next ones.
If the temporary connection cannot be opened, continue from step 3.
After cancelling, keep reading and discarding the rows until the statement response arrives.
Do not abandon the read, because that can block the server and delay the cancellation.
Do not reuse the connection before receiving the statement response.
A late cancellation must not hit another statement.

The temporary connection stays outside the pool limit and ends right after the request.
It runs no user SQL and receives no reservation.
It follows the TLS and credential rules of the connection definition.

After closing the connection, report that the result of the statement is unknown.
Open a replacement connection only when a new need exists and respecting the pool limit.
Do not run a rollback automatically after a cancellation.
Report the database effect to the user, without presenting it as an application action:

| Database | Effect of the cancellation on an open transaction |
|---|---|
| PostgreSQL | The transaction stays with an error until `ROLLBACK` |
| MySQL | The statement is undone, and the transaction stays open |
| SQLite | A change command undoes the whole transaction; a read keeps it |

A cancelled statement can return success, such as `SLEEP` on MySQL.

The cancellation may have no effect, and the database does not confirm the request directly.
Closing the connection also does not guarantee the stop of the command at the server.
A change command can finish and commit after the close.
The tests confirmed that effect on PostgreSQL and MySQL.
On PostgreSQL, `client_connection_check_interval` prevented that effect in the test.
The old command can keep locks and occupy a server session.

### State between executions and pending validations

**Defined for the first version:** do not guarantee the preservation of that state between separate executions of the same console, outside a transaction.
A later execution can receive another physical connection from the pool.
Do not reserve an idle connection only to preserve the console session.

The same connection stays guaranteed during the script and during an open manual transaction.

**Defined:** offer a database or schema selector in each console.
Apply the choice to each execution, even when it receives another physical connection.
Do not change the console text to apply that choice.
On PostgreSQL, the selector changes the schema, not the database.
`USE` and `SET` commands typed by the user stay limited to the execution or the transaction.

**Defined:** do not detect two definitions that point to the same physical database in the first version.
Do not limit the request queue, because each console keeps only one execution.

The cleaning per database is defined in section 14.

Do not assume that dropping a Rust task cancels the query at the server at once.

## 8. Transactions and autocommit

### Defined

New consoles start with **autocommit on by default**.
Allow switching to a manual transaction per console.
Offer a selector in the console bar and a shortcut for that switch.

In autocommit, SQL changes commit automatically according to the database semantics.
Return the connection to the pool only after the script ends and no active transaction exists.

In a manual transaction, the console keeps the connection until `COMMIT` or `ROLLBACK`.
Even after the transaction ends, a script or a read still in progress keeps its reservation.

A transaction reserves one connection, not the whole pool.

If an error leaves the transaction open, the connection stays reserved.
Do not return to the pool a connection with a transaction that still belongs to the console.

Ending the read of a result does not end a manual transaction.

### Availability of commit and rollback

**Defined:** disable the commit and rollback buttons while an execution stays pending or active.
That restriction includes the wait for connection, the read, and the finalization.
Do not use those buttons to interrupt the running statement sequence.

Make those controls available after the execution ends, when an open transaction still exists and the connection allows the action.
An open transaction does not block, by itself, a new execution in the same console.

That rule covers the buttons, not the SQL transaction commands that are part of the running script.
The close decisions are processed after the needed finalization of the execution or read.

### Console close

**Defined:** ask for confirmation when closing a console with a pending or active execution.

If the user confirms:

- Cancel the wait if the SQL has not started yet.
- Do not send the next statements of the script to the database.
- Finish the current statement or read before completing the close.
- Do not promise immediate interruption of a command already sent to the server.

If the user cancels that confirmation, do not request the stop and do not close the console.
Stopping the sequence does not undo commands already committed by the database.

If there is an open transaction, ask the user what to do:

- **Commit and close.**
- **Rollback and close.**
- **Cancel close.**

Without a choice, the console stays open.
Do not run commit automatically in that flow.
Do not restart automatically a script whose stop was already confirmed.

### Commit or rollback failure at close

**Defined:** keep the console open if the commit or rollback requested at close fails.
Show the error and offer a retry, when the connection state allows it, or cancel the close.
A retry needs an explicit action and uses the original session, only if it stays usable and the transaction state allows it.
Do not repeat the operation automatically or run it on another connection as a replacement of the original transaction.

If the connection is lost and the result cannot be confirmed, report that the transaction result is unknown.
Do not present a communication failure as proof of commit or rollback.
During application exit, resolve that dialog before moving to another console.
Cancelling the close does not undo actions already finished in the previous consoles.

### Application exit

**Defined:** ask separately for each console with an open transaction.
Do not replace those questions with a single summary screen.

Each question offers commit, rollback, or cancel of the exit.
Cancelling the exit keeps the application open.
Do not end the application with a pending transaction without the corresponding decision.
That rule applies to the exit requested by the interface.
A shutdown signal does not allow those questions.

Apply the console close finalization rules to pending or active executions.

**Defined moment:** run the choice of each console at once, before presenting the question of the next console.
Do not collect all the choices to apply them only at the end.
Process the result of the action before moving forward.

If the user cancels a later question, stop the exit and keep the application open.
Do not undo or announce as undone commits or rollbacks already finished in the previous questions.

### Switch to autocommit

**Defined:** block the switch between autocommit and manual mode while an execution stays pending or active in the console.
That restriction includes the wait for connection, the read, and the finalization.
Do not schedule a switch for after the execution.

Without a pending or active execution, when autocommit is requested with an open transaction, ask the user:

- **Commit and enable autocommit.**
- **Rollback and enable autocommit.**
- **Cancel the switch.**

Do not commit the transaction just because the user requests the mode switch.
Enable autocommit only after the chosen commit or rollback finishes with success.
Cancelling the switch does not request commit or rollback and keeps the previous mode.
If the action fails, report the error and do not enable autocommit automatically.

An open transaction does not represent, by itself, an active execution.
The user can run another statement in the same transaction after the previous execution ends.

### Control through SQL

**Defined:** allow controlling transactions through SQL in the editor, besides the interface controls.
That includes commands such as `BEGIN`, `COMMIT`, and `ROLLBACK`, according to the database dialect.

- The SQL commands and the interface controls act on the same session and transaction.
- Update the presented state according to the effective result of the commands.
- Reserve the connection when the SQL starts a transaction, even if the console is configured with autocommit.
- Keep that reservation between separate executions while the transaction stays open.
- Do not return the connection in the middle of a script just because an SQL statement ended the transaction.
- Do not commit automatically a transaction started by SQL just because of the default mode of the console.

The detection needs to consider the database state, not only words found in the editor.

## 9. Scripts, errors, and indicators

### Defined

Run the statements of the script in order.
Stop at the first error.
Do not run the next statements after that failure.

Show visually, beside the statement, whether it ends with success or error.

Illustrative example:

```text
✓  SELECT id FROM users;

✗  SELECT id FROM missing_table;

—  SELECT id FROM orders;
```

Defined states to represent the execution:

| State | Meaning |
|---|---|
| Running | The statement is in progress |
| Success | The statement ends without error |
| Error | The statement fails |
| Not run | The execution does not reach the statement |
| Stopped | The user interrupts the statement |
| Timeout | The statement exceeds the configured deadline |
| Cap reached | The statement ends the read at the row cap |

Use the same states in the indicators and in the execution of section 14.
The symbols and the exact position of the indicators still depend on the screen design.

### Indicator semantics

- Execution success does not mean commit.
- The console shows the transaction state separately.
- Rows received do not guarantee final success.
- An error can happen during the read of the next rows.
- The statement does not receive final success while its execution can still return an error.

The interface must allow consulting the database error message.

**Defined:** a script that ends normally shows the last result.
Any other end shows the **Messages** tab, because it explains the reason.

### Indicator after an edit

**Defined:** remove the indicator of a statement when the user changes its text.
That avoids presenting an old execution as the result of the current text.

### Specific commands

**Defined:**

- **`DELIMITER`:** accept on MySQL, handling the command in the client.
- **`COPY FROM STDIN`:** refuse with a clear message.
- **`CALL` with several results:** show one tab per result.
- **`EXPLAIN`:** show in the common grid. A plan from `Ctrl+W E` shows as a tree; see `docs/DECISIONS.md`.

### Technical points to validate

- Identify statement limits according to the dialect.
- Do not separate scripts only by a simple split on `;`.
- Preserve semicolons inside texts, comments, and database constructs.
- Map errors to the correct editor snippet.
- Handle several statements on the same line and statements with several lines.
- Tell error, user interruption, and not-run statement apart.

## 10. Results and pagination

### Defined

The grid allows consulting and copying data.
The user reopens direct cell editing on 2026-11-04; see the Grid editing decision in `docs/DECISIONS.md`.
Only eligible table tabs allow updates. Other grids stay read-only.
The user can still run change commands in the editor.

Use separate tabs per result.
Each result stays attached to the console and to the statement that produces it.

**Defined:** offer a **Messages** tab for notices, errors, and commands without rows.

### Grid presentation

**Defined:**

- Compute the column widths automatically, with a cap, and allow manual adjustment.
- Show long values in a detail window.
- Show a result without rows with the columns and the zero-row indication.
- Select data by range.
- Search only in the loaded rows, without querying the database.

The search reads the rows in 8 ms slices between keys.
It compares the full text of each value, case-insensitive.
`NULL` counts as the text shown in the grid.
The list keeps up to 1,000,000 results, and the count shows `+` at that limit.

### New execution in the same console

**Defined:** clear the previous result tabs when accepting the request of a new execution in the same console.
Do not keep a history of those tabs between executions.
Free the discarded data and their memory budget.
Keep the editor text and the transaction state.
Do not run commit or rollback as a consequence of that clearing.
Do not clear results of other consoles or of the table views.

The clearing happens when the request is accepted, even if the new execution needs to wait for a connection.
An attempt blocked by a pending or active execution does not clear the current results.

### Value representation

**Defined:** use the text provided by the database as the grid value.
Use the column type only in presentation, such as aligning numbers or formatting JSON.
Do not convert time zones or change the precision of the values.

### Data copy

**Defined:** copy data as **TSV without headers** by default.
Separate columns by tab and rows by line breaks.
Do not include column names automatically.

**Defined reference:** follow the pattern of the built-in TSV extractor of DataGrip/JetBrains, keeping the decision of not including headers.
Use that extractor as reference for escape, quotes, and representation of null and binary values.
That does not include implementing all the extractors, an extractor editor, or automatic export to files.

**Confirmed format:** a sample of DataGrip 2026.2, with the default TSV, was compared byte by byte.

| Case | Output |
|---|---|
| Separators | Tab between columns and LF after each line, including the last |
| Header | Absent |
| `NULL` | Empty field |
| Empty string | `""` |
| Value with tab, LF, or quotes | In quotes, with the inner quotes doubled |
| Backslash and the text `NULL` | Unchanged |
| Binary with valid text | The text itself |
| Other binaries | `0x` followed by uppercase hexadecimal |

**Defined:** diverge from DataGrip only in the cases where it loses data.

| Case | DataGrip | SQLN |
|---|---|---|
| Trailing `NULL` columns on the line | Removed | Keep the tabs |
| Trailing spaces of the last value | Removed | Keep the spaces |
| Carriage return (CR) | Becomes LF, without quotes | Keep the CR, in quotes |

Dates follow the value representation of this section, with the database text.
DataGrip formats dates in the client, such as `2026-10-30 12:34:56.000000`.
The sample does not cover a result without rows.

The copy is an explicit action; it does not turn the result storage into automatic persistence.

### Clipboard

**Defined:** use the system clipboard when available.
When unavailable, try OSC 52 on capable terminals, including for use over SSH.
Do not use temporary files to copy data.

Report copy failures or terminal incompatibility, without presenting an unsuccessful transfer as complete.
The OSC 52 support depends on the terminal and its permissions.
Without a delivery confirmation, report that the request was sent to the terminal, without guaranteeing the clipboard content.
Do not read the clipboard just to try to confirm a copy.
The copy is explicit; do not copy results automatically when loading them.

### Result storage

**Defined for the first version:** keep the result rows only in memory.

- Keep the loaded rows while the corresponding tab stays open.
- Discard the rows when the tab closes.
- Do not use temporary files or automatic persistence of the application to store the grid rows.
- Do not include those rows in the automatic saving of the consoles.
- On reopening a console, recover the script, without restoring its previous results.
- Do not run queries automatically to rebuild results on reopen.

That rule also applies to the data of the table views.
It does not control the internal storage used by the database itself while running the queries.

### Grid data budget

**Defined:** use a configurable global cap of **256 MiB**, equal to **268,435,456 bytes**.
That budget covers the data kept for all the grids of the instance, from consoles and table views.
Do not apply that value separately to each result or connection.

When the budget is reached:

- Suspend new row loads.
- Keep the already loaded rows, without erasing them.
- Report that the data budget was reached.
- Allow freeing space by closing tabs or raising the configured limit.
- In the consoles, treat the budget as the row cap and end the statement read.
- In the table view, refuse **Load more** until space exists.

Do not use temporary files as an alternative to the limit.
Do not run commit or rollback automatically because of the budget.
Freeing space does not authorize loading rows the user has not requested yet.

The cap controls the grid data, not the total memory of the process.
Internal driver buffers and other components can consume extra memory.
**Defined for large values:** indicate when a value exceeds the available limit or the configured cap.
Show that the limit prevents the load, keeping the already loaded rows.
Do not represent that value as `NULL`, empty string, or truncated content without a warning.
Keep the load suspension described above; that indication does not authorize passing the grid cap.

The accounting and the detection of large values are implemented and tested with SQLite.

### Table view

Offer an own view when opening a table in the object navigator.
Use as reference the double click on a table in the JetBrains IDEs.

That view differs from the console view with editor and results together.
Show only the table data in this phase.
The structure stays in an own tab. The user reopened that rule on 2026-10-31; see `docs/DECISIONS.md`.
Allow opening views in that view, with a label that identifies them.
The catalog read for autocomplete stays needed.
The table grid follows its own pagination and the approved Grid editing rules in `docs/DECISIONS.md`.
It also respects the pool and its connection limit, without a hidden extra connection.

Tables and consoles share the same main tab strip.
That organization is confirmed.

Opening again an already open table only selects its existing tab.
Do not create a second tab for the same table.
The identification considers the connection and the qualified table name, not only its simple name.
That rule does not mix table tabs with query results of the consoles.

**Defined layout:**

- A console tab shows editor and results.
- A table tab uses the central area for the data, without needing a visible SQL editor.
- The navigator of connections and objects stays available in both views.

#### Table view pagination

**Defined:** fetch each page with a new query.
Do not keep a read open between pages.
Use a pool connection only during each query and return it after.
**Load more** can wait for a free connection.

- Load **100 rows by default** on each page.
- Allow configuring the page size.
- Offer **Load more** when more rows exist.
- Append the new rows to the previous ones, without replacing the already loaded rows.
- With pages of 100 rows, go from 100 to 200 loaded rows, and so on.

The application must report how many rows it already carries.
The details of navigation between pages and rows follow the subsection "Approved table view design".

Without a unique sort, the database does not guarantee the same order between queries.
Changes of other sessions between the pages can also affect the result.
In those cases, rows can repeat or go missing.
That decision does not change the rule of not applying an initial automatic sort.

**Defined:** offer the action to stop a table view query, with the interruption sequence of section 7.

**Defined:** report that risk in the table view when no sorting is applied.

**Defined:** ask for one extra row on each query to know whether a next page exists, without counting the total.
**Defined on 2026-11-01:** the `c` key counts the rows on demand. The pagination continues without counting. See `docs/DECISIONS.md`.

#### Filter and sorting bar

**Defined:** include a bar above the grid with a `WHERE` filter and sorting, following the JetBrains reference.
That feature enters the first version of the table view.

- Apply the field edit with `Enter`, when the focus is on that bar.
- Do not run queries automatically while typing.
- Offer autocomplete in `WHERE` and `ORDER BY`, besides the main editor.
- Allow sorting by click on a column header.
- Apply at once the sorting chosen by click, without requiring `Enter` for that gesture.
- Keep the `WHERE` already applied when sorting by click.
- Do not silently apply a `WHERE` draft that was not confirmed yet.
- Update the sorting representation in the `ORDER BY` field.

**Defined keys in the filters:** `Tab` accepts the autocomplete suggestion.
Accepting a suggestion with `Tab` does not run the query.
`Enter` applies the current content of the fields even when the autocomplete list is open.
Do not use `Enter` to implicitly accept the highlighted suggestion in that bar, unless `suggestionKeys` is `enter` or `both`. In that case, `Enter` accepts without running, and the next `Enter` applies.

In the main editor, `Enter` breaks the line, according to section 5.

**Defined initial sorting:** do not apply an automatic sort when opening a table.
Do not sort by primary key by default.
The user chooses the sorting through the field or the header.

**Defined:** use this format and this query generation, according to the consulted reference:

- `WHERE` field with the SQL condition, without repeating the word `WHERE`.
- `ORDER BY` field with columns and directions, without repeating the words `ORDER BY`.
- Allow sorting by several columns, such as `created_at DESC, id ASC`.
- Run filter and sorting at the database, not only over the already loaded rows.
- On apply, start a new table query and return to the first page.
- Keep the on-demand pagination and the configured page size.
- Do not change the console text to apply the table view filters.

Field content examples:

```text
WHERE    active = true
ORDER BY created_at DESC, id ASC
```

**Defined:** refuse a filter or a sorting that do not form a single statement with the table query.
Validate with the tokenizer before sending the query, because the drivers accept several statements.
Put `LIMIT` on a new line, so a `--` comment in the filter does not remove it.

**Defined error handling:** if the filter or the sorting are invalid, clear the grid and show the error.
Do not keep the data of the previous query as an alternative to the failed query.
Keep the fields available to correct the text.
An old response cannot refill the grid after that failure.

#### Lifetime of the table state

**Defined:** keep filters and sorting only while the table tab stays open.
When the tab closes, discard that state.
Reopening the table starts without filters or sorting, even if the previous tab had applied values.
Reopen the table tabs of the last session with `reopenTabs`. The tab loads the rows on the first selection; a tab that starts on screen waits for `Enter` or **Apply**.
That rule does not change the saving of text, cursor, and scroll of the consoles.

**Defined:** offer the `[Apply]` and `[Clear]` buttons in the bar.
`[Apply]` has the same effect as `Enter`.
`[Clear]` clears the fields and applies the query without a filter or sorting.

The layout and the identification of the table tabs follow the subsection "Approved table view design" below.

#### Approved table view design

**Defined:** the user approves this design on 2026-10-31.
The code implements the points below.

- `Enter` on a table or view of the navigator opens the table view. The double click within 400 ms on the same cell does the same.
- `Right` and `Left`, and the click on the row mark, open and close the columns in the navigator.
- The tab label is `name (table)` or `name (view)`, and a tab with a query in progress shows `●`.
- The tab strip shows consoles and tables together in opening order, without separation by type. Diagrams and compares share that order.
- The view has, from top to bottom: the title, the `WHERE` and `ORDER BY` fields, the buttons, the grid, the summary, and one warning line.
- The title shows the connection, the schema, the relation, and the type. The pool and data use stays at the right.
- The buttons are `[Apply]`, `[Clear]`, `[Load more]`, and `[Stop]`. A button without effect at the moment appears dimmed.
- With more than one page, the line under the grid shows `[Previous]`, `Page 2 of 3+`, and `[Next]`. Each page is a slice of the accumulated rows, and `[` and `]` do the same in the grid. The user asked for these buttons on 2026-10-31.
- The summary shows the loaded rows and the state: loading, more rows, no more rows, or budget reached.
- The warning line shows the unstable order risk while no sorting is applied. An error or stop message takes the place of the warning.
- The error of a first page appears in the grid area. The error of a later page keeps the rows and appears in the warning line.
- The header click, or the `s` key, sorts by one column in ascending order. Another click on the same column inverts the order, and a third removes the order.
- Another sorting, such as two columns, restarts from ascending when a header is clicked.
- Apply, clear, sort, and load more refuse the request while the tab has a query in progress.
- `Ctrl+W k` goes to the `WHERE` field, and `Ctrl+W j` returns to the grid.
- `Ctrl+Space` opens the suggestions in a field. The field keeps one line, and pasted text loses its line breaks.
- Closing a table tab with a query in progress stops the query and frees the data that arrive later.
- Removing a connection closes its table tabs. A table query in progress blocks **Disconnect** and the removal.
- Closing the last console with another tab open selects a remaining tab and does not create an empty console.
- Closing the table tab on screen selects the tab at the right or, at the end of the strip, the tab at the left.
- The page size comes from `preferences.pageSize` in `config.json`, with 100 as default.

### Row cap in the consoles

**Defined:** read each console result up to a row cap and end the read.
Do not keep a read open waiting for the user.
Keep the read rows in the grid and allow walking them without a new query.
Do not offer **Load more** in the consoles.
Do not repeat the command to obtain more rows.
With **Limit in SQL** on, the runner adds `LIMIT` with the cap plus one to eligible plain reads.
The editor text stays unchanged. **Messages** records the added limit. See the 2026-11-03 decision.

Allow configuring the cap.
**Defined:** use 1,000 rows as default.

At the cap, report that the result may have more rows.
To see more rows, the user raises the cap and runs the command again.
That new execution is always an explicit action.

The driver or the server can use own buffers.
Do not promise that the database execution or the physical transfer stays exactly within the cap.

#### End of the read at the cap

**Defined:** choose the action according to the statement and the transaction state.

| Situation | Action at the cap |
|---|---|
| Read query, without an open transaction | Cancel the statement and wait for its response |
| Open transaction | Consume the remaining rows without keeping them |
| Change command with rows in return | Consume the remaining rows without keeping them |
| Unidentified command type | Consume the remaining rows without keeping them |

**Defined:** a query only counts as a read when the text has no write words or calls of unknown functions.
A fixed list gathers the common functions that do not write, such as `count`, `lower`, and `coalesce`.
An unknown function counts as an unidentified type, because it may write data.
That classification error costs only time, and the statement deadline stays valid.

The cap cannot change the effect of a command.
Cancelling a change command can undo it.
On PostgreSQL, cancelling inside a transaction leaves the transaction with an error.
The text does not prove that a query is read-only, because it can call a function that writes data.
Consuming the remaining rows counts in the SQL timeout and can take time on large results.

**Defined for SQLite:** end the read at the client when `sqlite3_stmt_readonly` confirms the read, with or without a transaction.
In the other cases, consume the remaining rows.
SQLite applies `INSERT`, `UPDATE`, and `DELETE` with `RETURNING` in the first step, before delivering rows.

### Effect on scripts

1. Run the current statement.
2. Read the rows until the result ends or the cap is reached.
3. End the read.
4. Move to the next statement, without waiting for a user action.
5. Stop at the first error, including during the read.

### Wait states

| State | Reason |
|---|---|
| Waiting for connection | The pool does not provide a free connection yet |

A console execution does not wait for a user action to continue the read.

### Stop script without closing the console

**Defined:** offer a **Stop script** action independent of the console close.
If the execution still waits for a connection, cancel the wait without sending SQL.
If it is already active, request its interruption and do not send the next statements.
Keep the console open, its text, and the already loaded rows.
Do not request commit or rollback automatically because of that action.
Do not promise immediate cancellation of the command at the server.

Apply the interruption rule of section 7 to an active statement.

The connection can only be reused after the safe finalization planned for the driver.

The cancellation sequence and the state of each database are in sections 7 and 14.

### Close of a result tab

**Defined:** do not allow closing a tab while its statement still reads rows.
Report that **Stop script** interrupts the execution.
With the read finished, close the tab without confirmation.
The other tabs keep their numbers, and **Messages** does not close.

- Closing a result tab does not interrupt the script.
- Do not close the console just because a result tab gets closed.
- Do not request commit or rollback automatically in that flow.

### Other pending points

Do not rerun commands automatically to get more rows.

## 11. Global and project settings

### Defined

Global settings and project settings exist.

Those scopes cover:

- Connection definitions.
- Consoles and their scripts.
- Configurable preferences of the application.

### Project identification

Look for the `.sqln/` folder in the current directory and the upper directories.
Use the found folder closest to the current directory.

Do not use the presence of `.git/` as a replacement for that rule.

**Defined:** if `.sqln/` is not found, ask whether the user wants to create a project in the current directory.

- If accepted, create `.sqln/` in the current directory.
- If refused, continue only with the global settings.
- Do not create the project folder without confirmation.
- Do not replace that question with a silent choice of the global scope.

**Defined:** offer the option `--project <folder>` to indicate the project on the command line.
That option replaces the search in the upper directories.

**Defined:** also offer `--log <file>`, `--version`, and `--help`.

### Visible connections

Show global and local connections together.

When the names collide, add the origin mark:

```text
analytics
production (local)
production (global)
```

The annotation in the name is needed when a collision exists.
A local connection does not silently replace a global connection with the same name.

Each console references the identity and the scope of the connection, not only its visible name.

### Global path and format

**Defined:** use `~/.config/sqln/` as the default global folder on macOS and Linux.
Respect `XDG_CONFIG_HOME` when it is absolute: use its `sqln/` subfolder. Ignore a relative value.

Use **JSON** for the settings, both global and project.
The SQL console scripts stay in `.sql` files.
Include a `version` field in each JSON file.

### File path resolution

**Defined:** resolve relative paths according to the scope of the connection, not according to the current directory of the terminal.

| Scope | Resolution base |
|---|---|
| Project | Project root, which contains `.sqln/` |
| Global | Global settings folder, respecting `XDG_CONFIG_HOME` |

That rule applies to SQLite files and certificate files.
Absolute paths stay absolute.

**Defined on 2026-10-31:** the rule above applies to a relative path saved in `config.json`.
In the form, a typed SQLite file starts from the current directory, and the connection saves the full path.
A saved and unchanged path keeps its text and the base of the scope, in the test and in use.
The test and the saved connection always open the same file.

Example: `data/app.db` in a project connection points to `<project>/data/app.db`.
The base is not `<project>/.sqln/`.
A global connection keeps its global base even when used inside a project.

### File organization

Use `config.json` as the name of the settings file of each scope.
**Defined:** keep the metadata of each console in a `.json` beside its `.sql`.
Allow versioning those two files.
Keep cursor and scroll in the local state, ignored by Git.

**Defined:** use this organization of the subfolders and of the interface state:

```text
~/.config/sqln/           ← or $XDG_CONFIG_HOME/sqln/
├── config.json
├── consoles/
│   ├── <console-id>.sql
│   └── <console-id>.json
└── workspace.json

<project>/.sqln/
├── config.json
├── consoles/
│   ├── <console-id>.sql
│   └── <console-id>.json
└── workspace.json
```

Separate the data according to its purpose:

| Item | Content |
|---|---|
| Configuration | Preferences, shortcuts, and connection definitions in JSON |
| Consoles | Editor texts in `.sql` files |
| Console metadata | Identity, visible name, connection ID, and selected database or schema |
| Working state | Cursor position, scroll, and other interface data |

The JSON schema is in [docs/ARCHITECTURE.md](ARCHITECTURE.md).
The name and the content of `workspace.json` do not represent a final contract.

### Project files and Git

**Defined:** allow versioning the project settings and scripts.
Ignore the local interface state in Git.

- Do not ignore the whole `.sqln/` folder by default.
- Separate versionable files from the state specific to the machine or the session.
- Do not add files to Git or create commits automatically.
- Do not save connection passwords in the project configuration.

The scripts can hold sensitive data typed by the user.
The user controls which versionable files enter Git.

**Defined:** create `.sqln/.gitignore` when creating the project, with the local state files.
Do not run Git commands in that creation.

### Preference precedence

**Defined:** apply this precedence:

```text
Application defaults → global configuration → project configuration
```

The project overrides only the values it declares.
The others keep the global values or the application defaults.

Example: if the global uses the conventional mode and the project declares Vim, the interface uses Vim in that project.
That precedence does not change the scope of the connections or the storage of the consoles.

The combination of connections is already defined: show both scopes, without replacement by name.

**Defined levels:** configure the SQL timeout per connection.
Configure the page size and the row cap at the global and project levels.

### Configuration write and reload

**Defined:** when writing `config.json`, take the lock, reread the file, and write atomically.
On an external change of the configuration, ask before reloading.
Do not change active connections because of that reload.

### Invalid configuration

**Defined:** if the load of a JSON configuration fails, report the file and the error position.
Block database connections until the configuration is fixed.
Do not load only the valid scopes as a silent alternative to the failed configuration.
Do not start queries while the configuration error stays unresolved.

**Defined:** the user fixes the file outside the application and uses the **Reload** action.
The messages must not reveal passwords or other secret values.

### Settings dialog

**Defined for the first version:** open **Settings** with `Ctrl+W ,`.
Offer the global or project scope and the keyboard, page size, row cap, and data budget fields.
Show when a value comes from the global scope.
Allow removing a project definition to use the global value.
Write only after **Save**. **Cancel** changes nothing.
Use **Shortcuts** to record, remove, and restore bindings per keyboard mode and scope.
Settings checks conflicts before Save. Direct edits of `preferences.keymap` remain supported.

**Defined for the keyboard mode change:** keep text, cursor, and scroll.
If the change erases a selection or the history, warn before applying it.
That rule also applies to a change through a configuration reload.
The code switches the mode in the editor itself, so the selection and the history continue, and no warning is needed.

### File schema

The JSON schema and the local state list are defined in [docs/ARCHITECTURE.md](ARCHITECTURE.md).

## 12. Console scope and saving

### Defined

**The console always follows the scope of the connection.**

| Connection | Console place |
|---|---|
| Global | Global storage |
| Of the project | Storage of the same project |

Opening a global connection inside a project does not move its consoles to that project.

Different projects can access the same consoles of a global connection.

Connections with equal names in different scopes keep distinct console sets.

### Console names

**Defined:** generate an automatic name for new consoles, such as `console-1`.
Do not require filling a name before opening the new console.
Allow renaming later.

The rename does not change the console identity, its files, its attached connection, or its scope.
**Defined:** use `Ctrl+W N` to rename the current console and offer the action in the console list.
Refuse empty or repeated names in the same connection.
The uppercase `N` is not the `Ctrl+W n` shortcut of the next tab.

### Connection change

**Defined:** allow changing the connection of a console without an active execution and without an open transaction.
Use `Ctrl+W a` to open the selector, including in a console without a connection.
Show the current connection, the new connection, and the destination folder before the confirmation.
If the scope changes, report the new file location before finishing.
Keep text, name, cursor, and scroll.
Clear result grids and database or schema selection. Keep the Messages command history with its recorded connection labels.
Save the text before the change. Cancel the change if that save fails.
Do not run or convert the SQL automatically because of that change.

### Automatic saving

Save the content of the open consoles automatically.
That includes new consoles and changes to existing consoles.

**Defined interval:** save after **500 ms without new changes**.
Restart that count when the user edits again.
Do not write on each individual change.

Write pending changes before closing the console or exiting the application.

**Defined write failure:** keep the text in memory and allow continuing to work with a visible alert.
Do not block new executions only because of that save failure.
The commit and rollback controls stay available to resolve existing transactions, according to the allowed execution state.

Before closing without writing the pending changes, require explicit confirmation.
Do not discard the text just because a save attempt failed.
A confirmation to close without saving does not undo previous writes already finished by the automatic saving.

Closing a console does not remove its saved script.
The user can reopen it later.

**Defined:** write scripts in UTF-8 and preserve the line ending.
Write scripts and settings atomically.

### External changes to the script

**Defined:** on detecting that another program changed the script, warn and ask the user:

- **Reload the file:** use the changed content on disk.
- **Overwrite:** write the current editor text over the file.

Do not reload automatically, even when no local changes exist.
Do not overwrite the file or discard the text in memory without the corresponding choice.
Pause the automatic saving of that document while the conflict waits for a decision.
Reloading the text does not run SQL automatically and does not change the text of an execution already started.

**Defined for a removed script:** ask the user and keep the text in memory.
Do not recreate the file without the corresponding choice.

**Defined for batch changes:** present a single dialog, with a choice per file.

**Defined for detection:** poll file size and modification time each second, then compare the hash. Check the hash before writing.

### Opening and reopening

Present a list to reopen existing consoles.
Show only the consoles of the selected connection.
Do not include consoles of other connections in that list.
Reopen at startup the consoles of the last session, with the `reopenTabs` option (default on). The user changed this rule on 2026-10-31.

When asking for a new console, offer the reuse of an existing console.
Keep an explicit option to create another console.

If the chosen console is already open, only select its existing tab.
Do not create a second tab for the same console.
That rule does not prevent creating several different consoles on the same connection.

**Defined flow:**

```text
Request console
       │
       ▼
List of existing consoles
       │
       ├── Reopen a console
       └── Create a new console
```

Reopening a console does not run its queries automatically.
Do not try to recover transactions or physical connections of the previous run of the application.

### Multiple instances

**Defined:** block the opening of the same console in a second instance while it is in use.
Do not allow concurrent editing or opening a read-only copy as a replacement of that rule.
Release the exclusivity when the console closes.

That lock protects the file and the console identity.
Each instance keeps its own pools; do not create a pool service shared between processes.

**Defined:** use the system file lock, released at process end.

### Deletion of consoles and connections

**Defined:** ask for confirmation before removing a saved console or a connection definition.
Tell closing the tab apart from deleting the saved script.
Say in the confirmation which entries or files will be removed.
Cancelling the confirmation does not remove those items.
Do not use the deletion to bypass the execution finalization rules or the open transaction resolution.

**Defined:** removing a connection keeps its attached scripts on disk.
The confirmation to remove the definition does not authorize deleting those scripts in cascade.
Do not attach automatically the kept scripts to another connection by a name coincidence.

### Scripts without connection

**Defined:** show a **Scripts without connection** list for the kept scripts whose connection definition was removed.
Allow opening and editing those scripts without running SQL.
Require an explicit connection choice before allowing execution.
Confirm the attach and respect the scope of the chosen connection.
If the scope changes, report the new file location before finishing the attach.
Do not attach by name or run SQL automatically as a consequence of the attach.
Keep the saving and exclusivity rules of the console during the edit of those scripts.

### Editor state on reopen

**Defined:** recover the cursor position and the scroll when reopening a console.
Do not limit the recovery to the text with the cursor at the start.
Do not restore results or run SQL because of that recovery.
Keep that state as local interface information, separate from the versionable script text.

### Console list

**Defined:**

- Show connection, scope, and last change in the console list.
- Separate stable configuration from state updated during the edit.

#### Approved design of the files and the lists

**Defined:** the user approves this design on 2026-10-31.
The code implements the points below, including the discard label and the unreadable local state warning.

- The console creates its files at the first edit. An empty console creates no file.
- The automatic name is the smallest `console-N` that no console of the connection uses, open or saved.
- The saving writes the text 500 ms after the last edit. The main loop checks the clock at each step.
- A write that fails shows the alert in the hint bar and marks the tab with `!`. The text stays in the editor, and a new attempt happens after 5 s.
- A write refused because the file changed or disappeared pauses the saving of that console. The user decision resumes the saving.
- The **Save now** action (`Ctrl+S`) works in the editor, in both modes. In a console with a pending decision, `Ctrl+S` opens the question again.
- The question about scripts changed outside SQLN lists all the scripts, one per line. `Left` and `Right` choose the action, **Apply** requires a choice per line, and **Decide later** (or `Esc`) closes the question and keeps the saving paused.
- The **Recreate** action, for a removed script, writes the editor text and the identity file. The **Close and discard the text** action closes the console and discards the text.
- Closing a console, or exiting, writes the text first. If the write fails, or the file is in conflict, or the console has text and no connection, the **Unsaved text** question offers **Cancel** and **Close without saving**.
- A console without a connection and without a file (the case of the initial console without a database) saves nothing.
- `n` in the navigator opens a list with **+ New console** and the saved consoles of the connection, the newest first. Without saved consoles, `n` creates a console without a list.
- The console list shows the connection scope in the title and marks an open console with `(open)`. Picking an open console selects its tab. `Delete` removes the script of the row, after a question that cites the folder.
- A console that another instance uses does not open, and a message reports the reason.
- `s` in the navigator opens the **Scripts without a connection** list, with the scripts whose `connectionId` does not exist in the files. The search covers both scopes.
- A script without a connection opens and saves normally. Running it opens the connection list. **Associate** asks for the confirmation with the destination folder, and only then attaches the console. The attach runs no SQL.
- Attaching to a connection of another scope copies the files, checks the copy, and removes the originals, in the order of `docs/ARCHITECTURE.md`. The attach writes the pending text first. If that text does not write, the attach stops.
- Removing a connection removes only its entry in `config.json`. Its open consoles stay without a connection, and the scripts stay on disk.
- An invalid configuration shows a dialog with the message and the file. The dialog offers **Close** and **Reload**. While the error lasts, SQLN does not run, does not open a table, does not read the catalog, does not test a connection, and does not register a connection. The hint bar reminds `Ctrl+W r`.
- `Ctrl+W r` reads the files again. A connection with a session or an open transaction keeps its definition. A connection that left the files leaves the application only when it is idle.
- The application checks the hashes of the `config.json` files each second. A change opens the **Configuration changed** question, with **Ignore** (the initial focus) and **Reload**.
- A project created by the question is valid only for the session that created it. The folder of a `--project` option that has no `.sqln/` yet receives the same question.
- The connection form shows **Scope** only when a project exists, with the project as the initial value. The scope of a saved connection does not change.
- The connection name is unique in each scope. The two scopes can repeat a name, and the navigator adds `(local)` or `(global)`.
- The SQL limit of each connection appears in the form, in seconds. The value `0` runs without a deadline.
- The layout (collapsed navigator, width, and editor ratio) goes to the `workspace.json` of the project, or of the global scope without a project. The cursor and the scroll of each console go to the `workspace.json` of the console scope.
- The tab list of the last session goes to the same `workspace.json` of the layout. The execution badge and the time do not enter that file.
- A startup with an unreadable `workspace.json` uses the initial state and shows a non-blocking warning.

### Pending validation

- Validation of the console lock on a network disk.

The keyboard mode change rule is defined in section 11.

## 13. Passwords and credentials

### Defined

Use the system password vault as the main storage.

Password retrieval order:

1. System vault.
2. Authorized global configuration, only when no vault exists.
3. Password prompt for session-only use.

A temporary password stays in memory, per connection, until the application closes.
**Defined:** use `zeroize` to erase the password when dropping it.
That guarantee does not reach the internal copies of the drivers.
All the consoles of that connection can use it to get pool connections.

Do not write that temporary password without an authorization that applies to the chosen storage.

### Vault present

Use the vault when it exists and allows access to the credential.

If it is locked or reports an error:

- Do not treat that as an absent vault.
- Do not use the global configuration as a replacement for the vault.
- Allow entering the password for the session as the last resort.

**Defined:** with the vault locked or with an error, offer **Try again**, **Use password in this session**, and **Cancel**.
An unknown failure is an error, not an absent vault.

### Vault absent

Allow using a password saved in the global configuration with explicit authorization.

If no usable password exists, ask for the password for the session.
In that case, the user can authorize its write into the global configuration.

The authorization does not allow writing the password into the project configuration.
The scope of the connection definition and of the consoles does not change.

### Plain-text warning

Saving into the global configuration means storing the password in plain text.
Report that fact before asking for authorization.

The save option does not start authorized.
Do not write automatically after a vault failure.

**Defined technical measures:**

- Configuration file with permission `0600` on macOS and Linux.
- File access limited to the owner.
- Do not record passwords in logs or error messages.
- Attach the credential to the connection identity, not to the visible name.

File permissions do not encrypt the content.

**Defined:** the authorized password stays in `credentials.json`, in the global folder, with permission `0600`.
The file keeps one entry per credential identity, for global and project connections.
`config.json` never receives the secret, in any scope.
The project connection does not change scope and does not receive a reference to the secret.

### Credential cycle

**Defined:** apply the six rules below.

1. **Write failure:** keep the password only in memory, warn that it was not saved, and offer an explicit retry.
   Do not switch automatically to global plain-text storage when the vault fails.
2. **Vault starts to exist:** offer migration with confirmation.
   Confirm the vault write before offering the removal of the global plain-text password.
   Do not erase the previous password if the write fails; report any incomplete migration.
3. **Project moved:** keep references by stable identity, not by the connection name or the folder path.
   When a folder change cannot be told apart from a copy, ask for confirmation before reusing a credential.
4. **Project copied or cloned:** ask for confirmation to reuse an existing credential on this computer.
   On another computer without that credential, ask for the password; never carry the secret through the project or Git.
5. **Credential attach:** tie it to the logical scope, connection ID, destination, and user, including the security context of the transport.
   Renaming does not change the attach; changing the destination does not authorize sending the previous password automatically.
   Do not accept project references as authorization to access credentials of other applications.
6. **Connection removal:** do not erase the vault password automatically.
   Offer separate deletion with confirmation, warning that other copies of the project may use the same credential.

### Copy or folder change

**Defined:** record on this machine the known path of each project.
Ask before reusing a credential if that path changes.

### Authentication methods

**Defined for the first version:** accept password and client certificate.
IAM and Kerberos stay out of the first version.

### Refused password

**Defined:** if the database refuses the password, ask for a new password once.
If it also fails, suspend new attempts of that definition until a user action.

**Defined:** during the suspension, the application does not try to connect on its own.
Each explicit user action opens the password prompt.
A password accepted by the database or the connection edit ends the suspension.

### Credential presentation

**Defined:** ask for the password before the first use that needs it, including test and catalog read.
Start the prompt with session-only use. Saving needs an explicit choice.
Without a vault, the global save option shows the **plain text** warning.
The trust confirmation shows destination, user, and TLS, with **Cancel** as initial focus.
Deleting a vault password needs a confirmation separate from the connection removal.

### Defined technical integration

| Platform | Service |
|---|---|
| macOS | Keychain |
| Linux | Secret Service compatible service |
| Windows, if supported | Credential Manager |

**Defined:** use `keyring-core` with a platform-specific integration, according to `docs/ARCHITECTURE.md`.
The architecture defines the absence, missing access, and error criteria.
Treat an unknown failure as an error, never as an absence.
Confirm those criteria in real tests before releasing plain text as an alternative.

### Implementation and pending validation

The code implements password and trust prompts, vault recovery, authorized global plain text, migration, and separate deletion.
It classifies password refusal and suspends automatic attempts after the second refusal, without repeating SQL already sent.
Repeat Secret Service tests on a real Linux desktop. The recorded Keychain and container results are in section 1.

## 14. Technical proposal in Rust

The application uses the libraries of this table, including the vault adapters.
Section 1 records Keychain and container Secret Service tests. Real Linux desktop validation stays pending.

| Responsibility | Library | Note |
|---|---|---|
| TUI interface | Ratatui | Rendering structure and terminal components |
| Terminal and events | Crossterm | Keyboard, mouse, size, and terminal modes |
| Async execution | Tokio | Separate database work from interface processing |
| TLS | `rustls` with `ring` and `tokio-postgres-rustls` | Defined; system roots and `webpki-roots` |
| PostgreSQL | `tokio-postgres` | Defined |
| MySQL | `mysql_async` | Defined |
| SQLite | `rusqlite` | Defined; needs an own thread |
| Pool | Own implementation | Defined |
| Integrated editor | EdTUI | Defined; needs the conventional mode layer, text replacement, and autocomplete |
| Password vault | `keyring-core` and platform integration | Keychain on macOS and Secret Service on Linux |

The own pool controls the limit, the queue, and the reservation of connections per console, transaction, and read.

The editor is one of the main technical risks.
An editing library does not deliver contextual SQL autocomplete automatically.

**Defined:** use EdTUI as the editor base.
The `ratatui-textarea` offers no syntax highlight and no Vim mode.

The application implements:

- The conventional mode layer, with `Shift`+arrows, IDE shortcuts, and selection without the character under the cursor.
- Typing over a selection, without writing to the clipboard.
- Text replacement.
- The autocomplete suggestion list, positioned by `cursor_screen_position`.
- Syntax highlighting through the project tokenizer and `editor::frame`, after EdTUI draws the visible text.

**Defined:** do not use `syntect`, because EdTUI generates RGB colors and resets the state on each line.
The application requires the ANSI theme, `NO_COLOR`, and multi-line comments.
The `astrodark` theme uses RGB, but that does not change the reason: the own tokenizer covers the three themes.

### Driver decision

**Defined:** use `tokio-postgres`, `mysql_async`, and `rusqlite`, behind the connector contract.
Do not use SQLx.
Implement an own pool.

The tests of [docs/RESULTS.md](RESULTS.md) justify that decision:

- On SQLx, `timeout` did not interrupt the consumption of a result; on the dedicated drivers, it worked.
- The SQLx pool with a long deadline did not return an error for a refused connection.
- SQLx does not expose the cancellation data nor the MySQL session reset.

The accepted cost is keeping three APIs, the own pool, and the TLS configuration on each driver.
`rusqlite` is synchronous and needs an own thread.

### Own pool requirements

- Limit the connections per definition, with a default of five.
- Serve the queue in arrival order.
- Wait without a deadline for a free connection, with cancellation of the wait.
- Return a connection error as a failure, without turning it into a wait.
- Apply the 30-second deadline only to the connection establishment.
- Keep the connection inside the limit during the cleaning.
- Drop the connection whose cleaning cannot be confirmed.

### Cleaning and transaction state per database

**Defined:** use these mechanisms, according to the tests:

| Database | Session cleaning | Transaction state |
|---|---|---|
| PostgreSQL | The steps of `DISCARD ALL` without `DEALLOCATE ALL`, and `DEALLOCATE` of each `PREPARE` | Clock probe, through the simple protocol |
| MySQL | `Conn::reset` and back to the database of the connection; without a database, close the session | `SERVER_STATUS_IN_TRANS` indicator of the response; after an error, `DO 0` |
| SQLite | Drop the connection | `is_autocommit` |

The `tokio-postgres` does not expose the transaction state.
The PostgreSQL probe needs a round trip to the server after each statement, about 0.3 ms on `localhost`.
The PostgreSQL probe uses `SELECT transaction_timestamp() <> statement_timestamp()`.
The `mysql_async` exposes the last response, and that indicator needs no round trip.
Thus, `SHOW WARNINGS`, `ROW_COUNT()`, and `SET TRANSACTION` keep working in the next statement.
An error response does not carry the indicator, and a deadlock undoes the transaction; therefore, an error needs `DO 0`.
It hit 1,000 of 1,000 reads, without log errors; the `25P02` error marks a transaction with an error.

### Rules confirmed by the tests

- Dispatch one statement at a time, on the same reserved connection.
- Do not send the next statement before ending the read of the current one.
- Preserve the autocommit semantics of the database, without creating a transaction for the whole script.
- Do not undo automatically changes already committed when a later statement fails.
- Respect implicit commits of the database, as happens with DDL on MySQL.
- Validate the SQLite file paths before opening the database. The `:memory:` text opens one database in memory in the single session of its pool.

On PostgreSQL, several statements in one call form an implicit transaction.
That does not equal reserving the connection and running each statement in autocommit.

### Validations of the dedicated drivers

The tests ran with test certificates, on PostgreSQL 18.6 and MySQL 8.4.13.
These rules apply to the connectors:

- **PostgreSQL:** set `SslMode::Require` on remote hosts, because the default `Prefer` accepts a connection without TLS.
- **MySQL:** always set `ssl_opts` on remote hosts, because the default does not use TLS.
- **Verification:** with `rustls`, a wrong CA and a wrong name refused the connection, without an insecure alternative.
- **Client certificate:** worked on both databases.
- **Client key on MySQL:** `mysql_async` 0.37 accepts PKCS#1, PKCS#8, and SEC1. The driver reports invalid keys.
- **Connection deadline:** wrap the whole connection in `timeout`, because `connect_timeout` did not cover a silent server.
- **`rustls`:** build only the `ring` provider. The `mysql_async` uses the default provider of the process, and two providers would need to install one of them at startup.
- **Passwords:** the dedicated drivers do not read environment variables or password files.
- **Logs:** do not record `mysql_async::Opts`, because its `Debug` output shows the password.
- **Connection loss:** both network drivers reported the loss on an idle connection, in a transaction, and during an execution.
- **State on MySQL:** do not use `SET TRANSACTION` as a probe, because a test proved that it erases the user `SET TRANSACTION READ ONLY`.
- **Close on MySQL:** the `drop` of `Conn` reads the result to the end with the socket open. The connector drops the connection outside the runtime to close the socket.
- **Types on PostgreSQL:** the simple protocol does not inform the types, so the connector uses `prepare` before the statement.
- **Error on `prepare`:** return that error without repeating the statement, because the repetition would show only `25P02`.
- **`COMMIT` in a transaction with an error:** refuse, because PostgreSQL answers with a `ROLLBACK` without an error.
- **Late cancellation:** wait for the request to finish before reusing the session. Without that end, the session is dropped.
- **End of the request on PostgreSQL:** `cancel_query` returns after writing the request. The connector reads until the server closes that connection.

Not yet tested: keepalive with a real network failure and the operating system CA.

### Proposed functional contract of the SQL connectors

Isolate the driver behind the responsibilities needed by the initial databases:

- Open and end a physical connection.
- Run a native SQL statement, without silently changing its text.
- Provide rows up to the cap and report whether the result ended.
- Finalize the read before allowing the next statement.
- Cancel a statement through a temporary connection and close the physical connection as the last resort.
- Report the transaction state, including failures and unknown situations.
- Integrate commit and rollback requested by the interface with the transactions controlled by the SQL text.
- Clean the session state before returning the connection to the pool.
- Ask the catalog through the same connection limit.

The interface does not know connection, row, or error types specific to the drivers.
Convert driver data into own application events and values before sending them to the interface.

This contract describes responsibilities, not final trait or method signatures.
It covers the initial SQL connectors.
The future sources need to declare their capabilities, without implementing SQL operations they do not support.

### Proposed application organization

Start with a single Rust crate and separate responsibilities into modules.
Do not create a separate service or a plugin system in this phase of the proposal.

Responsibilities:

- **Interface:** rendering, focus, mouse, shortcuts, and state presentation.
- **Execution:** statement sequence, read up to the cap, and connection reservations.
- **Connectors:** source-specific operations and driver integration.
- **Catalog and autocomplete:** cached metadata and suggestions according to the text and the dialect.
- **Configuration and documents:** scopes, preferences, consoles, and automatic saving.
- **Credentials:** vault access, authorized global exception, and temporary passwords in memory.
- **Result budget:** global accounting of the grid data, load suspension, and release when tabs close.

Keep the interface state in the main loop.
Communicate the asynchronous work with actions and events, without blocking the rendering.
The interface does not touch the driver directly.

Each connection definition keeps shared resources:

- Pool with the configured limit, five by default.
- Reference to the credential and eventual temporary password.
- Catalog in cache.

Each console or table view keeps its execution state and its results.
The corresponding executor keeps the same reservation through the whole script.
After the script, it continues with the reservation if an active manual transaction exists.

The modules, the types, and the communication channels are in [docs/ARCHITECTURE.md](ARCHITECTURE.md).
This organization is a technical proposal, not a final implementation contract.

### Proposed independent states

Separate the execution state from the mode and the transaction state.

| Dimension | Proposed states or values |
|---|---|
| Execution | Idle, waiting for connection, running, finishing, done, cap reached, error, timeout, stopped, cancelled before start, or interrupted for close |
| Transaction mode | Autocommit or manual |
| Transaction | None, open, with error, or unknown state after a communication failure |
| Reservation | No connection, waiting, in use, or cleaning |

An execution can end and leave the transaction open.
In that case, allow another execution in the console using the same reservation.
Do not clean or return that connection to the pool while the transaction stays open.

### Proposed actions and events

```text
Keyboard or mouse
       │
       ▼
Routing according to focus, mode, and context
       │
       ▼
Interface action or command to the executor
       │
       ▼
Execution, result, and transaction events
       │
       ▼
State update and rendering
```

Tie commands and events to the identity of the console or the table view and to the corresponding execution.
An old response must not update another execution or a different tab.

Apply the one-execution-per-console restriction in the interface and in the command reception by the executor.
The transaction controls do not represent a new script execution.
They act on the already existing transaction, according to the allowed state.

The shortcut priorities are in [docs/KEYMAP.md](KEYMAP.md).

### Proposed separation of responsibilities

```text
Interface
├── Sources and connections
├── Consoles and editors
└── Results and messages
         │
Execution management
├── State of each console
├── Pending requests
├── Reservation for transactions
└── Read up to the cap
         │
Connectors
├── PostgreSQL
├── MySQL
└── SQLite
```

The autocomplete uses the editor text and the cached catalog.
The catalog read respects the same pool of the connection.

The current contracts are in [docs/ARCHITECTURE.md](ARCHITECTURE.md) and in `src/connector/mod.rs`.

## 15. Verification scenarios

These scenarios serve as the acceptance base for the defined requirements.
The results of the technical tests are in [docs/RESULTS.md](RESULTS.md), and the product tests are in `tests/`.

### Consoles and interface

- Open several consoles without mixing their texts or results.
- Run the selection as the default action when a selection exists.
- Without a selection, run the statement under the cursor.
- Run the whole script only through the corresponding action.
- Refuse a second execution in the same console while a pending or active one exists.
- Apply that restriction during the pool wait, execution, read, and finalization.
- Do not create an extra execution queue for the same console.
- Allow simultaneous execution on different consoles, according to the pool limit.
- Allow a new execution after the previous one ends, including inside an open transaction.
- Show editor and results at the same time in the console view.
- Open the table view with a double click on a mouse-capable terminal.
- Allow keyboard navigation even when mouse support exists.
- Edit one console while another runs or waits for a connection.
- Start in the conventional mode when no configuration selects another mode.
- Switch between conventional shortcuts and Vim through configuration.
- Use Vim navigation in the object navigator, lists, tabs, grids, and dialogs.
- Insert `j` and `k` in fields under edit without triggering the interface navigation.
- Keep the mouse available in both keyboard modes.
- Resize the panel division by drag and collapse the side navigator.
- Keep editor and results visible together after resizing or collapsing the navigator.
- Get suggestions of schemas, tables, views, and columns without querying the database on each key.
- Suggest built-in functions with signatures from local lists; preserve existing parentheses and arguments.
- Not run SQL when the cursor is outside a statement.
- Keep the execution running while editing the text of the same console.
- With suggestions open in the editor, accept with `Tab` and break the line with `Enter`, according to `suggestionKeys`.
- Offer search and replace in the editor.
- Suggest the dialect keywords without a catalog available.
- Suggest qualified names, quoted identifiers, and aliases.
- Not write the catalog cache to disk.
- Read the columns of a table only when using it.
- Not send data outside the configured databases.
- Offer `--project`, `--log`, `--version`, and `--help`.
- Select cells by drag in the grid.
- Not interpret external data, names, and errors as terminal control sequences.
- Show a message on a small terminal, without losing the state.
- Copy with `Ctrl+C` when there is a selection and do nothing without a selection.
- Restore the terminal and write the pending texts after a panic.
- Write the pending texts and exit on a shutdown signal.
- Answer a key in under 50 ms with 256 MiB of loaded data.

### Connection form and transport

- Register a connection through the form or a URI, keeping the same scope and credential rules.
- Select project by default when registering with an active project, allowing to choose global.
- Offer only the global scope when there is no active project.
- Require a name and reject repeated names in the same scope, case-insensitive.
- Apply the same name validation when renaming a connection.
- Allow equal names between global and project, keeping distinct identities and marking the origin.
- Require a disconnect before changing access data, TLS, or the pool limit of a connected definition.
- Not keep a pending change of those parameters for later application.
- Not persist a password inside a URI in the project configuration.
- Require TLS with certificate verification by default on remote PostgreSQL and MySQL hosts.
- Not fall back insecurely after a verification failure.
- Allow explicit TLS disabling and passwordless TLS access to localhost.
- Not use the project scope as a reason to skip TLS on a remote host.
- Open SQLite databases in files, and open `:memory:` as one database in memory that lives in the single session of its pool.
- Refuse writes on a read-only connection, including after the session cleaning.
- Consider local `localhost`, `127.0.0.0/8`, `::1`, and Unix socket.
- Connect on first use, without opening connections at application startup.
- Show host, port, user, and TLS before the first connection to a destination on this machine.
- Ask again when one of those values changes.
- Not ask that confirmation when the user registers or changes the connection on this machine.
- Not write the destination confirmation into the project files.
- Test the connection through the form with the same TLS rules.
- Accept a Unix socket as a local connection.
- Connect with a custom CA and a client certificate.
- Ask before creating a missing SQLite file.
- Use the SQLite default for `foreign_keys`, unless the connection says otherwise.
- Respect the SQLite lock deadline and show a visible error.

### Pool and transactions

- Use a limit of five per connection definition when no other value is configured.
- Make the sixth request wait if the five connections are busy.
- Keep the wait without a maximum deadline while the pool stays busy.
- Cancel a waiting request without sending its SQL to the database.
- Not affect queries or transactions of other consoles when cancelling the wait.
- Tell the queue wait apart from a connection failure.
- Apply the default 30-second deadline to the connection establishment, respecting a different configured value.
- Offer a default SQL timeout of 30 seconds, configurable and with an option to run without a deadline.
- Apply the timeout per statement, without summing the time of the other script statements.
- Subtract the pool wait from the SQL timeout.
- Include the row read in the statement deadline.
- Not confuse the SQL timeout with the connection deadline or the pool wait.
- Not repeat SQL or resolve transactions automatically after a timeout.
- Not use the connection establishment deadline to end the wait for pool availability.
- Not use the connection establishment deadline as a duration limit of an SQL query.
- Respect the changed limit in the connection configuration.
- Use a limit of one without opening a connection per console.
- Make a second console wait when the single connection is busy.
- Use free connections while another console keeps a transaction.
- Start new consoles with autocommit on.
- Allow selecting a manual transaction per console.
- Verify the autocommit effects on scripts according to the semantics of each database.
- Not create a transaction for the whole script just by sending its statements together to the driver.
- Not undo automatically changes already committed if a later statement fails.
- Keep the transaction reservation until commit or rollback.
- Not run commands of another console inside a reserved transaction.
- Use the same connection between all the statements of a script, including in autocommit.
- Keep session settings and temporary tables between the statements of the same script.
- Clean that state before offering the connection to another execution.
- Not clean a connection during a script, read in progress, or active transaction.
- Not change persistent table data to clean the session.
- Not make the connection available while its cleaning is in progress or cannot be confirmed.
- Drop the connection whose cleaning cannot be confirmed.
- Not return the connection to the pool between the statements of a script.
- Release the reservation after the script ends, when there is no active transaction.
- Not require the same connection in later executions outside a transaction.
- Make the catalog read respect the pool limit.
- Confirm the behavior on the three initial databases.
- Replace a lost idle connection without repeating SQL.
- Fail a lost reserved connection without repeating SQL.
- Require confirmation before the next execution after losing an open transaction.
- Apply the selected database or schema on each execution, even with another physical connection.
- Keep the temporary cancellation connection outside the pool limit, without running user SQL.

### Scripts and indicators

- Offer Stop script without closing the console or erasing its text or the loaded rows.
- Cancel the wait when Stop script happens before the SQL send.
- Not send the next statements after accepting the stop, without promising immediate cancellation at the server.
- Send the cancellation before closing a connection, in Stop script and the SQL timeout.
- Wait 5 seconds for the statement response after the cancellation.
- Close the connection after that deadline, without an open transaction.
- Ask for confirmation before closing the connection with an open transaction.
- Wait for the end of the statement if the user refuses the close, without sending the next ones.
- Close the connection if the temporary connection cannot be opened, respecting the confirmation with an open transaction.
- Not reuse the connection before the response of the cancelled statement.
- Not let a late cancellation hit another statement.
- Report an unknown result after closing the connection during a statement.
- Not run commit or rollback automatically when stopping the script.
- Stop at the first statement with an error.
- Not run the next statements.
- Show success and error beside the corresponding statements.
- Not confuse a successful execution with a commit.
- Show an error that appears after the first rows.
- Map results and errors to the correct statement in scripts with several lines.
- Accept `DELIMITER` on MySQL.
- Refuse `COPY FROM STDIN` with a clear message.
- Show one tab per `CALL` result.
- Show `EXPLAIN` in the common grid.
- Stop a table view query with the interruption sequence.
- Refuse a filter with another statement, such as `1=1; DELETE FROM t`.
- Keep the `LIMIT` when the filter ends with a `--` comment.

### Results

- Mark when a value exceeds the data limit, keeping the existing rows and without faking a null or empty value.
- Compare the TSV output with the JetBrains default extractor for special values, without headers.
- Copy data as TSV by default, without headers.
- Copy `NULL` as an empty field and an empty string as `""`.
- Keep the trailing `NULL` columns of the line, the trailing spaces, and the CR in quotes.
- Quote values with tab, LF, or quotes, doubling the inner quotes.
- Copy binaries as valid text or, in the other cases, as `0x` and uppercase hexadecimal.
- Keep the row and column order of the selection in the copy.
- Use the system clipboard when available.
- Try OSC 52 when the system offers no clipboard and the terminal allows that protocol.
- Report failure or missing support without using temporary files.
- Show results in separate tabs.
- Clear the previous results of the console when accepting a new execution, including before waiting for a connection.
- Free the budget of the discarded data, keeping the text and the open transaction.
- Not clear the results when rejecting an execution attempt because the console is already busy.
- Not affect results of other consoles or table views with that clearing.
- Share the main tab strip between consoles and table views.
- Show only data in the table view. The structure stays in an own tab.
- Select the existing tab when opening the same table again, without duplicating the tab.
- Keep same-named tables of different connections as distinct items.
- Show a `WHERE` filter and sorting bar above the table grid.
- Offer autocomplete in the `WHERE` and `ORDER BY` fields.
- Edit the fields without running queries automatically.
- Apply the edit with `Enter` when the focus is on the bar fields.
- Accept the autocomplete suggestion with `Tab` without running the query.
- Apply the fields with `Enter` even with autocomplete open, without implicitly accepting the highlighted suggestion, unless `suggestionKeys` is `enter` or `both`.
- Clear the grid and show the error when a filter or a sorting is invalid.
- Not restore old data because of events of a previous query.
- Apply at once a sorting chosen by header click.
- Keep the already applied filter, without applying a `WHERE` draft because of that click.
- Not apply an automatic sort when opening a table, even with a primary key.
- Keep filters and sorting while the tab is open and discard them at close.
- Reopen a table without recovering the filters or the sorting of the previous tab.
- Restore table tabs when `reopenTabs` is on, without rows, filters, sorting, or a database read at startup.
- Verify filter, sorting, and pagination together.
- Fetch each page of the table view with a new query.
- Return the connection to the pool between the pages of the table view.
- Wait for a free connection when loading more rows with the pool busy.
- Ask for one extra row on each page, without using `COUNT`.
- Report the unstable order risk when no sorting is applied.
- Apply the fields with `[Apply]`, with the same effect as `Enter`.
- Clear the fields and apply the query without a filter or sorting with `[Clear]`.
- Show the text provided by the database, without converting time zones or changing precision.
- Resize the columns and open long values in a detail window.
- Select data by range.
- Search only in the loaded rows, without querying the database.
- Open a view in the table view, with a label that identifies it.
- Show notices, errors, and commands without rows in the Messages tab.
- Show a result without rows with the columns and the zero-row indication.
- Allow checked cell updates only in eligible table tabs; preserve read-only behavior in other grids.
- Allow change commands through the editor.
- Load 100 rows per page in the table view, by default.
- Respect the configured page size.
- Fetch the next page only after the load-more action.
- Append the new rows to the previous ones, without replacing the already loaded rows.
- Keep the previous rows available in the grid after loading more.
- Keep the grid rows only in memory, without cache files or temporary storage of the application.
- Apply the default budget of 256 MiB to the sum of the data of all the grids of the instance.
- Respect a changed budget in the effective configuration.
- Suspend new loads at the cap, without erasing loaded rows.
- Free the corresponding budget when closing tabs and discarding their data.
- Not fetch a page not requested yet just because budget was freed.
- Treat the exhausted budget as the row cap in the consoles.
- Refuse Load more in the table view while the budget is exhausted.
- Not run commit or rollback automatically because of the budget.
- Discard the data when the corresponding tab closes.
- Reopen the script without restoring results or running SQL automatically.
- Read each console result up to the configured cap, without waiting for a user action.
- End the read at the cap and report that the result may have more rows.
- Not offer Load more in the consoles.
- Keep editor text unchanged; add `LIMIT` only to eligible sent SQL when `capInSql` is on. Never repeat SQL automatically for more rows.
- Allow walking the read rows without a new query.
- Use 1,000 rows as the default cap.
- Cancel a read query at the cap only without an open transaction.
- Consume the remaining rows without keeping them in the other cases.
- Not allow closing a result tab while its statement still reads rows.
- Move forward only after the driver finalizes the previous result.
- Stop the script if the read finalization returns an error.
- Not request automatic commit when ending the read.
- Keep the reservation of a manual transaction after the read ends.
- Handle console results with zero rows, below the cap, exactly at the cap, and above the cap.
- Handle table pages with zero, fewer than 100, exactly 100, and more than 100 rows.

### Configuration and persistence

- Find the closest ancestor `.sqln/` folder.
- Use the project indicated by `--project` in the place of the `.sqln/` search.
- Ask about the project creation when that folder is not found, without creating it automatically.
- Create `.sqln/` in the current directory only after confirmation.
- Continue only with the global scope if the user refuses the creation.
- Use `~/.config/sqln/` as the global default and respect `XDG_CONFIG_HOME`.
- Resolve SQLite files and certificates relative to the project root for project connections.
- Resolve those files relative to the global folder for global connections, even inside a project.
- Not change absolute paths or use the current directory as the base of those connections.
- In the form, write the full path of a typed SQLite file, from the current directory.
- Read and write settings in JSON, keeping the scripts in `.sql`.
- Report the file and the position of errors in the configuration load.
- Block connections and queries until an invalid configuration is fixed, without a fallback to only the valid scopes.
- Keep the project settings and scripts versionable, ignoring the local interface state.
- Not add files to Git automatically.
- Apply project preferences over the global ones only for the declared values.
- Keep the global preference when the project does not declare that value.
- Use the application default when no scope declares the value.
- Show global and local connections together.
- Add `(local)` and `(global)` to the names when there is a collision.
- Reopen a console with the correct connection even when the names collide.
- Save global consoles in the global storage.
- Save local consoles inside the corresponding project.
- Save changes to open consoles automatically after 500 ms without new edits.
- Restart the saving interval when a new edit happens.
- Write pending changes before closing a console or exiting the application.
- Keep the text in memory and show an alert if the saving fails.
- Allow new executions despite the save failure, respecting the other console restrictions.
- Require confirmation before discarding pending changes when closing without saving.
- Ask before reloading or overwriting a script changed externally, even without local changes.
- Not run SQL or modify an execution already started just by reloading the file.
- Block the opening of the same console in a second instance while it is in use.
- Release that lock when the console closes and after an unexpected process end.
- Offer the console list and restore saved tabs at startup when `reopenTabs` is on, without running SQL.
- Show in that list only the consoles of the selected connection.
- Select the existing tab when picking an already open console, without duplicating the tab.
- Allow several different consoles for the same connection.
- Offer reuse before creating a new console.
- Generate an automatic name when creating a console, without requiring a name first.
- Rename a console without changing its identity, connection, or scope.
- Not run SQL automatically when reopening a console.
- Recover cursor and scroll on reopen, without restoring the grid data.
- Separate that local state from the versionable scripts.
- Keep the console and connection attach after cloning the project.
- Create `.sqln/.gitignore` when creating the project, without running Git commands.
- Reread `config.json` before writing and write atomically.
- Ask before reloading an externally changed configuration, without changing active connections.
- Write scripts in UTF-8, preserving the line ending.
- Ask when a script is removed externally, keeping the text.
- Use a single dialog for batch external changes, with a choice per file.
- Change the connection of a console only without an active execution and without an open transaction.
- Fix an invalid configuration outside the application and use Reload.
- Include a `version` field in each JSON file.
- Write scripts and settings atomically.
- Detect external changes and compare the hash before writing.
- Ask for confirmation before deleting a saved console or a connection definition.
- Not remove those items when the user cancels the confirmation.
- Not treat closing a tab as an authorization to delete its script.
- Keep the scripts on disk when removing a connection, without reattaching them by a name coincidence.
- List those scripts in Scripts without connection and allow editing without running SQL.
- Block execution until an explicit connection choice and confirmation.
- Report the new location when the attach changes the script scope.
- Not run SQL automatically after attaching the script.
- Keep the lock between processes when editing a script without a connection.

### Credentials

- Use the vault password when available.
- Not use the global file when the vault exists but is locked or with an error.
- Allow global storage only in the absence of a vault and with explicit authorization.
- Report that the global storage uses plain text.
- Never save a password in the project configuration.
- Ask for a session password when no usable credential exists.
- Reuse the temporary password between consoles of the same connection.
- Not persist the temporary password when the application closes.
- Not reveal passwords in logs and error messages.
- Keep the password only in memory and report the failure when the authorized write does not work.
- Offer an explicit retry, without switching automatically to plain text after a vault failure.
- Ask for confirmation to migrate a global password when the vault starts to exist.
- Confirm the vault write before offering the removal of the global password, without erasing the previous one after a failure.
- Report an incomplete migration, including a failure to remove the plain text after writing to the vault.
- Keep references by stable identity when moving a project, without depending on the folder name or path.
- Ask for confirmation before reusing credentials on copies or when copy and folder change cannot be told apart.
- Ask for the password on another computer without the credential, without carrying secrets through the project or Git.
- Tie credentials to the logical scope, ID, destination, user, and transport security context.
- Not send the previous password automatically after changing the destination nor access credentials of other applications through a project reference.
- Keep the vault password when removing a connection; require a separate confirmation to delete it.
- Warn that other copies of the project may use the credential before deleting it.
- Ask for a new password once after a refusal and suspend new attempts after that.
- Erase the session password from the application memory when dropping it.
- Offer a retry and a session password with the vault locked.
- Accept password and client certificate.
- Record the project path on this machine and ask if it changes.

### Transactions controlled by SQL

- Start a transaction with SQL in the editor and keep its connection reserved after the script ends.
- Run another statement in the same transaction using the already reserved connection.
- End through SQL a transaction started by the interface.
- End through the interface a transaction started by SQL.
- Reflect errors and implicit commits without depending only on the mode configured in the console.
- Not confuse the rollback of a savepoint with the end of the main transaction.

### Controls and transaction mode

- Disable the commit and rollback buttons during a pending or active execution, including during the read.
- Enable the resolution of open transactions again after the execution ends, according to the connection state.
- Allow SQL transaction commands inside the script without confusing them with another concurrent execution.
- Block the mode switch while an execution stays pending or active, including during the read.
- Not schedule a mode switch for after the execution.
- Ask before enabling autocommit when an open transaction exists.
- Offer commit, rollback, and cancel of the switch.
- Enable autocommit only after the success of the chosen commit or rollback.
- Keep the previous mode and not send commit or rollback when the user cancels the switch.
- Report failures without enabling autocommit automatically.
- Switch the mode through the bar selector and the shortcut.

### Closing

- Ask for confirmation when closing a console with a pending or active execution.
- Not interrupt the sequence if the user cancels that confirmation.
- Cancel the wait without sending SQL if the execution has not started yet.
- After confirmation, finish the current statement without sending the next ones to the database.
- Not promise that the shutdown interrupts the command at the server at once.
- Ask when closing a console with an open transaction.
- Offer commit, rollback, and cancel of the close.
- Keep the console open without a choice.
- Keep the console open and present a retry or cancel of the close after a commit or rollback failure.
- Allow a retry only through an explicit action on the original session, when usable and compatible with the transaction state.
- Report an unknown result when the connection drops without confirming the result.
- Not repeat the operation on another connection or move to the next console before resolving the current failure.
- Not run commit automatically.
- Not restart automatically a script whose stop was already confirmed.
- Ask separately for each console with an open transaction when exiting the application.
- Not replace those questions with a single summary screen.
- Run the choice of one console before asking about the next.
- Not postpone all the commits and rollbacks until the end of the questions.
- Keep the application open if the user cancels the exit.
- Not undo or present as undone the actions already finished in the previous consoles.

## 16. Next exploration steps

### 1. Draw the screens

Detail:

- Main screen and space distribution.
- Navigator of connections and objects.
- Console tabs.
- Editor and indicators per statement.
- Console view with simultaneous editor and results.
- Table view opened through the object navigator.
- Filter and sorting bar above the table grid.
- Result tabs and grid.
- Execution, wait, row cap, and exhausted data budget states.
- Console list to open or reuse.
- Connection creation and edit.
- Global and project settings.
- Password prompt and authorization for plain text.
- Close confirmation with an open transaction.

### 2. Define the navigation

Detail:

- Focus between panels.
- Mouse navigation: clicks, double click, scroll, and possible drag actions.
- Navigation between consoles, tables, and results.
- Actions in the conventional and Vim modes across the interface.
- Shortcut priority according to focus, text input, dialogs, and autocomplete.
- Interaction with autocomplete suggestions.
- Configurable shortcuts and terminal conflicts.
- Load-more action in the table view.
- Stop script action and its interruption states.
- Access to errors and transaction states.

### 3. Detail the architecture

Detail:

- Console, execution, read, and transaction states and events.
- Identity and scope of connections and consoles.
- Responsibility limits between interface, execution, and connectors.
- Reservation and return of connections.
- Statement separation and mapping to the editor text.
- Integration between editor, catalog, and autocomplete.
- Safe persistence and automatic saving.
- Vault integration and temporary passwords.
- Validation of the candidate libraries.

These steps are complete, and the product implementation was authorized on 2026-10-30.
The record of the technical tests is in [docs/RESULTS.md](RESULTS.md); the `spikes/` folder holds only the capture artifacts, outside Git.
The detail of this section is in [docs/UI.md](UI.md), [docs/KEYMAP.md](KEYMAP.md), and [docs/ARCHITECTURE.md](ARCHITECTURE.md).
Those documents are approved.

## 17. Scope and delivery of the first version

### Delivery

**Defined:** develop the complete first version before the release.
Do not publish partial versions.
Version the work locally during development.

**Defined:** after the validations of section 14, follow this internal order: SQLite, scripts, and pool; transactions; autocomplete; complete interface; table; configuration; credentials.
Section 1 shows what is already done in that order and what is missing.

- **Defined license:** `MIT OR Apache-2.0`.
- **Defined distribution:** binaries and `cargo install` at release.
- **Defined publishing:** `publish = false` left the `Cargo.toml`, because `cargo install sqln` depends on crates.io.

### Outside the first version

**Defined:** these items stay outside the first version.

- MongoDB and Kafka. The user reopened Redis and Valkey on 2026-10-31; see `docs/DECISIONS.md`. The user reopened the query history on 2026-11-01.
- In-memory SQLite databases.
- Grid inserts, deletes, binary editing, and edits without a primary key. Checked table updates replace the earlier exclusion on 2026-11-04.
- User-defined function and signature autocomplete. Built-in functions and signatures already have local completion.
- Extractor editor. The user reopened the export to file on 2026-10-31; see `docs/DECISIONS.md`.
- Integrated SSH tunnel.
- `COPY FROM STDIN`.
- Own view for `EXPLAIN`. The user reopened it on 2026-11-02; see `docs/DECISIONS.md`.
- Session preservation between executions, outside a script or transaction.
- IAM and Kerberos authentication.
- Detection of definitions that point to the same physical database.
- Catalog cache on disk.
- Opening `.sql` files outside `.sqln/`.
- Interface translation.
- Homebrew distribution.

Windows support stays optional.

### Performance

**Defined:** answer a key in under 50 ms with 256 MiB of loaded data.

### Tests

**Defined:** test the connectors on real databases, in containers.
Test the interface with the Ratatui `TestBackend`.
Use [compose.test.yaml](../compose.test.yaml) and the scripts of `scripts/`.

### Checks before the release

- Shortcuts in Terminal.app and Linux terminals.
- Letters after `Ctrl+W`, `F1`, and `Shift+F5` with the application.
- Mouse drag and paste.
- Locked Keychain and the Linux vault, with a real Linux desktop.
- Keepalive with a real network failure.
- Operating system CA and certificate with an IP.
- OSC 52 and clipboard on Linux.
- Result without rows in the TSV format.
- Security and license audit of the dependencies.
- Signing and notarization of the binaries on macOS.
- Confirm package-name and trademark availability through external checks. The repository alone does not establish availability.

## 18. Consulted references

- [JetBrains — Query consoles](https://www.jetbrains.com/help/datagrip/query-consoles.html)
- [JetBrains — Sessions](https://www.jetbrains.com/help/datagrip/managing-connection-sessions.html)
- [JetBrains — Explore data in data editor](https://www.jetbrains.com/help/datagrip/explore-data-in-data-editor.html)
- [JetBrains — Sort data](https://www.jetbrains.com/help/datagrip/tables-sort.html)
- [JetBrains — Data extractors](https://www.jetbrains.com/help/datagrip/data-extractors.html)
- [JetBrains — CSV Formats and DSV rules](https://www.jetbrains.com/help/datagrip/settings-tools-database-csv-formats.html)
- [JetBrains — Export and copy by extractor](https://www.jetbrains.com/help/datagrip/export-data.html)
- [Ratatui — Backends](https://ratatui.rs/concepts/backends/)
- [EdTUI — Documentation](https://docs.rs/edtui/latest/edtui/)
- [tokio-postgres](https://docs.rs/tokio-postgres/latest/tokio_postgres/)
- [mysql_async](https://docs.rs/mysql_async/latest/mysql_async/)
- [rusqlite](https://docs.rs/rusqlite/latest/rusqlite/)
- [SQLx — Pool](https://docs.rs/sqlx/latest/sqlx/struct.Pool.html)
- [SQLx — PoolOptions, acquisition, and return callback](https://docs.rs/sqlx/latest/sqlx/self/struct.PoolOptions.html)
- [SQLx — RawSql and streaming results](https://docs.rs/sqlx/latest/sqlx/struct.RawSql.html)
- [SQLx — raw_sql and multi-statement semantics](https://docs.rs/sqlx/latest/sqlx/fn.raw_sql.html)
- [SQLx — Connection](https://docs.rs/sqlx/latest/sqlx/trait.Connection.html)
- [SQLx Core — Transaction indicator implementation](https://docs.rs/sqlx-core/latest/src/sqlx_core/connection.rs.html)
- [SQLx PostgreSQL — Transaction manager](https://docs.rs/sqlx-postgres/latest/src/sqlx_postgres/transaction.rs.html)
- [SQLx MySQL — Transaction manager](https://docs.rs/sqlx-mysql/latest/src/sqlx_mysql/transaction.rs.html)
- [SQLx — Query](https://docs.rs/sqlx/latest/sqlx/query/struct.Query.html)
- [Keyring — Documentation](https://docs.rs/keyring/latest/keyring/)
- [Keyring — Interface to the system vaults](https://docs.rs/keyring/latest/keyring/v1/)

The references support the initial technical assessment.
`Cargo.toml` and `Cargo.lock` define the current dependencies. Historical driver results do not validate every later version.
