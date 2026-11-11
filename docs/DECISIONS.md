# SQLN — Decision record

This document records the choices and the rejected alternatives.
The complete rules are in the [plan](INITIAL_PLAN.md).
Do not reopen a decision without a new fact.
Later dated decisions replace earlier choices on the same subject. Earlier tables preserve the historical alternatives.
Implementation notes describe their recorded stage unless a current-state note says otherwise.

## Side of the explorer — 2026-11-10

The user asks to choose the side of the explorer and approves the points below.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Place of the choice | **Explorer side** field in `Settings > Appearance`, after **Default theme**: Left (default) or Right. It goes to `explorerSide` of `preferences`, with the global and project scopes | A command that swaps the side, saved in `workspace.json`; both | User choice |
| 2. Focus keys | They follow the screen. With the explorer on the right, `Ctrl+W l` goes to the explorer and `Ctrl+W h` leaves it | `Ctrl+W h` always goes to the explorer | User choice |

| Implementation point | Decision | Rejected | Reason |
|---|---|---|---|
| Text of the focus keys | "Go to the left panel" and "Go to the right panel" | Text that changes with the side | The help, the palette, and the shortcuts read one static text per command |
| Home screen and hints | The path to the explorer uses the focus key of its side, such as `Ctrl+W l a` | Always `Ctrl+W h` | The shown key must act |
| Divider | A drag of the divider changes the width on both sides; the console keeps its minimum width | — | Same rule as the left side |
| Change | The save of Settings moves the explorer at once | A restart | Same rule as the other preferences |

## Kafka and Schema Registry — 2026-11-10

The user asks for Kafka support with the Schema Registry and approves the points below.
This request reopens the rule "MongoDB, Redis, and Kafka stay out of the first version" only for Kafka.
It also reopens the rule "Search: literal text, no regular expression" only for the Kafka filter.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Scope | Browse and read, `TAIL`, consumer groups and lag | Produce messages; topic administration | User choice |
| 2. Library | `rdkafka` 0.39 (librdkafka) | `rskafka`; own client with `kafka-protocol` | User choice. librdkafka has groups, SASL, and offsets by time |
| 3. Formats | Avro, JSON Schema, and Protobuf | Avro only | User choice |
| 4. Console | Own commands, one for each line; `#` at the start of a line is a comment | Only the explorer | User choice. Same model as Redis |
| 5. Connection type | "Kafka", with `kafka://` and `kafkas://`, port 9092 | Two types | Same pattern as `redis://` and `rediss://` |
| 6. Bootstrap | One host. The metadata of the cluster gives the other brokers | A list of hosts | `NetworkTarget` has one address. YAGNI |
| 7. TLS | SQLN rule: local without TLS, remote verifies, `?sslmode=disable` turns it off; `kafkas://` always uses TLS | — | The password only travels without TLS by explicit choice |
| 8. Authentication | Optional user. With a user: SASL `PLAIN`, `SCRAM-SHA-256`, or `SCRAM-SHA-512` (default) | GSSAPI and OAuth | GSSAPI needs Cyrus SASL in C |
| 9. No consumer group | A read never subscribes and never commits (`enable.auto.commit=false`, `enable.auto.offset.store=false`), only `assign`. librdkafka needs a `group.id` for a consumer, so a read uses the local id `sqln-read`; without `subscribe` and without a commit the cluster never creates that group. `GROUP <id>` asks only for committed offsets (OffsetFetch) and never joins | No `group.id`; a random group; the legacy librdkafka API through FFI | Spike: librdkafka refuses `assign` without `group.id`. The user chose the local id. A test compares the group list before and after each read |
| 10. Read only | Each command only reads, so the read-only option changes nothing | — | The scope has no write |
| 11. Commands | `TOPICS`; `PARTITIONS <topic>`; `CONSUME <topic> [PARTITION n] [FROM earliest\|latest\|<offset>\|@<time>] [WHERE <filter>] [LIMIT n] [SCAN n]`; `TAIL <topic> [PARTITION n] [FROM …] [WHERE <filter>]`; `GROUPS`; `GROUP <id>`. Clauses come in any order, each one time | — | Covers the scope |
| 12. `CONSUME` | Reads to `LIMIT` matches (default: the row cap) or to the high offset of each partition at the start. `SCAN n` limits the read messages. **Stop** ends the read and keeps the rows. A filter, a scan limit, or a stop adds `Scanned N messages; M matched.` in **Messages** | — | A finite result, as a query |
| 13. `TAIL` | A continuous command of the 2026-11-09 contract: it grows until **Stop**; at the cap, the oldest rows leave. The default start is `latest` | — | Reuses the live flow |
| 14. Filter: place | In the client, after the decoding: console (`CONSUME`, `TAIL`) and the filter field of the topic tab, with the same syntax | Server filter | Kafka has no server filter |
| 15. Filter: syntax | `key = 'text'`; `key\|value\|headers CONTAINS 'text'` (literal, without case); `key\|value MATCHES 'regex'` (crate `regex`, linear time, size limit; `(?i)` ignores case); `value.a.b.0 <op> <literal>` with `=`, `!=`, `<`, `<=`, `>`, `>=` and a text, a number, `true`, `false`, or `null`; `AND`, `OR`, `NOT`, parentheses; `AND` before `OR` | — | User choice: the four kinds |
| 16. Filter: cases | A missing field or another type is false. A value without JSON makes a field condition false; `CONTAINS` and `MATCHES` read the text only when it is UTF-8. A syntax error or a bad regex refuses before the read, with its position | — | No surprise and no silent failure |
| 17. Columns | `partition`, `offset`, `timestamp` (UTC), `key`, `value`, `headers` (JSON), `schema` | — | Grid search and copy work |
| 18. Decoding | For key and value: the byte 0 and a configured registry decode with the schema to JSON; a failure keeps the bytes and adds a notice; else UTF-8 text; else bytes. A tombstone is `NULL` | — | Never loses data in silence |
| 19. Explorer | Two fixed schemas: `topics` and `groups`. `Enter` on a topic opens a tab with the last 500 messages that match the filter (empty: all), newest first, with **Refresh** and without pages. The read goes back in windows of offsets for each partition until 500 matches or the start. `Enter` on a group opens its lag | Pages of a global offset | An offset across partitions has no stable page |
| 20. Lag | `GROUP <id>`: `topic`, `partition`, `committed`, `end`, `lag`, `member`. `GROUPS`: `group`, `state`, `protocol`, `members` | — | Read only |
| 21. Schema Registry | Optional fields of a Kafka connection: URL, user, and password. `config.json` gets `"schemaRegistry": {"url", "username"}` and `"sasl"`; `version` stays 1. The password follows the **Credential** choice of the form: **Vault** writes it to the system vault, **Session** keeps it in memory | Always the vault | The user authorizes each secret storage |
| 22. Schema cache | By id, for the life of the connector | Expiry | A schema id never changes |
| 23. Completion | Fixed list of commands, then the topics or groups of the catalog, then the clause words | — | Topics are few, unlike Redis keys |
| 24. AI and MCP | No AI completion and no MCP read for Kafka | — | Same rule as Redis |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Build | `rdkafka` with `cmake-build`, `ssl-vendored`, `libz-static`, `zstd`, and `tokio`. A clean build needs `cmake` and `perl` | System librdkafka | One binary, without a system library |
| TLS of librdkafka | OpenSSL inside librdkafka only. `ssl.ca.pem` gets the system roots of `connector::tls::system_roots` in PEM; a CA file goes to `ssl.ca.location`; a client pair goes to `ssl.certificate.location` and `ssl.key.location` | A second set of roots | The other connectors keep `ring`; the roots stay the same, except the bundled `webpki` roots, which have no certificate form |
| TLS of the registry | `reqwest` with `connector::tls::client_config` | — | One TLS rule |
| Calls that wait | Metadata, offsets, group list, and committed offsets run in `spawn_blocking` with a 10-second limit | The executor thread | The interface thread never waits |
| One read, one consumer | Each `CONSUME`, `TAIL`, and topic-tab round creates its own consumer and closes it on a blocking thread | One consumer for the session | A later read never receives a message of an earlier read |
| Stop | The read watches the stop token of the sink. The `Canceller` does nothing | — | The read ends by itself |
| Logs of librdkafka | Dropped | The `log` crate | They can hold names and values |
| Batch password | `--execute` asks for the password on the terminal for a Kafka user without a saved password | The Redis rule: ask only with a saved reference | A SASL user always needs a password |
| Refused password | A consumer receives the broker errors as events of its queue. A failed call reads these events, so an authentication error asks for a password | Only the result of the call | The call result says only "transport failure" |
| Registry password | The connector reads the vault one time, on a blocking thread, when the registry has a user and the session has no password. The vault account is the registry URL and the user | The vault in the app | The batch mode and the app use the same path |
| Shared word reader | `connector::words` holds the `redis-cli` word reader and `command_word`, for Redis and for the Kafka completion and tab text | A copy for Kafka | DRY |
| Topic tab | `TableTab::filter_only` shows only the filter field; `kind_word` names the tab `topic` or `group`; the page text is `SQLN.TOPIC <topic> <count> [WHERE …]`, which is not a console command | A new tab kind | Reuses the table tab |

### Known limitations of Kafka

| Subject | Limitation |
|---|---|
| Bootstrap | One host. If that broker is down, the connection fails |
| TLS roots | Without a CA file, librdkafka uses only the system roots |
| Authentication | No GSSAPI and no OAuth. The tests do not cover mutual TLS or SCRAM |
| Topic tab | The order is the producer timestamp. A partition with timestamps out of offset order can give an approximate list |
| `GROUP` | It asks the committed offsets of each partition of each topic, and one end offset for each committed partition. A large cluster makes many calls |
| Internal topics | Names that start with `__` stay out of the explorer. `TOPICS` lists them with `internal` set to `true` |
| Registry | A Protobuf message index outside the file, or an Avro logical type without a JSON form, keeps the bytes with a notice |

## Continuous commands — 2026-11-09

The user asks for the Redis streaming commands, with a generic contract for other sources later.
This request reopens point 5 of "Redis and Valkey — 2026-10-31" and the refusal of `SYNC` and `PSYNC` in that section.

| Point | Approved rule |
|---|---|
| Commands | `SUBSCRIBE`, `PSUBSCRIBE`, `SSUBSCRIBE`, `MONITOR`, `SYNC`, `PSYNC`, and `XREAD` or `XREADGROUP` with `BLOCK` before `STREAMS` |
| Display | The result grid of the console grows until **Stop** |
| Limit | At the row cap or at a full budget, the oldest rows leave the grid; the summary shows the count of dropped rows |
| Script | The continuous command must be the last command of the run; otherwise the run is refused with a reason |
| Protection | Pub/Sub and `XREAD` with `BLOCK` count as reads: a read-only connection runs them, and a production connection does not ask. `MONITOR`, `SYNC`, `PSYNC`, and `XREADGROUP` keep the current rules |
| Time limit | No SQL time limit for the continuous command; the earlier commands of the run keep it |
| Other modes | Console only. Batch execution, MCP and agent tools, and auto-refresh refuse with a reason |
| RDB | `SYNC` and `PSYNC` show one `rdb` row with the size of the file; SQLN reads and drops its bytes |
| `XREAD` with `BLOCK` | Always continuous, in every mode. Batch and MCP now refuse it; use `XREAD` without `BLOCK` for one read |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Generic contract | `sql::is_continuous` names the commands; `RunOptions::live`, the live mode of `ResultSink`, and `ResultTab::append_live` are the same for each source | One flow for each source | Another source adds only its rule and its connector loop |
| Stop | The sink has a stop token; the connector waits for it with the network and ends by itself. The runner does not call the `Canceller` | The `Canceller` of the session | `CLIENT UNBLOCK` does not stop a subscriber |
| Connection | Pub/Sub, `MONITOR`, `SYNC`, and `PSYNC` use their own connection, with the TLS rule of SQLN, RESP2, and no `SELECT`: those commands cross databases | The session connection | A stop closes that connection, and the session of the pool stays clean |
| Reader | Own small RESP2 reader | Driver parser | `aio::Monitor::new` is private in `redis` 1.8, and the driver parser does not accept the RDB file |
| `XREAD` | Runs again on the session connection. A `$` becomes the last entry that exists at the start; each answer moves the ID of its stream; a `>` stays. **Stop** sends `CLIENT UNBLOCK` | A second connection | No entry goes between two reads, and a stopped read keeps the session |
| `PSYNC` | Sends `REPLCONF ACK` with the offset each second | No acknowledgment | The server closes a replica without acknowledgment after `repl-timeout` |
| Columns | Pub/Sub: `received_at`, `kind`, `pattern`, `channel`, `message`. `MONITOR`: `time`, `database`, `client`, `command`. `SYNC`/`PSYNC`: `received_at`, `kind`, `value`. `XREAD`: `stream`, `id`, `fields` | One text column | Uses the grid search and copy |
| Subscription answers | A notice in **Messages** | A row | The rows hold only messages |
| Status | `Live` in the status line and in the summary while the result grows. After **Stop**, the grid stays on screen | `Running` | The state needs a text label |

### Known limitations of continuous commands

| Subject | Limitation |
|---|---|
| Rows in transit | The rows between the connector and the grid do not count in the budget. The event queue holds at most 256 batches of 200 rows |
| Dropped rows | A copy, an export, or an analysis reads only the rows that stay in the grid |
| Width | At 80 columns, the last columns of a Pub/Sub row need a horizontal move |
| `PSYNC` | `+CONTINUE` and a diskless transfer with an `EOF` mark are not tested. SQLN does not announce `capa eof` |
| Valkey and Redis versions | `SSUBSCRIBE` needs Redis 7 or Valkey |

## Batch execution of scripts

The user approves several instructions and standard input for `--execute`.
This extends the recorded rules of batch execution without the terminal interface; that entry keeps the first delivery.

| Point | Approved rule |
|---|---|
| Batch entry | `--execute <SQL>` or `--execute -`; the dash reads SQL from standard input, before SQLN resolves the connection |
| Input size | Refuse standard input above 10 MiB and input without one instruction; both are use errors |
| Instructions | Several instructions run in one execution, in order, on one session |
| Several result sets | Write every set in order: CSV and TSV repeat the header, JSON writes one array per set on its own line |
| Stop on error | The default stops at the first failed instruction; `--continue-on-error` runs the rest and reports each failure with its number |
| Single transaction | `--single-transaction` opens one transaction, commits when every instruction succeeds, and rolls back on any failure or signal; Redis refuses the flag |
| Output | `--output csv\|json` is optional; without it, standard output stays empty and each set reports its row count on the error stream |
| Reports | `rows affected` and `rows returned` carry the instruction number on the error stream |
| Unchanged | Targets, credentials, trust, protection, deadlines, signals, exit codes, and the no-write state rules of the first delivery stay |

Implementation notes:

- With `--single-transaction`, SQLN finishes and flushes result output before the final `COMMIT`. A flush failure triggers rollback and keeps the output failure as the primary error. SQLN does not finish the encoder twice.
- Output can reach the consumer before `COMMIT`, even when `COMMIT` later fails. A successful flush does not guarantee consumer storage.
- A script that ends its own transaction makes the final `COMMIT` of `--single-transaction` fail; the report explains it.
- On PostgreSQL inside `--single-transaction`, a failed instruction can abort the transaction, so later instructions fail with the server message; SQLN reports every failure and rolls back.
- A signal keeps the first-delivery behavior: SQLN stops, drops the session, and the server rolls back an open transaction.

## Local SQL syntax warnings

The user approves a local syntax checker that reuses the existing `sqlparser` dependency.
This extends the SQL analysis tools. It does not replace the lexer selected in the Libraries decision.

| Point | Approved rule |
|---|---|
| Scope | Syntax warnings for PostgreSQL, MySQL, SQLite, and DuckDB; no style or catalog validation |
| Execution | Local background analysis after a short pause; no SQL execution or database access |
| Editing | Preserve text, cursor, selection, conventional mode, and Vim mode |
| Display | Located underlines and messages; explain parser coverage limits |
| Protection | No corrections or execution blocking; existing execution checks remain unchanged |

Implementation notes: the pause is 400 ms. Reports match revision and dialect.
The checker normalizes ASCII named parameters without reading their values or changing source offsets.
The active console uses the existing blocking executor, with at most one pending job per editor.
Limits are 256 KiB, 128 instructions, 32 warnings, 512 message characters, and recursion depth 64.
Each instruction has a limit of 1024 parser tokens, excluding whitespace and comments.
The checker skips larger instructions before AST construction to bound recursive destruction. Other instructions still check.
Skipped or partial checks carry a reason. A worker failure asks for an edit to retry.
The palette action `sqlDiagnostics` opens the existing scrollable report, with no default shortcut.
No configuration or saved-format changes. The parser has both false positives and false negatives.
In particular, some MySQL routine syntax is unsupported. The database remains the final syntax validator.

## Analysis of loaded results

The user approves column profiles, ordered text charts, and differences between consecutive console executions.
The existing EXPLAIN ANALYZE implementation remains; no second execution path is needed.

| Point | Approved rule |
|---|---|
| Profile | Current column: loaded row count, NULL count and percentage, distinct values, and numeric-only min/max and histogram |
| Data | No new query. A partial result or current table page carries a loaded-rows scope label |
| Charts | Bar or line from two numeric columns. Preserve result order and keep every repeated X occurrence separate, without aggregation |
| Retention | Optional per console; one previous complete result in memory, under the shared data budget |
| Matching | The user selects key columns. Reordering rows does not create differences |
| First comparison | Exactly one complete data result per execution; no query plans or partial results |
| Failure | Explain the failure or memory limit and cancel. No partial report, automatic retry, or additional query |

Implementation notes:

- `Action` registers `gridProfile` (`p`), `gridChart` (`b`), `gridKeepPrevious` (`B`), and `gridComparePrevious` (`D`).
- Existing menus, palette, hints, custom shortcuts, conventional mode, and Vim mode share availability.
- Distinct counts exclude NULL and reuse the equality of `compare::data`; numeric min/max keep decimal precision.
- Histograms use ten equal-width approximate ranges, or one range for a constant column. Unsupported numeric values carry an exclusion count.
- Charts use occurrence positions, not numeric X distances. Line sections keep every point without sampling; point text preserves the values.
- Comparison requires identical ordered column names and unique selected keys on both sides. It produces a report, not synchronization SQL.
- Retention moves the old rows without cloning or releasing their budget charge. An execution failure or incomplete result cancels comparison and releases the snapshot.
- Work advances in bounded steps and rejects changed generations. Scratch indexes, points, and report text use the shared budget.
- Reports use the existing scrollable, copyable text window. Cancel releases the job and its reservations. No saved format changes.

## Execution without the terminal interface

The user approves batch execution for shell scripts, cron, and CI.
This extends the command-line target rules without changing `--script`: opening a SQL file still executes nothing.

| Point | Approved rule |
|---|---|
| Entry | `--execute <SQL>` with required `--output csv\|json`; no terminal interface, workspace restoration, or MCP |
| Connection | One path or URI, or a saved connection by `--connection <name>`; `--scope global\|project` resolves ambiguous names |
| SQL | Exactly one instruction; no script file or SQL from standard input in this delivery |
| Results | One result set, with the existing CSV/JSON encoder rules; no grid cap, SQL limit insertion, or reread |
| Deadline | Preserve the connection SQL timeout, independently of streaming output |
| No result | Empty standard output; affected rows go to standard error |
| Messages | Notices and failures go to standard error; questions use the controlling terminal |
| Authorization | `--yes` authorizes protected execution and missing database creation; otherwise ask on the terminal, with No as default |
| Trust | A saved network destination and project retain a separate trust confirmation; `--yes` never grants trust |
| Credentials | Password file, then `SQLN_PASSWORD`, then saved credential or URI password, then a hidden terminal question |
| Password file | UTF-8; remove one final LF or CRLF, preserve other whitespace; failure does not fall back |
| Protection | Read-only, TLS, production, and missing-WHERE protections remain; use shared risk analysis |
| End | Stop on output failure or SIGINT/SIGTERM, use existing cancellation, and never retry SQL |
| Transactions | Do not leave a transaction open after a one-instruction invocation; roll it back and report failure |
| Status | `0` success, `1` operational failure or refusal, `2` invalid arguments |
| State | No console, workspace, history, or export-preference writes; no password saving or credential migration |

Standard output cannot be recalled after a write. A failed invocation can leave partial or invalid output.
Database writes can commit before an output failure or cancellation. One instruction can produce multiple result sets; detecting a second set fails instead of merging it.
CSV data retains control bytes and formula-like text, as file export does. Redirect machine output to a file or pipe when values are untrusted.

## Themes with colored bases — 2026-11-08

The user asked for themes with different base colors, to distinguish environments at a glance.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| List | Add Everforest (green dark), Kanagawa (ink blue), Night Owl (navy), Rose Pine Dawn (warm light), Everforest Light (green light), and Ember (red dark) | More neutral dark themes | The tinted bases separate development, staging, and production at a glance |
| Production mark | `ember` uses a red-tinted base | A setting that colors only the status line | The base is visible on every screen, including the grid and the dialogs |
| Contrast | Adjust some original tones to keep 4.5 on body text and 3.0 on marks and labels | Keep the original tones | Bright fills and tinted bases need their own text colors |
| Definition | Each theme remains a `Scheme` constant and a `ThemeName` variant, with a reserved id | Environment-specific settings | The existing per-connection theme already selects a palette |

## More built-in themes — 2026-11-08

The user asked for more themes inspired by popular developer palettes, including high-contrast choices.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| List | Add Dracula, One Dark, Monokai, Solarized Dark, Solarized Light, GitHub Dark, Rose Pine, High Contrast, and High Contrast Light after Nord | Only dark themes; only one high-contrast theme | The palettes cover different tastes, and the two high-contrast themes serve accessibility on dark and light terminals |
| Definition | Each theme is a `Scheme` constant and a `ThemeName` variant, and its id stays reserved | Ship them as custom theme files | Built-in themes do not depend on files or scopes |
| Contrast | Some original tones change to pass 4.5 on body text and 3.0 on marks and labels | Keep every original tone | Some official tones fail the minimum contrast, as the first themes already did |
| Order | Append the names after Nord | Reorder the existing list | Saved values stay valid, and the settings keep their current choices in place |

## SQL files and table import

The user chose to remove the proposed SQL row importer and use the external-file console flow instead. Table import remains limited to CSV, TSV, JSON, and JSON Lines. Opening a SQL file does not execute it; the user reviews and runs it through the existing console execution flow and its protections.

## External `.sql` files as consoles — 2026-11-07

The user asked for the open of arbitrary `.sql` files as consoles, and chose the question at the open and the command-line flag.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Question | The open asks: a **copy** of the text, or a console **linked** to the file | One fixed mode | Both uses are common; the user chose the question |
| 2. Copy | A normal console: the autosave and the saved consoles apply without a change | Mark the copy read-only | The copy follows the rules of every console |
| 3. Link | The console writes the file only on an explicit action: `Ctrl+S` and the questions of a close | Autosave to the external file | A write of a file of the user needs an act of the user |
| 4. Conflicts | The write checks the bytes that the console read: a change by another program asks reload or overwrite, a removed file asks to recreate | Silent overwrite; a lock file | The rules of the atomic write and `Trouble` already cover the cases, and an external file has no sidecar |
| 5. Place | A linked console lives in the session only: it enters no workspace and no store | Persist the link in the workspace | The session format would change for one case; the file keeps the text |
| 6. Entry points | The palette command `openSqlFile` (no default key), the file browser, and `--script <file>` on the command line | A fixed shortcut | The palette serves commands without a key, and the flag serves the shell |
| 7. Connection | The connection of the active console, then the only connection, then a picker; without connections the command refuses with a message | Open without a connection | A console runs its text against a connection |
| 8. `--script` | The file must exist, and a start without a target needs a saved connection; the same question of the open follows | Run the script at the start | The flag opens a console; it does not run anything |
| 9. Encoding | UTF-8, with the byte order mark kept on each write; a file in another encoding is an error | Windows-1252 | The editor holds text, and the import already covers other encodings |

Implementation notes: `script_file/external.rs` holds the link; the console field is `external` beside `file`, and `ensure_file` skips a linked console, so it never becomes a script of the store. The tab row marks a linked console with changes. Canceling the open creates no console; canceling a conflict during a close also cancels the close or exit. External saves use unique temporary files, retain permissions, and follow symbolic links without replacing them. If a link points elsewhere after opening, the save refuses and asks the user to reopen it. Mixed line endings are retained for matching lines and paired changed lines; new lines use the file's existing default.

## Grid inserts and deletes — 2026-11-06

The user asks for pending inserts and deletes in the grid editing of table tabs, beside the pending updates.
This entry extends "Grid editing — 2026-11-04"; every rule of that entry stays.

