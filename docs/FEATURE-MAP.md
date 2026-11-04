# SQLN — Feature map

## State of this document

This document lists each feature of the application, its main files, and how it works.
The [architecture](ARCHITECTURE.md) gives the deep rules; the [plan](INITIAL_PLAN.md) gives the functional rules.
The file paths are relative to `src/`.

## Map

| # | Feature | Main files |
|---|---|---|
| 1 | Application shell and main loop | `app.rs`, `app/keys.rs`, `app/mouse.rs`, `app/hints.rs`, `app/close.rs`, `terminal.rs` |
| 2 | Key map and discovery | `input.rs`, `app/commands.rs`, `ui/which_key.rs`, `palette.rs`, `filter_list.rs`, `settings/shortcuts.rs`, `ui/text.rs` |
| 3 | Navigator and tree filter | `navigator.rs`, `ui/navigator.rs`, `catalog.rs`, `generate.rs` |
| 4 | Connections and connection form | `connection.rs`, `form.rs`, `ui/form.rs`, `app/connections.rs`, `connector/target.rs` |
| 5 | Database connectors | `connector/mod.rs`, `connector/postgres.rs`, `connector/mysql.rs`, `connector/sqlite.rs`, `connector/redis.rs`, `connector/tls.rs` |
| 6 | Connection pool | `pool.rs` |
| 7 | Script execution | `exec/mod.rs`, `exec/runner.rs`, `exec/sink.rs`, `console.rs` |
| 7a | Query plan | `plan.rs`, `console.rs`, `result.rs`, `app/keys.rs` |
| 7b | Messages command history | `command_log.rs`, `console.rs`, `ui/messages.rs`, `exec/runner.rs` |
| 8 | Run protection | `app/guard.rs`, `sql/classify.rs` |
| 9 | Transactions | `exec/runner.rs`, `ui/statusline.rs` |
| 10 | SQL editor | `editor/mod.rs`, `editor/conventional.rs`, `editor/undo.rs`, `editor/buffer.rs`, `editor/search.rs`, `find.rs` |
| 11 | SQL analysis and formatter | `sql/lexer.rs`, `sql/statement.rs`, `sql/classify.rs`, `sql/format.rs` |
| 12 | Autocomplete | `complete/mod.rs`, `complete/context.rs`, `complete/grammar.rs`, `complete/vocabulary.rs`, `complete/keywords.rs`, `catalog.rs` |
| 13 | Catalog and structure tab | `catalog.rs`, `structure.rs` |
| 13d | DDL of an object | `definition.rs`, `app/definition.rs`, `compare/ddl.rs`, `compare/sql.rs` |
| 13a | Server sessions | `structure.rs`, `table.rs`, `app/tables.rs` |
| 13b | ER diagram | `diagram/mod.rs`, `diagram/sql.rs`, `diagram/layout.rs`, `diagram/mermaid.rs`, `diagram/tab.rs`, `ui/diagram.rs`, `app/diagram.rs` |
| 13c | Structure and data compare | `compare/mod.rs`, `compare/sql.rs`, `compare/schema.rs`, `compare/ddl.rs`, `compare/data.rs`, `compare/form.rs`, `compare/tab.rs`, `ui/compare.rs`, `app/compare.rs` |
| 14 | Results grid | `result.rs`, `grid/mod.rs`, `grid/row.rs`, `grid/budget.rs`, `grid/search.rs`, `grid/tsv.rs`, `grid/json.rs`, `clipboard.rs` |
| 15 | Table view and key navigation | `table.rs`, `app/tables.rs`, `ui/table.rs` |
| 16 | Export and file explorer | `export/mod.rs`, `export/form.rs`, `ui/export.rs`, `app/export.rs`, `browser.rs`, `ui/browser.rs`, `app/browse.rs` |
| 16a | Import | `import/mod.rs`, `import/delimited.rs`, `import/json.rs`, `import/statement.rs`, `import/feed.rs`, `import/form.rs`, `exec/runner/import.rs`, `app/import.rs`, `ui/import.rs` |
| 17 | Query history | `history.rs`, `store/history.rs`, `app/history.rs`, `ui/history.rs` |
| 18 | Folders and configuration | `store/mod.rs`, `store/config.rs`, `store/file.rs`, `app/persist.rs`, `settings.rs`, `ui/settings.rs` |
| 19 | Console saving and saved consoles | `script_file.rs`, `store/scripts.rs`, `store/workspace.rs`, `app/scripts.rs`, `app/conflicts.rs`, `ui/conflicts.rs`, `ui/name.rs`, `app/session.rs` |
| 20 | Credentials and trust | `creds.rs`, `trust.rs`, `app/auth.rs`, `ui/auth.rs` |
| 21 | Appearance and themes | `ui/theme.rs`, `ui/schemes.rs`, `ui/icons.rs`, `ui/statusline.rs`, `ui/wrap.rs` |
| 22 | Operation and diagnostics | `main.rs`, `terminal.rs`, `log.rs` |

