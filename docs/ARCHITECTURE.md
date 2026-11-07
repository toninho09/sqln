# SQLN — Architecture

## State of this document

**Defined:** this document is approved.
It details the architecture of the [plan](INITIAL_PLAN.md).
The connector signature below is a summary, not the complete trait. See `src/connector/mod.rs` for the contract.
The facts about the drivers come from [RESULTS](RESULTS.md).

## 1. Overview

The project is a single Rust crate.
The interface runs on the main thread and never waits for the database.
Each console has its own task, owner of the reserved session. This document calls that task an actor.
The execution manager keeps one pool per connection, and all connections use a single runtime and a single event channel.

```text
Main thread                        Tokio runtime
┌─────────────────────┐  Command   ┌───────────────────────────┐
│ Terminal events     │ ─────────▶ │ Execution manager         │
│ Key router          │            │  ├── Script runner        │
│ Application state   │ ◀───────── │  └── Pool per definition  │
│ Drawing             │  Event     │       └── Connection actor│
└─────────────────────┘            └───────────────────────────┘
                                     Own thread: rusqlite
```

## 2. Modules

| Module | Responsibility |
|---|---|
| `agent` | Transport-independent assistant operations, read policy, private instance registry, and IPC |
| `mcp` | MCP schemas and stdio bridge to authorized open instances |
| `app/agent` | Access decisions, tasks, loaded-state reads, and edit proposals |
| `app` | Application state, actions, and state updates |
| `ui` | Drawing of the screens and the widgets, themes (`theme`), color schemes (`schemes`), symbols (`icons`), status bar (`statusline`), and prefix menu (`which_key`) |
| `ui/kit` | Shared geometry, semantic styles, modal chrome, form rows, buttons, inputs, lists, prompts, feedback, measurement, and panel titles |
| `app/commands` | Shared command availability, contextual lists, priority, and target labels; `Action` remains the catalog |
| `palette` | State and word filter of functional commands, including action names and technical aliases; `ui` draws it |
| `filter_list` | Filtered list core, used by the palette, the history window, and the compare dialog lists |
| `help` | State of the `F1` key reference: groups, filter, and scroll |
| `browser` | State of the file explorer of the path fields: folder, rows, and filter |
| `input` | Key map: regions, actions, sequences, and `keymap` rules |
| `navigator` | Tree of connections, consoles, schemas, relations, columns, and other database objects |
| `form` | Fields and validation of the connection form |
| `connection` | Connection definitions and the state of each connection in the application |
| `editor` | EdTUI, conventional layer, search, and replace |
| `sql` | Tokenizer, statement separation, and statement classes |
| `complete` | Local autocomplete suggestions |
| `ai` | One OpenAI-compatible chat protocol, credentials, context, and metadata transforms. The Settings dialog edits the provider fields |
| `catalog` | In-memory cache; schemas, tables, routines, sequences, triggers, and types first, columns and foreign keys on demand |
| `definition` | DDL of one object: the queries per database and the text with its header |
| `generate` | SQL templates of the navigator |
| `console` | Editor, results, messages, and state of a console |
| `parameters` | Panel of the query parameters of one console: fields, kinds, values, and Set state |
| `command_log` | In-memory command history, bounded text, tracing, and the Messages viewport |
| `plan` | Query plan commands, database output readers, and tree rows |
| `mark_list` | Searchable list with a mark for each selected name |
| `result` | Grid of one result with cursor and search, used by consoles and tables |
| `table` | Table tab: pages, fields, sorting, suggestions, and query |
| `data_edit` | Pending rows, budgeted snapshots, key predicates, update SQL, review, and cursor restoration |
| `structure` | Catalog queries of the structure tab, per database, scope, and section |
| `diagram` | ER diagram: catalog queries, model, layout of boxes and lines, Mermaid text, and the state of a diagram tab |
| `compare` | Structure and data compare: catalog queries, model of a schema, differences, sync DDL, rows by key, the dialog, and the state of a compare tab |
| `line_input` | Single-line text field, used by the filter bar fields |
| `exec` | Executions, scripts, cap, and cancellation |
| `pool` | Limit, queue, reservation, and cleaning |
| `connector` | Contract and PostgreSQL, MySQL, SQLite, and Redis/Valkey connectors |
| `grid` | Rows in one block each (`row`), widths, search, budget, TSV, and readable JSON shape (`json`) |
| `export` | Export formats, temporary file, and modal form |
| `import` | File readers, `INSERT` text, batches, the error log, and modal form |
| `find` | Text comparison of the two searches |
| `clipboard` | System clipboard and OSC 52 |
| `terminal` | Terminal setup and restore |
| `store` | Folders of the scopes, `config.json`, console scripts, `workspace.json`, atomic writes, and locks |
| `script_file` | Saved state of a console: hash, line ending, save wait, and write problem |
| `creds` | Vault and session passwords, with `zeroize` |
| `trust` | Confirmed destinations and known paths |
| `settings` | Form of the preferences per scope, with the autocomplete provider fields |
| `log` | Optional event log, without SQL or values |

`ui` uses driver-independent connector models, such as `Namespace` and `TransactionState`, plus target helpers.
`store::config` uses SQLite and network target definitions. Neither layer uses native driver connections.
Only `connector` knows native driver types.

