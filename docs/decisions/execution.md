# Decisions — Execution

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Execute a subquery

The user approves a separate action for the smallest subquery at the cursor.

- **Run subquery** uses the command palette. The configurable action name is `runSubquery`.
- The action has no default shortcut, so existing custom shortcuts stay valid.
- The action extracts the smallest complete parenthesized `SELECT` or `WITH` query that contains the cursor.
- It removes the outer parentheses and does not add aliases, parameters, or CTE dependencies.
- A selection does not change this scope. Text, cursor, selection, and the normal Run scopes stay unchanged.
- A confirmation shows the source line and the SQL prefix before execution. Cancel has initial focus.
- Existing execution checks, production warnings, parameter binding, authentication, and transaction rules still apply.
- Missing subqueries give a reason and a next action. The action never falls back to the whole statement.
- Correlated references and missing CTE dependencies can fail. The database error uses the existing Messages path.
- The action is available in an idle relational SQL editor, not in Redis or Kafka consoles.
- Lexical token ranges identify candidates. The database remains responsible for SQL validation.
- No saved format changes.

## Execution scope and first tasks

The user approves explicit execution labels and task-based startup guidance.

- Run shows **Run selection** or **Run statement**. The script action shows **Run script**.
- A row above a nonempty, idle editor shows the one-based executable line range and the whole-script scope.
- Preview and execution share statement resolution. Text revisions cache statement ranges; selection changes update their own ranges.
- Without a selection, the line-number column highlights the statement under the cursor, including its number padding.
- The highlight follows the same executable line range. Blank gaps and comments outside statements show no target.
- The execution-mark column stays separate. Text selection, literal cell editors, and inline diff review keep their existing styles.
- Pending AI text does not change the target; the highlight follows the actual console text.
- The highlight uses existing theme selection colors and bold text. Without colors, it uses reverse video and bold text.
- Selections without statements and cursor positions outside statements keep the existing refusals. No confirmation or execution rule changes.
- Console toolbars wrap complete buttons onto additional rows. Mouse hit areas include both coordinates.
- This replaces the earlier narrow agent toolbar rule that hides Access before Tasks.
- The home screen groups connection tasks and saved work. SQL file opening uses the existing palette command or its custom shortcut.
- Without a connection, the home screen explains that SQL files require a connection first.
- Saved consoles of the selected connection and scripts without a connection keep their separate lists.
- A command without a shortcut shows the palette path and its command label. An arrow means to choose that command.
- Startup conditions, focus, restored tabs, credentials, file choices, and execution protections stay unchanged.
- These screens do not open database sessions or run SQL. Small areas remove the logo and header spacing before task rows.

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