## 1. Application shell and main loop

Files: `app.rs`, `app/keys.rs`, `app/mouse.rs`, `app/hints.rs`, `app/close.rs`, `terminal.rs`.

`run` owns the application state and the screen. The main loop waits for a key for up to 20 ms, and then drains execution events for up to 8 ms per frame. The application draws only after a change. `app/keys` applies the key priority of [KEYMAP](KEYMAP.md), and `app/mouse` handles clicks, the wheel, and the drag of dividers and columns. `app/hints` builds the hint row from the state on screen. `app/close` asks the exit questions in order, one console at a time, and applies each answer at once. `terminal` sets up the terminal and restores it at exit and on panic.

## 2. Key map and discovery

Files: `input.rs`, `app/commands.rs`, `app/keys.rs`, `app/hints.rs`, `ui/which_key.rs`, `palette.rs`, `filter_list.rs`, `ui/text.rs`.

`input` maps each key to an action in each focus region. It supports multi-key sequences and reads the user `keymap` rules from `config.json`. `Ctrl+W` opens the prefix menu (`ui/which_key`) with the second keys in groups. `fits_cursor` keeps the commands that have an effect now and belong to the panel of the cursor, and the keys of the other commands keep their action. `Ctrl+W Tab` opens its explorer page: `explorer_commands` lists what acts on the current row, and a key of the page goes through the `Navigator` region of the key map, so the page needs no bindings of its own. `Ctrl+W Space` opens the functional commands of the current panel with effective local shortcuts. `app/commands` shares availability, ordering, and target labels between discovery paths; `Action` remains the catalog. The palette includes editor and grid commands and keeps actions without shortcuts. Explorer row actions require explorer focus; add connection and scripts remain global. `filter_list` matches label, key, action name, and technical aliases. The normal hint row reserves the palette shortcut and fits more whole commands as width grows. The same list filter core serves the history window. `ui/text` keeps all interface text in one file. A new console without text shows the first screen (`ui/welcome`), with the main keys, and the first character ends it. Without a tab, the same widget draws the home screen with a logo: the active console is then the hidden console, `App::home` is true, and `home_allows` keeps only the explorer commands and the commands that need no tab.

## 3. Navigator and tree filter

Files: `navigator.rs`, `ui/navigator.rs`, `catalog.rs`, `generate.rs`.

The navigator shows the tree of connections, consoles, schemas, relations, columns, and the groups of the other objects of a schema. `Connection::consoles` holds the console rows, so a read of the tree does not read the disk. `refresh_tree_consoles` rebuilds them after a change of a console, only for the connections open in the tree. `/` filters the tree in memory, from the loaded catalogs, and opens no connection. `g` lists the SQL templates of a relation and the **DDL** entry, and the application inserts the generated text. `y` copies the row name without quotes. An object copy omits routine arguments and the table of a trigger.

