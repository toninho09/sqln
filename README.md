# SQLN

A terminal database client with several connections, SQL consoles, and table tabs.

SQLN brings an editor, a database navigator, and a results grid into one terminal.
Use familiar editor keys or Vim mode. Keep scripts with a project, or use global connections across projects.

**Databases:** SQLite, PostgreSQL, MySQL, Redis, and Valkey.

> SQLN is in development. Public packages and signed release binaries are not available yet.
> Start with the source installation below. Review the [known limitations](#known-limitations) before use with important data.

![SQLN SQL console with a highlighted query, database navigator, and results grid](docs/images/query-console.png)

*Write SQL, browse the catalog, and inspect results in one terminal.*

## Contents

- [Features](#features)
- [Screenshots](#screenshots)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Keyboard guide](#keyboard-guide)
- [Tables and results](#tables-and-results)
- [Transactions and execution](#transactions-and-execution)
- [Configuration and saved work](#configuration-and-saved-work)
- [Credentials and connection security](#credentials-and-connection-security)
- [Command-line reference](#command-line-reference)
- [Troubleshooting](#troubleshooting)
- [Known limitations](#known-limitations)
- [Development and automation](#development-and-automation)
- [Documentation](#documentation)
- [License](#license)

## Features

- **Several connections and consoles.** Keep independent scripts and table tabs open together.
- **Two editor modes.** Use conventional keys or Vim, with search, replace, undo, and SQL highlighting.
- **Catalog autocomplete.** Complete schemas, tables, views, columns, aliases, and SQL keywords.
- **SQL tools.** Format statements, toggle comments, and generate SQL templates from the navigator.
- **Table browsing.** Apply SQL filters, sort columns, move between pages, and count rows on request.
- **Results inspection.** Select cells, search loaded rows, inspect a row vertically, and format JSON values.
- **Export.** Write CSV, TSV, JSON, JSON Lines, or SQL inserts from the export dialog.
- **Import.** Load CSV, TSV, JSON, or JSON Lines into a table, with column mapping and a preview.
- **Execution controls.** Run a statement, a selection, or a script, with row limits, deadlines, and cancellation.
- **Query plans.** See the plan of a statement as a tree, with the share of each node.
- **Server sessions.** List the sessions of PostgreSQL or MySQL, cancel a query, or end a session.
- **Transactions.** Use autocommit or manual mode, with transaction state and age on screen.
- **Saved work.** Save consoles automatically and reopen tabs with their cursor, scroll, and layout.
- **Credential storage.** Use session passwords, macOS Keychain, or Linux Secret Service.
- **Appearance.** Choose themes, per-connection colors and icons, and optional Nerd Font symbols.
- **No telemetry.** SQLN does not send usage reports.

## Screenshots

These captures use sample SQLite data and the Astrodark theme.
A virtual terminal captures the running application. Font appearance can differ in your terminal.

### Table browsing

Browse rows in a table tab, set the sort order, and load the next page without changing your SQL console.

![SQLN table browser showing orders sorted by total, with page controls and database objects in the navigator](docs/images/table-browser.png)

### Export

Choose the output format, rows, file, and formatting options before writing an export.

![SQLN export dialog with CSV, TSV, JSON, JSON Lines, and SQL INSERT formats](docs/images/export-dialog.png)

## Installation

### Requirements

- macOS or Linux.
- A terminal with Unicode support. Truecolor is optional; Nerd Fonts are optional.
- A current stable Rust toolchain, with Cargo, for source installation.
- A native compiler and linker. SQLite is bundled and needs a C compiler.

The local development toolchain is Rust 1.98.0. The project does not declare a minimum Rust version.
CI checks the current stable toolchain.

On macOS, install the command-line development tools if you do not have them:

```sh
xcode-select --install
```

On Debian or Ubuntu, install the native build tools:

```sh
sudo apt-get update
sudo apt-get install --yes build-essential
```

Install Rust with [rustup](https://rustup.rs/) if needed.
A database server is not required to build SQLN or to use SQLite.
Linux password storage needs a running Secret Service in the desktop session.

### Install from source

Run these commands from a checkout of this repository:

```sh
cargo install --path . --locked
sqln --version
sqln
```

Cargo places the executable in `~/.cargo/bin` by default. Add that folder to `PATH` if needed.

To run without installation:

```sh
cargo run --release --locked
```

To pass an application argument through Cargo:

```sh
cargo run --release --locked -- ./existing.db
```

The database file must already exist. SQLN does not create a missing SQLite database from a command-line target.

### Workflow build artifacts

The **Build binaries** workflow produces archives for:

| Platform | Target |
|---|---|
| Linux, x86-64, GNU libc | `x86_64-unknown-linux-gnu` |
| macOS, Apple Silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |

Manual runs provide workflow artifacts. Version-tag runs also attach the archives to a GitHub Release.
See [manual binary builds](#manual-binary-builds) and [releases](#releases) for instructions.
Linux artifacts use Ubuntu 24.04 and are not static binaries. Older Linux systems can require a source build.
The macOS artifacts are not signed or notarized. They do not replace signed release binaries.

## Quick start

### Open a database

Open an existing SQLite file:

```sh
sqln ./existing.db
```

Or connect to a server:

```sh
sqln 'postgres://alice@localhost/my_database'
sqln 'mysql://alice@localhost/my_database'
sqln 'redis://localhost/0'
sqln 'valkey://localhost/0'
```

For PostgreSQL and MySQL, SQLN asks for a password when the URI omits it in an interactive terminal.
For password-protected Redis or Valkey, use the connection form to enter the password without shell-history exposure.

A command-line connection lasts for the session only. It does not create a saved connection or saved consoles for that target.
To save a connection, start `sqln` and add it through the navigator.

**Do not place passwords in shell commands.** A URI password can appear in shell history and process arguments.

### Use a saved connection

1. Start `sqln`.
2. Press `Ctrl+W`, then `h`, to focus the navigator.
3. Press `a` to open the connection form.
4. Select the database type.
5. Enter the address and other connection fields.
6. Use **Test** to check the connection.
7. Save the connection in the global or project scope.
8. Press `Enter` on the connection to open a console.

Use `Tab` to move through form fields. Passwords stay in the session unless you explicitly select storage.

### Run your first query

For SQLite, PostgreSQL, or MySQL, enter:

```sql
SELECT 1 AS example;
```

Press `F5`, or press `Ctrl+W`, then `e`, to run it.
The results appear below the editor. Use `Ctrl+W j` to move down to the grid.

For Redis or Valkey, use a command instead:

```text
PING
```

Each Redis command occupies one line. Redis consoles do not run SQL.

Press `Ctrl+P` for the command palette or `F1` for help at any time.

## Keyboard guide

`Ctrl+W e` means: press `Ctrl+W`, release it, then press `e`.
The prefix menu shows the available second keys. Press `Esc` to cancel it.
Uppercase keys matter: `Ctrl+W C` and `Ctrl+W c` are different actions.

### Main actions

| Action | Shortcut |
|---|---|
| Command palette | `Ctrl+P` or `Ctrl+W :` |
| Help | `F1` or `Ctrl+W ?` |
| Change panel | `Ctrl+W h/j/k/l` or `Ctrl+W`, then an arrow |
| Next / previous main tab | `Ctrl+W n` / `Ctrl+W p` |
| Run selection or current statement | `F5` or `Ctrl+W e` |
| Run the whole script | `Shift+F5` or `Ctrl+W s` |
| Show the sessions of the server | `Ctrl+W S` |
| Open the commands of the explorer | `Ctrl+W Tab` |
| Show the plan of a statement | `Ctrl+W E` |
| Run a statement and show its plan | `Ctrl+W A` |
| Stop execution | `Ctrl+W x` |
| Save console now | `Ctrl+S` |
| Rename console | `Ctrl+W N` |
| Change or attach console connection | `Ctrl+W a` |
| Format SQL | `Ctrl+W f` |
| Toggle line comments | `Ctrl+W /` |
| Query history | `Ctrl+W H` |
| Settings | `Ctrl+W ,` |
| Close tab | `Ctrl+W c` |
| Quit | `Ctrl+W q` |

`Ctrl+Enter` and `Shift+Enter` also run queries when the terminal supports enhanced keyboard input.
Use the prefix alternatives if the terminal does not deliver a shortcut.

**`Ctrl+C` copies; it does not stop execution or quit SQLN.** Use `Ctrl+W x` to stop and `Ctrl+W q` to quit.

### Editor

Conventional mode is the default. Change **Keyboard** in Settings to select Vim mode.

| Action | Conventional | Vim |
|---|---|---|
| Insert text | Type | `i`, `a`, or `o` |
| Select text | `Shift` + arrows | `v` |
| Undo | `Ctrl+Z` | `u` |
| Redo | `Ctrl+Y` | `Ctrl+R` |
| Copy / cut / paste | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | `y` / `d` / `p` |
| Search | `Ctrl+F` | `/` |
| Search and replace | `Ctrl+F`, then `Tab` | `Ctrl+F`, then `Tab` |
| Open suggestions | `Ctrl+Space` | `Ctrl+Space` in insert mode |

Suggestions also appear as you type. `Tab` accepts a suggestion by default; `Enter` inserts a new line.
Change **Autocomplete** in Settings if you prefer `Enter` or both keys to accept suggestions.

### Navigator

| Action | Key |
|---|---|
| Open connection, table, or view | `Enter` |
| Open console list / create console | `n` |
| Add / edit connection | `a` / `e` |
| Refresh catalog | `r` |
| Filter loaded tables and views | `/` |
| Inspect structure | `i` |
| Show server sessions | `S` |
| Generate SQL templates | `g` |
| Copy object name | `y` |
| Disconnect | `D` |
| Remove saved password, with confirmation | `p` |

The tree filter searches loaded catalogs only. It does not open a connection to find more objects.

The hint row of the navigator shows only the main commands. Press `Ctrl+W`, then `Tab`, to list each command of the current row.
That page uses the same keys as the table above, and it works from any panel. Its title names the row that the commands act on.

The [full key map](docs/KEYMAP.md) covers forms, dialogs, export, mouse controls, and custom shortcuts.

## Tables and results

Press `Enter` on a table or view to open its table tab.

- Press `f` for the `WHERE` field, or `o` for `ORDER BY`.
- Enter a SQL expression in `WHERE`, without the `WHERE` keyword.
- Press `Enter` to apply the filter and order.
- Press `s` to sort the current column, or click a column header.
- Press `[` and `]` for previous and next pages.
- Press `m` to load more rows.
- Press `c` to count rows for the applied filter. SQLN does not count automatically.

Use an explicit, unique `ORDER BY` when stable page order matters.
Pages use offsets; concurrent database changes can still move rows between pages.
Redis and Valkey use key-value views instead of SQL filter fields.

### Inspect, copy, and export

In a results grid:

- Press `Enter` to inspect a cell, or `r` to inspect a row vertically.
- Press `f` in a value or row window to toggle JSON formatting.
- Press `Ctrl+F` in conventional mode, or `/` in Vim mode, to search loaded rows.
- Copy a selection as TSV with `Ctrl+C`, or `y` in Vim mode.
- Press `e` to open the export dialog.

Without a selection, copy uses the current cell. Grid search never queries the database.

Export supports CSV, TSV, JSON, JSON Lines, and SQL inserts.
Choose a selection, loaded rows, or all rows. An all-rows export can run a new read and observe newer data.
Only the **Export** button writes the file. `Esc` cancels the dialog.

### Import a file

Press `I` on a table in the navigator, or in the grid of a table tab.

1. Choose a file. The dialog detects the format and the delimiter.
2. Check the mapping in **Columns**. A column without a file column keeps its default.
3. Choose what a failure does in **Options**.
4. Check the first rows in **Preview**, and press **Import**.

| Mode | On a failed row |
|---|---|
| All or nothing | The first error undoes the whole import. This mode is the start of each dialog |
| By batch | The first error stops the import. The saved batches stay |
| Log errors | The bad row goes to a log file, and the import continues. It stops after the error limit |

The import only adds rows to an existing table.
The log holds the rows of the file in plain text.
A connection marked as production asks before the import.

### Query plans

Press `Ctrl+W E` to see the plan of the selection or of the statement at the cursor.
The **Plan** tab shows a tree. The **Share** column shows the part of the work of each node.
The database does not run the statement for this plan.

Press `Ctrl+W A` to run the statement and see the real time and rows of each node.
SQLN asks first when the statement can write. On PostgreSQL, the change stays.

| Database | `Ctrl+W E` | `Ctrl+W A` |
|---|---|---|
| PostgreSQL | Cost and rows | Cost, rows, time, and loops |
| MySQL 8.0.16 or newer | Cost and rows | Cost, rows, time, and loops |
| SQLite | Steps only | Not available |

A plan uses one statement. The query history does not record plans.

### Server sessions

Press `S` in the navigator to open the sessions of a PostgreSQL or MySQL server.
Press `Ctrl+W S` in any panel to open the sessions of the connection of the current tab.
The list shows each session that the database user can see, except the session that reads the list.

- Press `q` to cancel the query of the session under the cursor. The session stays open.
- Press `K` to end the session. The database undoes its open transaction.
- Press `R` to read the list again.

SQLN asks before `q` and `K`. A read-only connection refuses both keys.
The database user needs the right to act on the sessions of other users.

## Transactions and execution

Consoles start in autocommit mode. Each console runs one statement at a time within its script.
A statement error, timeout, or stop ends the remaining script.

| Action | Shortcut |
|---|---|
| Toggle autocommit / manual mode | `Ctrl+W m` |
| Commit | `Ctrl+W C` |
| Rollback | `Ctrl+W R` |

**Commit and rollback shortcuts act immediately, without another confirmation.**
The status line shows transaction state and the age of an open transaction.
Closing a console with an open transaction requires a decision.

SQLN uses a connection pool. Do not rely on temporary tables or session settings surviving between separate executions.
Keep dependent statements in one script or an open transaction.
Redis and Valkey do not offer the SQL manual-transaction controls.

Row caps and the result-memory budget limit how much data SQLN loads. A capped result is not the full result set.
Change limits in Settings and statement deadlines in the connection form.
Cancellation depends on the database response; **Stop** is not a rollback guarantee.

SQLN asks before `UPDATE` or `DELETE` without `WHERE` and before state changes on a production-marked connection.
These checks are aids, not a security boundary. Use database permissions and backups to protect important data.

## Configuration and saved work

### Global and project scopes

| Scope | Folder | Use |
|---|---|---|
| Global | `$XDG_CONFIG_HOME/sqln`, or `~/.config/sqln` | Personal connections and defaults |
| Project | `<project>/.sqln` | Project connections, preferences, and consoles |

The global path also applies on macOS. `XDG_CONFIG_HOME` must be absolute to override the default.
SQLN searches upward from the working folder for the nearest `.sqln` directory.
A `.git` directory alone does not define an SQLN project.

Select a project folder explicitly:

```sh
sqln --project /path/to/project
```

SQLN offers to create the project configuration if the folder has no `.sqln` directory.
Preferences apply in this order: built-in defaults, global settings, then project settings.
Project settings override only the values they declare. Both scopes contribute saved connections.

Use `Ctrl+W ,` for Settings. The dialog shows scopes and inherited values.
Select **Shortcuts** to see the list of all configurable commands and their assigned shortcuts.
Select a command to record key sequences, remove shortcuts, or restore its inherited keys.
Select **Mouse OK** to include `Enter` in a recording.
Settings blocks conflicting shortcuts before saving them to the selected scope's `config.json`.
See [shortcut configuration](docs/KEYMAP.md#configuration) for the complete steps.

### Files

| File or folder | Content |
|---|---|
| `config.json` | Connections and preferences |
| `consoles/<id>.sql` | Saved console text |
| `consoles/<id>.json` | Console identity and metadata |
| `workspace.json` | Open tabs, cursor, scroll, and layout |
| `locks/` | Locks for concurrent instances |
| Global `history.jsonl` | Query history |
| Global `trust.json` | Confirmed destinations and project paths |
| Global `credentials.json` | Explicitly authorized plain-text fallback passwords, when used |

Saved consoles autosave 500 ms after the last edit. `Ctrl+S` saves immediately.
SQLN reports write failures and detects external script changes instead of silently replacing them.
Tabs reopen by default; turn off **Reopen tabs** in Settings if needed.

Project creation writes `.sqln/.gitignore` for workspace state, locks, and temporary files.
Connection definitions and saved SQL can still enter Git. Review them before sharing a project.
Autosave and history can contain sensitive SQL and literal values. They are not encrypted.

### Configuration example

Prefer Settings for routine changes. This complete minimal `config.json` shows the file format:

```json
{
  "version": 1,
  "preferences": {
    "keyboardMode": "conventional",
    "theme": "auto",
    "icons": "unicode",
    "reopenTabs": true,
    "queryHistory": false,
    "suggestionKeys": "tab"
  },
  "connections": []
}
```

Do not replace an existing configuration with this example: its empty connection list removes saved definitions from that file.
SQLN reports invalid configuration with its file and position. Correct it before restarting or reloading.
Press `Ctrl+W r` to reload configuration.

Available themes: `auto`, `astrodark`, `ansi`, `tokyo-night`, `catppuccin-mocha`, `catppuccin-latte`, `gruvbox`, and `nord`.
Use `ansi` for the terminal palette. Set `NO_COLOR=1` to remove colors.
Select `nerd` icons only if the terminal uses a Nerd Font.

## Credentials and connection security

### Password storage

Passwords stay in the session by default. Saving requires an explicit choice.

- **macOS:** system Keychain.
- **Linux:** Secret Service, such as GNOME Keyring, in the user session.
- **No supported vault:** SQLN can offer a global plain-text fallback after explicit confirmation.

A vault error does not silently enable plain-text storage. SQLN offers retry, session use, or cancellation.
A real Linux desktop unlock dialog still needs manual validation; container tests do not cover that dialog.

Imported destinations and new project paths require trust confirmation before stored-credential access.
Use `p` in the navigator to remove a saved password. Removing a connection asks about password deletion separately.

### TLS

Remote addresses use TLS with certificate and hostname verification by default.
`localhost`, loopback IP addresses, and Unix sockets default to no TLS.
Use `sslmode=verify-full` to require verified TLS explicitly, including for a local endpoint:

```sh
sqln 'postgres://alice@db.example.com/app?sslmode=verify-full'
sqln 'mysql://alice@db.example.com/app?sslmode=verify-full&sslrootcert=/path/to/ca.crt'
sqln 'rediss://redis.example.com/0'
sqln 'valkeys://valkey.example.com/0'
```

The form also accepts CA files and client certificates.
URI parameters include `sslrootcert`, `sslcert`, and `sslkey`; a client certificate requires both certificate and key.
MySQL client keys can use PKCS#1, PKCS#8, or SEC1 format. PostgreSQL client keys use PKCS#8 in the test setup.

SQLN does not use an unverified TLS fallback. Even `sslmode=require` and `verify-ca` use full verification.
An explicit `sslmode=disable` removes TLS; use it only when the connection environment permits unencrypted traffic.
`rediss` and `valkeys` always require verified TLS.

### Local data and diagnostics

Query history is on by default. SQLN filters recognized secret-bearing statements, but SQL literals can still contain sensitive data.
Disable **Query history** when needed. This does not remove old history or saved console text.

`--log` records event kinds without SQL, result values, or passwords.
Before sharing any diagnostic file, inspect it for private paths and connection details.

## Command-line reference

```text
sqln [options] [target]
```

`target` is one existing SQLite file or one connection URI.
Supported URI schemes include `postgres`, `postgresql`, `mysql`, `mariadb`, `redis`, `rediss`, `valkey`, and `valkeys`.

| Option | Purpose |
|---|---|
| `--project <folder>` | Select the project root |
| `--log <file>` | Write the event log |
| `-h`, `--help` | Show command-line help |
| `-V`, `--version` | Show the version |
| `--dialect <name>` | Set `postgres`, `mysql`, `sqlite`, or `redis` for an unconnected console; development option |

SQLN is an interactive application. It does not provide a batch SQL runner or read scripts from standard input.
A `.sql` path is not a script-opening argument; file targets are SQLite databases.

## Troubleshooting

| Problem | What to check |
|---|---|
| `sqln` is not found | Add Cargo's executable folder to `PATH`, or use `cargo run --release --locked` |
| A shortcut does not arrive | Use `Ctrl+W` alternatives, or open the palette with `Ctrl+W :` |
| Copy or paste behaves differently on macOS | `Cmd+C` copies the terminal selection; `Ctrl+C` copies the SQLN selection |
| Native mouse selection does not work | SQLN captures the mouse; iTerm supports native selection while you hold `Option` |
| Icons appear as empty boxes | Choose Unicode icons, or install and select a Nerd Font |
| Theme colors look wrong | Choose `ansi`, or use `NO_COLOR=1 sqln` |
| A remote connection fails TLS verification | Check the hostname, CA chain, system clock, and configured CA file |
| The Linux vault is unavailable | Check the session D-Bus and Secret Service; use session-only passwords if necessary |
| SQLite cannot open a file | Check that the file exists and that its folder and file permissions allow access |
| A result contains fewer rows than expected | Check the row cap, page controls, and result-memory budget |
| A temporary table disappears | Keep dependent statements in one script or transaction; pooled sessions reset between executions |
| A query continues after Stop | Cancellation needs a database response; inspect server activity and transaction state |

## Known limitations

- Windows support is not part of the current CI or binary matrix.
- SQLite needs an on-disk file. In-memory databases are not supported.
- Grid cells are not editable. Use SQL for data changes.
- Arbitrary external `.sql` files cannot open directly as consoles.
- There is no integrated SSH tunnel, IAM authentication, or Kerberos authentication.
- MariaDB has no tree plan, so `Ctrl+W E` shows the error of the server. MySQL has no tree plan for a write on one table.
- PostgreSQL `COPY FROM STDIN` and `COPY TO STDOUT` are not supported.
- Search in results covers loaded rows only. Exporting all rows can issue a new read.
- Import cannot create a table, update rows, or read SQL files. A MyISAM table cannot undo an import.
- Undo and redo have entry limits but no memory budget. Large scripts and repeated edits can use substantial memory.
- Native terminals, clipboard behavior, network disks, real network failures, and desktop vault prompts need further manual validation.
- Workflow builds do not provide signing, notarization, or a dependency audit.

## Development and automation

### Local checks

```sh
make fmt     # Format Rust code.
make lint    # Check formatting and Clippy.
make test    # Run tests without requiring database servers.
make check   # Run lint and the complete default test suite.
make bench   # Print the performance and memory reports and run the latency targets.
```

Database-dependent tests skip when their environment is absent. A passing local check alone does not prove database integration.
Native-vault tests require explicit authorization. Performance tests do not run as benchmarks in the default debug suite.

### Database integration

Docker Compose is required. The test stack uses disposable databases and loopback-only ports.

```sh
make test-integration
```

The command starts the databases, loads the test environment, runs the tests, and stops the databases.

For TLS coverage, use a Bash shell:

```bash
(
  set -e
  trap 'docker compose -f compose.test.yaml --profile tls down' EXIT
  scripts/test-certs.sh
  docker compose -f compose.test.yaml --profile tls up -d --wait
  . scripts/test-env.sh
  export SQLN_TEST_TLS=1
  make check
)
```

The certificate script replaces `.test-certs/` with disposable test certificates. Never use them outside the test stack.
The TLS stack includes PostgreSQL, MySQL, and Redis endpoints.

### Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on pushes to any branch, including `main`:

- Formatting, Clippy, and the default test suite on Linux and macOS.
- Database integration on Linux, with PostgreSQL, MySQL, Redis, Valkey, and TLS endpoints.
- Database logs on failure and stack cleanup after the integration job.

Opening a pull request does not start another run. The pull request shows checks from its branch commit.
CI tests that commit, not a temporary merge with `main`. A merge into `main` starts a new push run.
Tag pushes do not start this workflow.

CI does not run benchmarks or authorized native-vault probes.
It does not replace real-terminal or desktop-vault checks.

### Manual binary builds

[`.github/workflows/binaries.yml`](.github/workflows/binaries.yml) supports manual builds through **workflow_dispatch**.
A manual build does not publish a release, even when you select a tag.
GitHub shows **Run workflow** after the workflow exists on the default branch.

1. Open the repository's **Actions** tab.
2. Select **Build binaries**.
3. Select **Run workflow** and the source ref.
4. Download the artifact for your platform after the job completes.
5. Extract the downloaded artifact ZIP.
6. Verify the archive checksum with `shasum -a 256 -c <archive>.sha256`.
7. Extract the `.tar.gz` archive.
8. Run `./sqln --version` inside the extracted folder.

Each archive includes `sqln`, this README, both license files, and build information with the source commit and Rust version.
Artifacts remain available for 14 days, subject to repository retention settings.
Checksums detect changed archive bytes; they are not signatures or proof of publisher identity.

The workflow builds and checks `--help` and `--version`. Run CI for the same commit before distributing an artifact.
It does not publish to crates.io, sign binaries, or notarize macOS builds.
No additional publishing or signing secrets are required.

### Releases

Pushing a tag that starts with `v` starts the three binary builds in the same workflow.
After all three builds pass, a separate job verifies checksums and publishes a GitHub Release for that tag.
The release includes the three `.tar.gz` archives, their checksum files, and generated release notes.

**A version-tag push authorizes public release publication.** Do not push the tag until the version is ready.
The workflow does not wait for branch CI. Confirm its success for the same commit before creating a release tag.

1. Update the package version in `Cargo.toml` and `Cargo.lock`.
2. Commit the version change.
3. Push the commit.
4. Confirm that its CI checks pass.
5. Create the matching tag, such as `git tag v0.1.0`.
6. Push the tag with `git push origin v0.1.0`.
7. Download the archives from the repository's **Releases** page after the workflow completes.

The release job alone has `contents: write` permission. It uses GitHub's built-in `GITHUB_TOKEN`.
Build jobs have read-only repository access. Manual runs skip the release job.
A failed build prevents publication. This process does not publish to crates.io or sign or notarize binaries.

### Contributing

Keep changes small and add tests for changed behavior. Run `make check` before submitting a change.
Run integration tests when a change affects connectors or database integration.
Report the OS, terminal, SQLN version, and reproduction steps for a defect. Remove secrets from reports and sample SQL.

## Documentation

- [Feature map](docs/FEATURE-MAP.md): features and their source files.
- [Keyboard reference](docs/KEYMAP.md): complete navigation and shortcut guide.
- [Architecture](docs/ARCHITECTURE.md): execution, storage, credentials, and module rules.
- [Decisions](docs/DECISIONS.md): approved choices and tradeoffs.
- [UI reference](docs/UI.md): interface structure and layouts.
- [Consolidated plan](docs/INITIAL_PLAN.md): scope, implementation state, and pending release checks.
- [Technical test results](docs/RESULTS.md): initial driver and terminal investigations.
- [Pagination validation](docs/PAGINATION-VALIDATION.md): pagination checks and their limits.

## License

SQLN is available under either the [MIT license](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option.