| # | Point | Approved rule | Rejected | Reason |
|---|---|---|---|---|
| 1 | New row | `n` appends one pending insert row of empty cells at the end of the loaded rows; `F2` edits each cell with the existing modal | A whole-row dialog | One flow with the updates; the modal already edits values |
| 2 | Empty cell | An empty cell in a pending row means "not provided": the column stays out of the `INSERT` and the database default applies. NULL stays explicit | Store a distinct unset value | The default case needs no typed input |
| 3 | Delete mark | `d` toggles the delete mark of the row of the cursor. A row with cell changes returns to its original values, because the delete wins. A second `d` on a pending insert row removes it, and no statement runs for it | Refuse the mark while cells changed | The user decides; the review shows the final SQL |
| 4 | Keys of a marked row | A row marked for delete refuses cell edits until the mark leaves | Edit and delete at once | Two pending states of one row can disagree |
| 5 | Insert columns | Keys, identities, generated, and binary columns stay protected. A natural key without an identity stays editable in a pending row | Protect every key column | Natural keys need a value |
| 6 | Insert validation | A `NOT NULL` column without a default and without a provided value refuses the review and the apply with the column name | Send NULL | The database default and an explicit value stay distinct |
| 7 | All-defaults insert | With no provided column, SQLite and PostgreSQL write `DEFAULT VALUES` and MySQL writes `() VALUES ()` | Refuse | Every column can have a default |
| 8 | Apply order | One short transaction applies the updates, then the deletes, then the inserts. Deletes lock and verify the original row exactly as updates | One statement per apply; deletes first | Foreign keys can need the user's order, and the review shows it |
| 9 | Insert identity | No lock can hold a row that does not exist. A constraint failure rolls the whole apply back and keeps the pending values | Check after the insert | Nothing to compare before the write |
| 10 | Marks on screen | The row numbers of the grid carry `+` for a pending insert and `-` for a pending delete, with a color. The summary counts updates, deletes, and inserts, and only the non-zero parts show | Colors only | A text mark works without colors |
| 11 | Guards | The existing pending-edit guards count inserts and deletes, so copy, export, import, filters, sort, reload, navigation, pages, and close stay blocked while any change is pending | New guards per change type | One source of truth |
| 12 | After the apply | The first page reloads as today; cursor restoration by primary key ignores a pending row that the reload does not find | Restore by insert position | The position of a new row is not a key |

## SQLite in memory — 2026-11-06

The user asks for `:memory:` SQLite databases. The initial plan rejected them in the first version; the explicit request is the new fact, and this entry replaces that rejection.

| # | Point | Approved rule | Rejected | Reason |
|---|---|---|---|---|
| 1 | Entry | The file field of the form and the command line accept exactly `:memory:`, without case. `file:` URIs and `mode=memory` stay refused | Any URI form | One text form keeps the validation simple |
| 2 | Pool | The pool holds exactly one place, and no idle timeout closes it, because a new session starts empty | A shared cache between sessions | The shared cache has locking rules of its own |
| 3 | Session | The session returns clean instead of discarded, so temporary tables persist for the session. A disconnect and the end of the application lose the database, which the form and the README say | Refuse the disconnect | The user asked for a scratch database, not a file |
| 4 | Source type | `SqliteTarget` holds an enum of the file and the memory source, and `:memory:` never reaches the driver as a path | A magic path value | The type carries the rule |
| 5 | Configuration | `config.json` stores `":memory:"` as the file of the connection. No saved file can hold it today, so no migration is needed | A new field | The text round-trips through the form |
| 6 | Queue | Compare, structure, and console reads of the connection share the one session in arrival order | A second session | Data consistency wins over parallel reads |

## Auto-refresh of the tab on screen — 2026-11-06

The user asks for a repetition of the current view at a fixed interval.

| # | Point | Approved rule | Rejected | Reason |
|---|---|---|---|---|
| 1 | Target | One command acts on the tab on screen: a console repeats its last run, and a table tab reads its page again with the same filter and order | Every tab at once | The user watches one place; background work needs a queue |
| 2 | Entry | The palette (`Ctrl+P`) and the grid page of `Ctrl+W Space` list `autoRefresh`, with no default shortcut | A fixed key | The feature is occasional |
| 3 | Interval | A dialog offers Off, 1s, 2s, 5s, 10s, 30s, and 60s | A free field | Seven covers the use; a field needs validation |
| 4 | Console repetition | The exact statements of the last run repeat, captured when the run starts. Editing the editor changes nothing until a new run; parameters use the current values | Run the text of the editor | The repetition must be predictable |
| 5 | Reads only | A console with a last run that is not all plain reads — or, on Redis, not all known reads — refuses to arm. The repetition never opens the run question | Ask at each repetition | A question every second stops the work |
| 6 | Table repetition | The tab reads the page of the cursor again, and the grid holds that page again | Reload every loaded page | The memory budget follows the page |
| 7 | Skip | A busy tab, an open modal or dialog, a blocked configuration, pending grid edits, or an unknown transaction skips the moment in silence; the interval stays armed | Stop on the first skip | The condition can pass one second later |
| 8 | Failure | An execution that does not complete turns the refresh off with a status note | Retry | A failing read every second hides the error |
| 9 | State | The status line of a console and the summary of a table show `auto Ns`. The state lives for the session; no configuration or workspace file changes | Persist the interval | The feature is a watch, not a setting |
| 10 | History | A repetition writes no line in `history.jsonl`; the Messages history keeps its budgeted records | Record every repetition | The file would grow without value |

## Explicit AI SQL completion

The user approves a command-driven completion flow, separate from MCP.
This adds optional `preferences.aiCompletion` settings. Existing configuration remains valid.

| Point | Decision |
|---|---|
| Entry | AI row above local autocomplete items. Up selects it from the first local item; the configured suggestion acceptance key starts the request. No separate default shortcut. |
| Scope | Consoles associated with PostgreSQL, MySQL, and SQLite. No Redis, unbound consoles, chat, agent tools, or generated SQL execution. |
| Providers | One OpenAI-compatible Chat Completions provider, with OpenRouter as the default endpoint. Optional local servers run outside SQLN. The removed DeepSeek and protocol settings are legacy values that old files may still carry. |
| Provider URL | Accept an API base or a full completion URL. Add the route only once; a legacy full FIM URL maps to the chat route on the same base. Model discovery uses `/models`. An empty endpoint uses OpenRouter. A real API and terminal test confirms these routes. |
| Context | Cache the complete accessible structural schema. Respect connection schema filters. Refresh rebuilds a previously loaded snapshot; no polling for external changes. Never read records for completion. |
| Large context | Reduce automatically, with a visible reduced-context label. Keep the cache complete. |
| Selection | Replace only the selected interval. Without selection, propose insertion at the cursor. |
| Review | Show a waiting animation, then Accept and Reject. Use an editor preview for insertion and a diff for replacement. Reject starts selected; acceptance is one undo step. |
| Configuration | Global defaults and a project override. The project can disable AI. Enabled inherits independently; provider configuration inherits as one block. |
| Credentials | System vault or the contents of a key file. Read the file for each request to support key rotation. No plaintext API-key persistence or environment-variable source in this delivery. |
| Consent | Once per project path, connection, endpoint, and credential reference. Local private persistence only. |
| Failures | Keep user text. Cancel stale requests; never retry or change provider automatically. |

Implementation limits and configuration are in [AI completion](AI_COMPLETION.md).

## Inline review of AI and MCP changes — 2026-11-05

The user asks for a review flow in the editor, as in a commercial IDE. Before this entry, the AI preview and the MCP proposals showed outside the editor, in floating windows and dialogs.
This entry replaces the **Review** row of the AI completion table, decision 8 of the MCP notifications entry for proposals, and the edit review dialog. Access requests keep their dialog and their queue.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | AI insertion without a selection | Ghost text at the cursor. Tab accepts, Esc rejects. The arrows keep it: Up and Down move the view, Left and Right do nothing (user correction of 2026-11-06). Other keys reject, and the key goes to the editor | The floating preview; arrows that move the cursor and reject | User choice |
| 2 | AI replacement and MCP proposal | An inline diff in the editor: removed lines with `-` and the error color, added lines with `+` and the success color, changed words marked | Side by side; the dialog | Side by side does not fit 80×24 |
| 3 | Unit of a decision | Each change (hunk), and all changes | The whole proposal only | User choice |
| 4 | Editor during a review | Read only. Another key shows the reason and the review keys | Free editing that rejects or moves the proposal | User text and the proposal stay consistent |
| 5 | Review keys | `a` and `r` for the current change, `A` and `R` for all, Up/Down, `j`/`k`, and `]`/`[` for the next and previous change. Each key is an `Action` with custom keys | Ctrl combinations | `Ctrl+Y` is Redo, and `Ctrl+Enter` needs keyboard enhancement |
| 6 | Esc | AI: reject. MCP: **Later**, and the proposal stays pending in the card and the MCP row | Reject for both | User choice |
| 7 | Enter and initial focus | No button has the focus. Enter decides nothing | Reject with the focus | An explicit key or a click decides |
| 8 | Opening of an MCP review | **Review** (card, MCP row, palette) selects the console tab and shows its first change | Automatic display; the dialog | Typing never goes to a review that the user did not open |
| 9 | Tab mark | `± ` before the name of a console with a pending proposal | No mark | The user sees where a proposal waits |
| 10 | Application and undo | An accepted change goes into the text at once, as one undo step. **Accept all** is one undo step | Application at the end of the review | User choice |
| 11 | Reply to the agent | A new final status `partial`, with the counts of accepted and rejected changes | `applied` when one change is accepted | The agent knows what the user kept |
| 12 | Queue after the last decision | The editor stays. The card shows the next proposal with its place | Select the tab of the next proposal | A jump to another tab moves the user |
| 13 | Agent tab SQL that the user changed | The same inline diff | The dialog | One flow |
| 14 | Controls | Buttons on the current change, and a bar of one row at the bottom of the editor. A narrow bar drops labels; the keys stay | Buttons on each change | At 80×24, the text keeps its room |

## Persistent AI ghost and buttons at each change — 2026-11-06

The user asks for buttons near the change and a suggestion that stays until a decision, because the generation has a cost.
This entry replaces decisions 1 and 14 of the inline review entry for AI ghost text, and the AI part of decision 4: only a replacement review keeps the editor read only.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Buttons of a ghost | A dedicated row after the ghost text; a ghost has no bar | The bar on the last row | The user asked for controls near the change |
| 2 | Life of a ghost | It stays until the user accepts or rejects it. Keys, clicks, commands, cursor movement, tab changes, and schema refreshes keep it | Any key or click rejects | The generation has a cost; only the user discards |
| 3 | Silent discards | An edit of the anchor line or any undo discards the ghost, in silence | Re-anchor on any edit; a confirmation | The continuation can be wrong, and no dialog interrupts |
| 4 | Edits on other lines | They move the anchor with the text | Keep the old row | The ghost stays correct |
| 5 | Acceptance | It applies the ghost at its anchor, not at the cursor, as one undo step | Apply at the cursor | The anchor is where the model answered |
| 6 | Answer in flight | It shows at its requested anchor; an edit on another line moves the anchor. An edit of the requested line stops the request at once, in silence | Stop the request when the cursor moved; wait for the answer and show nothing | The cost argument applies, and the answer never lands on changed text |
| 7 | Buttons of a diff | Each pending change has its Accept and Reject buttons. The bar keeps the place, the origin, Accept all, Reject all, and Close only | Buttons on the current change only; the bar decides one change | User choice; the keys still follow the current change |
| 8 | Tab mark | `± ` also shows a pending AI ghost | Agent proposals only | The user sees where a suggestion waits |
| 9 | Run | It executes the text without the ghost, and the ghost stays | Discard on run | Only acceptance changes the text |

## OpenRouter reference review — 2026-11-06

The user asks to follow the current OpenRouter chat completion reference.
The review confirms the endpoint, the bearer auth, the message form, the response parse, and the top-level `reasoning_effort` field of the app. Two points change:

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Output field of the body | The request body carries `max_completion_tokens`, the current field of the reference. The output range starts at 16 | Keep `max_tokens`, which the reference marks deprecated | The old field stays accepted but deprecated, and some providers refuse an output below 16 |
| 2 | Explicit no-reasoning value | The Reasoning effort choice gains `None`, which sends `reasoning_effort: "none"`. Disabled keeps the omission | Keep only the omission | The reference lists `none` in the field enum; it is the explicit request that the model does not think, and the OpenAI reference accepts it too |

This entry amends point 3 of "Provider reasoning effort — 2026-11-06": the value list now holds Disabled, None, Low, Medium, and High. The reference also lists `minimal`, `xhigh`, and `max`; SQLN does not send them, because strict OpenAI-compatible endpoints reject them.

## Provider reasoning effort — 2026-11-06

The user asks for a setting that chooses the reasoning effort of the autocomplete model and can disable the thinking.
The user chooses the OpenAI `reasoning_effort` field of the chat body, the omission of the field as the disabled state, and the values without `minimal`, after a review of the OpenRouter parameter rules.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Wire field | The request body carries the top-level `reasoning_effort` of the Chat Completions standard; the OpenRouter base accepts it as the OpenAI alias | The `reasoning` object of OpenRouter, or both fields | The provider is one OpenAI-compatible endpoint; strict servers reject unknown objects, and duplicated fields risk a 400 |
| 2 | Disabled | The choice Disabled omits the field, so the endpoint uses its default; this is not a guarantee that the model stops thinking | Send `reasoning.enabled: false`, or send `minimal` as off | Omission is safe for any endpoint. The OpenRouter switch that turns thinking off is its own field, and `minimal` is a value of the GPT-5 family only |
| 3 | Values | Disabled, Low, Medium, High | Include Minimal | Low, Medium, and High are the documented values of the OpenRouter effort set and valid OpenAI values for GPT-5 and the o-series. Minimal is outside the documented OpenRouter set for models that are not GPT, and the direct OpenAI API rejects it for the o-series. Adding it later is one enum variant and one text |

Saved settings carry the field only when it is not Disabled. A file without the field, or with an unknown value, reads as Disabled.

## Autocomplete fields in Settings — 2026-11-06

The user asks to remove the autocomplete modal and to move its fields into the Settings dialog, on one page.
This entry suspends the dialog flow of the inline review entry and of "AI settings in two sections": the Autocomplete section of the sidebar holds the fields directly.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Place | All autocomplete fields live in the Settings dialog, in the Autocomplete section, on one page, without the Provider, Credentials, and Limits pages | Keep the modal | User request; one dialog for all settings |
| 2 | Scope | The Scope choice of Settings selects the scope of the autocomplete fields, as for the other fields | A scope row inside the section | One scope model for the whole dialog |
| 3 | Apply | The button ends. A save of Settings writes the provider and the key; a failed vault write offers retry or a session key before the save continues | Keep Apply | Settings already has Save and Cancel with their protections |
| 4 | Key at save | The save writes a typed key to the system vault first, then the file. The key never goes into `config.json` | Write the key at each edit | The vault write is the point of no return of a save, as the file write is |
| 5 | Untouched scopes | A save of a scope without an AI block and without an edit writes no block | Always write the fields | The file keeps only what the user set, as the other optional keys do |
| 6 | Test | A **Test** row keeps the `/models` connection check; a success goes to the status bar, a failure to the note of the dialog | Remove the test | The check helps before a save |

## AI settings in two sections — 2026-11-06

The user asks to split the AI part of Settings into two sections: **MCP** and **Autocomplete**.
This entry suspends row 12 of the MCP settings entry: the sections take the names MCP and Autocomplete, and the IA section ends.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Split | Two sections in the sidebar: MCP holds the four MCP fields and the client help; Autocomplete holds the Configure… row that opens the dialog | One section with two pages | The sidebar is the navigation of Settings; one row per place keeps the rule of the other sections |
| 2 | Names | MCP and Autocomplete | Keep IA | User request; each name names its subject |

## One OpenAI-compatible provider — 2026-11-06

The user asks to remove the FIM completion mode and to keep only the OpenRouter standard, then widens the request to one OpenAI-compatible provider.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Protocols | The completion always uses the OpenAI Chat Completions standard | Keep the FIM protocol | User request: one provider standard. The feature is a development preview, so no release blocks the change |
| 2 | Provider kinds | One kind: OpenAI compatible. The Deepseek and Openrouter kinds become legacy values of saved settings, accepted and treated like Compatible | Keep the kind selector | One experience, as with the selection removal of the same day |
| 3 | Empty endpoint | The default endpoint is `https://openrouter.ai/api/v1` | Refuse an empty endpoint | The default of the previous OpenRouter kind stays out of the box; other endpoints set the URL |
| 4 | Legacy full FIM URLs | A URL that ends in `/completions` maps to the chat route on the same base | Reject the URL | Old files stay valid without a migration; the credential identity keeps the configured URL |
| 5 | Legacy `protocol` field | Saved files with the field load; SQLN ignores the value and new saves omit it | Break the parse of old files | Follows the file rule: optional keys keep older files valid |

This entry suspends the DeepSeek parts of the provider decisions in the summary table above.

## AI completion at the cursor only — 2026-11-06

The user asks to remove the completion of a selection, so one completion experience stays.
This entry suspends the replacement decisions of the inline review entry for AI: points 2, 4, and 8 of that entry no longer apply to AI, and the exclusivity decision 8 of the parallel entry now covers agent reviews only.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Scope | The AI completion works at the cursor only, for now. No request replaces a selection | Keep the selection path inactive behind a setting | One experience for now; the removal is a user request, and the git history keeps the code |
| 2 | Request under a selection | It refuses with a status message, and the selection stays | Clear the selection and complete at the cursor | User choice; the selection is never discarded in silence |
| 3 | Lists | The AI row hides while a selection stays; the local list of `Ctrl+Space` still shows in insert mode | Keep the AI row and refuse on accept | The row that cannot run does not show |
| 4 | Agent proposals | They keep their inline diff, which ends the AI requests of the console; hidden ghosts return after the review | Remove the diff review itself | The agent review is another feature and shares the model |
| 5 | Diff of the answer | The answer always shows as ghost text at its anchor | A diff review when the answer spans lines | One completion experience |

## Several AI completions at once — 2026-11-06

The user asks for more than one AI completion in the same console, on different lines.
This entry extends the persistent ghost entry: its decisions 3, 5, 8, and 9 now cover each ghost, and a new setting bounds the answers that wait.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Parallelism | Requests run in parallel, with one setting for the most answers that wait at once per console: ghosts that show plus requests that run | A queue of one request at a time | The user asked for parallel work; one number keeps the model simple |
| 2 | Limit | Configurable in Settings > Autocomplete > Limits, range 1 to 9, default 3 | No limit; a fixed number | Provider limits and screen room vary |
| 3 | Same line | A request on a line that has a ghost replaces that ghost, even at the limit | Block the request | The newest answer for a line wins |
| 4 | Decide keys | `Tab` and `Esc` act on the line of the cursor, always, even with one ghost | The only ghost decides from anywhere | One rule for every case, user choice |
| 5 | Edit of an anchor line | It closes only the ghost of that line; the others stay | Close every ghost | Each ghost follows its own line |
| 6 | Undo | Any undo takes every ghost of the console | Track the ghosts that the undo affects | The affected set is fragile to track |
| 7 | Esc with several runs | It ends the run of the line of the cursor; the bar button **Cancel** ends them all | End every run | The user asked for the line rule; the button names all |
| 8 | Selection diff | It stays exclusive: opening it takes the ghosts and their runs, and the editor refuses new requests until it closes | Ghosts frozen behind the diff; diffs blocked by ghosts | The diff is read only; one mode at a time |
| 9 | Tab mark | `±N` counts the ghosts that wait | `±` without a count | The user sees how many answers wait |

## AI completion with a user request — 2026-11-06

The user asks to send a request with an AI completion, to say what the completion must do.
This entry extends the cursor-only entry: the request rides the same flow, and the plain row keeps its direct start.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Entry | A second AI row, **Complete with AI with a request…**, sits above the plain row in the suggestion list. The palette offers the same command. No separate default shortcut | Only the palette command; always ask on the plain row | The row is visible while typing; the plain row stays one key away |
| 2 | Input | A modal of one line opens on accept. `Enter` sends, `Esc` cancels, and an empty input sends a plain completion | An inline row in the editor; a footer line | The modal follows the rename pattern and keeps the editor untouched |
| 3 | Body | The request travels as `request` in the user content, next to `context` and `metadata`, only when present. The system instruction tells the model to follow it | A second user message; a comment in the SQL | One body shape; strict endpoints reject extra roles of unclear order |
| 4 | Limit | 1024 characters at most, and the text counts against the context window and the memory budget | No bound | The request shares the limits of the context |
| 5 | Review | The answer shows as ghost text at the anchor, with the same accept, reject, and edit rules as a plain completion | A different review for requests | One completion experience |
| 6 | Privacy | The request never echoes in the status bar, pending messages, or logs | Show the request while it waits | User text stays out of diagnostics |

## Prompt-cache-friendly completion request — 2026-11-06

The user asks to raise the prompt-cache hits of the completion requests.
This entry amends the large-context rule of the Explicit AI SQL completion table: reduction keeps one canonical order and a provider-derived budget.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Content order | The user content serializes `metadata` first and `context` last | Keep `context` first, as before | Providers cache identical leading bytes; the stable schema section then hits the prompt cache across requests |
| 2 | Object order | One canonical schema and name order for every request | Order by SQL references, foreign-key neighbors, then current schema | Any per-request reorder changes the metadata bytes and ends the cache; the schema snapshot order already gives the canonical order |
| 3 | Relevance | A `focus` field in the context lists the objects that the SQL references and their direct foreign-key targets, in the canonical order, only when present | Drop the relevance signal | The model still sees what the SQL names, without changing the cached section |
| 4 | Metadata budget | The budget derives from provider limits alone: the window minus the output, minus a fixed SQL reserve of 16 KiB capped at half of the remainder | The remainder after the SQL of each request | The section stays identical for one snapshot and provider configuration, whatever SQL the request carries |

## MCP settings in the IA section — 2026-11-05

The user asks for an **IA** section in Settings, to set the MCP behavior per project.
This entry replaces the rule "Configuration, script, and workspace formats do not change" of the Local MCP assistant table for the four keys below, and row 2 of the MCP tools entry: a saved default access can give full access.
The keys are optional, so older files stay valid without a migration.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Content | SQLN preferences for the agent, in `config.json` | Write the MCP client files of the project | User choice |
| 2 | Help | A text below the fields with the Claude Code command and a reference to `docs/MCP.md`. SQLN does not configure the client | Configure the client from the dialog | User choice |
| 3 | **MCP** (`mcp`) | Off: the instance has no socket, and `list_instances` does not find it. On by default | No field | A `PROD` project can turn off the agent |
| 4 | Change of **MCP** | At once: a save opens or closes the socket. Off revokes the open sessions and cancels their tasks | At the next start | The user sees the effect of the save |
| 5 | **Max access** (`mcpMaxAccess`) | The highest option of the access request and of the session dialog: Allow, Allow all, or Full access. Full access by default | No limit | The project limits the agent |
| 6 | Lower maximum | Sessions and remembered clients above it go down to it at the save, with a notice in the status bar and the session Messages | Keep them until the end | User choice |
| 7 | **Default access** (`mcpDefaultAccess`) | Ask (default), Allow, Allow all, or Full access. A value other than Ask gives that level without a question, card, or bell, with a notice, as the remembered access does. It never goes above **Max access** | Only the initial focus of the request | User choice |
| 8 | Levels of the default | All levels, limited by **Max access**. The run protections stay | Up to Allow all | User choice |
| 9 | **Remember clients** (`mcpRememberClients`) | Off: each new session asks again. On by default. The memory stays out of the file | No field | User choice |
| 10 | Order | A remembered level comes before the default access | Default first | The remembered level is a choice of the user |
| 11 | Scope | Global and Project, as the other sections | Global only | User choice |
| 12 | Name | **IA** | **AI**, **Agent**, **MCP** | User choice |

## Agent tab lifetime and place — 2026-11-05

The user asks for the agent tab to open when the agent connects, to stay open while the agent is connected, and to stand at the right of the tab row.
This entry replaces the rule that the first query creates the agent tab.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Opening | At the access approval, or at once for a remembered client. A rejection opens no tab | At `request_access` | Before approval, the agent reads nothing |
| 2 | Tab without a connection | One session tab without a connection. The first task or placed SQL binds it. Another connection opens another agent tab | One tab for each session; a tab on the active connection | Tasks keep running in parallel on different connections |
| 3 | Close while connected | The last agent tab of a session with access asks: **Cancel** (initial focus) or **Revoke and close**. Other agent tabs of the session close as usual | Refuse the close; close and reopen on the next notice | The user keeps control, and the tab ends with the access |
| 4 | After the session | The tab stays with `MCP off ·` and closes without a question | Close it automatically | Its Messages and results stay readable |
| 5 | Place | Agent consoles come after the other tabs, at the right end of the row before **Quit** when the row has room. Agent diagram and comparison tabs keep the order of opening | Mixed with the consoles | The agent tab stays apart from the user's consoles |
| 6 | Lost tab | A session with access that loses its last tab, for example when its connection is removed, gets a new session tab | No tab until the next task | The session keeps one tab |

## Session controls in the agent tab — 2026-11-05