`RowKind::Group` is a group of one kind (`catalog::Group`) under a schema, and `RowKind::Object` is one object of `Schema::objects`, with `Row::relation` as its place. `push_groups` adds the groups after the relations, and a group without objects stays out. The cursor key keeps the group and the label of the object, which is unique in its group (`Object::label`), so the cursor stays on an overload after a new read.

## 4. Connections and connection form

Files: `connection.rs`, `form.rs`, `ui/form.rs`, `app/connections.rs`, `connector/target.rs`.

`Definition` holds the connection settings, and `Source` selects a SQLite file or a network target.
`ConnectionKey` holds the saved connection ID and its global or project scope. The form holds text only and builds a definition on request. `app/connections` owns the catalogs and the actions of the navigator and the form. A file or a URI from the command line enters `connector/target`; that connection lives for the session only and writes no file. `p` in the navigator erases the saved password, and the removal of a connection offers that deletion separately.

## 5. Database connectors

Files: `connector/mod.rs`, `connector/postgres.rs`, `connector/mysql.rs`, `connector/sqlite.rs`, `connector/redis.rs`, `connector/tls.rs`.

`Connector` declares the dialect and the `Capabilities`. `Session` runs one statement, delivers the rows to `ResultSink`, and reports the transaction state. Only this module knows driver types. PostgreSQL reads through the simple protocol, so each value keeps the database text. MySQL keeps the text and reads notices with `SHOW WARNINGS`. SQLite runs `rusqlite` on its own thread, because the driver is synchronous. Redis runs one command per line, with the quote rules of `split_words`. TLS uses verification by default, except on `localhost`, loopback, and Unix sockets; `connector/tls` reads CA and client certificate files.

Cancellation uses a `Canceller` separate from the session: `CancelToken` on PostgreSQL, `KILL QUERY` on a temporary connection on MySQL, `InterruptHandle` on SQLite, and `CLIENT UNBLOCK` on Redis. Cleaning returns the session to its base state (`DISCARD ALL` without `DEALLOCATE ALL`, `Conn::reset`, drop, or `DISCARD`, the `CLIENT` modes, and `UNWATCH`).

## 6. Connection pool

Files: `pool.rs`.

Each connection definition has one pool. The default limit is five. The pool serves a free idle connection, opens below the limit with a 30 s deadline, or queues the request in arrival order. On reuse, `is_alive` removes a lost connection. Cleaning happens before the return, and a session with unconfirmed cleaning is dropped. **Disconnect** closes the idle sessions and raises the generation; sessions of an old generation close on return.

## 7. Script execution

Files: `exec/mod.rs`, `exec/runner.rs`, `exec/sink.rs`, `console.rs`.

The console sends a copy of the text and receives an `ExecutionId`. `sql` separates the statements, and the pool delivers a reservation or queues the request. Each console has one task (`exec/runner`) that owns the reserved session. The task runs one statement at a time, applies the row cap plus one row, and asks the transaction state after each statement. With `cap_in_sql`, `sql::with_row_limit` adds that number as `LIMIT` to a plain read without its own limit, and `ExecEvent::LimitAdded` reports it. On error, timeout, or stop, the sequence ends. `exec/sink` batches 200 rows, applies the cap and the budget, and sends notices before the end of the statement; a cap cancellation is not a statement error.

Stop triggers the `Canceller` and starts a 5 s deadline. The task keeps reading and discards the rows. Without an answer and without a transaction, `exec` closes the connection. With an open transaction, the interface asks for confirmation. The application drops an event whose `ExecutionId` is not current, so an old response never refills a grid.

## 7b. Messages command history

Files: `command_log.rs`, `console.rs`, `ui/messages.rs`, `exec/runner.rs`.

Each console keeps a 10 MiB in-memory command history. New runs and connection changes keep it; closing the console discards it.
Records show effective commands, separate parameters, results, and elapsed time. Oversized text has an explicit truncation marker.
Internal and shared records stay hidden until **System commands** is on in Settings > Results. Hidden records still use the budget.
Shared operations capture the active console of the same connection at operation start. Import, authentication, and hidden driver operations stay out.
The view follows new records only at the end. This history is separate from diagnostic logs and `history.jsonl`.