`app::auth` integrates `creds` and `trust` into first use, with the password and trust dialogs.
`creds` offers identities, session passwords, and the vault adapters.
`trust` offers the reading and the writing of global confirmations.
The vault calls are synchronous, but `Manager::spawn_blocking` runs them off the interface thread.
`app` applies the key priority and the mouse. `app/auth.rs`, `app/keys.rs`, `app/hints.rs`, `app/mouse.rs`, `app/close.rs`, `app/connections.rs`, `app/tables.rs`, `app/persist.rs`, `app/scripts.rs`, `app/session.rs`, `app/conflicts.rs`, `app/history.rs`, `app/guard.rs`, `app/export.rs`, and `app/browse.rs` divide that code.
`store` keeps the folders and files. It does not depend on `app` or `ui`.
Its configuration reader uses driver-independent connector target types.

### Local assistant operations

`sqln mcp` runs without a terminal. It discovers private Unix sockets. It uses the instance UUID of the call, or the only open instance.
The bridge waits for task ends with repeated state reads. Each IPC request still gets one immediate answer.
The SQLN process owns authorization and application state. Its IPC thread sends typed requests through a bounded queue.
The interface processes at most eight requests or 2 ms per frame. It never waits for a database operation.
Assistant executions carry a cancellation token and a read-only flag. They never inherit a user's reserved transaction, and never leave one open.
Without full access, a parser policy checks submitted SQL; native connector read-only controls provide a second check.
Full access removes both checks for executions only. A read-only connection and the run questions still apply.
Catalog, DDL, diagrams, and comparisons reuse existing readers and transformations under the same credential gate.
Edit proposals use editor revisions. Each accepted change is one undo step. Only UI actions approve access, accept proposals, or enable direct editing.
The shared data budget counts retained snapshots and text artifacts. Results use generation-checked pages without a reread.
No assistant authorization, remembered access, or direct-edit setting enters saved files. See [MCP](MCP.md) for the complete contract.

### Inline review

AI responses and agent proposals use the review in the editor. `editor/review` holds the model: one diff review of line changes (hunks), and a list of ghosts, each an insertion at its anchor. It builds the rows of the view, applies an accepted change through the revision-checked edit path, and moves the later changes. A ghost stays until the user decides: an edit on another line moves the anchor, an edit of the anchor line closes only that ghost, and an undo takes them all. Several answers wait at once, bounded by a setting; their requests run in parallel and each shows an animated mark at its anchor. `ui/review` draws the rows, the gutter marks, the buttons of each ghost and of each pending change, and the bar of a diff. `app/review` routes the `Review` key region, the mouse, and paste, and reports each decision to its origin; the decide keys act on the line of the cursor. The editor is read only during a diff review, which ends the AI requests of its console. Diffs are built off the interface thread.

### AI SQL completion

AI completion is separate from MCP. `app/ai` captures the editor interval and revisions, checks destination consent, and coordinates the request. The suggestion list holds two AI rows: the plain row starts a request at once, and the request row opens a one-line modal whose text travels as `request` in the body. `editor/ai` exposes immutable targets and applies a proposal through the existing single-undo edit path. Local `complete` suggestions remain independent of providers.

`exec/ai` reads complete structural metadata through the credential gate and an independent pool lease. Snapshots count against the shared budget and follow catalog refresh generations. Unknown session states close their leases. `ai` serializes OpenAI-compatible chat requests, reduces oversized context deterministically, and bounds HTTP responses. Credential files are read again for each request. Vault keys use a separate `sqln-ai` service. Calls, preparation, and diff construction stay off the interface thread.