The user reports that the agent tab does not show its session: the access, the tasks, and many notices are in the status bar, the cards, and the palette only.
This entry replaces decision 11 of the MCP notifications entry for the items below. The global commands, the cards, and the MCP row stay.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Access in the tab | An **Access: level** button in the bar of each agent tab opens the session dialog: change the access or revoke it (`agentSession`) | The picker of all sessions only | The user sees the access of the agent in its tab |
| 2 | Tasks in the tab | A **Tasks N** button opens a list of the session's tasks. A selected task opens **Keep running** (initial focus) and **Stop** (`agentTasks`) | A Tasks tab next to Messages; one row for each task above the results | One bar row. At 80×24 the results keep their room |
| 3 | Scope | The button and the list cover the whole session, on all its connections. The console **Stop** still stops the tasks of that console only | The tasks of the tab's connection only | The access and the tasks belong to the session |
| 4 | Log | Every notice of an agent action also goes to the session's Messages: runs and edits of user consoles, proposals and their decision, discarded proposals, opened tabs, access changes, and revocation | The status bar only | The Messages keep a complete record of the agent |
| 5 | Log target | The session's agent tab for that connection. Without it, the last agent tab of the session. Without an agent tab, the status bar only | Open an agent tab for the note | No tab opens for a note |
| 6 | Narrow bar | Tasks goes before Access. A narrow bar drops Access first; the palette and a custom key keep it | Shorter labels | A running task can need a stop |

## Agent SQL in the agent tab — 2026-11-05

The user reports that SQL from the agent opens a new console for each request, and that a lowered access cannot go up again. This entry replaces decisions 3, 4, and 13 of the MCP notifications entry.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Place of the SQL | An editor in the agent tab | SQL blocks in Messages; a new console next to the agent tab | The user changes the SQL before the run, and tabs do not multiply |
| 2 | Editor visibility | Only while the tab holds SQL | Always | At 80×24, Messages and results keep their room |
| 3 | New SQL, SQL not changed by the user | Replaces it, as one undo step | Appends it | The editor shows the current SQL. Undo and the query history keep the old SQL |
| 4 | New SQL, SQL changed by the user | An edit proposal with a diff | Replace it | User text never disappears |
| 5 | Agent run of that SQL | `run_console` with the rules of its access level | Never | No special case |
| 6 | User run in the agent tab | A user run in Auto mode, with the current run confirmations. Commit and Rollback appear for a transaction that the user opened | A manual mode | An open transaction blocks the agent tasks of the tab |
| 7 | Notice of new SQL | Status bar, agent Messages, and the `SQL ·` tab mark | The access card and the bell | The card is for decisions that only the user makes |
| 8 | Save | **New console** copies the SQL to a user console | The agent tab saves as a script | The agent tab stays temporary |
| 9 | Old behavior | Removed from the MCP | `new_console: true` | Fewer options for the agent |
| 10 | Access changes after approval | The session dialog offers every other access option, to lower or raise the access | **Switch to proposals** only | The user could not raise the access again |

## Visible MCP access requests — 2026-11-05

The user reports that the MCP row hides access requests: a user can miss them. This entry adds to decision 7 of the MCP notifications entry.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Request notice | A card with the warning color, above the MCP row, with **Review** and **Later**. It takes no focus | Open the access dialog automatically; color the MCP row only | The request is visible, and typing never goes to a dialog |
| 2 | Card place | Bottom right of the main area | Top right | At 80×24, the top covers the editor text and its cursor |
| 3 | Bell | One terminal bell for each new request. A remembered client does not ring | No bell | The user sees a request in another window |
| 4 | Other cards | Edit proposals (**Review**, **Later**) and tasks that run for more than 3 seconds (**Stop**, **Hide**) also use the card. Placed SQL and failed tasks do not | A card for every notice | The card is for items that wait for the user or can need a stop |
| 5 | Several items | One card at a time: access, then proposals, then tasks, with the place in the queue | Stacked cards | At 80×24, stacked cards cover the results |
| 6 | MCP row | Stays, with the counts and the items that the user put off | Remove it | A hidden item stays visible |

## MCP tools, waiting, and full access — 2026-11-05

The user reports a poor agent flow: too many tools, a task identifier to poll after each request, and a database that the user cannot open to the agent.
The new facts are the user's request for full access and the number of calls per task.
This entry replaces the **Database** row of the Local MCP assistant table, the **Target** rule that every call names its instance, and the reason of decision 1 of the MCP notifications entry for remembered access.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Full access | A **Full access** option executes any SQL without review. Read-only connections, the `PROD` question, and the question for a write without `WHERE` still apply; the task waits for the answer | Writes without exceptions; a question for each write; a read-only database | The agent becomes useful, and the existing protections stay |
| 2 | Grant of full access | A fourth button in the access request, without the initial focus, for the session | A saved field per connection | No configuration format change |
| 3 | Instance selection | `instance_id` is optional when one instance is open. Several instances give `ambiguous_instance` | An ID in every call | Fewer steps. The bridge never selects one of several instances |
| 4 | Waiting | The bridge waits for the end of a task, 50 seconds by default, and `request_access` waits for the decision, 120 seconds by default. A late task continues with `job` | The agent polls each task | One call returns the rows |
| 5 | Remembered access | SQLN remembers the option of a client name until it exits. **Forget** and **Revoke** remove it. A rejection is not remembered | A new question for each session | The user accepts that the client name is not verified, for less friction |
| 6 | Tools | 11 tools. `generate_sql` leaves the MCP; the interface keeps its templates | 25 tools | The agent writes SQL from the catalog and the DDL |
| 7 | Agent transaction | SQLN rolls back a transaction that agent SQL leaves open, and the task fails with the reason | Keep the transaction for the user | Nobody ends a transaction of an agent console |

## MCP notifications and decisions — 2026-11-05

The user asks for clearer MCP notices and decisions, an option that approves everything in the access request,
an agent tab that shows only its messages, and commands that stop agent work. This entry replaces the **Edits** row of the Local MCP assistant table for sessions that use **Allow all**.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Scope of **Allow all** | Access and direct edits in all consoles, for this session only | Remember the client for later sessions | The client name is not verified |
| 2 | Protection of **Allow all** | A dialog button without the initial focus, with a warning in the text | A second confirmation | The warning, undo, and the `MCP n all` label are sufficient |
| 3 | Return to proposals | **Switch to proposals** in the session list, without revocation | Revocation only | Less friction |
| 4 | Agent tab content | Messages and result grids, without the editor | Messages only | The user sees the data that the agent read |
| 5 | **Stop all agent tasks** | Cancels tasks. Access stays | Also pause new tasks | Revocation already blocks new tasks |
| 6 | MCP row during execution | Shows **Stop** while an agent task runs | Status bar label only | Safety visibility |
| 7 | Access decision in the MCP row | The row has **Review** only | **Allow** and **Reject** in the row | An accidental click must not give access |
| 8 | Decision queue | After a decision, open the next pending item, with `N of M` | Return to the editor | Fewer steps |
| 9 | Notices of agent actions | Status bar and the agent Messages | Messages only | The user sees changes to their consoles |
| 10 | Default key of `agentStop` | None. The MCP row and the palette offer it | A default key | No free key is necessary now |
| 11 | MCP center | Keep separate commands | One panel for sessions, proposals, and tasks | Not necessary now |
| 12 | **Reject all** | Not offered | A button when N > 1 | Not necessary now |
| 13 | SQL that the user must run, such as `CREATE TABLE` | `open_user_console` opens a new user console with the SQL, without execution or a focus change. Agent consoles refuse edits | The agent asks the user to open a console | The write flow ends in a console that the user reviews and runs. The database stays read-only for the agent |

## Local MCP assistant

The user approves a stdio assistant for SQLN instances that are already open.
The implementation plan includes advanced read analysis in the first delivery.

| Point | Approved decision |
|---|---|
| Target | A local stdio bridge discovers open instances. Each call identifies its instance. Global-only configuration works. |
| Access | The user approves each MCP session in each SQLN instance. Client labels do not prove identity. |
| Database | Read only. Reuse existing connections, credentials, trust checks, pools, limits, and cancellation. |
| Agent console | A separate, identified console receives notifications. Background operations do not change focus. |
| User console | The agent can edit and execute its current SQL. A checked revision and explicit script/range identify the request. Results replace that console's results. |
| New SQL | Free SQL submitted through a tool executes only in that session's agent console. |
| Transactions | Refuse execution in a console with an open, failed, or unknown transaction. Offer the agent console without rerouting. |
| Edits | Show a proposal by default. Only the user accepts it. Direct editing is a user-enabled, per-console option until close or process exit. |
| Disconnect | Cancel only that session's operations. Preserve available text/results and other sessions' work. |
| Advanced analysis | Include catalog, DDL, SQL templates, plans, ER diagrams, and structure/data comparison. Generated scripts never execute automatically. |
| Outside scope | No HTTP, remote access, connection/configuration management, imports/exports, database writes, user transaction controls, or ACP yet. |

Implementation rules and limits are in [MCP](MCP.md). Configuration, script, and workspace formats do not change.

## Query parameters — 2026-11-04