## 7a. Query plan

Files: `plan.rs`, `console.rs`, `result.rs`, `app/keys.rs`.

`Ctrl+W E` and `Ctrl+W A` follow the path of a run with a `RunTarget::Plan`. `plan::Explain` adds the prefix of the dialect, and the editor marks keep the text of the user. A plan needs one statement, and a statement with `EXPLAIN` is refused.

`Console::explain` marks the execution. When the result ends, `plan_view` reads the text of the database and `ResultTab::show_plan` replaces the grid with the tree. PostgreSQL gives JSON, MySQL gives the tree text, and SQLite gives rows with a parent. The text stays after a limit, a text that the reader refuses, or a full budget, and **Messages** explains the reason. The `Share` column subtracts the children of each node, because a database reports a value with the nodes under it. It uses the real time when the plan has it, and the cost when it does not.

A plan has no statement for a new read, so an export uses the loaded rows. The query history does not record a plan.

## 8. Run protection

Files: `app/guard.rs`, `sql/classify.rs`.

Before a run, the application asks one question in two cases: a write without `WHERE` (`sql::lacks_where`), or a state change on a production connection (`sql::changes_state`). A connection with `"production": true` shows the badge. Redis asks before a command outside its read list.

## 9. Transactions

Files: `exec/runner.rs`, the `Session` methods of each connector, `ui/statusline.rs`.

A console runs in autocommit by default. Manual mode sends `TransactionCommand` for commit and rollback. Each driver reports its state its own way: `is_autocommit` on SQLite, `SERVER_STATUS_IN_TRANS` on MySQL, and a clock probe through the simple protocol on PostgreSQL. An unknown state demands confirmation before the next run. The status line shows the age of an open transaction, with the error color from 5 min.

## 10. SQL editor

Files: `editor/mod.rs`, `editor/conventional.rs`, `editor/undo.rs`, `editor/buffer.rs`, `editor/search.rs`, `find.rs`.

EdTUI holds the text. In the conventional mode, `editor/conventional` owns the selection with IDE keys, and EdTUI stays in insert mode. The Vim mode is the alternative; both modes share the run actions. `editor/buffer` holds the text operations on `Lines`. Search is literal and without case (`find.rs`), with one shared bar for both modes and replace support. `editor/undo` holds the history of the conventional mode, and `undoLimitBytes` limits its size. The Vim mode uses the stacks of EdTUI, which have no memory cap, and [ARCHITECTURE](ARCHITECTURE.md) lists that debt.

## 11. SQL analysis and formatter

Files: `sql/lexer.rs`, `sql/statement.rs`, `sql/classify.rs`, `sql/format.rs`.

The lexer tokenizes per dialect. The statement splitter keeps the positions and handles `DELIMITER`; the table view refuses a text with a second command or an open token. `sql/classify` decides three classes: a plain read (`is_plain_read`), a missing `WHERE` (`lacks_where`), and a state change (`changes_state`). The formatter moves only line breaks and spaces, so each token stays the same.

## 12. Autocomplete

Files: `complete/mod.rs`, `complete/context.rs`, `complete/grammar.rs`, `complete/keywords.rs`, `complete/vocabulary.rs`, `catalog.rs`.

`complete/context` reads the text around the cursor and finds the expected kind of name. The suggestions come from the catalog: schemas, tables, views, and columns read on demand, with aliases, quotes, and the keywords of the dialect. After `ON`, the suggestion uses the foreign keys that the application reads together with the columns. The `suggestionKeys` preference chooses the key.

`complete/grammar` selects built-in types for `CREATE TABLE`, `ALTER TABLE`, and `CAST` per dialect. Column options and query clauses use the cursor context. Functions show parameter signatures in the list. Acceptance inserts empty parentheses and puts the cursor inside. Existing parentheses and arguments stay unchanged. The console and table fields use the same rules. These suggestions use local lists, with no database request.

