# Decisions — Results and grid

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Result tabs by statement number — 2026-11-20

The user asks for result tabs like DataGrip: each statement of the console has a number, its result opens a tab with that number, and a new run of the same statement replaces only that tab.
This entry replaces the rule "clear the previous result tabs when accepting a new execution" of the [initial plan](../INITIAL_PLAN.md) and the retention of the [analysis of loaded results](#analysis-of-loaded-results).

| # | Point | Approved rule | Rejected | Reason |
|---|---|---|---|---|
| 1 | Identity | The place of the statement in the whole editor text when it runs. A selection or a subquery takes the number of the statement around it | Follow the edits of the editor; the text of the statement | Simple and predictable. Inserting a statement above renumbers the statements below |
| 2 | Name | `#3`; more grids of the same statement are `#3.2`, `#3.3`; a plan is `Plan #3` | `Result 3`; the start of the SQL | Short, so many tabs fit at 80 columns |
| 3 | Replacement | The first grid of `#n` in a run removes every tab of `#n`. A plan replaces only `Plan #n` | Keep the old grids | A tab must describe the last run of its statement |
| 4 | Without a grid | A statement that ran and gave no grid (a change, DDL, an error, a timeout, a stop, an unknown end) removes the old tabs of its number. A statement that did not run keeps them | Keep the old tab; mark it as old | The old grid no longer describes the statement |
| 5 | Other numbers | A run touches only the numbers of its statements; the other tabs stay until `x` or `Delete` | **Run all** clears the rest | Nothing leaves without a request |
| 6 | Order | By number; the grids of a statement in order; the plan after them | By recency | A replaced tab keeps its place |
| 7 | Tab on screen | The first grid of a run comes on screen. At the end, the rule "Tab after the execution" of [Execution](execution.md) applies to the results of this run | The first result | The recorded rule keeps a stop visible |
| 8 | Messages | Keeps the messages of every run, under the existing byte limit. `Messages (n)` counts the warnings of the last run | Only the last run | The user chose a log |
| 9 | Full budget | A run is refused when the free budget, plus the bytes of the tabs that it replaces, is under 1% of the limit. The status line asks to close result tabs or to raise the limit. A budget that fills during the read stops the result as before | Close old tabs automatically; truncate only | The user decides which results leave |
| 10 | Comparison | `gridComparePrevious` (`D`) compares the active tab with another complete tab of the console with the same columns: a list of tabs, then the key columns. `gridKeepPrevious` (`B`) is removed | Keep the retained previous result | Two open tabs already hold both sides |
| 11 | Old key maps | A saved `gridKeepPrevious` key loads without effect and without an error | Refuse the key map | Old configurations keep loading |

- Each tab keeps the statement, schema, bound parameters, and the one-statement flag of its run. A full export reread uses them, so it never reads the current editor text.
- Events of an old execution make no tab, and a replaced tab releases its budget once.
- No saved format changes: results are not saved.

## Table statistics and AI explanations

The user requested table statistics and AI explanations of errors and plans. The user chose SQL transmission with consent, Redis support, already loaded schema metadata, and both palette commands and fixed shortcuts.

| Point | Implementation |
|---|---|
| Statistics | The structure tab adds Statistics on PostgreSQL and MySQL. Each row names one metric. Counts are estimates; the read starts no `COUNT(*)` or maintenance operation |
| Error | Explain the newest failed user command since the current connection association, with its effective statement and error text. A connection change keeps earlier history visible but excludes its errors from AI explanations, even after a return to that connection |
| Plan | Explain the plan on screen with its original executed statement, not later editor changes. Never run `EXPLAIN` or `EXPLAIN ANALYZE` to answer this request |
| Metadata | Include an AI schema snapshot only when already loaded. Apply the context limit and mark reduced metadata |
| Consent | Each explanation asks for approval. Existing completion approval does not cover error text or plan predicates |
| Provider | Reuse the configured provider, credentials, and limits. Keep MCP authorization separate |
| Output | Show bounded text in the existing text window. Wait while another window or input prompt is open, including active editor and grid searches, parameter fields, and the navigator filter. Esc first closes the active input. Reserve text and worst-case display caches before showing the answer. Never apply or execute generated SQL |
| Commands | `aiExplainError`: `Ctrl+W M`; `aiExplainPlan`: `Ctrl+W T`. Both appear in the palette and support custom bindings |
| Cancellation | Esc, Stop, tab changes, console closure, and a new execution cancel the explanation and reject late responses |

## Visible result tabs

The user approves keeping the selected result tab visible when the row is too narrow.

- The result row follows the selected result, Plan, or Messages tab, including after resize or result closure.
- `<` and `>` mark hidden tabs. A click selects the nearest hidden tab on that side.
- Existing result navigation, custom shortcuts, conventional mode, and Vim mode stay unchanged.
- Drawing and mouse hit testing share the same positions and original result indices.
- Layout work stays bounded by the row width, not the number of results.
- Navigation preserves editor text, cursor, and selection. It runs no SQL and changes no saved format.

## Cell inspection and editor controls

The user approves interactive scrollbars, cell search, JSON validation and formatting, and selected character counts.
The user does not request independent scrolling without cursor movement.

- Console and cell-editor scrollbars accept track clicks and thumb drags. Resize ends the drag.
- Scrollbar input uses the existing cursor-in-view rule. It does not edit text, replace a selection, or run SQL.
- Read-only text windows support literal, case-insensitive search with `Ctrl+F` and the effective editor Find key.
- Enter/Down and Shift+Enter/Up walk matches. Escape closes the search before the window.
- Search preserves the copy selection, uses bounded work, and reads hexadecimal digits without expanding the entire value.
- Search stops at one million matches and labels the partial count. This matches the existing grid-search ceiling.
- Cell JSON checks show validity or the error line, character column, and reason, within the existing 1 MiB input limit.
- Checks do not add a save restriction. The existing column validation and save guards still apply.
- The cell `formatSql` binding and the **Format JSON** button format JSON explicitly as one undo step.
- Formatting preserves key order and digits, keeps text after refusal, and never keeps or saves the value automatically.
- Value footers, cell-editor hints, and the console status show selected Unicode character counts, including line breaks.
- Critical console status labels take priority over the selection count in a narrow terminal.

## JSON colors in cell windows

The user requests JSON colors in the read-only value window and the cell editor.

- Both places share JSON token detection and the existing theme styles.
- Keys use the identifier style. Strings use the string style. Numbers use the number style.
- `true`, `false`, and `null` use the keyword style.
- Only valid objects and arrays activate JSON colors. The editor checks each changed revision within the 1 MiB input limit.
- The value window uses its validated original and formatted shapes, within the existing display limits.
- Colors do not change text, key order, number digits, selection, copy, or save behavior.
- The SQL console keeps its SQL colors. No new preference or saved format is needed.

## Text selection in the cell window

The user requests text selection with the mouse and approves `Shift+arrows` and selection-aware copy.

- Drag selects text. A click sets a new selection start. `Shift+arrows` extends the selection.
- `Ctrl+C` copies the selection. Without a selection, it copies the complete text in the current shape.
- Copy preserves source characters and line breaks. Display wrapping adds no line breaks.
- Scrolling and resizing keep the selection. A JSON shape change or a row change clears it.
- Shared read-only text windows use the same selection and copy behavior. Existing scroll and row keys stay unchanged.

## Analysis of loaded results

The user approves column profiles, ordered text charts, and differences between consecutive console executions.
[Result tabs by statement number](#result-tabs-by-statement-number--2026-11-20) replaces the retention and the comparison of consecutive executions with a comparison of two open tabs.
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