The user requests named scalar parameters with a panel under the editor, as the approved plan describes. The detection uses the shared lexer, and the values reach the database as native driver bindings. Values never become SQL source text.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Syntax | `:name` with ASCII letters, digits, and underscore, first character not a digit. Names compare with case sensitivity | `?`, `$n`, `@name`, auto-numbered names | One syntax for the three SQL sources; the panel needs stable names |
| Detection | The whole editor text, off the interface thread, 200 ms after the last edit; quotes, comments, dollar quotes, and casts stay opaque | Only the statement at the cursor; detection on each key | The panel shows every parameter before the user chooses a scope |
| Scope | A run binds only the names of the chosen statements or script | Bind every field of the panel | A field without a use must not block a run |
| Types | Text, Number, Boolean, and NULL, chosen per field. Numbers validate by syntax without a float conversion; booleans accept `true` and `false`. On PostgreSQL, a Text or NULL parameter arrives as an unknown type, so the query infers it: uuid, date, timestamp, and jsonb columns accept the value without a cast. A Number stays `numeric`, because inference in arithmetic can pick an integer type | Dates, lists, identifiers, and SQL fragments; declare Text as `text` on PostgreSQL | Scalars cover the reuse case; identifiers would need catalog checks; a forced `text` type made the server refuse `uuid = text` |
| Empty text | `Enter` marks an empty text as set. Unset fields block the run and put the focus on the first one | Treat empty as set; treat empty as NULL | Empty text and SQL NULL stay distinct, as in the grid editor |
| Values | One value per name per execution, reused in every occurrence. Values stay in memory, per console, and never enter saved files | Persist values; share values between consoles | The saved work and the workspace formats stay without secrets |
| Editor text | The editor keeps `:name`; the markers are written only for the driver | Replace the editor text with markers | The text of the user stays readable and reusable |
| Bindings | PostgreSQL: extended protocol with prepared types and text parameter format. MySQL: prepared statements. SQLite: `raw_bind_parameter`. Decimal text binds as a string with a typed cast | Interpolate literals into the SQL | Native bindings separate values from SQL and keep injection out |
| Number casting | MySQL: `CAST(? AS DECIMAL(p,s))` with the scale of the text, range-checked to 65/30. SQLite: `CAST(? AS NUMERIC)`. PostgreSQL: `numeric` parameter type | A driver float or int type | A float conversion loses digits; SQLite compares by type |
| Result values | PostgreSQL keeps the text of the database, also for types without a decoder, through a transport adapter that requests text results for marked statements only | Binary results with per-type decoders | Preserves the decision "Grid values: database text", including extension types |
| Mixing | `:name` cannot combine with native markers in one statement. The PostgreSQL `?' operator and array slices stay valid | Rewrite native markers | One binding path; rewriting risks changing the meaning |
| Errors | The driver offset maps back to the editor text through the replacement ranges | Show the offset of the rewritten SQL | The editor underlines the place in the text of the user |
| Added `LIMIT` | Applied before the rewrite, so the driver receives the limit | Bind without the limit | The row-cap decision keeps its effect |
| Messages | The record shows the original SQL and the separate values, as the command-history decision requires; the driver never sees the values in SQL | Record only the original SQL | The record must describe the request that reaches the driver |
| Query history | `history.jsonl` keeps the `:name` text only | Save values | The file holds user text, not values |
| Export reread | The captured bindings of the last execution serve a full export reread; closing results clears them | Ask again; interpolate | The reread must return the same rows without another question |
| Connectors | SQLite, PostgreSQL, and MySQL through `Capabilities::parameters`. Redis and Valkey show no panel | Offer fields everywhere | Redis commands take values, not named parameters |
| Keys | `Ctrl+W P` focuses the panel; a click on the title or the command collapses it. `Tab` walks type, value, and Set; `Enter` sets; `Esc` returns to the editor; plain keys edit the value | A modal dialog | The panel follows the Databricks model that the user chose |
| Persistence | No change to configuration, workspace, or script formats | Save fields or collapse state in the workspace | YAGNI; the panel state is session-only |

Implementation notes:

- The PostgreSQL transport adapter (`connector/postgres/text_io`) parses the frame stream: a `Parse` that starts with a marker names a statement, and only the marked `Bind` sends text result formats. Driver-internal queries keep binary results. A `Close` of the statement clears the name.
- The plain-path PostgreSQL connections moved to `connect_raw` to wrap the socket; TCP sets `nodelay` and keepalive as the driver did.
- MySQL binary-protocol rows reuse the text-cell rules; quoted values strip the protocol quotes only in that path.
- Without query context, an inferred parameter resolves to `text`, as in `SELECT :value`.
- Detection runs on a blocking task of the executor; the panel keeps only the newest revision answer.
- The panel keeps the value of a name across edits and drops it when the name leaves the text.

## Grid row selection — 2026-11-04

The user requests whole-row selection by a click on the row number, as in Excel.
Result and table grids use the existing cell selection and copy paths.
The selection includes every column of the loaded row, including hidden columns.
The behavior applies in conventional and Vim modes. It does not execute SQL.

## Grid editing — 2026-11-04

The user approves the grid editing plan and requests implementation.
This replaces the initial plan's read-only table grid and cell-editing exclusion.
It also replaces unconditional close for table tabs with pending edits or an active edit operation.

| Point | Approved rule |
|---|---|
| Scope | Updates in SQLite, PostgreSQL, and transactional MySQL table tabs; no views or console results |
| Identity | Require the complete original primary key, without NULL values |
| Protected columns | Keys, identities, generated columns, and binary values remain non-editable |
| Values | Preserve database text and precision; distinguish NULL, empty text, and the text NULL |
| Local work | Keep pending values in memory; no database write before explicit Apply |
| Review | Show original values, proposed values, and the generated SQL before execution |
| Apply | Use one short transaction per tab; no manual transaction between edits |
| Conflict | Reject changed or missing original rows; never overwrite them silently |
| Failure | Preserve local values; roll back on a known failure; never retry writes automatically |
| Unknown commit | Mark the result unknown and block another Apply until verification and local discard |
| Navigation | Keep edits across tab switches; require resolution before page changes, reload, filter, sort, copy, export, and import |
| Close | Apply, Discard, or Cancel; no immediate close during an edit operation |
| Persistence | No configuration or workspace format change; pending values do not survive shutdown |

Implementation notes:

- MySQL currently accepts InnoDB only. It refuses storage whose row-locking and rollback contracts are not verified.
- The table column reader of compare supplies key order, type, nullability, identity, and generated-column metadata.
- PostgreSQL and MySQL use locking reads; SQLite uses `BEGIN IMMEDIATE`.
- The executor validates every row before writes and checks each row again immediately before its update.
- The second check detects changes from an earlier trigger on the same session.
- A fresh metadata read after row locks detects schema changes.
- MySQL may report zero changed rows for equivalent values; the locked original row proves identity.
- The result grid holds the local row. Budgeted, reference-counted snapshots hold the original and the submitted row.
- Review formatting runs on a blocking task, without database access.
- Successful apply reloads the first page using applied filters and order, not drafts.
- Cursor and selection restoration uses primary keys, not old visual row positions.
- Ordinary execution limits, cancellation, credentials, trust, production, pool cleanup, and stale-event checks still apply.
- Generated review text stays outside the row budget, as generated console SQL does. Pending row snapshots use the data budget.
- Database rollback does not reverse external effects or writes to nontransactional storage from triggers.

## Command discovery and width-dependent hints — 2026-11-04

The user approved a complete functional command palette, panel command pages, and a one-line hint row with width-dependent content.
The user chose `Ctrl+W Space` for the panel page and option A for explorer scope.

- `Action` remains the command catalog. Discovery paths share command availability and use the effective shortcuts.
- `Ctrl+W Space` lists the bound commands of the current panel. Existing prefix shortcuts keep their meaning.
- `Ctrl+W Tab` remains the explicit path to explorer row commands from another panel.
- The palette offers explorer row actions only with explorer focus. Add connection and scripts without a connection remain global.
- The palette includes functional editor and grid actions, including actions without shortcuts. Basic movements stay in help.
- The normal hint row reserves the palette shortcut, then fits whole commands by context priority. Wider screens show more commands.
- Stop and transaction actions take priority. A failed transaction offers rollback, not commit.
- The order is fixed, not based on usage history. The hint row does not rotate and does not add a second row.
- Existing confirmations and database execution rules stay unchanged.

## Objects, DDL, and key navigation — 2026-11-04

The user compared SQLN with DataGrip and chose three gaps: more objects in the explorer, the DDL of an object, and the navigation by foreign key from a cell. The user chose points 1 to 4 and approved points 5 to 27 with the plan.

New fact: the DDL of an object needs text that SQLN writes when the server has no function for it. The structure tab rejected a DDL text (Structure tab — 2026-10-31, point 2), and that rule of the structure tab stays. The compare already writes DDL from the catalog.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Objects | Routines (functions and procedures), sequences, triggers, and types (enum and domain) | Routines, sequences, and triggers only | User choice |
| 2. DDL output | A new console of the connection, as `S` of the compare | Insert at the cursor of the console of the connection, as `g`; a read-only window | User choice |
| 3. Table and sequence of PostgreSQL | SQLN writes the text with the DDL writer of the compare. A comment at the top says so | Refuse with the reason | User choice. PostgreSQL has no `SHOW CREATE` |
| 4. Key direction | Both: the row that the key reads, and the rows of other tables that read this row | Only the row that the key reads | User choice |
| 5. Tree | One group per kind under the schema, after the tables and views: `Routines (n)`, `Sequences (n)`, `Triggers (n)`, `Types (n)`. An empty group does not appear. Tables and views stay where they are | Rows without groups; groups for all kinds, also for tables (DataGrip) | The current navigation does not change. Many functions do not hide the tables |
| 6. Read | The objects come with the read of the catalog, in one more query per database. `r` reads them again | A read when a group opens | The tree filter finds them. One read, with a small cost |
| 7. Extension objects (PostgreSQL) | Not shown (`pg_depend.deptype = 'e'`) | Show them | PostGIS puts hundreds of functions in `public`, and they hide the functions of the user |
| 8. Aggregate and window functions (PostgreSQL) | Not shown (`prokind` `a` and `w`). Internal triggers are not shown either | Show them | `pg_get_functiondef` refuses an aggregate |
| 9. Label | A routine shows `name(arguments)` on PostgreSQL (`pg_get_function_identity_arguments`) and `name` on MySQL. A trigger shows `name (table)`. Each overload has its own row | The name only | Two overloads need two rows and two DDL texts |
| 10. Tree filter | `/` also finds objects by name, under their schema and group | Tables only | The user looks for a function by name |
| 11. Keys on an object | `Enter` opens the DDL. `g` lists only **DDL**. `y` copies the name without the arguments. `i`, `d`, `c`, and `I` have no effect, and the hint row hides them. On a group, `Enter` and `Right` open it | An own key for the DDL | The keys that the user knows |
| 12. DDL of a table and a view | A last entry **DDL** in the `g` list. No new key | An own key `G` | `g generate` is already in the hint row and on the `Ctrl+W Tab` page |
| 13. Content of a table DDL | The table, its keys, and its indexes. Triggers stay separate objects | Include the triggers | The rule of `SHOW CREATE TABLE` of MySQL |
| 14. Header | `-- <kind> <schema>.<name> · <connection>`. A text that SQLN writes also gets `-- SQLN wrote this text from the catalog. The text of the server can differ.` Each statement ends with `;` | No header | The user knows where the text comes from |
| 15. DDL console | A new console of the connection, with the schema of the object, as `open_compare_script`. Nothing runs | — | Point 2. The run protection acts when the user runs the console |
| 16. Read path | A temporary task of the pool, with the password gate, the SQL time limit, and the **Messages** record. On PostgreSQL, `search_path` holds only the schema, as in the compare. The task closes at the end. An error goes to the status line and opens no console | A new `Session` method | The rows arrive as grid rows, so the model of the compare serves. The text stays in a pure module that tests can check |
| 17. MySQL without the privilege | `SHOW CREATE` gives a `NULL` text: the status line says "The server did not give the text. Check the privileges of the user." | An empty console | The reason is clear |
| 18. Key navigation: place | Only the table tab of a table. Not a console result, a structure, a view, or Redis | Also a console result | The simple protocol of PostgreSQL does not give the table of a column |
| 19. Key navigation: key and discovery | `F` in the grid of a table tab, in both modes. `Ctrl+W F` on the first page of the menu, in the table panel. `F follow` in the hint row. Palette and `F1` | Only `F1` and the palette | Lesson of 2026-11-03: the user looks for a new command in `Ctrl+W` and in the hint row |
| 20. Key navigation: choice | A list with each key of the row: `→ customers (id)` for a key that leaves the table, and `← orders (customer_id)` for a key that reaches it. The keys of the column of the cursor come first. With one key, SQLN goes there at once. A key with `NULL` in the row stays out. Without a key, the status line gives the reason | Always the list | Fewer keys in the common case |
| 21. Key navigation: target tab | The tab of the target table. An open tab gets the focus, its `WHERE` gets the new filter, its `ORDER BY` stays, and it reads the first page. A busy tab refuses with the current message | Always a new tab | The rule "A table that is open only gets the focus" |
| 22. Key navigation: filter text | `"column" = literal`, with `AND` for a key with several columns, and the literals of the dialect | A hidden filter | The user sees and edits the filter |
| 23. Key navigation: keys that reach a table | Read with the columns of the table, in one more query. A failure only turns off this use. They stay in `Relation::referencing`, with the shape of `foreign_keys` | A read at the press of `F` | The same path and event as the keys that leave |
| 24. Key navigation: keys not read yet | `F` asks for the columns and acts when they arrive, as a template that waits | Ask the user to press the key again | One key is enough |
| 25. Restart | Nothing new to save. The DDL console is a normal console | — | No new tab kind |
| 26. Delivery | Three parts in order: the tree, the DDL, and the key navigation. Each part with tests and documents | One part | The DDL uses the objects of the tree. The key navigation does not depend on the others |
| 27. Out of scope | Completion of user functions; DDL from a table tab or a structure tab; a return from a navigation; indexes in the tree (the structure tab lists them) | — | Not necessary now |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Model | `Schema::objects` holds `Object` rows of an `ObjectKind`, with `Group` for the tree. `relations` does not change | Objects as relations of a new kind | Completion, the table tab, and the filter of the tables read `relations` only |
| Label | `Object::label` is `name(arguments)` for a routine with arguments, and `name (table)` for a trigger. A routine without arguments shows its name only | Always `name()` | MySQL gives no arguments, so the two databases look the same |
| Arguments of PostgreSQL | `pg_get_function_identity_arguments`, which keeps the names of the arguments, such as `x integer` | Types only | The DDL query finds the routine by the same text |
| Read path | A task of the pool for the DDL only, with `Use::Definition`, and the events go to `on_definition_event` before the tabs | A new `Session` method | The rows arrive as grid rows, so `compare::schema::build` and `ddl::create_text` serve the table of PostgreSQL |
| Table of PostgreSQL | `compare::sql::postgres_table_sql` filters the three queries of the compare by the table. `compare_sql` gives the same queries as before | Read the whole schema | A large schema reads only one table |
| Column of `SHOW CREATE` | A fixed place: the second for a table and a view, the third for a routine and a trigger | Find the column by its name | The grid rows have no column names. MySQL 8.4 and MariaDB give the same places |
| Console | `open_text_console` serves the DDL and the script of the compare. The text goes into the editor of the new console | The paste path of the application | A dialog that opens during the read does not receive the text |
| `F` without keys | A view, a structure, a sessions tab, and a table whose keys are known to be absent refuse with a reason. The hint row hides `follow` there | Hide the key only | The user knows why nothing happens |
| SQLite key without target columns | Both directions read the primary key of the target, as the diagram does. The `JOIN` suggestions also get these keys | Leave the key out | `REFERENCES customers` is a common form of SQLite |
| Grouping of keys | `group_foreign_keys` joins the pairs of one name and one table | One name only | Two tables can have keys with the same name that reach one table |
| Shared literal | `value_literal` moved from `compare::data` to `sql::literal` | A copy | The compare and the key navigation write the same literals |
| `Ctrl+W F` | In the **Connection** group of the first page, with the label `Follow a key` | A new group | The group already holds the diagram and the compare of the tab |

### Known limitations of the objects, the DDL, and the key navigation

- The DDL of a PostgreSQL table that SQLN writes has the limits of the compare: no partitions, inheritance, comments, grants, owner, storage options, policies, or triggers. The names of its schema have no schema, so the new console has that schema.
- A sequence of an identity column of PostgreSQL gets its `CREATE SEQUENCE` text without the identity. Only the `OWNED BY` of a `serial` column enters the text.
- MySQL gives the DDL of a routine or a trigger only with the privilege to see it. MariaDB sequences do not appear.
- The objects of a schema hidden by the list of visible schemas do not appear, as its tables do not.
- Key navigation acts only in a table tab. A console result does not tell the table of a column.
- A table whose catalog SQLN does not hold, such as a table of a hidden schema, has no keys to follow.
- The filter of a key compares text. A value that the database writes in another form, such as a float with many digits, can give no row.

## Quit button in the tab row — 2026-11-04

The user asked for a button that quits SQLN, at the right end of the tab row.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Look | The text `Quit` in the style of the console buttons: `[Quit]` without color | Only `×`; the power symbol `⏻` | User choice. `×` already closes each tab, and Monaco can show `⏻` as an empty box |
| 2. Click | The same as `Ctrl+W q`: it quits at once, and it asks only for a running statement or an open transaction | Always ask | User choice. The scripts save, and the tabs reopen at the next start |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Place | One free column after the button and one before it. The tabs use the rest of the row, so the active tab still scrolls into view | The button over the last tab | A tab never hides the button |
| Home screen | The button stays | Hide it without a tab | The tab row is always on screen |

## Search path of a console schema in PostgreSQL — 2026-11-04

The user reported `type geography does not exist (42704)` on a query that works in DataGrip. A console with a chosen schema sent `SET search_path TO "<schema>"`, so `public` left the path, and with it the types and functions of PostGIS. A console gets a schema from the selector, from a saved script, or from the compare.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Console path | The chosen schema first, then the default path of the session (`"$user", public`, or the path of the role and the database) | `"<schema>", public`; only the schema | User choice. Extensions in `public` or in another schema of the default path stay visible. A name without a schema still looks in the chosen schema first, and `CREATE` without a schema still uses it |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Statement | `set_config('search_path', concat_ws(', ', <schema>, NULLIF(current_setting('search_path'), '')), false)` | A fixed text of the default path | The default path can change by role or database. An empty default path gives only the schema |
| Compare | `RunOptions::only_schema` keeps `SET search_path TO "<schema>"` for the reads of a compare | The same path as the console | The compare needs the names of other schemas with their schema, such as `public.geography`, for its sync script |
| Repeats | The cleanup of the session restores the default path, and a reserved session does not receive the context again | Remove the schema from the default path first | The path does not grow. A repeated name in the path does no harm |

## Visible schemas of a connection — 2026-11-04

The user asked to mark, in the configuration of a connection, the schemas of PostgreSQL or the databases of Redis that the explorer shows. The user chose points 1 to 4 and approved points 5 to 13 with the plan.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Form | A field with a list that SQLN reads from the server, with a search and a mark on each row. The field keeps the names, and the user can also write them. An empty field shows all | A text field only | User choice |
| 2. Effect | The whole connection: tree, tree filter, schema selector of the console, compare dialog, and completion see only the marked schemas. A statement can still use any schema | Only the tree | User choice. Redis also skips the `SCAN` of a hidden database |
| 3. Current schema | Always visible: `current_schema()` of PostgreSQL, `DATABASE()` of MySQL, the database of a Redis connection | Visible only when marked | User choice. The console runs there, and completion reads it |
| 4. Open connection | The field stays in the form of a connected connection. **Save** reads the catalog again and keeps the sessions | Change only after a disconnect | User choice |
| 5. Databases | PostgreSQL, MySQL, and Redis. SQLite has no field | Only PostgreSQL and Redis | In MySQL a database is the schema |
| 6. Marked empty Redis database | It appears. Without a list, the old rule stays: an empty database appears only when it is the current one | Only a database with keys | The user marked it |
| 7. Redis list | The databases with keys, the current database, and the marked databases. Another number is written in the field | `0` to `N - 1` from `CONFIG GET databases` | A managed service often refuses `CONFIG` |
| 8. Patterns | Exact names only, with case | `tenant_*` | Not necessary now |
| 9. Tree note | `(filtered)` after the name of a connection with a list | No note | The user sees why a schema is not there. `Loading` and `Failed` come first |
| 10. Place | First field of **Options**. The label is **Schemas** for PostgreSQL and **Databases** for MySQL and Redis. The `...` button and `Ctrl+O` open the list | Own section | The form of an open connection keeps two sections |
| 11. List keys | Write to filter, `Up` and `Down`, `Space` marks, `Enter` writes the field, `Esc` cancels. A click marks a row | Other keys | Same keys as the other lists |
| 12. Field text | Names with commas between them, such as `public, sales`. Spaces, empty names, and repeats go away. A Redis name must be a number | A list of lines | One line, as the other fields |
| 13. `config.json` | `"schemas": ["public", "sales"]`. A missing or empty list shows all. `version` stays 1 | A new version | Same rule as `idleTimeoutSeconds` |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Read of the names | The path of **Test**: one short session with the definition of the form, also with the connection open. `ExecEvent::ConnectionTested` carries the names | The pool when the connection is open | One path. The password requests of **Test** stay as they are, and the list reads the server of the form |
| Filter | `Session::read_catalog_of` reads the catalog and calls `Catalog::keep_schemas`. Redis replaces it and reads only the databases of the list | A `WHERE` in each catalog query | One rule for each database. The catalog of PostgreSQL and MySQL is one query |
| Names | `Session::read_schema_names` reads the catalog. Redis reads only `INFO keyspace` | One query for each database | Only Redis has a cost in the full read: one `SCAN` for each database |
| Window | `draw_list_window` of the palette, over the form | A list below the field | The list can hold many names |

Known limits:

- A name with a comma does not fit in the field.
- PostgreSQL and MySQL still read the relations of each schema. The filter reduces the memory and the screen, not the query.
- A new schema does not appear until the user marks it.
- The list opens one more session, outside the pool limit, also with the connection open.
- The list needs a valid form. A Redis field with a name that is not a number stops it.
- An older SQLN refuses a file with `schemas`.

## Lists with a search in the compare dialog — 2026-11-03

The user asked for a dialog that fills step by step: a connection shows its schemas, and a schema shows its tables, with a search. The user chose point 5 and approved the other points as recommended.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Form | A list below the field, filtered while the user writes, as in the filter of a table tab | A separate window with a filter, as the palette | User choice |
| 2. Fields | Connection, schema, and table, with the same list | Only schema and table; only table | User choice |
| 3. Open | The list opens when the field gets the focus. It shows all names and marks the current one. `Enter` chooses and moves to the next field, so a connection leads to its schemas and a schema to its tables | Only when the user writes or presses `Ctrl+Space` | User choice |
| 4. `Left`/`Right` on a connection or a schema | They change the choice, as before. `Space` and the Vim keys `h`, `j`, `k`, `l` write in the search | A plain text field | User choice. A connection list keeps its old keys |
| 5. Accept | `Enter` or `Tab` choose the marked name in the three fields. `Esc` closes the list and keeps the value. A table keeps a free text: `Esc`, then `Tab` | `Enter` and `Tab` on a connection or a schema, and `suggestionKeys` on a table | User choice |
| 6. Table list | Only tables | Tables and views | The dialog refuses a view |
| 7. New schema with a table text | The text stays | Clear it; keep it only when the new schema has the table | User choice |
| 8. Search | Each word in any part of the name, with no case, as in the palette | Only the start of the name | User choice |
| 9. Mouse | A click on a row of the list chooses it | Only the keyboard | User choice |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Mark | The name equal to the text, or, without a text, the current choice. A text without an equal name marks the first name. An empty table marks nothing | Always the first name | `Tab` on an empty table keeps the whole schema, and on an empty target keeps the source table |
| Click on a field with a list | Opens its list | Change the choice, as on **Kind** | The list shows the choices |
| Empty list | A field without a name to show acts as a field without a list: `Up`/`Down` move the focus, and `Esc` closes the dialog | `Esc` closes an empty list first | An empty list is not on the screen |
| Drawing | `draw_anchored_list` draws the list of the dialog and the suggestions of the editor | A second copy | One rule for the place, the scroll, and the mark |

## Compare in the hint row and the `Ctrl+W` menu — 2026-11-03

The user did not find the compare in the `Ctrl+W` menu or in the hint row. This request reopens point 17 of the compare (below), as the ER diagram reopened the rule of 2026-11-02.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Hint row of the explorer | `c compare` after `d diagram` on the rows of a schema, a table, and a column. The connection row stays as it was | Also on the connection row, which loses `Ctrl+W commands` at 80 columns; at the end of the row, which hides it at 80 columns | User choice. Each of these rows fits in 80 columns |
| 2. First page of `Ctrl+W` | `v Compare` in the **Connection** group, as `D ER diagram` and `S Sessions`. With the focus in the explorer, the row decides. A table tab uses its table (**Data**), a console uses its current schema (**Structure**), a diagram uses its focus, and a compare tab opens the dialog with its own sides | `Ctrl+W V`; `Ctrl+W o` | User choice. `Ctrl+W c` closes the tab, and `Ctrl+W C` commits |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Palette | `Ctrl+W v` replaces the explorer entry | Both entries | The rule of the ER diagram: one entry acts on the row in the explorer |
| Structure tab | A structure of a table uses its table. Other structures and the sessions use their schema | Refuse | The rule of the ER diagram |

## Schema compare and data compare — 2026-11-03

The user asked for two features of DataGrip: a schema compare between two databases, with a generated sync DDL, and a data compare between tables of different databases. The comparison with DataGrip of 2026-11-02 listed the schema compare as an open gap. The user chose points 1 to 9, and approved points 10 to 27 with the plan.

New fact: the sync script needs DDL that SQLN writes. The structure tab rejected a DDL text because "a text assembled by the tool could differ from the real one" (Structure tab — 2026-10-31, point 2). That rule of the structure tab stays.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Pairs | Any pair of SQL sources. A schema compare between two dialects shows the differences, but only two sources of the same dialect get a DDL script. A data compare works between any SQL sources. Redis and Valkey refuse | Same dialect only; a data compare between dialects only | User choice |
| 2. Result | A **Compare** main tab lists the differences. A key opens the sync script in a new console of the target. Nothing runs by itself, and the run protection acts when the user runs the console | A console with the report as comments; a tab that runs the script | User choice |
| 3. Objects | Tables and columns (type, null, default), primary key, foreign keys, unique keys, indexes, and `CHECK` where the database shows it | Also views; also sequences, triggers, and functions | User choice |
| 4. Removals | `DROP` and `DELETE` go into the script as marked comments. A dialog option makes them statements | Always statements; never in the script | User choice |
| 5. Data key | The primary key of the source. The target needs the same key columns. A table without a primary key refuses with the reason | A choice of key columns; the whole row as the key | User choice |
| 6. Data size | The source rows stay in memory, and the memory budget counts them. The target rows arrive as a stream. Above the budget, the compare stops with no partial result | Both sides sorted by the key, with no limit | User choice. Two collations can give two orders |
| 7. SQLite | A change that `ALTER TABLE` cannot do rebuilds the table with the SQLite procedure. The triggers of that table are lost | A comment for a manual step | User choice |
| 8. Types between dialects | Type and default show side by side and do not count as a difference | Type families; exact text | User choice |
| 9. PostgreSQL 10 and older | Out of scope | Support | The ER diagram rule (`conparentid`) |
| 10. Column order | Ignored | A difference | PostgreSQL and MySQL cannot move a column without a rebuild |
| 11. Data column only in the source | Refuse, with "compare the structure first" | Compare the common columns | The script would not make the target equal. A column only in the target stays out of `INSERT` |
| 12. Identity of indexes and constraints | By name on PostgreSQL and MySQL. A SQLite foreign key by its definition, because it has no name. The same name with another definition is a removal and a creation | By definition on all databases | The DataGrip model, and the DDL uses the name |
| 13. Booleans and dates between dialects | They count as a change (known limit) | Normalize booleans | Without the column type, `t` and `1` have no safe rule |
| 14. Value equality | `NULL` equals only `NULL`. Two numbers compare by their decimal value in text (`10.50` equals `10.5`, no `f64`). Text and binary compare by bytes | Exact text | Large integers stay exact, and the scale of `NUMERIC` gives no false change |
| 15. Type change on PostgreSQL | `ALTER COLUMN ... TYPE t USING c::t` | No `USING` | Without `USING`, many changes always fail |
| 16. Key | `c` in the explorer | `C` | It opens the dialog on a connection, schema, or table row |
| 17. Discovery | The `Ctrl+W Tab` page, the palette, and `F1` (rule of 2026-11-02) | Also the hint row and the first `Ctrl+W` page | The hint row at 80 columns is full |
| 18. First kind in the dialog | A connection or schema row opens **Structure**. A table row opens **Data** | Always **Structure** | The likely use of each row |
| 19. Structure on a table row | Only that table, against the table with the same name in the target. The field can change | The whole schema | The DataGrip model |
| 20. Choice of items | No choice. The user edits the script in the console | `Space` marks items | Not asked. The console already edits |
| 21. Data view | Wide: a `Change` column and the columns of the table. An update takes two rows (source, then target), and `Change` names the changed columns | One row per changed value | A copy and an export give real rows. The grid has no color per cell |
| 22. Keys of the tab | `S` opens the script in a new console, `y` copies the script, `R` reads again, and `Ctrl+W x` stops | `Enter` opens the script | `Enter` opens a value in the grid. `S` and `y` are free in the grid region |
| 23. Transaction in the script | PostgreSQL and SQLite: `BEGIN` and `COMMIT` around the script. MySQL: no transaction for DDL, because DDL commits; `BEGIN` and `COMMIT` for data | No transaction | All or nothing where the database allows it |
| 24. Data script | One statement per row | Batches of `INSERT` | An error names the row |
| 25. Time limit of a data read | None, as in the export. `Ctrl+W x` stops | The SQL time limit, as in the diagram | A large table needs more than 30 s |
| 26. Restart | The tab does not come back | Save it in `OpenTabs` | The rule of the structure tab and the diagram |
| 27. Delivery | Two parts: **Structure** first, then **Data**. Each part has its tests, its documents, and `make check` | One delivery | The data part reuses the tab, the dialog, and the literals |

Implementation decisions of the structure part:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Tab | `Tab::Compare` with its own `CompareTab` and one executor task per side | `TableTab` with a new scope | One task binds one connection, and a compare reads two |
| Read | The queries of `compare::sql` through `Manager::run`, in one execution per side, with no row cap and the schema in the context of the session | A new `Session` method in each connector | The diagram rule: no connector change, and the read keeps the time limit, the stop, the pool queue, the password, and the budget |
| Names on PostgreSQL | The read sets `search_path` to the schema, so the definitions name the objects of that schema without the schema. The script starts with `SET LOCAL search_path` to the target schema | Replace the schema text in each definition | The database writes the names |
| Defaults on MySQL | `QUOTE()` makes a literal of a text default. An expression keeps its parentheses, and `CURRENT_TIMESTAMP` stays as it is | The raw `COLUMN_DEFAULT` | Each side gives text that a DDL statement accepts |
| `serial` on PostgreSQL | A column with its own sequence and no identity is `serial`. Its default does not count, and a new table writes `serial`, `bigserial`, or `smallserial` | Compare the `nextval` text | The name of the sequence follows the table, and a new table needs a sequence |
| Unique key on MySQL | An index | A constraint | MySQL keeps a unique key as an index |
| Index of a foreign key on MySQL | The index with the name of a foreign key stays out | Compare it | MySQL makes it for the key |
| Primary key | Compares by its columns. The name does not count | By name | A rename of a key does not change the table |
| Between two dialects | Keys and indexes compare by their columns, a unique key matches a unique index, and the checks stay out | By name | Each database names a key by its own rules |
| Copy key | `Y` | `y` of point 22 | In the Vim mode, `y` copies a cell in the grid |
| Table focus | The source table takes the name of the target table, so the script acts on the target table | A map of names in the script | One rule for the grid and the script |
| SQLite rebuild without removals | The columns and the indexes of the target stay. A kept column without a default loses `NOT NULL`, and a note lists the keys of the target that the rebuild leaves out | Refuse the rebuild | The source text holds only the source columns |
| Script console | Always a new console of the target, with the schema of the target | `write_sql` | `write_sql` writes in the console of the connection, which can hold the work of the user |
| Dialog | One page with seven fields | A sidebar | The fields fit in 80 × 24 |
| PostgreSQL version | 12 or later | 11 | The read uses `attgenerated`. PostgreSQL 11 has had no support since 2023 |

Implementation decisions of the data part:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Steps | Three executions in order: the columns of both tables, then each row of the source, then each row of the target | One execution for each side | The check of the key and the columns comes before the read of the rows, and the target meets a complete index |
| Column read | The column query of the structure compare, with a filter of the table | A new query | One reader of the key, the identities, and the generated columns |
| Gate | Each step passes the password gate (`Use::Compare`) | One gate for the compare | Each read can find a new password request |
| Time limit | `RunOptions::untimed` for the reads of rows | The flag of the export | Decision 25, without an export file |
| Match | A hash of the key cells in a map, and a check of each key cell | A sort by the key | Decision 6 |
| Form of a value | `NULL` has no form, a number has its decimal form, and another value has its bytes. Equality and the hash read the same form | Two rules for the hash and the equality | A key must find its row with the same rule that compares it. `5` as text equals the number `5` |
| Budget of the target | An equal row leaves the budget in its batch, and a changed row stays for the grid | Keep each target row | Two equal tables hold only the source rows |
| Columns the target makes | A generated column and, on PostgreSQL, a column with `GENERATED ALWAYS AS IDENTITY` outside the key stay out of the compare and of the script | Compare them | The target refuses their values |
| Identity key | `OVERRIDING SYSTEM VALUE` in each `INSERT` when a key column of PostgreSQL has `GENERATED ALWAYS AS IDENTITY` | Leave the key out | The key must keep the value of the source |
| Literals | `sql::literal`: the import form of text and bytes, and the export rule of numbers. The import and the export use the same functions | A third writer | One writer for each form, and the export keeps its own output |
| Script console | The paste path of the templates | A new editor | A paste of 100,000 lines takes 32 ms in a release build |
| Grid | Built at the end, in the budget, as a plan is | Rows as they arrive | The inserts are known only at the end |

### Known limitations of the data compare

- The source table must fit in the data budget, and the index of the keys stays out of the budget.
- The two tables are read at different times, so a change between the reads appears as a difference.
- Between two dialects, booleans (`t` and `1`), dates, and times of other formats count as changes. A literal of the source can also refuse its column in the target.
- A text `5.0` does not equal a number `5`, because only two numbers compare by value.
- A target column outside the source stays out of `INSERT`, so it takes its default.
- A large script makes a large console, and the script is outside the budget.
- The `UPDATE` and `DELETE` of a target with another key than the source can change more than one row.

### Known limitations of the structure compare

- The compare leaves out views, sequences, triggers, functions, partitions, grants, comments, column order, and table options. On MySQL the DDL keeps the character set and the collation of a column, but they do not count as a difference.
- SQLite gives no check through a pragma, so a check does not count. A rebuild copies the checks of the source text.
- A rename appears as a removal and a creation.
- A SQLite rebuild loses the triggers of the table.
- On PostgreSQL, a change of an identity or of a generated column gets a note in the script, not a statement. A `USING` cast can fail at run time. A primary key that a foreign key of another table needs refuses its removal. `SET NOT NULL` fails on a column with `NULL` rows.
- On MySQL, DDL commits at once, so an error leaves the statements before it applied. `int(11)` and `int` of two versions count as a difference. A `bit` default such as `b'1'` gets a wrong literal.
- The two sides are read at different times, so a change between the reads appears as a difference.
- A foreign key of a table to itself, in a table focus with another target name, keeps the source name.

## Messages command history — 2026-11-03

The user requests a DataGrip-like command history, separate for each console.
This replaces the previous clearing of Messages on execution or connection change.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Lifetime | Accumulate in memory until the console closes | Persist the history; clear at each execution | User choice |
| Scope | User commands, transaction control, internal and shared commands | Only editor SQL | User choice |
| Imports | Exclude the entire import operation, including session setup and cleanup | Record generated inserts | User choice; preserves the import exclusion |
| Shared commands | Capture the active SQL console of the same connection at operation start; discard without a match | All consoles; last console of the connection | User choice |
| Text | The effective SQL and separate bound parameters; safe byte escapes for Redis | Original editor text; interpolated SQL | The record must describe the request that reaches the driver |
| Protocol commands | Technical operation names, without invented SQL | Exclude commands without SQL | User choice |
| Driver boundary | Application calls only; exclude authentication and hidden driver operations | Raw protocol capture | Keeps credentials outside the history and needs no driver changes |
| Budget | 10 MiB per console; remove oldest records as whole blocks | Unlimited memory; a fixed command count | User choice |
| Oversized record | Truncate with an explicit marker, without changing the database request | Exceed the budget; omit all text | User choice |
| System command visibility | Settings > Results > System commands, Off by default; hide Internal and Shared records without deleting them | Stop capture; show by default | User choice |
| Messages layout | Compact blocks with a separator heading, SQL syntax colors, and one result line with rows and time | Bordered boxes; a flat message list | User approval |
| Messages colors | Use the editor token styles and the recorded dialect; color the result, not all SQL, on an error | Parse status text from SQL; remove syntax colors after an error | Keeps SQL, notices, and results distinct |
| Command time | Measure each driver operation with a monotonic clock; show milliseconds on completion or interruption | Total script time only; include pool waiting | User request |
| Scroll | Follow only at the end; preserve the current record while reading older records | Always follow; never follow | User choice |
| Compatibility | Keep result selection and the warning count of the current execution | Let internal records select Messages or count as warnings | History does not change execution controls |

## Home screen — 2026-11-03

The user asked for a screen for the cases without a console. Before this change, a start without a connection opened a console that could not run, and the close of the last tab ended the application. This reopens two rules: "Closing the last tab ends the application" (`docs/UI.md`) and "End of the last table | End the application" (Implementation — 2026-10-31).

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Start | The home screen only when no connection exists, no tab of the last session comes back, and the command line names no database. With a connection, the first console uses the first connection, as before | The home screen at each start without a reopened tab | User choice |
| 2. Focus | The explorer gets the focus. A hidden explorer keeps it hidden, and the focus does not move | No change of the focus | The explorer keys (`a`, `n`, `s`, `Enter`) act at once |
| 3. Contents | An ASCII logo, the version, one line of state, and the main keys | The name and the keys only; a list of connections and scripts with its own keys | User choice. The list needs a new key region |
| 4. `Ctrl+W c` on the home screen | No effect, and the `Ctrl+W` menu hides it. Only `Ctrl+W q` ends the application | End the application | No tab is open |
| 5. Close of the last tab | The home screen appears, also after the last table or diagram | End the application | Follows from the request |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| State | The hidden console of "Last console" (Implementation — 2026-10-31). `App::home` is true when that console is on screen | Zero consoles | The code assumes an active console |
| Allowed commands | `home_allows` keeps the explorer commands and the commands that need no tab. `perform` and `command_available` use it, so the palette and the `Ctrl+W` menu hide the others | One check in each command | One list. `switch_tab` divided by zero with no tab |
| Start option | `Options::start_home`, which only `main` sets | The home screen for each `App` without a connection | 88 tests create an `App` without a connection and type SQL |
| Drawing | `ui/welcome` draws both screens. `Welcome::logo` asks for the logo, and a short area shows the name instead | A second widget | One layout of keys |
| Explorer row | The keys show the full path, such as `Ctrl+W h a`, and the explorer row needs only `a` | Only the explorer key | The path acts from any panel |

## Idle limit and connection form with a sidebar — 2026-11-03

The user asked for an idle limit in the connection form, with 300 seconds as the default. Before this change, SQLN never closed a session for inactivity. Only the server or the network closed it.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. What closes | Only the sessions that wait in the pool. A console with an open transaction keeps its session | Also the session of a transaction, with a rollback by the server | A close would lose work without a commit and without a question |
| 2. Start of the count | Each session counts from its return to the pool | The connection counts from its last use, and all sessions close together | The pool keeps the session in use and closes the others |
| 3. Value 0 | No limit, as in the SQL limit | Refuse 0 | The same rule as the SQL limit |
| 4. Saved connections without the field | 300 seconds | No limit | It is the default of the request |
| 5. Sources | All, also SQLite and Redis | Only the network sources | One rule |
| 6. Message | None. The explorer mark changes, and the next use opens a session | A message in the status line | The close needs no action of the user |

The new field did not fit: the largest form (a network source with the choice of a scope) had 21 fields and filled 80 x 24 exactly. The user then reopened the row "Connection form" of "nvim-style interface" (four arrangements, no scroll), and approved a sidebar with sections, as in Settings, the import, and the export.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Sections | The five groups: General, Address, Credentials, Security, and Options | Split Options into Limits and Appearance | The groups already exist, and Options fits |
| 2. Section without fields | Out of the sidebar | Shown as disabled | SQLite has no Credentials and no Security |
| 3. Error in another section | The form opens the section of the field and puts the focus on it | Only the note, as in Settings | The required fields, such as Host and User, live in other sections |
| 4. Buttons | Fixed at the bottom of each section | In one section | As in Settings |
| 5. Form of a connected connection | Also the sidebar: General and Options | One page | User choice |
| 6. Keys and mouse | As in Settings: `Tab` walks the sidebar, the fields, and the buttons; `Up` and `Down` on the sidebar change the section; a click opens a section | Own keys | One model for the dialogs |
| 7. First section | General, with the focus on Type, or on Name in an edit | The last section used | The focus of before stays |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Timer | One task of `Manager` for each pool, with a `WeakPool`. It sleeps until the next idle session reaches the limit | A check at the next use | A check at the next use does not free the server |
| Event | `ExecEvent::IdleClosed`, which redraws the explorer and opens a display-only form for changes, as `Disconnected` does without its message | `Disconnected` | That event writes "Disconnected." |
| Error field | `ConnectionForm::definition` returns `Invalid { field, message }` | A second check that finds the field | One check |
| Height | All sections keep the height of the largest section of the form | The height of each section | The window does not move between sections |
| Width | 76 columns: the sidebar, the label, and the longest choice | 72 | The value needs 26 columns |
| Sidebar code | The form uses the functions of `ui/settings.rs` | Own copies | One drawing for the dialogs |

### Known limitations of the idle limit

- The cleanup of SQLite already closes the connection at each return to the pool, so SQLite never has an idle session, and the limit has no effect there.
- A change of the limit needs a disconnection, as each change of the definition does, because the pool keeps its limit.

## Horizontal wheel on the grid — 2026-11-03

The user asked for a horizontal scroll of the grid with the touchpad. Before this change, only the ER diagram read the horizontal wheel.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Step | One column for each event, and the cursor moves with it, as the vertical wheel moves the cursor by rows | Several columns for each event | One gesture of a touchpad sends many events |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Place | Only the grid of a console or of a table tab. The navigator and the editor ignore the horizontal wheel | Each panel | These panels have no columns to move |
| `Shift` and the vertical wheel | No own code | Read `Shift` | macOS already sends that gesture as a horizontal wheel |

Known limitation: a terminal that does not send horizontal wheel events gives no horizontal scroll. iTerm2 has no check yet.

## `LIMIT` with the row cap — 2026-11-03

The user asked to stop a full scan of a large table when a read has no `WHERE` and no `LIMIT`. The new fact: the server does not know the row cap. The planner then plans for all rows, so `ORDER BY` on an indexed column can sort the whole table before the first row. In a transaction, the reader also reads every row to the end, because a cancellation ends the transaction. In MySQL, the cancellation opens a new connection for `KILL QUERY`, and the rows keep coming meanwhile.

This decision reopens the rule "Do not repeat the command or add `LIMIT` to the user text" of the plan, section "Row cap in the consoles", and its item in the acceptance list. The rule against a repeated command stays.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Approach | The database receives `LIMIT` with the row cap and one row | A limit in the driver (`sql_select_limit` in MySQL, a cursor in PostgreSQL); a cancellation in a transaction with a `SAVEPOINT` | One rule for the three databases. The planner knows the limit, also in a transaction |
| 2. Own `LIMIT` above the cap | No change. The cancellation at the cap stays | Replace it with the cap | The text of the user keeps its own clause |
| 3. Message | One line in **Messages** for each statement that receives the limit | No message | The user sees that the database received another text |
| 4. Setting | Settings > Results > **Limit in SQL**, key `capInSql`, global and per project | Fixed | User choice |
| 5. Default | On | Off | The goal is to stop the full scan |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Rule | A plain read of `sql::is_plain_read` that starts with `SELECT`, `WITH`, `VALUES`, or `TABLE`, without `LIMIT`, `OFFSET`, `FETCH`, `FOR`, or `LOCK` in the main level | Each read | A `LIMIT` changes the effect of an unknown function, as a cancellation does. `EXPLAIN` would show another plan. `FOR` and `LOCK` refuse a `LIMIT` after them in MySQL |
| Value | Row cap and one row | The row cap | The row after the cap proves that the result has more rows, as in the table tab |
| Place | The clause goes on a new line at the end | The same line | A line comment at the end would hide the clause |
| Runs | The runner adds the clause, so the table tab, the structure tab, and the consoles use one rule | Only the consoles | The sink keeps the same rows. An export and the diagram have no cap, so they keep the text |
| Message level | `Info`, through `ExecEvent::LimitAdded` | A notice of the database | A notice counts as a warning on the tab, and the clause is not a warning |
| Error place | The error place stays inside the text of the user | The place in the sent text | The editor marks the last token of the statement |

### Known limitations of the added `LIMIT`

- An incomplete statement, such as `SELECT * FROM`, can receive an error that names `LIMIT`. The line in **Messages** gives the cause.
- An own `LIMIT` above the cap, and each statement out of the rule, keep the old flow: a cancellation without a transaction, and a read to the end in a transaction.
- `ORDER BY` on a column without an index still reads the whole table, because the server must sort. With `LIMIT`, the server keeps only the first rows.
- A `CALL` of MySQL does not change.

### Saved settings on an open connection

Before this change, a saved row cap reached a connection only when SQLN added or replaced it: at the start, or after a change of its definition. The user approved the fix and its rule.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Moment | A saved value acts from the next execution. A running script keeps its values | From the next statement of the running script | One script does not change its cap in the middle |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Shared value | `Binding` keeps the settings in an `Arc<Mutex<_>>`, and the consoles of the connection share it, as they share the `Budget` | A `Command::Settings` message | A running task drops a message in `refuse`, so it would need a pending value |
| Read | `ConsoleTask` copies the settings when it receives a command, before each execution and import | A read in each statement | The rule of point 1 |
| Write | `apply_config` calls `Manager::update_settings` for each connection, with the definition on top of the settings | Only the row cap | One path for each value of the connection |
| Pool | The pool keeps its size and its connect time limit | A new pool | These values come from the definition, and the form needs a disconnect to change them |

## ER diagram in the hint row and the `Ctrl+W` menu — 2026-11-03

The user did not find the diagram in the hint row of the explorer or in the first page of the `Ctrl+W` menu. This request reopens, for this command only, the rule of 2026-11-02 that keeps only the main commands in the hint row of the explorer.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Hint row of the explorer | `d diagram` on each row where the key acts: connection, schema, table, and column | Only in the `Ctrl+W Tab` page | The user did not find the command |
| 2. First page of `Ctrl+W` | `D ER diagram` in the **Connection** group, as `S Sessions`. With the focus in the explorer, the row decides. A table tab uses its table, and a console uses its current schema | Only in the `Ctrl+W Tab` page | The same model as the sessions |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Key | `Ctrl+W D` | `Ctrl+W d` | `Ctrl+W d` chooses the schema |
| One rule | `row_diagram` and `diagram_target` give the target or the reason of a refusal. The key, the hint row, the menu, and the explorer page read them | A check for each place | The places agree |
| Connection without its catalog | `d` reads the catalog, and the diagram of the current schema opens when it arrives (`pending_diagrams`). Each waiting connection opens its diagram | Hide the key until the catalog arrives; keep only the last request | The connection row is the first row of the tree. `pending_sql` already waits in this way. A second request does not cancel the first, because the user asked for both |
| Label | `ER diagram` | `Show the ER diagram` | The long label fills the 19 columns of a row and touches the next column |
| Palette | `Ctrl+W D` replaces the explorer entry | Both entries | One entry acts on the row in the explorer, as the sessions do |
| Structure tab | A structure of a table uses its table. Other structures and the sessions use the current schema | Refuse | Each tab of a source with tables has a target |
| Width | At 80 columns, the hint row of a connection keeps each command | Leave out `F1 keys` | The row fits |

## ER diagram — 2026-11-03

The user asked for an ER diagram of the schema, as in DataGrip. The comparison with DataGrip of 2026-11-02 listed it as an open gap. The user chose points 1 to 8, and approved points 9 to 20 with the plan.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Lines | Orthogonal lines with box characters, from the row of the key column to the row of the column that it reads, with an arrow at the table that the key reads. The selected table shows its lines in the focus color | Marks such as `→ table` without lines | The DataGrip model |
| 2. Origin | Schema: all its tables. Table: the table and the tables that its keys join, one step away. Connection: the current schema. Column: its table | Schema only; the whole connection | Each row has a useful picture, and a connection can have many schemas |
| 3. Box | Name, type, and key mark of each column. A key shows only the key columns | Always all columns; always keys only | More tables fit on the screen when the user asks |
| 4. Export | `y` copies the Mermaid `erDiagram` text | A file dialog; PlantUML and DOT; nothing | GitHub and GitLab show Mermaid text as a diagram, and a copy needs no dialog |
| 5. Navigation | The arrows, and `h`, `j`, `k`, `l` in the Vim mode, select the nearest table in a direction, and the view follows. `Shift`+arrows, `PageUp`, `PageDown`, the wheel, and a drag move the view | The arrows move the view and `Tab` selects | Selection is the main use, and `Shift`+arrows passed the iTerm keytest |
| 6. Objects | Tables only: PostgreSQL `r`, `p`, and `f`, MySQL `BASE TABLE`, SQLite `table` | Tables and views | A view has no keys, so it would take space with no line |
| 7. Key to another schema | A box with `schema.table` only, in a dim style | No line | The line keeps its meaning |
| 8. Key | `d` in the navigator, in the `Ctrl+W Tab` page, and in the palette | `E` | The key was free in both modes, and `D` disconnects |
| 9. Restart | The tab does not come back | Save it in `OpenTabs` | The structure tab rule: the cost of opening is one key |
| 10. Refresh | `R` reads again. DDL does not update the tab | An automatic read | The structure tab rule |
| 11. Redis and Valkey | Refuse with a reason in the status bar | An empty diagram | A key has no columns and no keys |
| 12. Actions on the selected table | `Enter` opens its data, and `i` opens its structure | No action | The keys keep their meaning of the navigator |
| 13. Size | No table limit. Only the memory budget limits the read, and a partial read shows `partial` | A table limit | The structure tab rule |
| 14. Search in the diagram | Out of this delivery | Search with `/` | Not asked |
| 15. Key of "keys only" | `K` | `t` | `k` is a Vim key, and `K` acts only in the diagram region |
| 16. PostgreSQL partitions | The partitions stay out, and the key of a partitioned table appears once | Each partition as a box | PostgreSQL copies the key to each partition |
| 17. Key from another schema | Also a box with the name only | No line | The same rule as point 7 |
| 18. Mermaid content | The boxes on screen: with "keys only", only the key columns | Always all columns | The copy is the picture on screen |
| 19. Click | A click on a box selects the table. No double click | A double click opens the data | `Enter` opens the data |
| 20. Symbols | Box characters for the lines in each set. The `plain` set uses `<`, `>`, `PK`, and `FK` | ASCII lines without color | The rule `─` already shows with `NO_COLOR` |

The first delivery kept `d` out of the hint row of the navigator, as the rule of 2026-11-02 says. The section above changed this on the same day.

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Tab | `Tab::Diagram` with its own `DiagramTab` and executor task | `TableTab` with a new `Scope` | `table_active` means a grid in many places, and a picture is not a grid |
| Read | Two catalog queries through `Manager::run`, in one execution, with no row cap | A new `Session` method in each connector; one `load_columns` per table | No connector change, and the read keeps the time limit, the stop, the pool queue, the password, and the budget |
| Key identity | Pairs join by table and key name | By key name only, as `group_foreign_keys` | PostgreSQL and SQLite repeat a key name in other tables |
| SQLite key without target columns | It reads the primary key of the target | Leave it out, as the catalog read does | The structure tab shows that key |
| PostgreSQL partitions | `NOT relispartition` and `conparentid = 0` | Read the partitions and join them | One filter in each query |
| Layout | Layers from the left by the longest path, one row per crossed layer, four sweeps by the mean place of the neighbors, and tracks from the left with the rule of the shared rows | Free placement; a path search per line | The same model gives the same picture, and 500 tables with 597 keys take 12 ms in a release build |
| Line storage | One sorted list of pieces per row | One map entry per cell | The map took three times longer |
| Thread of the layout | The interface thread | A worker thread | 12 ms for 500 tables |
| Nearest box | A box that starts past the edge of the selected box | A center past the center | A wider box below looked like a box on the right |
| First selection | The selected table of a table focus, or the box at the top left | The first name | The view starts at the start of the picture |
| Styles | Existing styles of the theme | New fields in the three themes | The same look with fewer changes |
| View row | `d` refuses with a reason, and the explorer page hides it | Open the diagram of the schema | A view has no keys |
| Mermaid label | The names of the key columns | The name of the key | A generated key name says less, and SQLite gives a number |
| Mermaid cardinality | `\|o` when a key column accepts `NULL`, else `\|\|`. `--` when each key column is in the primary key of the child, else `..` | One fixed form | The text keeps the facts of the catalog |
| Mermaid names | `e<n>` ids with the name as alias. A type or a column name keeps only letters, digits, `-`, `_`, parentheses, and brackets, and a changed name keeps the original as a comment | The raw names | A database name can hold any character |

### Known limitations of the diagram

- The diagram has no search, no zoom, and no export to an image file.
- A stub has no columns, so its line meets its name row.
- Two keys that read the same column share the end of their lines. A key that leaves a column that another key reads can share a row with it in the same gutter.
- A cycle of track rules in a gutter would let two lines share cells. The layout counts it in `conflicts`, and the tests expect zero.
- The layout runs on the interface thread. A very large schema delays the key that reads it or that toggles **keys only**.
- Mermaid loses the spaces of a type, such as `character_varying(30)`. The documentation of Mermaid does not describe a key from a table to itself, and no Mermaid version was checked with it.
- A SQLite column without a type shows `any` in the Mermaid text.

## Consoles in the tree of the explorer — 2026-11-02

The user found that the tree did not show the consoles, and asked for a **Consoles** node in each connection. The `n` list stays as it is.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. List | Saved and open consoles, with the `(open)` mark | Only saved consoles | Each console tab of the connection has a row, also without text |
| 2. Node | Only with consoles, and closed at the start | Open at the start; always present | The tables stay near the top, and a connection without consoles has no new row |
| 3. `Delete` on a console row | Removes the script, with the question of the list. An open console refuses | No effect | The key already removes a script in the list, and on this row it must not ask to remove the connection |
| 4. `r` on a console row | Renames, as in the list. An open console renames its tab | Read the catalog | A console has no catalog. `r` keeps its meaning on the other rows |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Place of the rows | `Connection::consoles`, rebuilt by `refresh_tree_consoles` | A read of the disk in `Navigator::rows` | The tree reads its rows at each frame and at each key |
| When the rows change | After a change of a console, and when a connection opens | At each event | The list reads the disk. Only a connection open in the tree reads it |
| Changes from outside | `r` on another row reads the list again | A watch of the folder | A second program rarely changes this list, and `r` already means a new read |
| Tree filter | Lists only relations | Also lists consoles | `KEYMAP.md` defines the filter for tables and views |
| Cursor | Holds the console name, and a rename moves it | Holds the script id | An open console without a file has no script id |
| Order | Saved consoles first, the newest first, then open consoles without a file | Open consoles first | The list of `n` uses this order |

Known limit: at the default width of the panel, a long name cuts the `(open)` mark. The tab row still shows the console.

## `Ctrl+W` menu by panel — 2026-11-02

The user asked that the `Ctrl+W` menu lists only the commands that fit the context of the cursor. `KEYMAP.md` already had this rule for the prefix options, but the code did not apply it.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Rule | A command stays when it has an effect now and belongs to the panel of the cursor | Only the state of the command | A run with the cursor in the navigator acts on the console, and not on the row |
| 2. Key of a hidden command | The key keeps its action | The key stops | The menu is a list, and no current command changes. The hint row uses the same rule |
| 3. Stop, commit, and rollback in the navigator | They stay while they act | Only with the cursor in the console | The session stays occupied until the user acts |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Place of the rule | `fits_cursor` adds the panel to `command_available` | Change `command_available` | The palette reads `command_available`, and this task does not change the palette |
| Run commands | The conditions of `editor_hint`: a connection or a file to run, and a text to plan, format, or comment | Own conditions | The menu and the hint row agree |
| Commit | `ending_commands` reads `fits_cursor`, so a failed transaction has no commit in both lists | Repeat the check | One point holds the rule |
| Empty group | `columns_of` leaves it out of the height | Remove it in `prefix_menu` | The explorer page uses the same code |

## Explorer commands in the `Ctrl+W` menu — 2026-11-02

The user found that `S` was not in the `Ctrl+W` menu, and asked for a shorter hint row in the explorer, with each command available in `Ctrl+W`. The user also asked that no current command changes. This request replaces the rule that the hint row lists each command that acts on a row of the explorer.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Access | A second page of the menu, `Ctrl+W Tab`, with the keys of the explorer | New letters in the first page; the same letters in the first page with the focus in the explorer; a list that runs nothing | Seven keys of the explorer have another meaning after `Ctrl+W`. A page keeps one letter per command and changes no key. A list that runs nothing would show `n New console` while `n` opens the next tab |
| 2. Where the page works | In any panel, on the row of the explorer | Only with the focus in the explorer | The title names the row, so the target stays on screen |
| 3. Hint row | Each row of the explorer shows only the main commands | Only the connection row | The page lists each command, so one rule serves each row |
| Sessions | `Ctrl+W S` stays in the first page, with the connection of the tab | Only in the page | The user asked for it. The key was free |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Keys of the page | A key goes through the `Navigator` region of the key map | Own bindings for each command | A custom key works in the page with no more code, and a new command needs one row |
| What the page lists | `explorer_commands`, the commands that act on the row | Each command at all times | The row of the footer used this rule, and a key without effect spends space |
| Connection commands | Only on the connection row | On each row | On a table, `Del` would remove the whole connection |
| Second key text | `prefix_keys` uses `key_label` | Keep the first character | `tab` would show as `t` |

## Query plan and server sessions — 2026-11-02

The user compared SQLN with DataGrip and asked for a plan of the two execution gaps. This request reopens the rule "Own view for `EXPLAIN`" of the first version.

Query plan:

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Screen | Result tab of the console, with a tree in the grid | Own tab in the main strip; own widget with expand and collapse | Reuses search, copy, the row window, and export |
| 2. Commands | `Ctrl+W E` for the estimate, and `Ctrl+W A` for a real run | Estimate only | The real time is the most useful data |
| 3. Real run of a statement that can write | Ask, and then run | Refuse; run in a transaction and undo | `is_plain_read` counts each unknown function as a write, so a refusal blocks valid reads. A transaction adds another run path |
| 4. Selection with several statements | Refuse, with a reason | One plan per statement | Simpler, and a `SET` in the selection would give an error |
| 5. History | A plan stays out | Save the text with `EXPLAIN` | The history keeps the queries of the user |

Server sessions:

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 6. Place | Tab in the main strip, with `S` in the navigator and in the palette | Modal window | Reuses the structure tab |
| 7. Databases | PostgreSQL and MySQL. SQLite refuses with a reason. Redis and Valkey stay out | Redis and Valkey with `CLIENT LIST` | `CLIENT LIST` works in a console, and a tab needs an own reader |
| 8. Reach | Each session that the database user can see | Current database only | The `database` column separates them |
| 9. Actions | `q` cancels the query, and `K` ends the session | End only | A cancel keeps the connection of the other person |
| 10. Question | Always | Production only | One wrong key ends the work of another person |
| 11. Read-only connection | Refuse both actions | Allow | The database does not refuse them, so SQLN keeps the promise |
| 12. Session of the list | Hide it | Show it with a mark | A session must not end itself |
| 13. Refresh | Manual with `R`, and automatic after an action | Also by interval | An interval makes a continuous read on the server |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Run path | `RunTarget` carries the statements or a plan through `run`, `RunAnyway`, and `RunConfirmed` | Own dialog actions for a plan | One path keeps the checks of a run: config, busy, and unknown transaction |
| Tree | Built when the result ends | Build inside the connector | The connector stays the same, and a read that fails keeps the text |
| Sessions tab | `Scope::Sessions` in `Structure` | New tab type | The tab keeps deadline, stop, pool queue, budget, password, and pages |
| Signal | `PageKind::Signal` through the task of the tab | A temporary connection | The tab already has the path, and the database reports the error there |
| MySQL list | `information_schema.PROCESSLIST` | `SHOW PROCESSLIST`; `performance_schema.processlist` | `SHOW` refuses `LIMIT`, and `performance_schema` needs a grant |
| Reload key | `R` serves each tab with `filters = false` | Only the sessions tab | A structure tab had only the mouse |

### Known limitations of the plan and the sessions

- MariaDB has no `FORMAT=TREE`, so the error of the server appears. A manual `EXPLAIN` still works.
- `FORMAT=TREE` needs MySQL 8.0.16 or newer.
- On MySQL 8.4, a plan of a write on one table gives only `<not executable by iterator executor>`, as one node.
- On MySQL 8.4, `EXPLAIN ANALYZE` of an `UPDATE` with a `JOIN` kept no change in a test. PostgreSQL keeps the change. The question says that a change can stay.
- A column of a plan can pass the width of the grid. `r` shows the row, and `Enter` shows the value.
- `information_schema.PROCESSLIST` is deprecated on MySQL 8.4. `tests/servers.rs` proves that it answers.
- After `K`, a session can stay in the list for an instant. `R` reads it again.
- A `KILL` can use the session that it ends, if the pool gives that session. The tab shows a closed connection, and the pool opens another.
- Without `pg_read_all_stats` or `PROCESS`, the database hides the query text of other users.
- Without `pg_signal_backend` or `CONNECTION_ADMIN`, the database refuses the action, and the tab shows the error.

## MySQL client key — 2026-11-02

`mysql_async` 0.37 reads client keys in PKCS#1, PKCS#8, and SEC1 form. The user removed the SQLN check that accepted only PKCS#1.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Key form | The driver decides | Keep the SQLN check | The check refused keys that the driver reads |
| Bad key | The connection fails with the error of the driver | A message of SQLN | The form of a key has no SQLN limit to explain |

## Import from a file — 2026-11-02

The user asked for an import of a file into a table, inspired by DataGrip, and approved the 31 decisions below.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Target | An existing table. A new table stays for a second delivery | Existing and new table | A new table needs types per dialect and an editable `CREATE TABLE` |
| 2. Formats | CSV, TSV, JSON, and JSON Lines | CSV and TSV only | User choice: the formats that the export writes, without SQL |
| 3. Failure | The user chooses: All or nothing, By batch, or Log errors | One fixed behavior | User choice |
| 4. Window | A dialog with sections in a sidebar | Steps; one screen with all areas | A terminal of 80 columns cannot show all areas |
| 5. Write | `INSERT` in batches, one path for the three databases. `COPY` and `LOAD DATA` only after a measure asks for them | Native load per database | One path. `LOAD DATA` needs `local_infile` on the server |
| 6. Open | `I` in the navigator and in a table grid, and a palette action. Tables only | Any grid | The import needs a target table |
| 7. Mapping | By name, without a case difference. Without a header, by position. Manual choice per column, with an option to ignore | Manual only | The usual file needs no manual step |
| 8. Existing rows | The import only adds rows | `TRUNCATE`; upsert | Fewer ways to lose data |
| 9. Parser | Own streaming code that knows if a field had quotes | `csv` crate | The export writes `NULL` as an empty field and empty text as `""` |
| 10. `NULL` in CSV | An empty field without quotes is `NULL`, `""` is empty text, with an option for the word `NULL` | Empty text for both | This is the rule of the export |
| 11. Encoding | UTF-8 (a BOM is ignored) and Windows-1252, with no new dependency | UTF-8 only; `encoding_rs` | Excel in Portuguese often saves Windows-1252 |
| 12. Delimiter | The dialog detects it from the extension and the first line, and the user can change it | Manual only | The usual file needs no manual step |
| 13. Binary in CSV | `0x...` in a binary column becomes a binary literal | Text | The export writes binary in this form |
| 14. Safety | No import on a read-only connection, Redis, or a view. Production asks with file, table, and connection | Same question as a run | A file has no statement to show |
| 15. Execution | In the table tab, without `sql_timeout`. Stop undoes All or nothing. In the other modes, the saved batches stay | Background with own connection | Same reason as the export |
| 16. Preview | The first 50 rows, with the current format and mapping | All rows | A file can be large |
| 17. Remember | `preferences.import` in the global `config.json`, without the failure mode | Session only | Same rule as the export |
| 18. End | Shows the inserted rows, the rows with errors, and the log path, and reloads the tab | Status line only | The user needs the path of the log |
| 19. Modes | All or nothing: `BEGIN`, batches, `COMMIT`; the first error runs `ROLLBACK`. By batch: each batch commits alone; the first error stops; the saved batches stay. Log errors: as By batch, but a failed batch runs again row by row, a bad row goes to the log, and the import continues | Savepoints | Each batch commits alone, so a failed batch leaves nothing to undo |
| 20. First mode | Always All or nothing. The mode is not saved | Remember the mode | The other modes leave partial data, so they need a choice each time |
| 21. Place of an error | All or nothing and By batch show the range of the batch and the reason. A format error and Log errors show the exact line | The exact line always | A multi-row `INSERT` does not name the row |
| 22. Log | `<source>-errors-YYYY-MM-DD-HHMM.log` beside the source, with an editable path. Each error has the line, the reason, and the original row | Reason only | The user can correct the rows from the log |
| 23. Error limit | Log errors stops after 1,000 errors, with an editable field | No limit | A wrong mapping would fail on each row |
| 24. Batch | A field with a default of 100 rows, and a limit of 1 MiB per statement | Fixed size | MySQL refuses a statement above `max_allowed_packet` |
| 25. JSON form | JSON is an array of objects. JSON Lines is one object per line. Another form is a row error | Nested paths | The form that the export writes |
| 26. JSON columns | The keys of the first 1,000 objects, in order of appearance. A later key is ignored, with a note at the end | Read the whole file first | A large file would wait for a full read |
| 27. Missing key | `NULL` | Column default | SQLite does not accept `DEFAULT` in `VALUES` |
| 28. Nested value | An object or an array becomes JSON text | Refuse the row | This fits a JSON column |
| 29. Boolean | `true` and `false` in PostgreSQL; `1` and `0` in MySQL and SQLite | Text in all | MySQL refuses `'true'` in a numeric column |
| 30. Binary in JSON | Base64 in a binary column is decoded. Wrong Base64 is a row error | Text | The export writes binary in this form |
| 31. Out of scope | New table, upsert, trim of spaces, `NULL` for a value that does not convert, disabled indexes, several files, a restart from a line, and SQL files | | Each stays out of this delivery |

These choices follow the rules above on the points that the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Options section | The dialog has the sections File, Format, Columns, Options, and Preview | Mode fields in File | Decisions 19 to 24 need a place |
| Literals | The file is untrusted text. PostgreSQL uses `E'...'`, and MySQL uses `_utf8mb4 0x...` for text with a backslash | `quote_literal` | These forms do not change with `standard_conforming_strings` or `NO_BACKSLASH_ESCAPES` |
| NUL byte | A value with a NUL byte is a row error | Remove the byte | The project does not change values |
| Number in JSON | A quoted literal with all digits | A bare number | PostgreSQL refuses a bare number in a `jsonb` or `boolean` column, and rounds it in an `integer` column |
| JSON encoding | Always UTF-8. The encoding field exists only for CSV and TSV | Encoding for each format | JSON files are UTF-8 |
| Log path | `Ctrl+O` and the `...` button open the file explorer, with **Use this folder** | Text only | User request: the log can go to another folder |
| Log file | The file starts at the first error, and a later run in the same minute adds to it | Always create it | A clean run leaves no file |
| Blank line | A blank line is skipped, except in a file of one column, where it is `NULL` | Always skip | The export writes a blank line for `NULL` in one column |
| Close and exit | A close of the tab or of the application during an import acts as Stop | A new question | All or nothing undoes its rows, and the other modes keep the saved batches |
| Other text in a binary column | Its UTF-8 bytes | A text literal | The export writes binary that is valid text as that text. SQLite would keep the type of text, and `bytea` would read each backslash |

### Known limitations of the import

- A MyISAM table cannot undo: All or nothing loses its promise, and Log errors can repeat rows of a failed batch.
- A generated column fails in the database when the mapping includes it. The user must ignore that column.
- In PostgreSQL, an `id` from the file does not move the sequence of the column.
- A second import of a partial file repeats rows in a table without a unique key.
- Valid `0x...` text in a binary column always becomes binary.
- The statements of an import do not enter the history or **Messages**.

## Undo history of the conventional mode — 2026-11-02

The user approved an own history for the conventional mode.
This decision reopens the row "Undo limit" of "Official version of EdTUI", and the rows "Undo" and "Redo" of "Execution and results", only for the conventional mode.
The new fact is the measure: 1.7 GiB in 100 entries of a 50,000-line script. The Vim mode keeps the stack of EdTUI, and its cap stays as technical debt.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| History | One step keeps the replaced text, in `editor::undo`, only in the conventional mode | Fork of EdTUI; keep the debt | The fork blocks publishing. The conventional mode is the default, and SQLN makes all its edits |
| Limit | `undoLimitBytes` per console, 64 MiB by default. Settings > Editor shows it in MiB, and a saved value acts at once | Fixed value | User choice |
| Limit of steps | None. Only the size limits the history | Keep 100 steps | With 100 steps, a larger size gives no more steps of typing |
| Step above the limit | The newest step stays, without a warning. The oldest steps leave | Remove it and warn | An undo of a large deletion stays possible, and that step takes less memory than the same text in the editor |
| Change of keyboard | The history stays. The Vim mode receives the 100 newest steps, as full copies | Clear the history and warn | The behavior of the history stays. A warning needs a new window |
| Redo at a change to Vim | The steps that wait for a redo end | Keep them | It needs a new order between two stacks for a rare case |

## Tree filter, database notices, bell, transaction age, SQL generation, and `JOIN` — 2026-11-01

The user approved seven items of the project analysis.
The production confirmation on Redis reopens decision 6 of "Redis and Valkey" only for a `PROD` connection. The new fact is the `PROD` mark, which came after that decision.

| Item | Point | Decision | Rejected | Reason |
|---|---|---|---|---|
| Filter | Shape | Inside the navigator tree | Search window | User choice |
| Filter | Scope | Compares the relation name and the schema name, only in the in-memory catalogs. Never connects | Compare columns | "Connect on first use" rule. Columns come on demand |
| Filter | Keys | `/` and `Ctrl+F` open. `Up` and `Down` move among relations. `Enter` opens the table. `Tab` goes to the filtered tree. `Esc` in the field cancels and returns the cursor. `Esc` in the tree clears and keeps the cursor, with the parents open | `Enter` only leaves the field | `Enter` opens the first table with one key |
| Filter | After opening | The filter stays, and the filtered tree accepts `i`, `y`, and `g`. `Left` and `Right` do not act on it | Clear on open | Allows opening several tables of the same filter |
| Notices | Moment | At the end of the statement | Live | Less code |
| Notices | MySQL | `SHOW WARNINGS LIMIT 100` automatic when the count goes above 0 | Show only the count | The session cleaning erases the notices between executions, so the user cannot read them later |
| Notices | Limit | 100 notices per statement, and one line with the rest | No limit | A `RAISE NOTICE` in a loop would fill the memory |
| Notices | Visibility | New `Warning` level, `Messages (n)` tab, and the active tab does not change | Open **Messages** | Keeps the "last result" rule |
| Redis | Class | Fixed read list. A name outside the list asks | `COMMAND INFO` before running | The question runs on the interface thread, without network. A false alert only costs one key |
| Redis | Without question | Reads, `SELECT`, `MULTI`, `EXEC`, `DISCARD`, `WATCH`, and `UNWATCH` | Ask on everything | Same role as `BEGIN` and `USE`. Each command of a `MULTI` counts on its own |
| Bell | Shape | BEL, in any situation | Only without focus; OSC 9; off by default | User choice. BEL works in every terminal |
| Bell | When it rings | Normal end or with failure, in console and export. Stop and table page do not ring | Ring always | Whoever used Stop is at the keyboard |
| Bell | Option | `bellAfterSeconds` in `config.json` and in Settings > General. 0 turns off, default 10 | Separate on and off | One field resolves both |
| Transaction | Shape | Whole minutes: `Transaction open 3m`, `Transaction open 1h 05m`. Below 1 min, no age | Seconds | Avoids drawing the screen without need |
| Transaction | Alert | Error color from 5 min, without an option | Option in Settings | YAGNI |
| Transaction | Source | Client clock, since SQLN sees the transaction open. Also applies in `Transaction failed` | Ask the database | No new query |
| Generation | Templates | `SELECT *`, `SELECT` with columns, `INSERT`, `UPDATE`, and `DELETE` | Without `UPDATE` and `DELETE`; clipboard only | User choice |
| Generation | Keys | `g` opens the list. `y` and `Ctrl+C` copy the name without quotes. On a column, `g` uses its table, and `y` copies the column | One key per template | Spends only one key |
| Generation | Text | `SELECT` with `LIMIT 100`. `INSERT` with `VALUES (<a>, <b>)`. `UPDATE` with `SET a = <a>, b = <b>` and `WHERE ;`. `DELETE` with `WHERE ;`. The schema leaves when it is the current one of the console | `?` in the values; `NULL` in the values | `<column>` and an empty `WHERE` are syntax errors in the three databases, so an incomplete text writes nothing. The plan approved `?`, but a test showed that SQLite writes `NULL` into a `?` without a value, and the user chose `<column>` in all dialects |
| Generation | Views | The list shows only the two `SELECT` | All templates | A view rarely accepts writes |
| Generation | Redis | `g` does not act. `y` works | Generate `GET` | The type of the key changes the command |
| Generation | Target | Cursor of the active console, if it belongs to the connection. Otherwise, the first open console of it. Otherwise, a new console. Does not run | Always a new console | It is the history rule |
| `JOIN` | Scope | Only right after `ON` | After `JOIN`; after `AND` | The two tables are already in the text. A reverse read would cost one query per table |
| `JOIN` | Read | The keys come together with the columns, with one extra query. A key failure does not fail the columns | Own method and event | No new authorization path |
| `JOIN` | Composite key | `a = b AND c = d` in one suggestion | One suggestion per column | The condition stays complete |
| `JOIN` | SQLite | A key without a target column stays out | Read the primary key of the target table | Rare case, and the list only stays without that suggestion |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Module of the generation | `src/generate.rs` | `src/sql/generate.rs`, as the plan said | The text uses `quote_name` from the autocomplete, and `sql` does not depend on `complete` |
| `unnest` | `unnest(k.conkey, k.confkey)` without schema | `pg_catalog.unnest` | PostgreSQL only reads two arrays in parallel in the schemaless form |
| Notices on MySQL | A user `SHOW` does not trigger the read | Always read | The count stays in the response, and the read would repeat the notices |

### Known limitations

- The filter does not find a table of a connection with the catalog out of memory. Open the connection first.
- In Redis, the filter reads only the up to 1,000 keys of the tree.
- A progress `RAISE NOTICE` appears only at the end of the statement, and a session closed for lack of response loses the notices.
- After a notice on MySQL, `ROW_COUNT()` in the next statement returns -1.
- In `CALL` on MySQL, only the notices of the last result appear.
- Table tabs and exports ignore the database notices.
- The bell depends on the terminal. A profile with the bell off does not ring.
- The transaction age counts since SQLN sees the transaction, not since the `BEGIN` at the server. In a long script, it starts at the end of the statement that opened the transaction.
- The age appears only in the console tab on screen.
- An `INSERT` or `UPDATE` template on a table without known columns does not open and shows a note.
- A module command that only reads asks in `PROD`, because it is outside the list.

## Official version of EdTUI — 2026-11-01

The user removed the `vendor/edtui` fork, and the project goes back to the published `edtui` 0.11.7.
This decision reopens the item "4. Undo" of "Decision close" and the row "Undo limit" of "Credentials and editor".

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Undo limit | No memory cap. Each stack keeps up to 100 entries | Keep the fork; send the cap to the original project; publish the fork under another name; own history | `cargo install` ignores `[patch]`, and the fork blocked publishing. Sending it to the original project would make the release depend on the maintainer. The own history does not cap the memory in the Vim mode |
| Big entry warning | Removed | Keep the warning | Without a cap, every entry stays in the history |
| Memory risk | Technical debt, described in `ARCHITECTURE.md`, section "Known editor limit" | Block the release | User choice. Common scripts stay far below the worst case |

## Vertical row, JSON, comment, and row count — 2026-11-01

The user approved four items of the project analysis.
The row count does not reopen the pagination rule: a page keeps asking for one extra row, without counting the total. The count is a separate action.

| Item | Point | Decision | Rejected | Reason |
|---|---|---|---|---|
| Row | Place | Window over the grid, with the `r` key | Vertical mode inside the grid | The window already has scroll, copy, and close. The mode would need new design, mouse, search, and selection |
| Row | Navigation | `Left`, `Right`, `h`, and `l`. Stops at the last loaded row, and the grid cursor follows | Read the next page | The window makes no query |
| Row | Long value | Up to 4,096 columns, with `…` | Whole value | A huge value would delay the key, and `Enter` shows the whole value |
| JSON | Shape | Automatic, with `f` to switch. **Format JSON** in Settings defines the initial shape, on by default | Automatic only | User request |
| JSON | Copy | `Ctrl+C` copies the visible shape | Always the original | The user picks the shape with `f`, and the grid copies the original |
| JSON | Engine | Rewrites only the spaces and validates with `IgnoredAny` | `serde_json::Value` | `Value` reorders the keys and loses digits, and the rule forbids changing values |
| JSON | Limits | Input up to 1 MiB, output up to 8 MiB. Above that, the value stays as text | No limit | Each line of a deep JSON repeats the indent, and 200 KB of `[` would generate tens of GB |
| Comment | Keys | `Ctrl+W /`, `Ctrl+/`, and `Ctrl+7` | Only `Ctrl+W /` | User request. It is an exception to criterion 1 of `KEYMAP.md`, and `Ctrl+W /` meets criterion 2. Without enhancement, the terminal sends `Ctrl+/` as `Ctrl+7` |
| Comment | Rule | Without a selection, the cursor line. Blank lines stay the same. All commented lose the comment; in the other cases, the comment enters at the smallest indent | Comment each line at its own indent | It is the IDE habit, and the text keeps its shape |
| Count | Shape | `c` key, with exact `COUNT(*)` and the applied filter | Automatic estimate; automatic `COUNT(*)` | `COUNT(*)` on a big table holds a connection, the estimate errs with a filter, and it does not exist on SQLite |
| Count | Path | The same as a page | New method in the manager | Deadline, Stop, pool, password, and budget apply without new code |
| Count | Validity | **Apply filters** and **Clear filters** clear the total, and sorting keeps it | Clear always | The order does not change the count |
| Count | Available | Only with more rows to load, in a table or view tab | Every tab | Without more rows, the grid already has the total. Redis key and structure have no count SQL |

### Known limitations

- The comment reads lines, not tokens. A line inside a string or a block comment receives the prefix.
- `--x` on MySQL is not a comment, but the removal treats it as a comment.
- The count gets old when another session changes the table, until the next **Apply filters**.
- A JSON column without an object or array shape, such as a number or a string, does not switch shape.

## Command-line target, run protection, formatter, and history — 2026-11-01

The user approved five items of the project analysis.
The history reopens section 17 of the plan, which kept it out of the first version.

| Item | Point | Decision | Rejected | Reason |
|---|---|---|---|---|
| Target | Connection | `sqln file.db` and `sqln postgres://...` last only for the session | Save into `config.json` | A new URI would create a form entry on each use |
| Target | Console text | Stays in memory, and exit warns before losing the text | Save into a script | The script needs a saved connection |
| Target | URI without password | PostgreSQL and MySQL: the terminal asks for the hidden password before the interface. Empty `Enter` goes on without a password. Redis does not ask | Fail with error | The password outside the URI does not go to the shell history |
| Target | Refused password | The error appears in Messages, and the user repeats the command | Ask in the interface | The interface prompt needs a `ConnectionKey` |
| Target | Amount | One per command | Several | YAGNI |
| Target | Startup | No **Create project**. The saved tabs do not open and stay in the file | Keep both | The user already said what to open |
| Target | Missing file | The console waits for the creation dialog | Attach to the first saved connection | The console would change database without a warning |
| `WHERE` | Statements | `UPDATE` and `DELETE` without `WHERE` at the main level | Include `TRUNCATE` and `DROP` | Those two have no `WHERE`; the production mark covers both |
| `WHERE` | Connections | All, except the read-only ones, without an option to turn off | Production only | A `DELETE` without `WHERE` costs a lot on any database |
| `WHERE` | Manual mode | Also warns | Skip | Single rule |
| Production | Effect | `PROD` badge and confirmation before an execution that is not a plain read | Only the badge | The badge alone does not prevent a mistake |
| Production | Without question | Plain read, `BEGIN`, `START`, `COMMIT`, `END`, `ROLLBACK`, `SAVEPOINT`, `RELEASE`, and `USE` | Ask on everything | `SET` asks, because `SET GLOBAL` changes the server |
| Production | Badge | In the status bar, with text and without leaving for lack of width | Tabs and navigator | The text works with `NO_COLOR` |
| Production | Active connection | The mark changes without reconnecting | Require a disconnect | The mark does not change the session |
| Dialog | Shape | One per execution, with focus on **Cancel**, count, and first risky statement | One per statement; type the name | A long script does not become a queue of questions |
| Formatter | Engine | Own, over the project lexer | `sqlformat` | The crate has no MySQL dialect, and the lexer already knows the project dialects |
| Formatter | Safety | Outside a line break, the gap between tokens only shrinks to one space. The check compares all tokens and refuses a difference | Redo the spaces | The lexer keeps `>=` as two tokens |
| Formatter | Strings | Two strings on two lines stay on two lines | Join on the same line | In SQL, joining the two on the same line is an error |
| Formatter | Scope | The selection; without a selection, the statement under the cursor | The whole script | It is the Run rule |
| Formatter | Statements | `SELECT`, `WITH`, `VALUES`, `INSERT`, `UPDATE`, and `DELETE` | All | Routines have their own body, and the risk does not pay |
| Formatter | Case | Keeps the typed text | Uppercase | Follows the autocomplete decision |
| Formatter | List | The first item stays on the clause line | Break after the clause | Needs no lookahead, and a one-item list stays on one line |
| History | File | `history.jsonl` in the global folder, permission `0600`, on by default | Per project; memory only | Old projects do not have that line in the `.gitignore` |
| History | Content | Each console statement that reached the database, with time, connection, and success or failure | Time and rows | YAGNI |
| History | Outside | Tables, catalog, export, and connections without a form entry | Record everything | Only the user text matters |
| History | Secrets | Does not record `PASSWORD`, `PASSWD`, `IDENTIFIED`, `REQUIREPASS`, and `MASTERAUTH`, nor Redis `AUTH`, `ACL`, and `HELLO` | Record everything | Follows the `mysql` client rule |
| History | Limits | File up to 8 MiB; at startup, a larger file keeps the newest 4 MiB. A statement above 64 KiB and a back-to-back repetition do not enter | No limit | The file grows without limit and the window reads everything |
| History | Write | One write per frame, with the file opened again | File open all the time | `trim` replaces the file, and another instance would keep writing to the old file |
| History | Window | `Ctrl+W H` and palette. Lists the console connection, with a word filter | All connections | The SQL of another database rarely serves |
| History | `Enter` | Inserts at the cursor and does not run | Run | The user reviews first |

### Known limitations

- A password in the URI stays in the shell history and in `ps`. Omit the password, and the terminal asks for it.
- A command-line connection does not react to a refused password. Repeat the command.
- `DELETE t FROM t JOIN u ON ...` on MySQL asks, even with the `JOIN` limiting the rows.
- A `WHERE 1 = 1` satisfies the protection.
- `SELECT my_function()` asks in production, because the text does not prove that the function only reads.
- In Redis, the production question uses a fixed read list, so a module command that only reads also asks. See the 2026-11-01 section about the tree filter and the database notices.
- The secret filter reads words. A secret inside a literal, such as `WHERE token = 'abc'`, enters the file.
- A column named `password` keeps the query out of the history.
- A statement written by another instance during `trim` can get lost.
- An unreserved word on MySQL, such as an `offset` column, can start a line. The SQL stays valid.
- `CREATE`, `ALTER`, and routines have no formatting.

## Hint line — 2026-10-31

The user asked that the footer always show the most relevant commands for the moment.
An audit found useful commands outside the line and commands without effect inside it.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Rule | Each state lists the commands that act on it, in relevance order | Fixed list per panel | A key without effect spends space, and a useful key stays hidden |
| Space | A command that does not fit leaves whole, and a short command after it still enters | Cut the line at the border; require each state to fit in 80 columns | The order decides what a small screen shows, and no key stays cut. This replaces the rule of closing the navigator line at 80 columns |
| Stop | First command in every busy tab | Only in the editor | A long read holds a session, and the focus may be in the grid |
| Transaction | `commit` and `rollback` come first in the editor and the results | Only on the buttons | The transaction holds a session until the user decides |
| Navigator | The commands change with the cursor row. The connection commands stay on the connection line | Same list on every line | On a table, `Del` would remove the whole connection, and that hint does not help there. The keys keep acting, and `F1` lists all |
| Without connection | The editor swaps `run` and `script` for `connection` and `navigator` | Keep `run` | Without a connection, `F5` only shows a warning |
| Window order | The hint choice follows the `on_key` order | Keep its own order | The line needs to name the keys of the window that receives the keys |
| Navigator labels (2026-11-01) | Each command names its own object, such as `n new console` and `Del remove connection`. The console commands come before the connection commands | Group titles, such as `Console:` and `Connection:`; keep `new`, `add`, `edit`, and `drop` | The user could not see whether a command acted on the console or the connection. At 80 columns, a SQLite connection loses `a`, `Del`, `Right`, and `F1`. The keys keep acting |

## Structure tab — 2026-10-31

The user asked for a way to see the schema of a table and of a database, and reopened the rule that excluded the structure tab from the first version.
The navigator showed only the column names.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Place | Own tab in the main strip, with a read-only grid | Switch inside the table tab; generated console; types in the navigator | Reuses search, copy, export, stop, and pool. Does not mix filter and pagination with the structure |
| 2. Table content | Columns (type, null, default, primary key), indexes, and foreign keys | Columns only; include the DDL | PostgreSQL has no `SHOW CREATE TABLE`, and a text assembled by the tool could differ from the real one |
| 3. Schema or database | All the columns, one row per column | Only the relations; nothing | The grid search finds a column in any table |
| 4. Redis and Valkey | Refuse with a warning in the status bar | Show type, TTL, and memory of the key | A key has no columns |
| 5. Keys | `i` opens the structure. `[` and `]` switch the section | Own key for the section | `[` and `]` already switch results in the console |
| 6. Restart | The structure tab does not come back in the next session | Save into `OpenTabs` | The cost of opening is one key |
| 7. Refresh | **Reload** and the section switch reread the grid. DDL does not update the tab on its own | Keep the three sections | The read is short, and without keeping anything the grid never gets old without a warning |
| 8. Many rows | The grid uses the pagination and the budget of the table | Read everything at once | A big database can have many columns |
| 9. Export | Selection, loaded rows, and **All rows** work, without a target table | Block the export | The grid is not a table, so SQL INSERT asks for the name, as in the console |
| 10. Connection row | `i` shows all the schemas the navigator shows | Only the current schema | Meets the request to see the whole database |
| 11. Column row | `i` opens the structure of the table of that column | Do nothing | The column belongs to a table |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Tab | `TableTab` with `structure` and `filters = false` | New tab type | The table tab already has deadline, stop, pool queue, budget, password, and pages |
| Read | SQL through `Session::run`, like a page | New method in `Session` | No connector changes, and the read accepts stop and deadline |
| Name in literal | PostgreSQL uses `E'...'` | `quote_literal` on the three databases | With `standard_conforming_strings = off`, a hostile catalog name could close a common literal |
| Hint line | User request: `i struct` appears in the navigator and in the grid of a table | Leave `i` only in `F1` and the palette | The user did not find the key. See the **Hint line** section |

### Known limitations of the structure

- An expression index shows `expression` on MySQL and SQLite. PostgreSQL shows the expression.
- A SQLite foreign key has no name, so the `name` column shows the key number.
- With `NO_BACKSLASH_ESCAPES` on MySQL, a name with a backslash does not find the table and the grid stays empty. The name stays inside the literal.

## File explorer — 2026-10-31

**Defined:** a `...` button beside the path fields opens an explorer, and the user approves the 10 decisions below.
The field stays editable. The explorer only helps to fill it.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Type | Own window in the terminal, without a new dependency | Native dialog with `rfd`; only complete the path | The native dialog fails on SSH and on a Linux without a portal, and takes the focus from the terminal. Completing does not show the folders |
| 2. Open by keyboard | `Ctrl+O` in the field. The hint bar shows the key, the `...` serves the mouse, and the `Tab` order does not change | The `...` as a `Tab` stop | The stop adds up to 3 steps in the network form |
| 3. Fields | File of Export, File of SQLite, CA, client certificate, and client key | Only File of Export and SQLite | The cost per field is small, and the screen stays coherent |
| 4. New file | **Use this folder** row at the top, only in Export and SQLite. It changes the folder and keeps the name of the field | Name field and buttons in the window; only existing files | One mode only. With an empty field, it receives the folder, and the user types the name |
| 5. Keys | As in the palette: letters filter and arrows move. `←` and `Backspace` with an empty filter go up one folder. `→` and `Enter` enter. Valid in both key maps | `j k h l` and `/` in the vim mode | Repeats an app pattern and avoids two key modes |
| 6. Files shown | All | Filter by extension | A SQLite database accepts any extension, and the typed filter resolves the rest |
| 7. Hidden | Hidden. A filter that starts with `.` shows all | Always visible; a key toggles | Does not use a new key. `Ctrl+H` does not serve, because many terminals send it as `Backspace` |
| 8. Mouse | Click moves the cursor, double click opens, and the wheel scrolls | Single click opens | With single click, the double click out of habit enters the folder and activates another row |
| 9. Start folder | Folder of the field path. Without it, the closest existing folder. Empty field: current folder | Always the current folder; remember the last folder | Export already starts in the saved folder, and the field already carries that folder |
| 10. Home | `~` with an empty filter goes to the home folder | Without a shortcut | One key, and the File field of Export already accepts `~` |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Path in the field | Always absolute | `~/...` | The SQLite field does not expand `~` |
| Export extension | **Use this folder** keeps the extension tied to the format. Picking a file unties it | Always untie | The folder change does not change the name |
| Replace | The window stays on the **Export** button | Ask in the explorer | The field can still change before the Export |
| Filter with text | Hides `../` and **Use this folder** | Keep the fixed rows | `Enter` acts on the first result |
| Path the field would change | A note refuses the path with a control character, with spaces at the ends, or outside UTF-8 | Fill anyway | The field would keep another path, without a warning |
| Folder link | Appears as a folder, and `../` returns to the source folder | Show the real target | The user goes back to where they were |
| Out of scope | Create folder, rename, delete, size, date, and favorites |  | Export already creates the missing folders |
| Modal sizes | User request: Export, the connection form, and the explorer grow to 140 columns and leave 8 free. The explorer stays 8 columns narrower than the modal, and its list goes from 14 to 35 rows | Fixed size; grow without limit | A long path appears whole, and a limit keeps the label near the value |

The user also approved the growth of the other modals. Each modal grows only with free space, leaves 8 columns free, and never gets smaller than before.

| Modal | Approved decision | Rejected | Reason |
|---|---|---|---|
| Settings | Up to 140 columns. The height does not change | Up to 100 columns | Settings and Export have the same layout |
| Command palette | Up to 100 columns and 30 rows. Stays on top | Only more rows | Shows more commands without scrolling |
| Schema, console, script, and connection lists | Up to 100 columns and 30 rows. The height follows the number of items | Fixed size | A long script name was cut |
| Script conflicts | Up to 100 columns and 30 rows. The name receives the new columns | Fixed size | The name identifies the script |
| Rename and password | Up to 100 columns. The height does not change | Fixed size | The connection destination and the name get more space |
| Questions | Up to 100 columns | Limit of 72 columns | A long path breaks into fewer lines |
| Cell detail and help | Up to 140 columns | Limit of 90 columns | A long value shows more text on each line |

The `Ctrl+W` menu does not grow, because it is not modal and stays pinned to the screen corner.

### Known limitations of the explorer

- The folder read stays on the interface thread, like `is_dir` and `exists` in `destination_of`. A slow network disk can freeze the screen during the read.

## Export to file — 2026-10-31

**Defined:** the user asks for the export of results through a modal and approves the 25 decisions below.
This request reopens the rules "Export to file" and "Extractors beyond the default TSV" of the list of items outside the first version. The extractor editor stays outside.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Range | The modal offers Selection, Loaded rows, and All rows. All rows rereads the database, without the grid and without the 256 MiB cap | Only loaded rows; only all rows | The grid keeps 1,000 rows by default, and an incomplete file without a request is the worst export error |
| 2. Reread in the console | Only with `is_plain_read`. Another command stays with Loaded rows only, and the modal explains the reason | Any command with a warning; never in the console | Rereading repeats the command, and a command that writes would repeat the write |
| 3. Formats | CSV, TSV, JSON, JSON Lines, and SQL INSERT | XLSX | XLSX brings a new dependency |
| 4. Destination | File only; `Ctrl+C` does not change | File and clipboard | One flow only |
| 5. Execution | In the tab task, on the same session. The modal closes, the tab becomes busy, the status line shows the written rows, Stop cancels, and the grid keeps the result | Background with its own connection | An open transaction sees the same data, and the tab does not take another connection |
| 6. Time | All rows ignores `sql_timeout`; Stop is the exit | Same `sql_timeout` | The 30-second default would end a big export |
| 7. Partial | Writes to `<name>.tmp`. On Stop or error, a window asks: keep (renames to the target) or erase | Always erase; always keep | It is a user choice in each case |
| 8. Existing file | Window with Replace and Cancel; Cancel returns to the modal | Refuse; create another name | No file changes without confirmation |
| 9. CSV and TSV options | Header (on), delimiter only in CSV (`,`, `;`, `\|`), NULL text (empty or `NULL`), and UTF-8 BOM (off) | Fewer options | Excel in Portuguese expects `;`, and without a BOM it breaks the accents |
| 10. Binary | CSV and TSV follow the grid rule. JSON uses Base64 on every `Value::Binary`. SQL uses the dialect literal | Grid rule in JSON too | Base64 is the habit in JSON |
| 11. Repeated names in JSON | Numeric suffix: `id` and `id_2` | Refuse the JSON; repeat the key | No value gets lost, and the JSON stays valid |
| 12. Group INSERT | The modal shows the choice: one per row (default) or grouped | Without a choice | It is a user choice in each case |
| 13. INSERT batch | Field only with grouped, default 100 | Fixed at 100 | MySQL refuses a command above `max_allowed_packet` |
| 14. Numbers in JSON | Number with the database text. `NaN` and `Infinity` become strings | Always string; option in the modal | The file keeps all digits |
| 15. Scripts | All rows only with a command in the execution or with an open transaction | Allow with a warning; reread the whole script | The session is cleaned between executions, so rereading only the `SELECT` after a `SET` may read another table without an error |
| 16. Redis and Valkey | Only Selection and Loaded rows, without SQL INSERT | Everything in the key tabs | There is no `is_plain_read` for Redis, and the key tab reads by pages |
| 17. Open the modal | `e` key in the grid and action in the palette | Also in the navigator; palette only | The key stays near the grid, and `e` is free in both key maps |
| 18. Start path | Saved folder or current folder, with `<source>-YYYY-MM-DD-HHMM.<ext>`. The extension follows the format until the user edits the path | No date; Downloads folder | The existing-file window almost never appears |
| 19. Remember choices | Format, options, and folder persist | Session only; do not remember | Whoever uses `;` does not choose again |
| 20. Several results | Only the active result; one modal generates one file | One file per result tab | It would multiply the existing-file and partial windows |
| 21. Navigator | Out of scope | Key in the navigator table | The user withdrew the request |
| 22. Where to save | `preferences.export` in the global `config.json`, when Export confirms | Active scope; `workspace.json` | The `config.json` of a project is versioned, and the folder would be a path of one machine only |
| 23. Layout | Sidebar with the formats, like the Settings dialog | One-column form | Shows all the formats at once, and the mouse picks with one click |
| 24. Start range | Selection, when it exists. Otherwise All rows, when available. Otherwise Loaded rows | Selection or Loaded rows | Avoids an incomplete file by mistake |
| 25. Simultaneous exports | One per tab. The modal refuses a destination in use | One at a time | Each tab has its task, and two equal destinations would share the same temporary file |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Table filter | All rows uses the applied filter and order | The typed text | The grid shows what was applied |
| `Enter` in the modal | Only the **Export** button exports. In another field, `Enter` goes to the next, as in the Settings dialog | `Enter` exports from any field | User request: avoids an export by accident |
| Hidden All rows | When the grid already has everything, only Loaded rows appears | Reread anyway | Avoids a query with no gain |
| When to open | Only with the tab free and an active result | Open on the **Messages** tab | Without a result there is nothing to write |
| Zero rows | Generates a file: header only, `[]`, or empty | Refuse | The empty result is also an answer |
| JSON | One object per line inside the array, without indenting fields; the output is UTF-8 | Full indentation | The file stays small and readable |
| File field | Accepts `~`, creates the missing folders, and refuses a folder as target | Require an existing folder | It is what a terminal user expects |
| Source name | Replaces `/`, `\`, `:`, and control characters with `-`; the time is local | UTC time | The name needs to be valid and readable |
| Without written rows | Stop or error before the first row erases the temporary file, without a window | Show the window | There is nothing to keep |
| **Keep** | Closes the format, so the partial JSON stays valid; the window warns when it replaces an old file | Leave the file open | The partial stays readable |
| Close during the export | Uses the current execution confirmation, and the temporary file is erased | Partial window | The tab disappears, and the window would have no context |
| Permission | System default | `0600` | The file belongs to the user, like any program file |
| Failure to save the choices | Shows a note, and the export continues | Cancel | The export is the main request |
| `preferences.export` in the project | Configuration error | Ignore | The field exists only in the global scope |
| Progress | The status line reads a counter of the export | One event per batch | The channel has 256 slots and the screen reads a limited number of events per frame, so the events could delay the write |
| Local time | `chrono` with the `clock` feature | `time`; `libc` | `time` refuses the local time zone in a multi-threaded process, and the project has no `unsafe` |

### Known limitations of the export

- A JSON column of the database becomes a string, because `Value` does not keep the column type.
- `is_plain_read` refuses an unknown function. `SELECT my_function(x)` stays with Loaded rows.
- All rows reads the data of now, which can differ from the grid.
- The CSV does not neutralize formulas such as `=SUM(...)`, because the project does not change values.
- The final rename of the temporary file does not check the target again.

## Redis and Valkey — 2026-10-31

**Defined:** the user asks for Redis and Valkey support and approves the 11 recommendations below.
This request reopens the rule "MongoDB, Redis, and Kafka stay out of the first version" only for Redis and Valkey.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Console input | One command per line, as in `redis-cli`; `#` at the start of the line is a comment | `;` ends the command | The `redis-cli` text works without change |
| 2. Explorer | Databases and up to 1,000 keys per database, with `SCAN`; `Enter` opens the value in a tab | Only databases with a paginated list; only databases | Allows browsing without writing commands |
| 3. Read only | Ask `COMMAND INFO` and refuse commands without the `readonly` mark; without a server response, refuse | Fixed list; remove the option | Covers Valkey and modules, and the server has no read-only session |
| 4. Transactions | No manual mode; `MULTI` and `EXEC` work in a script | Tie the manual mode to `MULTI` and `EXEC` | Inside `MULTI`, a read answers `QUEUED`, without data |
| 5. Continuous commands | Refuse `SUBSCRIBE`, `PSUBSCRIBE`, `SSUBSCRIBE`, and `MONITOR` (replaced by "Continuous commands — 2026-11-09") | Continuous flow in the grid | The console has no continuous flow |
| 6. Destructive commands | Without confirmation | Ask for confirmation | `DROP` also does not ask for confirmation |
| 7. TLS of `redis://` | SQLN rule: local without TLS, remote verifies; `?sslmode=disable` turns it off; `rediss://` always uses TLS | `redis://` never uses TLS | The password only travels without TLS by explicit choice |
| 8. Topology | Single server only | Cluster; Cluster and Sentinel | Cluster and Sentinel change the pool |
| 9. Result in the grid | Value in one cell, list in rows, map in the `field` and `value` columns, nested value as text | Everything as text | Uses the grid; needs RESP3 |
| 10. Autocomplete | Command names, with a fixed list | Also key names | The key list can be partial |
| 11. Connection type | One "Redis/Valkey" type, with `redis://`, `rediss://`, `valkey://`, and `valkeys://` | Two types | The protocol is the same |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Driver | `redis` 1.8, with `MultiplexedConnection` on its own stream | Driver TLS | The driver TLS does not use the `connector::tls` roots; the own stream keeps one TLS rule for all sources |
| Database in the form | Number, validated in the form, in the URI, and in `config.json` | Free text | Redis has no database names |
| User | Optional only for Redis | Require `default` | A server without ACL has no user |
| Identity commands | Refuse `AUTH`, `HELLO`, and `RESET` | Allow | The session returns to the pool, and another console would receive the new login |
| Replication | Refuse `SYNC` and `PSYNC` with the continuous commands (replaced by "Continuous commands — 2026-11-09") | Allow | Those commands also do not end |
| Tests | `tests/redis.rs`, separate from `tests/servers.rs` | Add to `tests/servers.rs` | That file repeats SQL on each server |

### Known limitations of Redis and Valkey

| Subject | Limitation |
|---|---|
| Read only | The protection stays only in the client. `PING`, `INFO`, `ECHO`, `TIME`, `SELECT`, `CONFIG`, `CLIENT`, `OBJECT`, `MEMORY`, and `XINFO` stay refused, because the server does not mark those names as `readonly`. The database selector keeps working |
| Stop | Only for a command that waits, such as `BLPOP`. In another command, SQLN closes the connection, and the server ends the command |
| Slow command | A long `KEYS *` holds the server, so the stop request may wait 5 seconds without a response |
| Cap and budget | The driver reads the whole response before the row cap and the grid budget. A huge `LRANGE k 0 -1` uses memory before the cut |
| Explorer | Shows up to 1,000 keys in the `SCAN` arrival order, sorted by name. Empty databases do not appear, except the current database and the databases of the list of the connection (2026-11-04). A key that is not UTF-8 does not appear and marks the list as partial. A command does not update the tree; use **Refresh** |
| Value pages | `HSCAN`, `SSCAN`, and `XRANGE` have no offset, so each page repeats the previous read, with the SQL `OFFSET` cost. A hash or set that changes can repeat or skip items |
| Module types | A module type, such as JSON, does not open in a tab. Use the console |
| `MULTI` | Valid only inside the same execution. Running `MULTI` alone discards the queue at the end of the execution |
| Topology | No Cluster and no Sentinel. A `MOVED` response appears as an error |
| Version | RESP3 needs Redis 6 or newer. An older server fails to connect |
| Autocomplete | The command list is fixed, without module commands and without keys |
| Command line | A value does not take two lines. Use `\n` in double quotes |

### Pending items of Redis and Valkey

- Validate in a real terminal: form, explorer, value tab, and vault.
- Test a Unix socket with a real server. Docker on macOS does not share that socket, and the code has only a build test.
- Test the Stop of a command that does not wait. A long Lua script would hold the other tests on the same server.
- Test a server with a restricted ACL, without `CLIENT ID` or without `COMMAND INFO`.
- Test managed services, which can rename commands.

## nvim-style interface — 2026-10-31

**Defined:** the user asks for a prettier interface, close to nvim and an IDE.
The user approves these four points before the implementation.
The code already implements the four points.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Colors | `astrodark` theme in RGB with truecolor; ANSI theme in the other cases; `NO_COLOR` without color | ANSI only | Own backgrounds and tones separate the panels; the palette comes from the nvim theme of the user. This decision replaces "use the ANSI colors of the terminal" |
| Layout | No boxes: tabs over the whole width, navigator, panel title, status bar, and hint line | Box with border on each panel | Each box spends two rows and two columns, and 80 x 24 is the minimum |
| Icons | Unicode by default; `icons: "nerd"` in `config.json` | Nerd Font only | The Monaco font of the user does not have those symbols |
| Connection icon | With `icons: "nerd"`, the database logo comes before the name, unless the connection has a chosen icon. In the Unicode set, `⛁` comes before the name and `pg`, `my`, `sq`, or `rd` after. `NO_COLOR` has no icon | Emoji, which takes 2 columns and is not a logo; image in the terminal, unreadable at 1 or 2 cells | Unicode does not have those logos. In iTerm, the Nerd Font Bundle shows the logos and keeps Monaco in the text |
| Themes | Color schemes as data in `ui/schemes`, picked in `Settings > Appearance`, with a preview before saving. Details in "Themes — 2026-10-31" | One style function per theme | Each new theme becomes a color table, and the tests check the contrast |
| Features | `Ctrl+W` menu, command palette, welcome screen, scroll bars, current line, and row number in the grid | Visual changes only | The user also asks for a more intuitive interface |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Page buttons in the table | **Previous** and **Next** move the cursor in the accumulated rows; **Next** loads the missing page | Replace the rows on each page, with a new query to go back | Keeps the rule of keeping the previous rows, the budget, and the query per page; going back does not repeat the query |
| Page buttons in the console | Do not offer | New query per page | The more-rows decision in the console applies: repeating the query changes the SQL and can show another result |
| Key that accepts a suggestion | `suggestionKeys` option: `tab` (default), `enter`, or `both`, in the editor and the table filter | Only `Tab`, as in the original plan | Reopened by the user on 2026-10-31; the default keeps the old rule, and `Enter` accepts only by explicit choice |
| Time precision | Three decimal places during the execution and in the last time, with an update every 100 ms | Whole seconds; 250 ms | User request; at 250 ms the last digits stay almost still |
| Execution state in the tabs | Badge with a colored background at the start of the tab, with the time during the execution | Symbol at the end of the tab, beside the name | At the end, `✗` would sit beside the closing `×`; a color block is hard to miss |
| End of the result badge | The badge leaves when the user leaves the tab, or with the next execution | Keep until the next execution; leave when entering the tab | A tab with a badge means a result the user has not seen yet |
| Start of the time | When the execution starts, without the password wait | From the run key | The time measures the database, not the password typing |
| Tabs of the last session | Reopen at startup, with the `reopenTabs` option and default on | Manual list only, as in the original plan; ask at startup | Reopened by the user on 2026-10-31: the text is already saved, and the reopening runs no SQL |
| Table of the last session | Load on the first selection; a tab that starts on screen waits for `Enter` or **Apply filters** | Load at startup | The connect-on-first-use rule stays: the opening does not connect and does not ask for a password |
| Tab list after a failure | Invalid configuration or a locked console keeps the file list | Write the list of the current session | A session that could not reopen the tabs cannot erase that list |
| Close tab with the mouse | `×` at the end of each tab, with the questions of `Ctrl+W c` | Close without questions; show `×` only on the active tab | A click cannot discard a transaction or a text; the `×` on each tab closes another tab without changing the screen |
| Hint line commands | Key with the menu key style, effect with the hint color, and two spaces between the commands | Separate with ` · ` | The color marks the start of each command; ` · ` passes 80 columns in the navigator, and two spaces fit in 80 |
| Message duration | 8 s; a key, a paste, or a click removes it before | Remove only with a key; the pointer and the wheel also remove | A message without a deadline stays frozen for a mouse user; moving the pointer is not an answer to the message |
| Credential wait | The message comes from the state of the attempt | Use the same deadline of the other messages | A long vault wait stays active after 8 s, and the end of the wait must remove the message |
| Height | Line under the tabs and panel title only with 30 lines or more | Always show | At 80 x 24, the editor and the grid keep the 18 rows of before |
| Theme without color | Keeps `[Run]`, without icons and without `○` | Same symbols as the other themes | Without color, the bracket identifies a button |
| ANSI theme | No background on the base, the panel, and the current line | Black background | The terminal profile can be light |
| Editor numbers | Own drawing, with `LineNumbers::None` in EdTUI | Change the fork | The fork must stay small until the release |
| Second palette key | `Ctrl+W :` | Only `Ctrl+P` | Rule 2 of `KEYMAP.md` asks for another path, and `Ctrl+P` is not in the `keytest` record |
| Keys in the palette | A navigator key appears only with the navigator in focus | Always show | Outside the navigator, that letter becomes text |
| Key label | `Ctrl+P`, with the uppercase letter | `Ctrl+p` | A lone letter keeps the case, because `n` and `N` are different keys |
| Connection form | Groups with four arrangements, from the most spaced to the compact, according to the screen. Reopened on 2026-11-03: a sidebar with sections | Scrolling inside the window | All the fields stay visible, and the 80 x 24 test stays valid |
| Form focus | Mark, highlighted label, and own background | Cursor only | The user could not see the selected field |
| SQLite file in the form | The typed text starts from the current directory, and the connection saves the full path; a saved and unchanged path stays as it is | Resolve the typed text by scope; convert a saved path too | The user asks for the path of the test. The test used the current directory, and the saved connection opened another file. Editing another field does not change a hand-written relative path |
| SQLite file in a project | Write the full path also into the project `config.json` | Write relative to the project root | It is the user request. Consequence: that path is valid only on this machine; a hand-written relative path stays valid |

## Decision close — 2026-10-31

**Defined:** the user accepts all 12 recommendations below.
The approval closes the decisions, but does not declare the changes as implemented.
Section 1 of the plan separates the current code from the missing adjustments.

| Point | Approved decision | Reference |
|---|---|---|
| 1. Rename consoles | `Ctrl+W N` and action in the list; refuse an empty or repeated name in the connection; keep identity and files | Plan, section 12 |
| 2. Change connection | `Ctrl+W a`, confirmation, previous save, text preservation, and result clearing; do not run or convert SQL | Plan, section 12 |
| 3. CTEs | Suggest identifiable names and columns of `WITH`, with aliases and scope; do not query the database to find them | Plan, section 6 |
| 4. Undo | Up to 100 entries and 64 MiB per console; remove the oldest and warn about a single entry above the budget | Plan, section 5 |
| 5. Hints and help | Show the effective `keymap` keys and only available actions, including in the prefix | Plan, section 5 |
| 6. Settings | `Ctrl+W ,`, scopes, inherited values, and the five fields of the sketch; warn before losing selection or history on keyboard change | Plan, section 11 |
| 7. Files | Approve the existing saving and conflicts; use **Close and discard text** for the removed file | Plan, section 12 |
| 8. Lists | Approve the console and scripts-without-connection lists, delete confirmation, and attach without execution | Plan, section 12 |
| 9. Configuration and state | Approve reload, lock on error, and scopes; warn without blocking when `workspace.json` is unreadable | Plan, sections 11 and 12 |
| 10. Tables | Approve opening, labels, sorting, and pages; mix tabs in the opening order and do not create a console when closing the last one with a table open | Plan, section 10 |
| 11. Credentials | Initial session-only use, explicit storage, confirmed recovery and migration; trust with focus on Cancel | Plan, section 13 |
| 12. Vault | `keyring-core`, Keychain, and Secret Service; unknown failure is an error, never an absence; validate on real services | Plan, section 13; architecture, section 9 |

Real terminal, mouse, network disk, certificates, and network failures stay as pending validations.

## State of the credential delivery

The code implements the credential phase, with authorized plain text, migration, refused password, and separate deletion.
Keychain and Secret Service in a container are validated; the Secret Service still needs a repeat on a real Linux desktop.
The [vault validation](INITIAL_PLAN.md#validation-of-the-real-vaults) records the native checks and pending desktop validation.
The tests with an in-memory vault and real databases do not replace that native validation.
The missing-vault criterion comes from the validation: only responses confirmed on Linux, never on macOS.

## Credentials and editor — 2026-10-31

**Defined:** the user approves these points after reading the code.
The approval closes the decisions, but does not declare the changes as implemented.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Keychain | Validate on this machine, with the user present | Postpone | Plain text depends on that validation |
| Secret Service | Linux container now and a real desktop before the release | Real desktop only | The container covers missing service, lock, and common operations, but not a desktop session |
| Plain-text password | `credentials.json` in the global folder, with `0600`, per identity, for both scopes | `password` in the global `config.json` and a section for project connections | `config.json` stays without secrets, and both scopes use one format |
| End of the suspension | Each explicit action opens the password prompt; an accepted password or the connection edit ends the suspension | Only **Disconnect** or edit | Executions and catalog stop insisting, without an extra step for the user |
| Undo limit | Small fork of `edtui` through `[patch]`; at release, send to the original project or publish under another name | Copy the code into `src/`; own history | The change stays in `Stack`; the copy brings 10,169 third-party lines, and the own history is fragile |

## Execution and results

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Result in the console | Read up to a cap and end the read | Open read with pause | A pause ended the connection on MySQL and holds resources |
| More rows in the console | The user raises the cap and runs again | New query per page | Repeats commands, changes the SQL, and can show other data |
| Default cap | 1,000 rows | 500 rows | lazysql value; Harlequin and DataGrip use 500 |
| End of the read at the cap | According to the command | Always cancel or always consume | Cancel can undo a change |
| Table view | New query per page | Open read between pages | Frees the connection; the unstable-order risk is accepted |
| Next page | Ask for one extra row | `COUNT` | Avoids cost at the database |
| Tab reading | Block the close | Confirm or discard | Fewer states |
| Table tab | Console task without an editor, with one page per `Run` | Own task for the pages | Reuses deadline, stop, cancellation, pool queue, and budget |
| Extra row of the page | Sink cap at `n+1`, with `LIMIT n+1` in the SQL | Cap at `n`, with the sink requesting the cancellation on the extra row | The cancellation opens a new connection in PostgreSQL and MySQL on each page |
| Table query clauses | One per line | Text on one line | A `--` comment in the filter would remove the `LIMIT` |
| Load more with a full budget | Refuse until some data leaves the budget; a value bigger than the budget refuses always | Try the query and fail again | Avoids a query that cannot finish |
| Table tab closed while reading | Stop the query and close without a question | Block like a console result | The query has no transaction, so stopping loses no data |
| Grid values | Database text; type only in presentation | Decode types in Rust | Keeps precision and accepts unknown types |
| End of the read on SQLite | End at the client with `readonly`; consume in the other cases | Always consume | An infinite result would end only by the deadline |
| Tab after the execution | Last result; **Messages** on any other end | Always the last result | A stop would stay invisible behind an empty grid |
| Grid cursor | Counts as a one-cell selection for `Ctrl+C` | Require a selection | The grid always has a current cell, as in DataGrip |
| Function in a query at the cap | Consume the rows | Cancel every query with `SELECT` | The cancellation undoes `SELECT writes(id)` silently; consuming costs only time |
| Grid search | 8 ms slices between keys, with 1,000,000 results in the list | List without a limit or read in one key | 2 million rows take 0.6 s; a short text would fill the memory |
| Manual width | `<`, `>`, `=`, and drag on the header | Mouse only | Every essential action has a keyboard path |
| Close result | `x` or `Delete` in the grid | `Ctrl+W c` | `Ctrl+W c` closes the console, and that difference avoids a loss by mistake |
| Stop a statement | Cancel and close as the last resort | Only close the connection | With the connection closed, an `INSERT` finished on both databases |
| Cancellation deadline | 5 seconds | Without a deadline | DataGrip offers a similar escalation |
| Cancellation connection | Temporary and outside the pool limit | Respect the limit literally | The protocol needs another connection |

## Files and saving

These choices are approved on 2026-10-31, with the adjustments recorded in the plan.
The code offers session, vault, and the authorized global plain-text exception, in `credentials.json`.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| JSON library | `serde` and `serde_json`, with `deny_unknown_fields` | Manual reading | An unknown field invalidates the file, as the plan requires; the error position comes from the reader |
| Identity of a console | UUID `v4` in the file names | Counter | Several computers create consoles of a project in Git without collision |
| File lock | `File::try_lock` of the standard library, in `locks/<id>.lock` | `fs2`, `fd-lock` | The plan asks for the system lock, and the standard library offers it without a new dependency |
| File hashes | `DefaultHasher` | `sha2` | The hash only compares contents inside one process, and never goes to a file |
| Change detection | Check each second, by size and date, and then hash | `notify` | The plan allows watching the folder or comparing the hash; the check is portable and needs no dependency |
| File creation | At the first edit of the console | At console open | An empty console leaves no litter in a repository |
| Configuration error | Blocks the connections, and `Ctrl+W r` reloads | Load only the valid scope | The plan forbids the partial scope |
| External change of the configuration | Question, with **Ignore** as initial focus | Automatic reload | The plan asks for the question, and the initial focus changes nothing |
| Connection that leaves the file | Leaves the application only when it is idle | Close the session | The plan forbids the reload from changing active connections |
| Password in the current code | Session or vault, with a `session`, `none`, or `vault` reference; the secret does not go to the file | Write the secret into the project | The authorized global exception still awaits implementation and real vault validation |
| Close order | Writes the text, then closes | Close and write | The write that fails keeps the console open |
| Layout | `workspace.json` of the project, or of the global scope without a project | One file per scope | The layout belongs to the session, not to the connection |
| Attach between scopes | Writes the pending text, copies, checks, and removes the originals | Move with `rename` | The destination folder can be on another disk |

## Table view

These choices are approved on 2026-10-31.
Current state: tabs follow opening order. Closing the last console selects a remaining tab without creating a replacement console.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Open a table | `Enter` and double click; `Right` opens the columns | `Enter` opens the columns | `docs/KEYMAP.md` already planned the `Enter` change |
| Tab order | Consoles and tables together in the opening order | Separate by type, as in the current code | Keeps the user opening sequence |
| Tab label | `name (table)` or `name (view)` | Name only | The plan asks for a label that identifies the view |
| Header click | Ascending, descending, and no order | Ascending only | One more click undoes the order, without using the field |
| Unstable order warning | One fixed line while no order exists | Only with more than one page | The plan asks for the warning whenever no order is applied |
| Query in progress | Refuse apply, clear, sort, and load more | Stop and restart | Fewer states; the old response does not find the tab in another state |
| Close the last console | Select an open table, without creating a console | Create an empty console, as in the current code | A table tab does not need a visible console |
| Filter fields | One line, with breaks converted to space | Multi-line editor | The filter is a short condition |

## Session and connection

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Session state | Database or schema selector per console | Fixed connection per console | Five consoles would occupy the pool |
| Connect | On first use | Explicit action or at startup | Asks for the password only when needed |
| Connection loss | Replace idle ones and fail the reserved ones | Repeat the command | Repeating can duplicate changes |
| Failed cleaning | Drop the connection | Recover | Simpler and safer |
| Connection name in the bar | Shorten the name | Shorten the state | The user needs to always see the transaction and the execution |
| Pool queue | Without a limit | With a limit | Each console has one execution |
| Late cancellation | Wait for the request before reusing the session | Reuse at the end of the statement | A late request could hit the next statement |
| End of the request on PostgreSQL | Read until the server closes the request connection | Driver `cancel_query` | It returns at write time, and the `UPDATE` after a `SELECT` at the cap was cancelled in a load test |
| `sslmode=require` | Verify the certificate | Encrypt without verifying, as in `libpq` | The plan forbids ignoring the verification |
| `sslmode=prefer` and `allow` | Refuse the text | Accept | Those modes can use a connection without TLS in silence |
| Default CA | System and `webpki-roots` | Only one of the sources | Company CAs stay in the system; `mysql_async` already brings the second one |
| Trust | Confirm per destination | Confirm per project | Follows the SSH model and does not depend on detecting clones |
| Read only | Field per connection, applied by the database | Confirm `DELETE` without `WHERE` | The database applies the rule |
| Table filter | A single statement | Accept any text | The drivers accept several statements |
| Cleaning on MySQL | `Conn::reset` and back to the database of the connection; without a database, close the session | Only `Conn::reset` | `COM_RESET_CONNECTION` keeps the database, and a `USE` would pass to the next console |
| Cleaning on PostgreSQL | The steps of `DISCARD ALL` without `DEALLOCATE ALL`, and `DEALLOCATE` of each `PREPARE` of the user | `DISCARD ALL` | `DISCARD ALL` removes the type lookup that `tokio-postgres` keeps in its cache, and the next enum read failed with `26000` |
| `DISCARD ALL` and `DEALLOCATE` of the user on PostgreSQL | Run them; then the columns are text, and the cleaning closes the session | Refuse them; handle `26000` in the `prepare` | In a transaction, the failed lookup already stops the transaction |
| `CLIENT REPLY` on Redis | Refuse | Accept | The session waits for each answer, and the execution had no end before Stop |
| Cleaning on Redis | `DISCARD`, `CLIENT SETNAME ""`, `TRACKING`, `NO-EVICT`, and `NO-TOUCH` `OFF`, `UNWATCH`, and `SELECT`, in one round trip | Refuse those `CLIENT` commands | The name and the modes passed to the next console |
| Selector with an open transaction | Refuse the change | Change with `SET` or `USE` | A reserved session does not receive the context, and a transaction with an error refuses `SET` |
| Disconnect with open work | Refuse and name the console | Ask and stop the console | Fewer states; the user resolves the transaction with the existing controls |
| Disconnect with a catalog read | Wait for the session for up to 5 seconds | Report the end right after closing the idle ones | The message would be false, and the form stayed stuck in the rename mode |
| Removed connection | The console stays open, without a connection | Close the consoles | The text is not lost, as in the scripts without connection |

## Libraries

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Drivers | `tokio-postgres`, `mysql_async`, and `rusqlite` | SQLx | `timeout` failed, cancellation and reset were missing, and the pool hid a refused connection |
| Pool | Own implementation | Library pool | Own rules of queue, reservation, and cleaning |
| Main loop | 20 ms wait on the key, with a bounded channel | Thread only for the keys | The bounded channel controls the database read, without another thread |
| Task per console | One Tokio task keeps the session | Session map in the manager | The transaction stays with its owner, and the cancellation stays with the read |
| Tokens per key | Reuse the highlight tokens in the autocomplete | Parse the text twice | With 50,000 lines, it went from 55 ms to 33 ms |
| Editor | EdTUI with a conventional layer | `ratatui-textarea` | No syntax highlight and no Vim mode |
| SQL analysis | Own tokenizer | `sqlparser-rs` or tree-sitter | Needs to tolerate incomplete text; approved prototype |
| Syntax highlight | Own tokenizer with `Highlight` | `syntect` through EdTUI | EdTUI generates RGB and resets the state on each line; the plan requires ANSI and `NO_COLOR` |
| Conventional mode editing | Own function over `state.lines` | EdTUI actions | `insert_str` counts bytes, and `Paste` treats a leading LF as a whole line |
| Undo | EdTUI stack, with groups per word | One entry per key | The stack keeps 100 entries; `DeleteChar(0)` creates the undo point |
| Redo | Counter over the EdTUI stack | Own stack | EdTUI does not clear `redo` after a new edit |
| Click in the editor | Own position calculation | EdTUI mapping | EdTUI counts one column per character and stops at the last character |
| Probe on PostgreSQL | Clock through the simple protocol | `SAVEPOINT` | `SAVEPOINT` generates log errors |
| State on MySQL | `SERVER_STATUS_IN_TRANS` of the response, and `DO 0` after an error | `SET TRANSACTION`; `performance_schema` | The probe erased the user `SET TRANSACTION` and cleared `SHOW WARNINGS`; `performance_schema` needs privileges |
| Types on PostgreSQL | `prepare` before the simple protocol | Heuristic over the text | `bytea` and numbers need the type; costs one round trip to the server |
| TLS provider | `ring`, except OpenSSL inside librdkafka for Kafka (2026-11-10) | `aws-lc-rs` | Avoids a second provider and the `aws-lc` build |

## Product

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Delivery | Complete first version, with local versioning | Public partial versions | Project decision |
| Phase order | Complete interface before the table view | Table first, with a provisional opening | The table opens through the navigator and uses the tabs; the provisional opening would be removed later |
| TSV | DataGrip format, minus the data losses | Exact DataGrip copy | The copy must not lose data in silence |
| Shortcuts on Mac | Without depending on `Ctrl`+arrows, `Home`, `End`, `PageUp`, and `PageDown` | Classic IDE shortcuts | Part does not work, and many keyboards lack those keys |
| Prefix | `Ctrl+W` in both modes | Close tab with `Ctrl+W` | Common path, without depending on the terminal |
| Quit | `Ctrl+W q` | Only close the last tab | The exit with several consoles needs a command; `q` follows Vim |
| Add connection | `a` in the navigator | Item in the tree | The empty navigator already offers a path, and the key follows `n`, `e`, and `r` |
| Simultaneous questions | Queue, with one open question | Replace the open question | Two consoles can ask for an answer at the same moment |
| Automatic name | Smallest available `console-N` in the connection, among open and saved consoles | Single application counter | The console sets belong to the connections |
| Password in the form | Masked field; session as the initial option and vault by explicit choice | Save automatically | The user authorizes the secret storage |
| Password in the URI | Mask on screen, with the same size | Show the typed text | The plan forbids showing passwords; the cursor stays correct |
| Commit by shortcut | Without confirmation | Ask for confirmation | Same behavior as the buttons |
| Metadata | One `.json` beside each `.sql` | Single index or comment in the script | Reduces Git conflicts and keeps the script clean |
| Telemetry | None | Optional | The application only touches the network to talk to the databases |
| List opening | While typing, besides `Ctrl+Space` | Only `Ctrl+Space` | JetBrains is the reference; `Enter` keeps breaking the line |
| Keyword case | Follows the typed text | Always uppercase | Respects the script style |
| Search | Literal text, case-insensitive | Regular expression | IDE pattern; reduces the scope |
| `COPY` with `STDOUT` | Refuse, like `STDIN` | Treat as a driver error | The protocol does not fit the grid, and the reason stays clear |
| Error position | Underline the token without moving the cursor | Move the cursor | The user can keep editing during the execution |
| Detail window | Wrap the lines on demand | Ratatui `Paragraph` or cut text | A 40 MB value cost 2.3 s per frame; now it costs 0.4 ms, with the full text |
| Replace all | `Ctrl+A` in the replace field | `Alt+A` | On macOS, `Option`+letter depends on the terminal |
| External `.sql` file | After the first version | In the first version | Reduces the scope |
| License | `MIT OR Apache-2.0` | GPL or proprietary | Rust ecosystem standard |
| Example catalog | Tests only, without `--no-catalog` | Active by default in the binary | Without a catalog, section 6 asks for keywords; invented tables mislead the user |
| Publishing | Remove `publish = false` at release | Keep, with binaries only | `cargo install sqln` depends on crates.io |
| Catalog responses | Number on each read; the old response leaves | Accept any response | An old read could erase from the navigator a table just created |
| Stop test | Wait for the real start of the statement | Wait fixed 300 ms | Under load, the connection took more than that, and the product acted correctly |

## Implementation — 2026-10-31

These choices follow the plan rules on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| New password | Check with a test connection before writing | Write and find out on the next execution | A refused password never reaches a storage |
| First refusal | Do not repeat the execution | Repeat on its own after the new password | Simpler to explain; the action requested during the suspension continues, because no SQL left |
| Last console | Hidden console, outside the tab strip | Allow zero consoles | The code assumes an active console; the user sees no difference |
| End of the last table | End the application. Replaced on 2026-11-03: the home screen appears | Show the hidden console | "Close the last tab" is already an exit path |
| Keyboard switch | Change the mode in the editor itself | Recreate the editor and warn | Nothing is lost, so no warning is needed |
| Panic report | Place and call stack, without the message | Include the message | The message can quote SQL or a value |
| Log | Event types, without database error text | Record the messages | A database error can quote a value |
| Signals | `SIGHUP`, `SIGINT`, `SIGQUIT`, and `SIGTERM` | Only `SIGTERM` | Closing the terminal sends `SIGHUP` |
| Plain-text file | Refuse a link and an open permission | Fix the permission on its own | The same care of `ssh` with a private key |

## Themes — 2026-10-31

The user asked for themes, runtime switching, and the default theme choice in Settings.
The later subsection **Per-connection theme** records the implemented connection-specific theme.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| List | Tokyo Night, Catppuccin Mocha, Catppuccin Latte, Gruvbox, and Nord, besides Astrodark and `ansi` | More themes | The tones differ at first sight, which helps the per-connection theme. Latte covers those who prefer a light background |
| Definition | `Scheme` in `ui/schemes.rs`; `Theme::from_scheme` distributes the colors | Copy the styles for each theme | A new theme does not repeat the style list |
| Color adjustments | Latte, Nord, and Tokyo Night change some tones of the original palette | Original palette without changes | Some original tones do not pass the minimum contrast: 4.5 on text and 3 on marks and labels |
| Preview | The choice changes the colors before saving, and cancel restores the saved theme | Apply only on save | The theme name does not show the colors. The preview also limits the damage on a terminal without truecolor |
| Section | `Appearance`, last in the list | Field in `General` | The per-connection theme and the icons have a natural place in that section |
| File values | `kebab-case`, such as `tokyo-night` | `lowercase` | `auto`, `astrodark`, and `ansi` stay valid, and the new names stay readable |

### Per-connection theme

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Who decides | The connection of the open tab, in console or table, changes the whole screen | Selected connection in the navigator; only the status bar | The production alert needs to follow where the command runs. The selection in the navigator runs nothing |
| Without its own theme | The tab uses the default theme | Inherit the theme of the previous tab | The color of a tab does not depend on the path to it |
| Choice point | `App::draw` computes the theme each frame, in `theme_choice` | Change the theme on each tab change | There are many tab-change paths. The computation in one point forgets none, and the preview needs no own code |
| Active connection | The form allows changing name and theme (`display_only`), and the button goes from `Save name` to `Save` | Require a disconnect | The user needs to mark production with the session open. A theme does not change access, so no session closes |
| Reload | `update_stored` applies the theme even with the connection in use | Postpone until disconnect | The file and the screen stay equal, and the transaction continues |
| Settings with its own theme | The screen shows the chosen default theme while the dialog is open | Keep the connection theme | Without that, the preview would seem broken in a tab with its own theme |
| Place in the form | Last field of the options | Beside the name | It sits at the end of `Tab` and does not change the order of the access fields |

With the **Theme** and **Icon** fields, the form uses the compact mode up to 31 rows. The mode with spaces between groups now needs 32 rows.

### Per-connection icon

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Choice | Fixed list of 14 icons in the form | Text field to paste a symbol | SQLN cannot ask the terminal font. The list guarantees an existing symbol, and a pasted symbol could become an empty box |
| Symbols | Unicode of 1 column, checked with Monaco on macOS, equal in all sets | Nerd Font symbols | Nerd Font is optional, and without it those symbols become an empty box. These symbols exist with or without it |
| Outside the list | `⚡`, which becomes a 2-column emoji, and `⛨` and `⚿`, wider than the cell | Accept any Unicode symbol | A wide symbol misaligns the navigator line |
| `NO_COLOR` | Shows the chosen icon | Hide, like the other icons | The choice belongs to the user, and the theme does not identify production in that mode |
| Database mark | Stays after the name, in all sets | Disappear, because the logo left its place | The chosen icon does not say which the database is |

### Known limitations

- An RGB theme on a terminal without truecolor can show wrong colors. The explicit choice wins, because `COLORTERM` is missing in some terminals with support, such as an SSH session.
- With `NO_COLOR`, the field changes the file but not the colors. The per-connection theme does not identify production in that mode.
- On a disconnect with the form open, it reloads the saved definition. A name or theme not yet saved goes back to the saved value.

### Pending items

- Validate the themes and the icons in a real terminal. The visual check used the screens drawn with the Monaco font, outside the terminal.

## UI kit — 2026-11-07

The user asked for a shared UI kit: easier screen creation, one visual language, and only slight differences between modals, fields, and buttons. The migration moved the shared pieces to `src/ui/kit` in phases that preserved the screen first, then applied the variation contract.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Structure | `ui/kit` holds geometry, semantic styles, modal chrome, form rows, buttons, inputs, lists, prompts, feedback, measurement, and panel titles | Keep the helpers in `widgets.rs` and each screen | One place for the base look; a screen declares fields and rows |
| Modal | One rounded chrome with title, sidebar, content, note, and buttons; only size, placement, sidebar, heading, body, and bottom title vary | One border or footer per screen | The position of the title, note, and buttons stays the same on every screen |
| Field | One row with the focus mark, label, value padding, and `field`/`field_focus`; labels use `Narrow` (16) or `Wide` (24) | A magic label width per dialog | A new field inherits the focus and padding rules |
| Button | `Dialog`, `Toolbar`, and `Inline` variants over one row; height 1, gap 2 | A manual loop per screen | Focus, disabled, and click areas stay equal |
| Note | `Info`/`Hint` use `hint`, `Success` uses `mark_good`, `Warning` uses `mark_neutral`, `Error` uses `error` | One mapping per screen | The same message looks the same on every screen |
| List | Selected `list_selected`, normal `list`; trees use `tree_selected`/`tree_cursor` | `dialog_focus` for list selection | Lists, trees, and fields keep different meanings |
| Visual changes | Form label 25 → 24; `Info` notes from `status` to `hint`; list normal rows from `dialog` to `list`; picker/conflicts selection from `dialog_focus` to `list_selected` | Keep every current value | The user asked for fewer variations; these are the only pixels that change |
| Exceptions | The parameters panel keeps its byte cursor and display-width renderer; the editor review keeps its key/label degradation above the kit | Force every renderer into the kit at once | Both have rules that the form row does not share |

## DuckDB — 2026-11-07

The user asked for DuckDB support. The delivery is the engine and the essential interface: the connector, the connection form, `config.json`, the command line, and the icon. Schema compare, the ER diagram, the DDL window, `EXPLAIN`, grid editing, and the foreign-key reads stay out.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Driver | The `duckdb` crate `1.20000.1` with `default-features = false, features = ["bundled"]` | Default features | The default of `libduckdb-sys` downloads a prebuilt library at build time; the bundled source builds offline and reproducibly, at the cost of a long C++ build |
| Model | One file or one database in memory, as SQLite | A network server | DuckDB is embedded; the model and the thread-per-session bridge of `connector/sqlite.rs` fit it exactly |
| Sessions | The connector opens the database once in an owner thread, and each session clones that connection with `try_clone` | `Connection::open` for each session | Two `open` calls on one file make two isolated instances: a write of one session stays invisible to the other, and the writes of the pool are lost |
| Owner start | The lock of the first open spans the wait, so concurrent first connections never open the file twice | A check after the open | Two opens of one file race for its lock, and one of them fails |
| Dead owner | A file clears the dead owner and starts a new one once; a database in memory keeps the failure | A restart for every source | The thread is the only holder of the database: a database in memory dies with it, and a restart would hand out an empty database without a word |
| In memory | `single_session` and no idle close, as an in-memory SQLite database | A pool of sessions | A new connection starts with an empty database |
| Result read | Arrow through `stream_arrow`, with `array_value_to_string` for each cell | `Row::get::<String>` | The driver refuses `String` for a list, a struct, or a map, and an owning read materializes every row before the row cap applies |
| Change count | `execute` when the prepared statement reports no result column, and also when the statement changes rows without `RETURNING` | Only the column count, as the SQLite connector does | DuckDB answers every row change, also under `WITH`, with one `Count` column that is already in the prepared statement, and it would reach the grid as a user result |
| Current schema | The catalog reads `SELECT current_schema()` of the session, as PostgreSQL does | A fixed `main` | A console changes the schema with `SET schema`, and the catalog is read on the session that holds it |
| Transaction state | The session keeps it from the statements it runs | `Connection::is_autocommit` | The driver reports that function as a constant `true` |
| Aborted transaction | `Failed`, and only `ROLLBACK` ends it | `Open` | DuckDB refuses every statement after an error inside a transaction, as PostgreSQL does |
| Read-only | `access_mode = READ_ONLY` when the file opens, from the definition | A check of each statement only | The engine enforces it, and a file cannot open read-write and read-only at once |
| Read-only of the settings | Refuse a data-changing statement in the connector | Open the file read-only | A DuckDB connection cannot change its access mode after it opens, so the global preference, the agent, and a reservation need a check outside the engine |
| External files | Refuse a `csv`, `tsv`, `parquet`, `json`, `jsonl`, or `ndjson` file, and name `read_csv`, `read_parquet`, or `read_json` | Open it, as the driver does | `Connection::open` of one of these starts a database in memory without a word, so a table of the session disappears at the end |
| Command line | The `duckdb:` scheme, with `duckdb:` and `duckdb::memory:` in memory | Detect a file extension | A bare path stays a SQLite file, which the current behavior requires |
| Form order | DuckDB at the end of the type choices | Next to SQLite | The tests of the form reach a kind by counting arrows |
| Attached databases | A schema of an attached database keeps the `catalog.schema` name, and the catalog, the column read, the structure, the completion, and the templates filter by catalog | Hide the schemas of the attached databases | `duckdb_schemas`, `duckdb_tables`, and `duckdb_views` list every catalog of the session, and a schema name alone cannot reach a table of one of them: the open wrote `FROM "public"."player"` and the engine answered that the schema does not exist. The anchor is the catalog of the connection, read on a clone that ran no statement, because `SET schema` changes the current database of a session and a pooled read can run on any session. A local schema whose own name has a dot does not open, because every read splits the name at the first dot |

## Custom themes — 2026-11-08

The user asked for theme creation in Settings, with a full palette editor, both scopes, a live preview, and no write before the dialog save.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Storage | One file per theme, `themes/<id>.json`, in the folder of each scope | Colors inside `config.json` | A palette of 30 colors would bloat the file, and a project theme becomes versionable and shareable |
| File format | `version`, `name`, and `colors` with every palette color as `#RRGGBB`, `denyUnknownFields` | A base plus overrides | One file stands alone, and a typo in a key names its file. The conversions between `Colors` and `Scheme` list every field, so the compiler checks the sync |
| Precedence | A project id wins over a global id, as the preferences do | Namespaces such as `project/x` | One rule for the whole configuration, and the list marks the origin |
| Name | The display name is free; the file id comes from the name, slug to `[a-z0-9-]`, and a reserved built-in name refuses the theme | A free id field | The user thinks in names, not ids. The reserved names keep a file from shadowing a built-in theme |
| Editor | A sub-mode of the Appearance section, as the shortcuts: name, base, and one hex input per color with a swatch | A separate dialog window | The settings modal already has sub-modes, and the focus, the mouse, and the keyboard stay in one place |
| Base | Only for a new theme; changing it replaces the whole palette | Merging the changes over the base | The palette is complete data, not a diff, so the base is only a starting point |
| Live preview | The editor draft resolves in `App::choose_theme` each frame, before the loaded catalog | A preview pane inside the editor | The theme of the whole screen is the preview, and the existing choice point needs no new code. A draft without a name resolves to the placeholder `draft` |
| Contrast | The editor warns with the count of places below the WCAG minima, shared with the tests of the built-in schemes | Block the save | The user owns the palette, and a terminal can be light or dark; the warning informs, the file is valid |
| Save | The dialog save writes new or edited themes, then `config.json`, then removes deleted themes; nothing before it | Write each theme file at the editor exit | The dialog keeps its contract "writes nothing before the save". Each file write is atomic, but the group is not: on failure, the dialog reports the error and retains the draft for retry |
| Delete | Refuse while in use, by the effective draft of the dialog or by a connection; otherwise the removal joins the save | Ask first with a dialog | The removal is reversible until the save, and the refusal explains the reason and the way out |
| Conflicts | Each write carries the hash of its load, under `locks/themes.lock` | Replace without a check | The rule of `config.json` applies: another program's change never goes away silently |
| Damage | A file that cannot load gives a note at the start, in Settings, and at each reload; the other files load | Block the configuration | A theme is not critical data, and one damaged file cannot stop the connections |
| Missing theme | The choice falls back to `auto`, with a note in the status bar | Keep the last colors or quit | The data is not critical, and the note names the theme and the fallback |
| External change | The poll hashes a manifest of the folder names and contents, and the existing reload question covers the themes | Reload without a question | The rule of `config.json` applies, and `ReloadConfig` already reloads both |
| Per-connection theme | The connection form cycles the built-in themes and the customs of both scopes | The connection picks its own scope only | The resolution is the merged catalog, and a project can share a theme that a global connection uses |