## 13. Catalog and structure tab

Files: `catalog.rs`, `structure.rs`.

The catalog is a cache in memory, outside the disk. Schemas and tables load first; columns and foreign keys load on demand. A key failure does not fail the columns. The structure tab runs the catalog queries of `structure`, per database, scope, and section.

## 13d. DDL of an object

Files: `definition.rs`, `app/definition.rs`, `compare/ddl.rs`, `compare/sql.rs`.

`definition::definition_sql` gives the queries of the DDL of a `Target`: a table, a view, or an object of `Schema::objects`. A database function gives the text where the database has one: `pg_get_functiondef`, `pg_get_triggerdef`, and `pg_get_viewdef` on PostgreSQL, `SHOW CREATE` on MySQL, and `sqlite_schema.sql` on SQLite. For a PostgreSQL table, `compare::sql::postgres_table_sql` reads the model of the compare for one table, and `compare::ddl::create_text` writes the table, its indexes, and its foreign keys. SQLN also writes a sequence, an enum, and a domain of PostgreSQL. `definition_text` adds the header and ends each statement with one semicolon.

`app/definition` opens a task of the pool for the read only (`Manager::open_console`), and the password gate passes it as `Use::Definition`. The read runs with only the schema in the path of PostgreSQL, as the compare does, so the names of that schema have no schema. `on_definition_event` takes the events of that task before the tabs. At the end, the task closes, and `open_text_console` opens a new console of the connection with the text, as the script of a compare does.

## 13a. Server sessions

Files: `structure.rs`, `table.rs`, `app/tables.rs`.

`S` in the navigator opens a structure tab with `Scope::Sessions`, so the tab reuses the pages, stop, budget, search, copy, and export. PostgreSQL reads `pg_stat_activity`, and MySQL reads `information_schema.PROCESSLIST`. The query leaves out the session that reads the list. The first column is the number of a session, the second is its user, and the last is its query.

`q` and `K` read the row of the cursor and ask first. The answer sends a `PageKind::Signal` request through the task of the tab. `signal_sql` receives a `u64`, so no text of a cell reaches the SQL. A signal that the database accepts marks the tab as `pending`: the tab on screen reads the sessions again at once, and another tab reads them when it comes back. An error of the database stays in the tab, and the rows stay.

## 13b. ER diagram

Files: `diagram/mod.rs`, `diagram/sql.rs`, `diagram/layout.rs`, `diagram/mermaid.rs`, `diagram/tab.rs`, `ui/diagram.rs`, `app/diagram.rs`.

`d` in the navigator, or `Ctrl+W D`, opens a `Tab::Diagram`, a third kind of main tab. `row_diagram` and `diagram_target` give the target and the reason of a refusal, so the key, the hint row, and the menu agree. A connection without its catalog waits in `pending_diagrams`, and the catalog read opens the diagram of the current schema. Each connection keeps its place in the list, so a second request does not cancel the first. The tab has its own executor task, so the read has the time limit, `Ctrl+W x`, the pool queue, the password, and the budget of a page. `diagram/sql` writes two queries per database: the columns of each table of a schema with their place in the primary key, and each pair of columns of the foreign keys that leave or reach the schema. One execution runs both, with no row cap, and the tab parses the rows only at the end. `Model::build` joins the pairs of each key by table and name, gives a SQLite key without target columns the primary key of its target, and makes a stub of each table of another schema. A table focus keeps the table and the tables that its keys join.

`diagram/layout` is pure and gives the same picture for the same model. The parent of a key stands on the left of its child: a depth search turns the keys of a cycle, and the longest path gives the layers. A key that crosses layers passes one row of each layer between. Four sweeps order each layer by the mean place of its neighbors. Each gutter gives each vertical part of a line its own track, from the left, and a line that meets the left layer on a row where another line meets the right layer takes a track on the left of it. The pieces of the lines stay in one list per row, and the glyph of a cell joins the arms of each piece. The tables without keys stand in rows under the layers. An arrow selects the nearest box that starts past the edge of the selected box.

