# Decisions — Files and persistence

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

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