`ui/ai` renders the request dialog, the waiting states, and scoped settings. A response shows ghost text at the cursor of its console, through the [inline review](#inline-review) model. A review changes the buffer only when the user accepts a change, with checked revisions and no execution. Pending responses never take another dialog's focus.

`preferences.aiCompletion` is optional. Enabled inherits independently; the provider block inherits as a unit. The global private `ai-trust.json` records consent, under its own lock. No SQL, metadata snapshots, responses, or key content enter those files. Scripts and workspace formats do not change. See [AI completion](AI_COMPLETION.md) for the contract and limits.

### Files and saving

`store::Store` has the global folder and, when it exists, the project folder. Each folder is a `Root`, with the folder of the `config.json` (`dir`) and the base of the relative paths (`base`). The project base is the root, not `.sqln/`.

| Part | Responsibility |
|---|---|
| `store::file` | `write_atomic`, `write_private_atomic` (exclusive temporary and permission `0600` on Unix), hash, locks, and cleanup of old `.tmp` files |
| `store::config` | Reading with `serde`, validation, precedence, `Definition` of each entry, and `change_connections` (reread under lock, change, and write) |
| `store::scripts` | `.sql` and `.json` files of a console, list, lock per console, copy between scopes, and line ending |
| `store::workspace` | `workspace.json`, optional state with invalid-file detection, written under `config.lock` |
| `store::history` | `history.jsonl`: one JSON line per statement, appended at the end of the file, read per connection, and truncated at the start of the file |
| `script_file::ScriptFile` | Hash, 500 ms wait, 5 s retry, pending write, and problem (`Failed`, `Changed`, or `Removed`) |
| `script_file::worker` | One bounded worker for script text, metadata, and moves between scopes |
| `app::persist` | Load at startup, reload, check each second, close order, and session state |
| `app::scripts` | Console list, scripts without connection, opening, removal, and attaching |
| `app::conflicts` | Question about scripts changed outside SQLN |
| `app::guard` | Question before an execution: statement without `WHERE` and production change |
| `app::history` | Notes of each statement and history window |

Code rules:

1. The load reads both files. An error in one file blocks the whole application, without a partial scope.
2. Each read error carries the file and the position (line and column). The message never carries a value from the file.
3. The write of `config.json` takes `locks/config.lock`, reads the file again, applies the change, and writes with the read hash. The file of the other scope does not change.
4. The `config.json` keeps only what the scope declares. The reading does not rewrite the default values.
5. The `id` of a script is a UUID and is the name of both files. The list ignores a `.json` that is unreadable or whose name differs from the `id`.
6. The lock of a console is the file `locks/<id>.lock`, with `flock`. The system releases it at process end.
7. `ScriptFile::due` compares the editor revision with the last written one. The main loop passes the clock, and the test controls the time.
8. The line ending of the file (`LF` or `CRLF`) stays in the `ScriptFile`. The editor always uses `LF`.
9. `ScriptFile::poll` reads the file only if the size or the date changed, and then compares the hash.
10. A connection definition carries the base folder. `Definition::connector` resolves the relative paths with it at open time.
11. Script writes use a dedicated worker, separate from credential access. Only one operation runs at a time.
12. Each script keeps at most one pending operation. The interface keeps later requests without additional text snapshots.
13. A write response updates only its captured editor revision. Later edits stay dirty. Close waits for the latest revision.
14. File polling skips a script while its operation runs. Emergency shutdown waits before it writes the remaining text.
15. A pending association blocks new SQL executions and export reads for its console until it succeeds or fails.

### Credential preparation

`app::auth` receives each operation before it sends it to the manager.
The preparation covers executions, catalog, columns, pages, and connection tests of saved definitions.
Ready connectors and local definitions without a file key do not come from imported configuration.

1. Check trust on the destination and the project path.
2. Ask for confirmation when needed, before reading the vault or sending credentials.
3. Get the password according to `credential`, or use passwordless authentication for `none`.
4. Update the connector of the pool without erasing text, schema, or editor state.
5. Send only the operation that still matches the current request.

A local form authorizes its destination. It does not implicitly authorize credential reuse on a new project path.
Testing an existing form also does not skip that confirmation.
The save option starts unchecked; a test with a typed password does not replace the password of the saved definition.

The queue keeps the SQL, the options, and the editor revision at the time of the request.
Editing during the wait does not change the SQL sent and does not assign its marks to the new text.
Each response has a token. The connection has a generation, and the test has a copy of the form.
Cancellation or change invalidates old responses.
Cancel also removes the requests of the same generation that await the same preparation.

`Manager::spawn_blocking` limits the system operations to one at a time, with an async semaphore.
Jobs wait without occupying blocking threads; a token prevents a cancelled job from starting vault access.
The responses use a private preparation channel, separate from the SQL execution events.
A native call already started may continue after cancel, but its response does not start SQL.

The interface offers explicit retry or a session password after a vault failure.
Failure to write the reference after a vault save keeps the credential and reports the incomplete state.
Without a vault, the reading uses `credentials.json`. A write failure without a vault offers plain text after the warning.
A password refusal clears the session and opens a new prompt. The new password passes `test_connection` before any write.

### Table tab

Each table tab has a console task of the execution manager, without a console editor or a manual transaction.
Grid Apply owns a short transaction on that task. It releases no session between validation and commit.
The tab requests a page with `Run`, and receives the same events as a console.
Thus, the deadline, the stop, the cancellation, the pool queue, and the budget apply without new code.
The task keeps the connection only during the query, because a table query never leaves a transaction open.

The count of the `c` key uses the same path, with `PageRequest::count`. `Load::count` prevents the number from changing the grid, `has_more`, `limit`, or `cut_short`.

1. `table::page_sql` builds the query: `SELECT *`, `WHERE`, `ORDER BY`, and `LIMIT n+1 OFFSET k`, each clause on one line.
2. The tokenizer refuses the text with a second command or an open token, before it sends the query.
3. `RunOptions::row_cap` limits the `ResultSink` to `n+1` rows, and the SQL `LIMIT` guarantees that cap. The sink never requests the cancellation.
4. The tab keeps the first `n` rows and drops the extra row, with its cost returned to the budget. The extra row proves that a next page exists.
5. The `OFFSET` of **Load more** is the number of rows already loaded.
6. The first page of a query clears the rows, and the `ExecutionId` drops old responses.

A table query and a console query share the same `ExecEvent`.
The identity of the tab comes from the console counter of the manager, so the numbers do not collide.

A Redis key tab uses the same path, without SQL:

1. `table::page_sql` builds `SQLN.VALUE <database> <key> <offset> <amount>` per `connector::redis::value_page`. That text is not a server command.
2. The session reads `TYPE` on each page, because the key may change between two pages.
3. The session uses the command of the type: `GET`, `LRANGE`, `HSCAN`, `SSCAN`, `ZRANGE`, or `XRANGE`.
4. A false `Capabilities::table_filters` removes `WHERE`, `ORDER BY`, **Clear filters**, and sorting from the tab. **Apply** becomes **Reload**.

### Grid editing

`ResultTab::edits` keeps the original and current row snapshots for pending table changes.
The displayed result holds the proposed row, so search and value inspection use the same values.
Reference-counted `SavedRow` objects own budget reservations and release them on their last drop.
Discard transfers the original reservation back to the grid without requiring free memory.

`app/data_edit` coordinates editing, review, authorization, submission, and close.
`data_edit/form` holds modal state; `ui/data_edit` draws it.
The cell editor reuses `SqlEditor` without SQL suggestions and never dispatches global commands.
`Action` and `app/commands` control command discovery and effective shortcuts.

`Manager::edit` sends inspection, review, or apply jobs to the table task.
Review formatting uses a blocking task and acquires no database session.
Apply passes the normal credential and trust gate, then reserves one session for the whole operation.
`exec/runner/data_edit` owns begin, locking reads, original-row checks, metadata verification, updates, and commit.
Private `ResultSink` capture keeps validation rows away from the visible grid and inside the budget.
PostgreSQL and MySQL lock rows with `FOR UPDATE`; SQLite begins with `BEGIN IMMEDIATE`.
Any known failure rolls back. A failed cleanup drops the session.
A failed commit response reports uncertainty and never starts an automatic retry.

`EditFinished` carries the operation outcome. The application rejects stale execution identities.
A successful apply reloads applied filters and order; draft fields stay unchanged.
Cursor and selection restoration matches primary keys among the reloaded rows.

### Undo history

The conventional mode has its own history in `editor::undo`. A step keeps only the text that an edit replaced, so a step of typing costs some bytes.
`undoLimitBytes` limits the history of each console, with 64 MiB as the default. Settings shows this value in MiB, in the Editor section.
The oldest steps leave first, and the newest step of the undo and redo lists always stays. Thus, one step can go above the limit, and the last edit always has an undo.
The history has no limit of steps.

Undo and redo are one operation: a step replaces the text between two positions and returns the step that reverses it.
A step of many ranges, such as a replacement of all matches, covers the text from the first range to the last one.

The steps of the conventional mode and the Vim mode stay in one order after a change of keyboard:

1. A change to the Vim mode gives the 100 newest steps to EdTUI, and the steps that wait for a redo end.
2. After a change to the conventional mode, `Ctrl+Z` reads the history first and then EdTUI, because the steps of EdTUI are older. `Ctrl+Y` reads them in the opposite order.

### Known editor limit

**Technical debt:** the undo history of the Vim mode has no memory cap. This item does not block the release.

Each undo entry of EdTUI keeps a full copy of the text, and each console has its own stacks.
`undo` and `redo` keep up to 100 entries each, and EdTUI does not clear `redo` after a new edit.
By measure, a 50,000-line script takes 16.8 MiB per entry and 1.7 GiB in the 100 `undo` entries. By estimate, it takes 3.4 GiB with both stacks full.
In the Vim mode, one entry is one change, and `undoLimitBytes` does not apply.
The entries also stay after a change to the conventional mode.
Common scripts stay far below that, and closing the console frees that memory.

`Stack` and the `undo` and `redo` fields of EdTUI are private, and the public API does not measure or remove entries.
Therefore, SQLN cannot cap that history without an EdTUI change.

**Exit:** when EdTUI publishes an API for it, apply `undoLimitBytes` to the Vim mode.

### Editor frame

EdTUI reads each row of the screen to its end, and it tests each character against a flat list of highlights.
With that rule, the time of a frame grows with the square of the size of the screen.
Therefore, SQLN gives EdTUI no highlights, and `editor::frame` gives it rows that stop at the end of the window.
After the draw, `editor::frame` writes the styles on the cells.
A frame then costs time only for the cells of the screen: a full screen of 300 by 100 cells takes about 3 ms per key.

The text of the user is not complete during the draw, so `FrameState` returns the rows when it drops.
A panic also drops it, because the last save of a panic reads the text.

Two rules copy code of the libraries. Check them after an update of EdTUI or Ratatui:

1. `SqlEditor::next_left` copies `update_viewport_horizontal` of EdTUI.
2. `FrameState::paint_row` copies the cell rule of `Span::render` of Ratatui.

`each_column_of_a_long_row_keeps_its_colors` guards both rules.

### Result rows

A block of memory for each value costs more than the text of a usual value.
Therefore, `grid::Row` keeps all the values of a row in one block, and a read gets a `Value` that borrows from that block.
The connectors write each row through `RowWriter`, so a read also has fewer allocations.
A row of 3 usual values takes about 100 B, and the same row took about 200 B with a block for each value.

Each cell has a head and the bytes of its value, so a read walks the row from its first column.
To read many columns of a row, use `Row::cells` and not `Row::get`, because each `get` starts again.

`Row` holds the only `unsafe` code of `src/`: a read of text skips the check of UTF-8.
That check took a third of the time of a copy or of an export, and each text in a row comes from a `str`.

The budget counts the block of each row, so the same limit holds about twice the rows.
The limit in memory stays the same.

A row can keep its place in the budget after the end of its execution. A data compare keeps each source row until the compare ends, because the target rows meet them in a later execution. A target row that equals its source row leaves the budget at once. The index of the keys stays out of the budget.

## 3. Concurrency

### Rules

1. The main thread receives terminal events and runtime events.
2. Database input and output stays off the interface thread. Some local file operations still run there; see the file explorer limitation in `DECISIONS.md`.
3. Each console has an actor with a command channel, and that actor keeps the reserved session.
4. Cancellation uses an object separate from the connection, because the actor may be busy.
5. `rusqlite` runs on its own thread, because it is synchronous.
6. Each row batch goes to the interface through a bounded channel.

### Main loop

The main thread waits for a key for up to 20 ms and then reads the execution events.
It does not wait while events arrive, because the bounded channel would then stop the database.
It reads events for up to 8 ms per frame, so a large load does not delay the keys.
The limit is a time and not a count, because the number of frames must not grow with the events.
A frame builds only what the screen shows: the visible tabs, the visible messages, and the styles of the visible columns.
The grid search advances 8 ms per loop turn. It checks the deadline after a row or 512 characters.
The search resumes inside a cell and streams binary hexadecimal text without a full copy.
The event channel is bounded, and that controls the speed of the database read.
The application draws only after a change.

### Why an actor per connection

`mysql_async` and `rusqlite` need exclusive access to the connection during a statement.
The actor keeps the read active and keeps consuming rows after a cancellation.
The tests showed that abandoning the read can block the server.

### Old events

Each execution receives an `ExecutionId`.
The application drops an event whose `ExecutionId` is not the current one of the tab.
That prevents an old response from refilling a grid.

## 4. Connector contract

```rust
trait Connector: Send + Sync {
    fn dialect(&self) -> Dialect;
    fn capabilities(&self) -> Capabilities;
    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError>;
}

trait Session: Send {
    fn canceller(&self) -> Arc<dyn Canceller>;
    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError>;
    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError>;
    async fn transaction_state(&mut self) -> TransactionState;
    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError>;
    async fn reset(&mut self) -> ResetOutcome;
    async fn is_alive(&mut self) -> bool;
    async fn read_catalog(&mut self) -> Result<Catalog, ExecError>;
    async fn read_catalog_of(&mut self, schemas: &[String]) -> Result<Catalog, ExecError>;
    async fn read_schema_names(&mut self) -> Result<Vec<String>, ExecError>;
    async fn read_columns(&mut self, schema: &str, relation: &str) -> Result<Vec<String>, ExecError>;
    async fn read_foreign_keys(&mut self, schema: &str, relation: &str) -> Result<Vec<ForeignKey>, ExecError>;
    async fn read_referencing_keys(&mut self, schema: &str, relation: &str) -> Result<Vec<ForeignKey>, ExecError>;
}
```

The `async-trait` crate makes `Box<dyn Session>` possible.
`read_foreign_keys` and `read_referencing_keys` have an empty default, so a source without keys does not write them. `Manager::load_columns` reads both lists together with the columns, and a key failure does not fail the columns. A key that reaches a relation has the shape of a key that leaves it: `columns` are the columns of the relation, and `target` is the other table.
`read_catalog` also reads the supported routines, sequences, triggers, and types into `Schema::objects`.
`read_catalog_of` keeps the configured schemas and the current schema. Redis skips reads of hidden databases.
`read_schema_names` supplies the connection form list. Redis uses `INFO keyspace` without scanning keys. `catalog::add_objects` gives each row to its schema and puts the objects in the order of the tree.
The DDL of an object runs in a task of the pool that only that read uses (`app/definition`). The task closes at the end of the read, and the text goes to a new console.
`Capabilities` declares whether the source has transactions, whether the console chooses a schema or a database, and whether the table tab accepts filter and order in SQL.
Cancellation and pagination enter that declaration when a source needs them, because the connectors use the same contract on those points.
Each console has a Tokio task, which keeps the session between executions while the transaction stays open.
The task keeps the future of `run` alive after a cancellation, to read the database response.
Closing the connection is the only case that abandons that future.

| Database | Cancellation | Cleaning | Transaction state |
|---|---|---|---|
| PostgreSQL | `CancelToken` | The steps of `DISCARD ALL` without `DEALLOCATE ALL`, and `DEALLOCATE` of each `PREPARE`; after a `DISCARD ALL` or `DEALLOCATE` of the user, close the session | Clock probe, through the simple protocol |
| MySQL | `KILL QUERY` on a temporary connection | `Conn::reset` and back to the database of the connection | `SERVER_STATUS_IN_TRANS` of the last response; `DO 0` after an error |
| SQLite | `InterruptHandle` | Drop the connection | `is_autocommit` |
| Redis/Valkey | `CLIENT UNBLOCK` on a temporary connection; no effect, close the session | `DISCARD`, the `CLIENT` name and modes, `UNWATCH`, and back to the database of the connection, in one round trip | Always `None`: the source has no manual transaction |

`run` sends one statement, delivers the rows to `ResultSink`, and obeys the `Flow::Stop` return.
One statement may deliver several results, such as `CALL` in MySQL, and the cap applies to each one.
`ResultSink` sends `ResultDone` at the end of each result.
Dropping `ResultSink` returns the rows still in the batch to the budget.
`ResultSink` applies the cap and the budget, and gathers the rows in batches of 200.
`ResultSink::notices` sends `ExecEvent::Notice` before the end of the statement. PostgreSQL keeps the notices in the connection task, with `poll_message`. MySQL reads `SHOW WARNINGS` when the count goes above 0. `MAX_NOTICES` limits both to 100 per statement.
`Value` keeps `Null`, `Text`, `Number`, or `Binary`, always with the database text.
`Conn::reset` keeps the current database, so a `USE` would stay in the session without the next step.
The cleaning reads `DATABASE()` and goes back to the database of the connection. Without a database in the connection, the session closes, because MySQL does not leave a database.
The variant serves presentation only, such as aligning numbers.
`Value::binary` classifies binary bytes once, before they reach the interface. The wrapper preserves the original bytes.
A preview reads only the visible prefix. TSV encoding borrows selected values without cloning the selection.

## 5. Script execution

1. The console requests the execution and receives an `ExecutionId`.
2. The application clears the previous results and frees the budget.
3. The editor delivers a copy of the text.
4. `sql` separates the statements and keeps their positions.
5. `pool` delivers a reservation, or the execution enters the queue.
6. The actor applies the selected database or schema.
7. For each statement, the actor runs `run`.
8. After each statement, the actor asks for the transaction state.
9. On error, timeout, or stop, the sequence ends.
10. Without an open transaction, `pool` cleans and returns the connection.

### Row cap

With `Settings::cap_in_sql`, the runner first sends `LIMIT` with the cap and one row on a new line at the end of a plain read. `sql::with_row_limit` decides from the text: the read must start with `SELECT`, `WITH`, `VALUES`, or `TABLE` and have no `LIMIT`, `OFFSET`, `FETCH`, `FOR`, or `LOCK` in the main level. `ExecEvent::LimitAdded` puts an `Info` line in **Messages**, and an error place stays inside the text of the user. An export and a run without a cap keep the text.

The consoles of a connection share its settings through `Binding`. A save in Settings calls `Manager::update_settings`, and each execution copies the values at its start, so a running script keeps its cap.

1. The actor delivers rows up to the cap, plus one row to know whether more exist.
2. In a read query without a transaction, the actor triggers the cancellation. `sql::is_plain_read` decides from the text.
3. In the other cases, the actor consumes the rest without keeping the rows.
4. In both cases, the actor reads until the statement response.
5. A cancellation caused by the cap does not become a statement error.

### Interruption

1. `exec` triggers `Canceller` and starts the 5-second deadline.
2. The actor keeps reading and discarding the rows. `Flow::Discard` marks that state.
3. With the response, the actor waits for the cancellation request to finish, and the connection follows the normal flow.
4. Without a response and without a transaction, `exec` closes the connection.
5. Without a response and with a transaction, the interface asks for confirmation.
6. If the request fails, the deadline ends at once.
7. If the request does not finish after the response, the session is dropped, because it could hit the next statement.

`Canceller::cancel` returns after the database handles the request.
In PostgreSQL, that requires reading until the server closes the request connection.
In MySQL, the response of `KILL QUERY` already confirms the request.

## 6. Own pool

Each connection definition has a pool, with this state:

| Field | Content |
|---|---|
| `limit` | Configured limit, with a default of five |
| `idle` | Free connections |
| `in_use` | Reserved connections |
| `opening` | Connections being established |
| `cleaning` | Connections being cleaned |
| `waiters` | Queue in arrival order |

The sum of `idle`, `in_use`, `opening`, and `cleaning` never goes above `limit`.
The temporary cancellation connection stays out of that sum.

1. With a free connection, deliver that connection.
2. Below the limit, open a connection with a `timeout` of 30 seconds over the whole operation.
3. At the limit, put the request in the queue, without a deadline.
4. When a slot frees, serve the first request still active.
5. On a connection error, return the error to the request, without a retry.
6. Cancelling a wait removes the request from the queue.
7. On reuse of an idle connection, call `is_alive`. A lost connection gives place to a new one.
8. Each idle session keeps the moment of its return. The last session to return is the first to leave, so the other sessions age.
9. With an idle limit (`idleTimeoutSeconds`, 300 by default), a task of `Manager` calls `Pool::close_expired` and closes each idle session that waited that time. The task holds a `WeakPool`, so it ends after the pool. A session in use stays, also the session of an open transaction. `ExecEvent::IdleClosed` redraws the explorer without a message.

A reserved session does not receive `apply_context` again, because a transaction with an error refuses `SET`.
A session with an unknown state is dropped.
**Disconnect** closes the idle sessions and raises the generation of the pool.
A session of a previous generation closes on return, such as one of an in-flight catalog read.
The `Disconnected` event waits for that session for up to 5 seconds, and the interface reports when it stays in use.

A connection with unconfirmed cleaning is dropped.
The replacement happens only when a new need exists.

## 7. States

| Dimension | Values |
|---|---|
| Execution | Idle, waiting for connection, running, finishing, done, cap reached, error, timeout, stopped, cancelled before start, interrupted for close |
| Transaction mode | Autocommit or manual |
| Transaction | None, open, with error, or unknown |
| Reservation | No connection, waiting, in use, or cleaning |

The interface refuses **Disconnect** and removal before it sends the command, while a console of the connection has an execution or a transaction.

| Command | Response event |
|---|---|
| `Run` | `Queued`, `Started`, `Rows`, `StatementFinished`, `ExecutionFinished` |
| `Stop` | `Stopping`, `ConfirmClose`, `ExecutionFinished` |
| `Commit`, `Rollback` | `TransactionChanged` or `TransactionFailed` |
| `CancelWait` | `ExecutionFinished` |
| `Disconnect` | `Disconnected` |
| `TestConnection` | `ConnectionTested` |

## 8. Files

### `config.json`

This schema applies to the global and project scopes.
No scope receives `password`. The plain-text authorized password stays in `credentials.json`.

```json
{
  "version": 1,
  "preferences": {
    "keyboardMode": "conventional",
    "pageSize": 100,
    "rowCap": 1000,
    "capInSql": true,
    "gridBudgetBytes": 268435456,
    "undoLimitBytes": 67108864,
    "keymap": {},
    "theme": "auto",
    "icons": "unicode",
    "reopenTabs": true,
    "queryHistory": true,
    "formatJson": true,
    "showSystemCommands": false,
    "bellAfterSeconds": 10,
    "suggestionKeys": "tab",
    "mcp": true,
    "mcpDefaultAccess": "ask",
    "mcpMaxAccess": "full",
    "mcpRememberClients": true
  },
  "connections": [
    {
      "id": "0f3726f5-16d2-46af-82f9-54736aaa81f8",
      "name": "app",
      "kind": "postgres",
      "host": "db.example.test",
      "port": 5432,
      "database": "app_db",
      "username": "app_user",
      "credential": "vault",
      "readOnly": false,
      "tls": { "mode": "verify", "caFile": null, "clientCert": null, "clientKey": null },
      "pool": { "maxConnections": 5 },
      "connectTimeoutSeconds": 30,
      "sqlTimeoutSeconds": 30,
      "idleTimeoutSeconds": 300,
      "theme": "nord",
      "schemas": ["public", "sales"]
    },
    {
      "id": "6a1d4c12-7c0e-4f0b-9a55-0c2f4b6f7d10",
      "name": "local",
      "kind": "sqlite",
      "file": "data/app.db",
      "foreignKeys": false,
      "busyTimeoutMillis": 5000,
      "readOnly": false,
      "pool": { "maxConnections": 5 },
      "sqlTimeoutSeconds": 30
    }
  ]
}
```

| Field | Values |
|---|---|
| `kind` | `postgres`, `mysql`, `sqlite`, or `redis` |
| `credential` | `vault`, `session`, `globalPlainText`, or `none` |
| `tls.mode` | `verify` or `disabled` |
| `theme` | Optional. A value of `theme` of `preferences`. Without the field, the connection uses the default theme |
| `icon` | Optional. `warning`, `skull`, `radiation`, `flag`, `crown`, `flask`, `star`, `diamond`, `tools`, `gear`, `home`, `heart`, `snowflake`, or `check`. Without the field, the connection uses the database icon |
| `schemas` | Optional. The schemas, or the Redis databases, that the catalog keeps. The current schema always stays. Without the field, or with an empty list, the catalog keeps all |

The `globalPlainText` value exists only in the global scope.
The value `sqlTimeoutSeconds: null` runs without a deadline, and the absence of the field means 30 seconds.
`idleTimeoutSeconds` defaults to 300. A value of 0 disables idle expiry. It does not close a reserved transaction session.

Reading rules, according to the code:

- `version` is required and equals 1. An unknown field invalidates the file.
- All fields of `preferences` are optional, and `keymap` has the `conventional` and `vim` sections.
- `preferences.export` keeps the choices of the export modal: `format`, `header`, `delimiter`, `nullText`, `bom`, `grouped`, `batch`, and `folder`. The application writes that field when it confirms an export, and it is valid only in the global file.
- `preferences.import` keeps the choices of the import modal: `encoding`, `header`, `delimiter`, `nullText`, `batch`, and `folder`. It exists only in the global file, as `preferences.export` does.
- An absent `credential` means `none`. An absent `pool` means 5 connections.
- A repeated `id` or `name` in the same file invalidates the file. The name ignores case.
- A relative path (`file`, `caFile`, `clientCert`, `clientKey`) stays as the user wrote it in the file. The resolution happens at connection open.
- The form writes the full path of a typed SQLite file, from the current directory. A saved and unchanged `file` keeps its text.
- A network connection without `tls` uses TLS with verification, except on `localhost`, loopback, and Unix socket.
`apply_context` reapplies `readOnly` on each reservation, because the session cleaning removes it.

### Console metadata

Each console has `consoles/<id>.sql` and `consoles/<id>.json`:

```json
{
  "version": 1,
  "id": "9b2e7d44-3f6a-4c55-8e21-1d4f0a7c9e33",
  "name": "investigate-orders",
  "connectionId": "0f3726f5-16d2-46af-82f9-54736aaa81f8",
  "connectionScope": "project",
  "schema": "public"
}
```

### Local state

`workspace.json` never enters Git.
If the file is unreadable, SQLN uses the initial state and shows a non-blocking warning.

Example:

```json
{
  "version": 1,
  "consoles": {
    "9b2e7d44-3f6a-4c55-8e21-1d4f0a7c9e33": {
      "cursor": { "row": 3, "col": 5 },
      "scroll": { "row": 0, "col": 0 }
    }
  },
  "layout": { "navigatorCollapsed": false, "navigatorWidth": 24, "editorRatio": 0.5 },
  "openTabs": {
    "tabs": [
      { "kind": "console", "scope": "project", "id": "018f…" },
      { "kind": "table", "scope": "global", "connection": "g1", "schema": "public", "relation": "users", "view": false }
    ],
    "active": 0
  }
}
```

### File list

| File | Place | Git |
|---|---|---|
| `config.json` | Global and project | Versionable in the project |
| `consoles/<id>.sql` | Global and project | Versionable in the project |
| `consoles/<id>.json` | Global and project | Versionable in the project |
| `workspace.json` | Global and project | Ignored |
| `locks/<id>.lock` | Global and project | Ignored |
| `.gitignore` | Project | Versionable |
| `trust.json` | Global only | Outside the project |
| `credentials.json` | Global only | Outside the project |
| `history.jsonl` | Global only | Outside the project |

Content of `.sqln/.gitignore`:

```text
workspace.json
locks/
*.tmp
```

`history.jsonl` keeps one line per statement, with `at`, `connection`, `ok`, and `sql`.
A connection receives `"production": true` to show the badge and confirm the changes. The field leaves the file when it equals `false`.

`trust.json` keeps the confirmed destinations and the known paths of the projects.
The implemented format has `version: 1`, `destinations`, and `projects`.
Each destination includes engine, TCP address or socket, user, and TLS configuration, without a password.
Each project records `connectionId` and a list of canonical `paths`.
The path does not take part in the vault account; a new path needs a separate confirmation.
`Trust::required` does not create files. An invalid or unreadable file returns an error, without authorizing the destination.
`Trust::confirm` takes `locks/trust.lock`, rereads the file, and writes with a hash check.
The file and its temporary file have permission `0600` on Unix.
No call writes trust into the project files.

`credentials.json` keeps the plain-text authorized passwords.
The file stays in the global folder and has permission `0600` on Unix, including the temporary file.
`PlainFile` refuses a link, a file with an open permission, and an invalid content, without overwriting.
Each entry uses the account of section 9 as key, for global and project connections.
Without a vault, the application reads that file by identity before it asks for the password.
Changing the destination, the user, or the TLS changes the key, so the previous password is not sent.

## 9. Credential reference

In the vault, each entry uses the service `sqln` and an account with this format:

```text
<scope>/<connection-id>/<engine>/<escaped-JSON-address>/<escaped-user>/<escaped-JSON-TLS>
```

`Identity::account` implements that format with percent escaping per component.
The engine also takes part in the identity, to separate PostgreSQL from MySQL.
The address includes host and port, or the socket path.
The TLS includes mode and the configured paths of CA, client certificate, and key.
Use the paths as configured, without including the project root in relative paths.
The separate confirmation of the project path controls reuse after a folder move or copy.
The escaping prevents collisions among separators, percent signs, and components with arbitrary text.

Changing the destination, the user, or the TLS changes the account.
Thus, the application does not find the previous password and needs to ask for another one.
Renaming the connection does not change the account.
The folder path is not part of the account.

**Defined:** use `keyring-core` with a store specific to each platform.
Use Keychain on macOS and Secret Service on Linux.
The documentation of `keyring` 4 recommends that path for applications.
`SystemVault` already offers reading, writing, and deletion with its own store, without changing the global default of the process.
`Credentials` keeps session passwords per `Identity` and holds the password in memory after a write failure.
`save_verified` writes and checks the reading; it removes no plain-text source.
Vault errors use fixed messages, without the provider text or password bytes.
`app::auth` already uses those adapters. The automated tests inject `Options::vault`, without touching the real vault.

The table below describes the real responses, according to the [plan validation](INITIAL_PLAN.md#validation-of-the-real-vaults).

| Real response | System | Result | Action |
|---|---|---|---|
| `NoEntry` | macOS and Linux | No credential | Ask for the password |
| Missing bus socket | Linux | `Absent` | Apply the missing vault rules |
| D-Bus `ServiceUnknown` | Linux | `Absent` | Apply the missing vault rules |
| `secret_service::Error::Unavailable` | Linux | `Absent` | Apply the missing vault rules |
| `Locked`, `NoResult`, and `Prompt` | Linux | `NoAccess` | Offer retry and session password |
| `-25244` | macOS | `NoAccess` | Offer retry and session password |
| `-128`, `-25293`, `-25299`, `-25307`, and `-25308` | macOS | `Failed` | Offer retry and session password |
| Unknown failure | macOS and Linux | `Failed`, never `Absent` | Offer retry and session password |

`is_absent` accepts only the three `Absent` responses, by the error type.
On macOS, the Keychain is part of the system, so no failure means a missing vault.
`SystemVault` uses its own store, so `NoDefaultStore` does not happen on that path.
A locked Keychain returns `-25293` or `-128`, not `NoStorageAccess`, and the interface offers the same options.

On missing access or error, offer **Try again**, **Use password in this session**, and **Cancel**.
Ask for the password before the first use that needs it, including test and catalog read.
Start the prompt with session-only use; saving needs an explicit choice.
The trust confirmation shows destination, user, and TLS, with **Cancel** as initial focus.
The real tests confirm these criteria on Keychain and Secret Service in a container.
Repeat the Secret Service on a real Linux desktop before the release.

### Refused password

`ConnectError::password_refused` marks only the database refusal: `28P01` in PostgreSQL, `1045` and `1698` in MySQL.
An error message does not prove the refusal, and `28000` also covers a `pg_hba.conf` rule.
`ExecEvent::PasswordRefused` carries the pool number. An old pool does not speak about the current password.
Several sessions may fail with the same password, so only the first event counts.

| State | Behavior |
|---|---|
| First refusal | Open the prompt. The new password passes `test_connection` before the write |
| Second refusal | Suspend the definition and show the warning |
| During suspension | Catalog and columns do not open a session; each explicit action opens the prompt |
| Password accepted or connection edit | End the suspension. The pending action continues, because no SQL reached the database |

A network or TLS failure during the check shows the error and does not count as a refusal.
With sessions still open, the new password only enters after a disconnect.

## 10. Recovery flows

### File write

1. Write the temporary file in the same folder. For `config.json`, `trust.json`, and `credentials.json`, use an exclusive name and permission `0600` on Unix.
2. Sync the file to disk.
3. Compare the hash of the target file with the last known hash.
4. Rename the temporary file over the target.
5. On failure, remove the temporary file, keep the text, and show the alert.

### Unexpected shutdown

1. At startup, remove the old `.tmp` files.
2. The system releases the locks of the ended process.
3. The database ends the open sessions and transactions, according to its semantics.
4. The application does not try to recover transactions and runs no SQL.

The saving can lose up to 500 ms of typing.

### Password write failure

1. Keep the password in memory.
2. Report that the password was not saved.
3. Offer an explicit retry.

### Migration to the vault

1. Detect a global password with a vault available.
2. Ask for confirmation.
3. Write the password to the vault.
4. Read the password from the vault and compare.
5. Offer removal of the global password.
6. On any failure, keep the global password and report the incomplete migration.

### Attaching a script without connection

1. The user picks a connection.
2. If the scope changes, the interface shows the new location.
3. The user confirms.
4. Copy the `.sql` and the `.json` to the destination.
5. Verify both files.
6. Remove the originals.
7. Update `connectionId` and `connectionScope`.

The application runs no SQL in that flow.

### Project moved or copied

1. Compare the project root with the paths of `trust.json`.
2. If the path is new, ask for confirmation before reusing a credential.
3. Record the new path after the confirmation.

### Connection loss

1. Idle connection: remove from the pool, without a message.
2. Connection running: fail the statement and do not repeat the SQL.
3. Open transaction: mark the state as unknown.
4. Before the next execution in that console, require confirmation.