`diagram/mermaid` writes the `erDiagram` text of the boxes on screen. Each entity uses an `e<n>` id and its name as an alias, because a database name can hold any character. A type or a column name keeps only the characters that Mermaid accepts, and a changed name keeps the original name as a comment.

## 13c. Structure and data compare

Files: `compare/mod.rs`, `compare/sql.rs`, `compare/schema.rs`, `compare/ddl.rs`, `compare/data.rs`, `compare/form.rs`, `compare/tab.rs`, `ui/compare.rs`, `app/compare.rs`.

`c` in the explorer, or `Ctrl+W v` in any panel, opens the dialog. `compare_here` gives the source of `Ctrl+W v`: the row of the explorer with the focus, the sides of a compare tab, the focus of a diagram, the table of a table tab, or the current schema of a console. The row gives the source: a connection gives its current schema, a schema gives itself, and a table or a column gives its table. A table starts a data compare, and a connection or a schema starts a structure compare. The target is the next connection, with the same schema and table, and each field can change. The dialog reads the catalog of a connection that has none. A connection, a schema, and a table field open a `FieldList` with the focus: the names that the text keeps (`filter_list::holds_words`), and a mark. The application gives each schema its tables (`SchemaChoice`), without views. `draw_anchored_list` in `ui/mod.rs` draws the list, as it draws the suggestions of the editor. Redis, Valkey, a view, and a console row refuse with a reason.

The tab is a fourth kind of main tab (`Tab::Compare`). It has one executor task for each side, so each read has the pool, the password, the stop, and the budget of its own connection. `compare/sql` writes the queries of each database: the columns, the keys and checks, the indexes, and on SQLite the text of each table and index. Each side runs them in one execution, with the schema in the context of the session and no row cap. On PostgreSQL that context is `search_path`, so the definitions name the objects of the schema without the schema.

`compare/schema` builds one model per side and compares them. Tables and columns match by name. Keys and indexes match by name on PostgreSQL and MySQL, and by their columns on SQLite and between two dialects. Between two dialects, the type and the default of a column do not count, and the checks stay out. A table focus keeps only that table, and the source table takes the name of the target table.

The grid has one row per difference: the change, the object, the table, the name, and a short text of each side. `S` opens the script of `compare/ddl` in a new console of the target, with the schema of the target, and `Y` copies it. Nothing runs the script, so the run protection acts when the user runs the console. The script removes the keys first, then the columns and the tables, creates the tables, changes the columns, and adds the indexes and the foreign keys last. A removal stays a comment unless the dialog asks for statements. SQLite rebuilds a table for a change that `ALTER TABLE` cannot make. Two dialects give no script.

A data compare reads in three steps. First, each side reads the columns of its table, and `compare::data::plan` checks the primary key of the source and the columns of the target. Then the source reads each row, and `DataCompare` keeps the rows in the budget with an index of their keys. Last, the target reads each row, with no time limit, and each batch meets the source rows at once: an equal row leaves the budget. A full budget ends the compare with no result. NULL equals only NULL, two numbers compare by their decimal value (`decimal`), and other values compare by their bytes. The grid has a `Change` column and the columns of the source: an update takes two rows, the source row and the target row. The script has one statement per row in one transaction, with the literals of the target dialect. A column that the target makes itself stays out of the script, and a PostgreSQL key with `GENERATED ALWAYS AS IDENTITY` gets `OVERRIDING SYSTEM VALUE`.

## 14. Results grid

Files: `result.rs`, `grid/mod.rs`, `grid/row.rs`, `grid/budget.rs`, `grid/search.rs`, `grid/tsv.rs`, `grid/json.rs`, `clipboard.rs`.

A console result and a table page share `result::ResultTab`: one grid with cursor, selection, and search. The grid keeps result tabs, the **Messages** tab, and a detail view. `grid/row` keeps each row in one block, and a read gets a `Value` that borrows from it. `grid/budget` counts the bytes of the loaded rows; a reservation frees its cost on drop. `grid/search` searches the loaded rows only, never the database, and advances 8 ms per loop turn. `r` opens the row window. `f` formats a JSON value with `grid/json`; only the white space changes. `grid/tsv` builds the copy text, with the DataGrip shape and the data that DataGrip loses; `clipboard` prefers the system clipboard and falls back to OSC 52.

## 15. Table view

Files: `table.rs`, `app/tables.rs`, `ui/table.rs`.

`table::page_sql` builds `SELECT *` with `WHERE`, `ORDER BY`, and `LIMIT n+1 OFFSET k`, each clause on one line. The extra row proves that a next page exists, and **Load more** uses the loaded rows as the offset. `c` counts the rows with `COUNT(*)` on the same path. The header sorts, and the filter bar accepts SQL with autocomplete. A source without `Capabilities::table_filters` hides `WHERE`, `ORDER BY`, **Clear**, and sorting, and turns **Apply** into **Reload**.

`F` and `Ctrl+W F` follow a foreign key of the row of the cursor. `Relation::foreign_keys` holds the keys that leave the table, and `Relation::referencing` the keys of other tables that reach it; both arrive with the columns (`Manager::load_columns`, `Session::read_referencing_keys`). `table::follow_choices` writes one `Follow` for each key without `NULL` in the row, with a filter of `quote_identifier` and `value_literal`. `open_relation` with a filter gives the filter to an open tab of the target, or opens a tab with it. Keys that are not in memory make `F` wait in `pending_follow` for `ColumnsLoaded`. A Redis key tab pages values by type, with the internal `SQLN.VALUE` text of `connector::redis::value_page`; that text is not a server command.

## 16. Export

Files: `export/mod.rs`, `export/form.rs`, `ui/export.rs`, `app/export.rs`, `browser.rs`, `ui/browser.rs`, `app/browse.rs`.

The dialog holds the choices and writes nothing. The formats are CSV, TSV, JSON, JSON Lines, and SQL inserts. The rows come from the selection, the loaded rows, or all rows; a full read of all rows needs a new read only when the grid does not hold each row. The options cover header, delimiter, null text, BOM, grouping, and batch. A confirmation writes the choices to `preferences.export` in the global file. `app/export` runs the flow: dialog, questions, rows, and the end of the file. The file explorer of `browser` serves the path fields of the export and of the connection form; `Ctrl+O` or the `...` button opens it.

## 16a. Import

Files: `import/mod.rs`, `import/delimited.rs`, `import/json.rs`, `import/statement.rs`, `import/feed.rs`, `import/form.rs`, `exec/runner/import.rs`, `app/import.rs`, `ui/import.rs`.

`I` opens the import dialog for an existing writable SQL table. CSV, TSV, JSON, and JSON Lines use column mapping and a preview.
The executor writes batches with no SQL time limit. All or nothing rolls back on failure; By batch keeps completed batches.
Log errors retries a failed batch row by row and writes failed rows to the chosen log, up to the error limit.
Only the Import button starts writes. Production asks first. Completion shows the result and reloads the table.
The global `preferences.import` stores format choices, not the failure mode. Each dialog starts with All or nothing.

## 17. Query history

Files: `history.rs`, `store/history.rs`, `app/history.rs`, `ui/history.rs`.

Eligible user statements from saved connections append a JSON line to global `history.jsonl`, with `at`, `connection`, `ok`, and `sql`.
Plans, imports, internal reads, oversized statements, and immediate repeats stay out. `app/history` writes the note of each statement. `Ctrl+W H` opens the window, and the filter uses the same core as the palette. The secret filter excludes recognized secret-bearing statements. It does not remove sensitive values inside arbitrary SQL literals. The `queryHistory` option turns the feature off.

## 18. Folders and configuration

Files: `store/mod.rs`, `store/config.rs`, `store/file.rs`, `app/persist.rs`, `settings.rs`, `ui/settings.rs`.

The store has the global folder and, when it exists, the project `.sqln/` folder; `--project` chooses another project. The load reads both files, and one error blocks the start with the file and the position in the message. Each write of `config.json` takes `locks/config.lock`, rereads the file, applies the change, and writes atomically: exclusive temporary, permission `0600` on Unix, sync, and rename. `app/persist` checks each second for external changes and offers **Reload**. The settings dialog edits the preferences of one scope, shows the inherited values, and removes a project definition.

## 19. Console saving and saved consoles

Files: `script_file.rs`, `store/scripts.rs`, `store/workspace.rs`, `app/scripts.rs`, `app/conflicts.rs`, `ui/conflicts.rs`, `ui/name.rs`, `app/session.rs`.

Each console has a `consoles/<id>.sql` file and a `consoles/<id>.json` file; the UUID is the `id` and the base of both names. The autosave waits 500 ms after the last edit and retries after 5 s on failure; `Ctrl+S` saves at once. The write is atomic, and a failure keeps the text and shows the alert. A `locks/<id>.lock` file with `flock` keeps one instance per console. `ScriptFile::poll` rereads the file only when the size or the date changed, and then compares the hash; one question lists the scripts that another program changed or removed. `workspace.json` keeps cursor, scroll, layout, and open tabs; at startup, the application reopens the tabs of the last session when `reopenTabs` is on. `app/scripts` lists the consoles, opens them, and manages the scripts without a connection; the rename prompt of `ui/name` serves the tab and the rows of the lists. The attach flow copies both files, verifies them, removes the originals, and runs no SQL.

## 20. Credentials and trust

Files: `creds.rs`, `trust.rs`, `app/auth.rs`, `ui/auth.rs`.

`app/auth` is the gate before each operation that needs a session: execution, catalog, columns, pages, and connection test. It checks the trust on the destination and the project path, asks the confirmations, and then gets the password from the `credential` reference: `none`, `session`, `vault`, or `globalPlainText`. The vault account is the service `sqln` with scope, ID, engine, address, user, and TLS in the name; a change of any part asks for a new password. The blocking calls run one at a time on `Manager::spawn_blocking`, off the interface thread, and a token stops a cancelled job.

A vault failure offers retry, session use, or cancel, never silent plain text. Without a vault, `credentials.json` serves, with migration to the vault after a confirmation. A refused password (`28P01` on PostgreSQL, `1045` and `1698` on MySQL) opens the prompt; the new password passes a test before any write, and the second refusal suspends the automatic reads. `Secret` and the passwords zeroize their buffers on drop. `trust.json` keeps the confirmed destinations and the canonical paths of the projects, written under `trust.lock`; an invalid file authorizes nothing.

## 21. Appearance and themes

Files: `ui/theme.rs`, `ui/schemes.rs`, `ui/icons.rs`, `ui/statusline.rs`, `ui/wrap.rs`.

The themes are `auto`, `astrodark`, `ansi`, `tokyo-night`, `catppuccin-mocha`, `catppuccin-latte`, `gruvbox`, and `nord`. The RGB schemes need a truecolor terminal, and `NO_COLOR` removes the colors. Each connection can select a theme and an icon. The icons come in Unicode and Nerd Font sets. The status line shows the focus, the connection, the state, and the use of resources. `ui/wrap` wraps the text of a window row by row, on demand.

## 22. Operation and diagnostics

Files: `main.rs`, `terminal.rs`, `log.rs`.

The command line accepts `--project`, `--dialect`, `--log`, `--help`, and `--version`, and one target: a SQLite file or a connection URI. The setup covers mouse, paste, and the small terminal warning. The restore runs at exit and on panic, with a panic report. Shutdown signals save pending text and exit without questions. Interface quit uses the close and transaction questions. `--log` records kinds of events, never SQL, values, or passwords. The BEL sounds after `bellAfterSeconds`, with a default of 10 s.
