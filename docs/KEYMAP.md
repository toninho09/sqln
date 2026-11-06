# SQLN — Navigation and shortcuts

## State of this document

**Defined:** this document is approved.
It details the navigation and the shortcuts of the [plan](INITIAL_PLAN.md).
The plan remains the reference for the functional rules.

## Assistant commands

The palette includes `agentSessions`, `agentReview`, `agentDirect`, `agentStop`, `agentSqlToConsole`, `agentSqlClear`, `agentSession`, and `agentTasks`. They have no default shortcut.
`agentSqlToConsole` and `agentSqlClear` are available in an agent tab with placed SQL; its bar shows them as **New console** and **Clear**.
`agentSession` and `agentTasks` are available in an agent tab of a connected session; its bar shows them as **Access: level** and **Tasks N**. `agentTasks` needs an active task.
The MCP row shows the effective key of `agentSessions` or `agentReview`, and its **Stop** button runs `agentStop`.
Assign custom shortcuts through the existing key map. Availability and labels use the shared command catalog.
An edit proposal opens as an inline review in the editor. See [Review](#review).

## Choice criteria

1. Use keys that iTerm delivered in the `keytest` test.
2. Offer another path for each essential action.
3. Not depend only on `Ctrl`+arrows, `Home`, `End`, `PageUp`, or `PageDown`.
4. Keep the same execution actions in the conventional and Vim modes.

The `input` module applies the `keymap` field, and the application reads it from `config.json`.
**Defined:** the bar, the `F1` help, and the prefix options show the effective keys of the `keymap`.
Do not show removed shortcuts or actions unavailable in the current panel.
Double click on a table and click on a header were not tested in a real terminal.
`Shift+F5` and `Ctrl+Enter` were tested with simulated sequences in a PTY, not in iTerm.
The letters after `Ctrl+W` and `F1` were also not tested in iTerm.
Terminal.app and Linux terminals were also not tested.

## Focus regions

| Region | Role |
|---|---|
| Navigator | Connections, objects, and Scripts without connection |
| Main tabs | Open consoles, tables, diagrams, and compares |
| Editor | SQL text of the console |
| Filter bar | `WHERE` and `ORDER BY` fields of the table view |
| Grid | Rows of the result or of the table |
| Diagram | Boxes and lines of an ER diagram |
| Review | Changes of an inline review in the editor |
| Result tabs | Results and **Messages** of the console |
| Dialog | Question that blocks the other regions |

## Key priority

The application tests each key in this order and stops at the first match:

1. Open dialog.
2. Open suggestion list.
3. Active `Ctrl+W` prefix.
4. Text field in focus, such as editor, filter, or search.
5. Shortcuts of the focused panel.
6. Global shortcuts.

During text input, a character always becomes text.
A key without an action produces no effect and no message.

## `Ctrl+W` prefix

The prefix works in both modes, without terminal features.
After `Ctrl+W`, press one key. `Esc` cancels the prefix.
A window shows the options, in groups, while the prefix is active.
The window uses the effective keys of the `keymap`, and the hint bar shows `Ctrl+W …  Esc cancel`.

The window lists a command only when the command has an effect now and belongs to the panel of the cursor. A group without commands leaves the window.
The key of a command outside the window keeps its action, so the table below lists each key.

| Panel of the cursor | Commands of that panel only |
|---|---|
| Editor | Run, whole script, plan, format, comment, history, and the commands of the next row |
| Results | Rename, mode, database or schema, and connection change |
| Table and filter bar | None |
| Diagram | None |
| Review | The review commands |
| Navigator | None |

Each panel also lists these commands:

- The keys that move the focus from that panel.
- Next tab and previous tab, with a second tab.
- Close the tab, toggle the explorer, the explorer page, sessions of a source with sessions, the ER diagram and the compare of a source with tables, palette, help, Settings, reload, and quit.
- `x` during a run, `C` and `R` during a transaction, and only `R` in a failed transaction. These stay in the navigator, because the session stays occupied until the user acts.

Table and diagram tabs have no script or manual transaction, so `C` and `R` do not appear there.
Grid Apply controls its own short transaction.

| Key | Action |
|---|---|
| `h`, `j`, `k`, `l`, or arrows | Change panel |
| `n` and `p` | Next tab and previous tab |
| `N` | Rename the current console |
| `a` | Change or attach the console connection |
| `,` | Open Settings |
| `c` | Close the tab |
| `q` | Quit the application |
| `b` | Collapse or show the navigator |
| `Tab` | Open the page with the commands of the explorer |
| `Space` | Open the page with the commands of the current panel |
| `S` | Show the sessions of the connection of the tab |
| `D` | Show the ER diagram of the tab or of the row |
| `v` | Compare the tab or the row with another source |
| `F` | Follow a key of the row of a table tab |
| `e` | Run |
| `s` | Whole script |
| `E` | Show the plan of the statement |
| `A` | Run the statement and show its plan |
| `x` | Stop the script or the table query |
| `f` | Format the SQL |
| `/` | Comment or uncomment the lines |
| `m` | Switch between autocommit and manual |
| `C` | Commit |
| `R` | Rollback |
| `d` | Choose database or schema |
| `P` | Focus the parameters panel |
| `r` | Read the configuration again |
| `?` | Open help |
| `:` | Open the command palette |

`Ctrl+W C` and `Ctrl+W R` do not ask for confirmation, like the commit and rollback buttons.
`Ctrl+W N` uses uppercase `N` and does not replace `Ctrl+W n`.
`Ctrl+W H` uses uppercase `H` and does not replace `Ctrl+W h`.
`Ctrl+W E` and `Ctrl+W A` use uppercase keys and do not replace `Ctrl+W e` and `Ctrl+W a`.
`Ctrl+W S` uses uppercase `S` and does not replace `Ctrl+W s`. With the focus in the navigator, it uses the connection of the row.
`Ctrl+W D` uses uppercase `D` and does not replace `Ctrl+W d`. With the focus in the navigator, it uses the row, as `d` does. A table tab shows the diagram of its table, and a console shows its current schema.
`Ctrl+W P` uses uppercase `P` and does not replace `Ctrl+W p`. It focuses the parameters panel when the editor text has parameters.
`Ctrl+W F` uses uppercase `F`. It acts as `F` in the grid of a table tab, and it gives the reason in another tab.
`Ctrl+W v` opens the dialog of a compare. With the focus in the navigator, it uses the row, as `c` does. A table tab gives its table (**Data**), a console gives its current schema (**Structure**), a diagram gives its focus, and a compare tab opens the dialog with its own sides.

`Ctrl+W Tab` opens a second page of the menu. The page lists the commands of the navigator that act on the current row, with the keys of the navigator, so `Ctrl+W Tab n` is the `n` of the navigator.
The page works from any panel, and its title names the row that the commands act on. `Esc` closes it, and a key without a command closes it too.
The keys of the page do not change `Ctrl+W n` or any other key of the prefix.

`Ctrl+W Space` opens the commands of the current panel: editor, results, table, diagram, compare, or navigator.
The page uses the effective panel shortcuts. For example, `Ctrl+W Space Ctrl+C` copies the current grid cell in conventional mode.
In Vim mode, use `Ctrl+W Space y` for that action. A configured sequence works on the page too.
`Esc` or an unmatched key closes the page without sending that key to the editor.
The editor page includes save, find, suggestions, undo, redo, selection, and clipboard commands.
The grid page includes filter, sort, load more, pages, column widths, selection, export, and import when available.
A command without an assigned shortcut stays in the palette, but has no row on the panel page.
The pages do not change the meaning of existing prefix keys, such as `Ctrl+W f` for format.
The console list offers the rename action with `r`.
The connection change asks for confirmation and does not run SQL. The rules are in section 12 of the plan.
The main tabs follow the opening order, without groups by type.
Closing the last console with an open table does not create another console tab.
Closing the last tab shows the home screen. Only `Ctrl+W q` ends the application.
On the home screen, the commands of a tab have no effect, and the `Ctrl+W` menu and the palette hide them.

In the conventional mode, `Ctrl+W` does not close the tab, unlike many IDEs.
The prefix is the common path to both modes, and the terminal delivers that key.

## Global actions

| Action | Shortcut | Alternative |
|---|---|---|
| Run | `Ctrl+Enter` | `F5` or `Ctrl+W e` |
| Whole script | `Shift+Enter` | `Shift+F5` or `Ctrl+W s` |
| Show the plan of the statement | `Ctrl+W E` | Command palette |
| Run the statement and show its plan | `Ctrl+W A` | Command palette |
| Stop script | `Ctrl+W x` | **Stop script** button |
| Format the SQL | `Ctrl+W f` | Command palette |
| Comment or uncomment the lines | `Ctrl+W /` | `Ctrl+/` or command palette |
| Query history | `Ctrl+W H` | Command palette |
| Help | `F1` | `Ctrl+W ?` |
| Command palette | `Ctrl+P` | `Ctrl+W :` |
| Quit | `Ctrl+W q` | Command palette |

`Ctrl+Enter` and `Shift+Enter` need keyboard enhancement in the terminal.
At startup, the application detects that feature and shows the available path in the hint bar.

The palette lists the functional actions available in the current state, including editor and grid commands.
Basic cursor and view movements stay in `F1` help. The list has no duplicate local and global versions of an action.
Stop and transaction commands come first when needed. Panel commands come before other application commands.
Type to filter by words in the name, shortcut, action name, or technical aliases such as `EXPLAIN`, `DDL`, and `Mermaid`.
`Up` and `Down` move the line, `Enter` runs, `Esc` closes, and a click runs the line.
An action without a key stays in the list. A panel key appears only where it works.
Navigator row commands appear only with focus in the navigator, and name their target.
Add connection and scripts without a connection remain global. `Ctrl+W Tab` remains the explicit cross-panel explorer path.
Apply filters and Clear filters also appear in the palette, without default shortcuts.
`Ctrl+P` has priority over the `MoveUp` of EdTUI during input in the Vim mode.

The comment action acts on the selection or on the cursor line, with `-- ` in SQL and `# ` in Redis.
A selection that ends at column 0 leaves that line out. If all lines have the comment, the action removes it.
A terminal without keyboard enhancement sends `Ctrl+/` as the byte of `Ctrl+7`, so both keys do the same action.
`Ctrl+/` is not in the `keytest` record. Use `Ctrl+W /` if the terminal does not deliver that key.

`Ctrl+C` does not end the application and does not stop the script.
In the editor, it copies a selection and does nothing without one.
In a grid, it copies the selection or the current cell.

## Hint row

The hint row keeps one line and uses the current terminal width.
It reserves the palette shortcut before fitting other whole commands. Wider screens show more available commands.
The order is fixed by context, not by a usage history. Custom shortcuts use their actual display width.

| Context | First commands, after Stop or transaction actions |
|---|---|
| Editor | Run, script, results, find, suggestions, format, history, plan, comment |
| Results | Clear selection, copy, value, find, row, editor, export, result tabs |
| Table | Clear selection, copy, value, find, filter, sort, row, follow key, more, pages, export, structure |
| Connection | Open console, new console, expand/collapse, refresh, edit, filter |
| Table in navigator | Open data, structure, columns, refresh, filter, copy name, SQL/DDL, diagram, compare |
| Sessions | Reload, copy, value, find, row; cancel query and end session come later |
| Compare | Open script, copy script, row, find, copy, value, reload |
| Diagram | Data, structure, key columns, Mermaid, reload |
| Review | Accept, reject, next, accept all, reject all, later or reject |

Dialogs, forms, searches, and suggestion lists keep their own instructions.
Status and progress messages can temporarily replace the hint row.

## Editor

| Action | Conventional | Vim |
|---|---|---|
| Insert mode | Always active | `i`, `a`, and `o` |
| Leave insert | No effect | `Esc` |
| Undo | `Ctrl+Z` | `u` |
| Redo | `Ctrl+Y` or `Ctrl+Shift+Z` | `Ctrl+R` |
| Select | `Shift`+arrows | `v` |
| Select all | `Ctrl+A` | `ggVG` |
| Copy, cut, and paste | `Ctrl+C`, `Ctrl+X`, and `Ctrl+V` | `y`, `d`, and `p` |
| Move by word | `Alt`+arrows | `w` and `b` |
| Search | `Ctrl+F` | `/` |
| Replace | `Ctrl+F`, then `Tab` | `Ctrl+F`, then `Tab` |
| Suggestions | `Ctrl+Space` | `Ctrl+Space` |
| Complete with AI | Select the AI row with `Up`, then use the suggestion acceptance key | Same; explicit opening also works in normal and visual modes when enabled |
| Complete with AI with a request | Select the request row with `Up`, `Up`, then use the suggestion acceptance key | Same; `Enter` sends the dialog, `Esc` cancels |
| Save now | `Ctrl+S` | `Ctrl+S` |

AI completion has no separate default shortcut. Configure `editorAiComplete`, `editorAiCompleteRequest`, `editorAiAccept`, `editorAiReject`, and `editorAiCancel` as needed.
An AI response opens an inline review. See [Review](#review). During a request, `Esc` or Cancel stops only AI work.
See [AI completion](AI_COMPLETION.md).

The list opens when you type a name or a dot, and `Ctrl+Space` opens the list without a prefix.
In the Vim mode, the list works only during insert.
With suggestions open, `Tab` accepts, `Enter` breaks the line, and `Esc` closes the list.
The **Autocomplete** option of the Settings dialog (`suggestionKeys`) changes the key that accepts: `tab` (default), `enter`, or `both`. The key that does not accept keeps its effect.
The arrows walk the list while it is open.

In the search bar:

| Action | Key |
|---|---|
| Change field | `Tab` and `Shift+Tab` |
| Next match | `Enter` in the search field, or `Down` |
| Previous match | `Up` |
| Replace the current match | `Enter` in the replace field |
| Replace all | `Ctrl+A` in the replace field |
| Close the bar | `Esc` |

On macOS, the terminal handles `Cmd+C` and `Cmd+V`.
`Cmd+V` reaches the application as a paste.
`Cmd+C` copies the terminal selection, not the editor selection.

## Parameters panel

The panel appears under the editor when the SQL holds `:name` parameters.

| Action | Key |
|---|---|
| Focus the panel | `Ctrl+W P` |
| Next and previous field | `Down` and `Up` |
| Next and previous column | `Tab` and `Shift+Tab` |
| Change the type | `Enter` or `Space` on the type |
| Mark empty text as set | `Enter` on the value |
| Toggle Set and Unset | `Enter` or `Space` on the state |
| Edit the value | Type on the value column |
| Return to the editor | `Esc` |
| Collapse and expand | Click on the title, or `Ctrl+W P` again after collapse |

A run asks for the fields without a value: the focus moves to the first one.
The type chooses Text, Number, Boolean, or NULL. Values stay in the panel and never change the editor text.

## Navigator

| Action | Conventional | Vim |
|---|---|---|
| Move | Arrows | `j` and `k` |
| Collapse and expand | `Left` and `Right` | `h` and `l` |
| Open | `Enter` | `Enter` |
| New console | `n` | `n` |
| Refresh catalog | `r` | `r` |
| Disconnect | `D` | `D` |
| Add connection | `a` | `a` |
| Edit connection | `e` | `e` |
| Delete the saved password | `p` | `p` |
| Delete, with confirmation | `Delete` | `x` |
| Scripts without connection | `s` | `s` |
| Show the structure | `i` | `i` |
| Show the ER diagram | `d` | `d` |
| Compare the structure or the data with another source | `c` | `c` |
| Show the sessions of the server | `S` | `S` |
| Generate SQL or the DDL | `g` | `g` |
| Import a file to the table of the row | `I` | `I` |
| Copy the row name | `y` or `Ctrl+C` | `y` or `Ctrl+C` |
| Filter tables, views, and other objects | `/` or `Ctrl+F` | `/` or `Ctrl+F` |
| Clear the filter | `Esc` | `Esc` |

`Enter` on a connection selects the first open console of it or creates a console.
Under a schema, the groups **Routines**, **Sequences**, **Triggers**, and **Types** follow the tables and views. `Right` or `Enter` opens a group.
`Enter` on an object, or a double click, reads its DDL and opens it in a new console. `g` on an object lists only **DDL**, and `g` on a table or a view lists **DDL** after the templates.
`y` on an object copies its name without the arguments of a routine or the table of a trigger. `i`, `d`, `c`, and `I` do not act on an object.
`n` opens the console list of the connection, with **+ New console** on the first line. Without saved consoles, `n` creates a console.
In the list, `Enter` picks the row, `Delete` (or `x` in the Vim mode) removes the script of the row, and `Esc` cancels.
`s` opens the list of scripts without connection, with the same keys.
A connection with consoles shows a **Consoles** node above its schemas. The node starts closed, and `Right` opens it.
The node lists the saved consoles, the newest first, and then the open consoles without a file. A console with a tab shows the `(open)` mark.
On a console row, `Enter` opens the console or selects its tab, and a double click does the same.
On a console row, `r` renames the console. An open console renames its tab.
On a console row, `Delete` (or `x` in the Vim mode) removes the script, after a question. An open console refuses, and the connection stays.
The **Consoles** node needs no session, so a failed catalog does not hide it. `r` on another row also reads the list of consoles again.
On a schema, `Enter` opens or closes the row.
On a table or a view, `Enter` opens the table view, or selects its tab when it already exists.
`Right` and `Left` open and close the columns of a table.
`i` opens the structure of the row: table, view, column, schema, or connection.
In the grid of a table, `i` opens the structure of that table.
In the structure tab of a table, `[` and `]` switch the section. In the other table tabs, those keys change the page.
`S` opens the sessions of the server of the row. SQLite, Redis, and Valkey refuse it with a reason.
`d` opens the ER diagram of the row. A schema shows all its tables. A table shows the table and the tables that its foreign keys join, one step away. A column shows the diagram of its table. A connection shows its current schema: before its catalog, `d` reads the catalog, and the diagram opens when it arrives. A view, and Redis or Valkey, refuse it with a reason.
`c` opens the dialog of a compare, with the row as the source: a connection gives its current schema, and a table or a column gives its table. A table starts a data compare, and a connection or a schema starts a structure compare. A view, a console row, and Redis or Valkey refuse it with a reason. The hint line can show `c compare` where the command is available, if the terminal has enough width. The key is also in the `Ctrl+W Tab` page, and `Ctrl+W v` acts from any panel.
The hint line changes with the cursor row and terminal width. It reserves the palette shortcut and fits whole commands in priority order.
`Ctrl+W Tab` lists the bound explorer commands, and `F1` shows the key reference. The connection commands appear on the connection line, but the keys act on any line of it, except `r` and `Delete` on a console row.
The labels say the object of the command, such as `n new console` and `a add connection`.
A command that does not fit leaves the line, and the key keeps acting.

`p` erases the saved password of the connection, with a confirmation. The hint appears only when a password is saved.
`g` opens the list of SQL templates of a table, a view, or the table of a column. The text goes to the cursor of the console of the connection and does not run. Redis has no templates.
`y` copies the row name, without quotes.

### Tree filter

The filter lists tables, views, and other objects that match each word, by their label or schema name. It leaves out consoles and columns. It reads only the in-memory catalogs and does not open connections.

| Action | Key in the field |
|---|---|
| Open the table or the DDL of the object under the cursor | `Enter` |
| Move between relations and other objects | `Up` and `Down` |
| Go to the filtered tree | `Tab` |
| Cancel and return the cursor | `Esc` |

The filter stays after you open a table or object. In the filtered tree, the navigator keys act, except `Left` and `Right`. `/` returns to the field, and `Esc` clears the filter with the cursor on the same row.
`Enter` or `Tab` with an empty field close the filter.

## Connection form

The form has a sidebar with the sections General, Address, Credentials, Security, and Options. A section without fields stays out of the sidebar. `Tab` walks the sidebar, the fields of the open section, and the buttons. Each section shows the buttons.

| Action | Key |
|---|---|
| Change section, in the sidebar | `Up` and `Down`; in the Vim mode, also `j` and `k`; or a click on the section |
| Next field | `Tab`, or `Enter` in a field; outside the sidebar, also `Down` |
| Previous field | `Shift+Tab`; outside the sidebar, also `Up` |
| Change an option | `Left` and `Right`; in the Vim mode, also `h` and `l` |
| Toggle **Read only** | `Space`, `Left`, or `Right` |
| Press a button | `Enter` |
| Cancel | `Esc` |

A save or a test with a bad value opens the section of that field and puts the focus on it.

In a text field, every letter becomes text.
In the Vim mode, `j` and `k` change field only outside text fields.

## Settings

Focus starts on the sidebar. `Tab` walks the sidebar, the scope, the fields of the open section, and the buttons.

| Action | Key |
|---|---|
| Change section, in the sidebar | `Up` and `Down`; in the Vim mode, also `j` and `k` |
| Next field | `Tab`, or `Enter` in a field; outside the sidebar, also `Down` |
| Previous field | `Shift+Tab`; outside the sidebar, also `Up` |
| Change an option | `Left` and `Right`; in the Vim mode, also `h` and `l` |
| Press a button | `Enter` |
| Cancel | `Esc` |

## Password prompt

The password stays masked, including during paste. Save starts unchecked.

| Action | Key |
|---|---|
| Enter password | Type or paste in the field |
| Next field | `Tab` or `Down` |
| Previous field | `Shift+Tab` or `Up` |
| Toggle save in the vault | `Space` or `Enter` on the option |
| Connect | `Enter` in the password field or on the **Connect** button |
| Cancel without starting the operation | `Esc` or **Cancel** button |

Outside the password field, the horizontal arrows change focus.
In the Vim mode, `h/k` and `j/l` also change focus outside the field.
Inside the field, those letters always become text.
The mouse selects the field, toggles the option, or presses a button.
If the vault fails, the session option does not offer global storage in plain text.

## Grid

| Action | Conventional | Vim |
|---|---|---|
| Move | Arrows | `h`, `j`, `k`, and `l` |
| First and last row | `Alt+Up` and `Alt+Down` | `gg` and `G` |
| First and last column | `Alt+Left` and `Alt+Right` | `0` and `$` |
| Select | `Shift`+arrows | `v` |
| Select all | `Ctrl+A` | `ggVG` |
| Copy as TSV | `Ctrl+C` | `y` |
| Export to a file | `e` | `e` |
| Import a file, in a table tab | `I` | `I` |
| Edit a table cell | `F2` | `F2` |
| Set a table cell to NULL | `Ctrl+N` | `Ctrl+N` |
| Review pending changes | `Ctrl+R` | `Ctrl+R` |
| Review and apply pending changes | `Ctrl+S` | `Ctrl+S` |
| Discard pending changes, with confirmation | `Ctrl+D` | `Ctrl+D` |
| Clear the selection | `Esc` | `Esc` |
| Open value | `Enter` | `Enter` |
| Open the row vertically | `r` | `r` |
| Search in the loaded rows | `Ctrl+F` | `/` |
| Shrink and grow the column | `<` and `>` | `<` and `>` |
| Automatic column width | `=` | `=` |
| Close the result | `x` or `Delete` | `x` or `Delete` |
| Previous and next result | `[` and `]` | `[` and `]` |
| Read a tab without a filter again | `R` | `R` |
| Cancel the query, in a sessions tab | `q` | `q` |
| End the session, in a sessions tab | `K` | `K` |

`q` and `K` act on the session under the cursor, and they ask first. A read-only connection refuses them.
`R` serves a structure, a key, and a sessions tab. A table tab uses **Apply filters**.
Without a selection, `Ctrl+C` copies the current cell.
The result that still receives rows does not close, and the **Messages** tab does not either.

In the value window and the row window, the keys are the same in both maps:

| Action | Key |
|---|---|
| Scroll | `Up` and `Down`, or `j` and `k` |
| Toggle JSON formatting, in the value window only | `f` |
| Change row, in the row window | `Left` and `Right`, or `h` and `l` |
| Copy the visible text | `Ctrl+C` |
| Close | `Esc` or `Enter` |

Grid editing requires a writable table with a primary key. Key, identity, generated, and binary columns remain protected.
The action names are `gridEditCell`, `gridSetNull`, `gridReviewEdits`, `gridSaveEdits`, and `gridDiscardEdits`.
The palette and panel menu show their effective shortcuts.
Pending changes disable copy, export, import, filters, sorting, reload, key navigation, and page changes.

In the cell editor, `F2` or `Ctrl+S` keeps the value locally, without a database write.
`Tab` selects the editor or buttons; the mouse selects and activates buttons.
Conventional `Esc` cancels. In Vim insert mode, `Esc` first selects normal mode.
The editor respects configured editor commands. Global application commands do not leave the modal.
In the review, arrows and PageUp/PageDown scroll; `F2` or `Ctrl+S` applies; `Esc` cancels.
Production asks for confirmation after review.

In the **Messages** tab, the arrows walk the text.

`e` opens the export. The palette offers the same action, also with focus in the editor.
Without a result, in the **Messages** tab, or with a busy tab, the key does nothing.

In the export:

| Action | Key |
|---|---|
| Next field | `Tab` or `Enter` |
| Change the format, in the sidebar | `Up` and `Down` |
| Change a choice | `Left` and `Right` |
| Open the file explorer, in the **File** field | `Ctrl+O` |
| Press **Export** or **Cancel** | `Enter` on the button |
| Cancel | `Esc` |

Only the **Export** button exports.

`I` opens the import of the table. In the navigator, it first opens the table of the row.
A view, a read-only connection, Redis, a structure tab, or a busy tab has no import, and the key does nothing.

In the import:

| Action | Key |
|---|---|
| Next field | `Tab` or `Enter` |
| Open another section, in the sidebar | `Up` and `Down` |
| Change a choice | `Left` and `Right` |
| Open the file explorer, in the **File** and **Log file** fields | `Ctrl+O` |
| Select a table column, in **Columns** | `Up` and `Down` |
| Change its file column, in **Columns** | `Left` and `Right` |
| Scroll the rows and the columns, in **Preview** | Arrows |
| Press **Import** or **Cancel** | `Enter` on the button |
| Cancel | `Esc` |

The file field reads the file when the focus leaves it.
Only the **Import** button imports.

`Ctrl+O` also opens the explorer in the path fields of the connection form.
The **Schemas** or **Databases** field opens a server list instead, as described below.

In the file explorer, the keys are the same in both maps:

| Action | Key |
|---|---|
| Filter the names | Type the text |
| Show hidden names | Filter that starts with `.` |
| Move the cursor | `Up`, `Down`, `PageUp`, and `PageDown` |
| Open the row | `Enter` |
| Enter the folder, with an empty filter | `Right` |
| Go up one folder, with an empty filter | `Left` or `Backspace` |
| Go to the home folder, with an empty filter | `~` |
| Close without changing the field | `Esc` |

With the mouse, a click moves the cursor, and a double click opens the row.

In the **Schemas** field of the connection form (**Databases** in MySQL and Redis), `Ctrl+O` or the `...` button opens the list of the server:

| Action | Key |
|---|---|
| Filter the names | Type the text |
| Move the cursor | `Up` and `Down` |
| Add or remove the mark | `Space`, or a click |
| Write the marked names in the field | `Enter` |
| Close without changing the field | `Esc` |

In the grid search:

| Action | Key |
|---|---|
| Next match | `Enter` or `Down` |
| Previous match | `Up` |
| Close the bar | `Esc` |

In the table view, the grid also offers:

| Action | Key |
|---|---|
| Focus `WHERE` | `f` |
| Focus `ORDER BY` | `o` |
| Sort by the current column | `s` |
| Load more | `m` |
| Count the rows of the applied filter | `c` |
| Follow a key of the current row | `F` |
| Previous and next page | `[` and `]` |

`F` lists the keys of the row: `→` for a key that leaves the table, and `←` for a key of another table that reads the row. The keys of the column of the cursor come first.
With one key, `F` goes there at once. A key with `NULL` in the row stays out. A view, a structure, and a sessions tab have no key to follow.

## Filter bar

| Action | Key |
|---|---|
| Accept suggestion | `Tab`, or the key of the **Autocomplete** option |
| Apply the fields | `Enter` |
| Change field | `Shift+Tab`, or `Tab` without suggestions |
| Open the suggestions | `Ctrl+Space`, or when you type a name |
| Close the suggestions | `Esc` |
| Return to the grid | `Esc`, with the suggestions closed |

`Ctrl+W k` goes to the `WHERE` field, and `Ctrl+W j` returns to the grid.
In table tabs, script and manual transaction actions have no effect. Grid Apply owns its short transaction.
Those actions are `F5`, `Shift+F5`, `Ctrl+W e`, `Ctrl+W s`, `Ctrl+W E`, `Ctrl+W A`, `Ctrl+W m`, `Ctrl+W C`, `Ctrl+W R`, and `Ctrl+W d`.
In the table grid, `x` and `Delete` also have no effect.
`Ctrl+W c` closes the table tab and frees its data.

## Diagram

| Action | Conventional | Vim |
|---|---|---|
| Select the nearest table in a direction | Arrows | `h`, `j`, `k`, and `l` |
| Move the view | `Shift`+arrows | `Shift`+arrows |
| Move the view by one page | `PageUp` and `PageDown` | `PageUp` and `PageDown` |
| Open the data of the selected table | `Enter` | `Enter` |
| Show the structure of the selected table | `i` | `i` |
| Show only the key columns, or all the columns | `K` | `K` |
| Copy the diagram as Mermaid text | `y` | `y` |
| Read the diagram again | `R` | `R` |

An arrow selects the nearest box that starts past the edge of the selected box in that direction. The view follows the selection.
A key column is a column of the primary key, of a foreign key, or that a foreign key reads.
`y` copies the `erDiagram` text of the boxes on screen, so with **keys only** the text has only the key columns.
`Ctrl+W x` stops a read. `Ctrl+W c` closes the diagram tab. The script and transaction actions have no effect.

## Review

An inline review of an AI response or an agent proposal uses these keys while the editor has the focus. They are the same in both modes.

| Action | Name | Key |
|---|---|---|
| Accept the current change | `reviewAccept` | `a` or `Tab` |
| Reject the current change | `reviewReject` | `r` |
| Accept all changes | `reviewAcceptAll` | `A` |
| Reject all changes | `reviewRejectAll` | `R` |
| Next change | `reviewNext` | `Down`, `j`, or `]` |
| Previous change | `reviewPrevious` | `Up`, `k`, or `[` |
| Close: **Later** for an agent proposal | `reviewClose` | `Esc` |
| Move the view | — | `PageUp`, `PageDown`, `Left`, and `Right` |

Ghost text takes only the keys of `reviewAccept` that are not characters, such as `Tab`, and `reviewClose`. Every other key goes to the editor and keeps the ghost text, so the editor stays editable around it. A character never decides: it is text, and an edit of the anchor line closes the ghost of that line.
With several ghosts, the decide keys act on the ghost at the line of the cursor. `reviewClose` (`Esc`) ends the request of that line, or rejects its ghost; with several requests, the bar button **Cancel** ends them all.
In a diff of an agent proposal, the editor is read only: the AI requests of the console end, and new requests refuse until the review closes.
`editorAiAccept` and `editorAiReject` decide the ghost at the line of the cursor.

## Compare

The compare tab uses the keys of the grid, except the keys of a table view, of the import, and of the list of results.

| Action | Conventional | Vim |
|---|---|---|
| Open the sync script in a new console | `S` | `S` |
| Copy the sync script | `Y` | `Y` |
| Read both sides again | `R` | `R` |

`S` and `Y` act only when the compare has differences. A structure compare also needs the same dialect on both sides. `Y` is uppercase, because `y` copies a cell in the grid of the Vim mode.
`Ctrl+W x` stops the reads. `Ctrl+W c` closes the compare tab. The script and transaction actions have no effect.

In the dialog, `Tab` and the arrows move between the fields, `Left`, `Right`, and `Space` change a choice, and `Enter` on **Compare** opens the tab. `Esc` cancels.

A connection, a schema, and a table field open a list of names with the focus. Text filters the list, also `Space` and the Vim keys. `Up` and `Down` move the mark, and `Enter` or `Tab` choose the marked name and move to the next field. `Left` and `Right` still change a connection or a schema. `Esc` closes the list and keeps the value, and a second `Esc` cancels. A field without a name to show uses the keys of a plain field.

## Question about changed scripts

| Action | Key |
|---|---|
| Change row or button | `Up`, `Down`, `Tab`, and `Shift+Tab`; in the Vim mode, also `j` and `k` |
| Change the action of a row | `Left`, `Right`, and `Space`; in the Vim mode, also `h` and `l` |
| Choose the action and go to the next line | `Enter` |
| Apply, with focus on **Apply** | `Enter` |
| Decide later | `Esc`, or `Enter` on **Decide later** |

**Apply** needs an action on each line.

## Dialogs

| Action | Key |
|---|---|
| Change option | `Tab` and `Shift+Tab`; in the Vim mode, also `h` and `l` |
| Confirm the focused option | `Enter` |
| Cancel | `Esc` |

The initial focus always stays on the option with no effect on the database or the disk.
`Enter` never triggers commit, rollback, or delete without explicit focus on that option.

## Mouse

| Gesture | Effect |
|---|---|
| Click | Select an item and change focus |
| Click on a tab | Switch tab |
| Click on the `×` of a tab | Close the tab, with the questions of `Ctrl+W c` |
| Click on **Quit** | Quit, with the questions of `Ctrl+W q` |
| Click on the `EXPLORER` title | Give focus to the navigator |
| Click on the table header | Apply sorting: ascending, descending, and no order |
| Click on a row number in a result or table grid | Select all columns of that loaded row, including hidden columns |
| Double click on a table | Open the table view, with two clicks on the same cell within 400 ms |
| Double click on a console row | Open the console, or select its tab |
| Click on a bar field or button | Give focus to the field, or press the button |
| Click on **Previous** or **Next** | Change page in the table grid |
| Click on a row of the script question | Change the action of the row, or press the button |
| Wheel | Scroll lists, editor, and grid |
| Horizontal wheel or touchpad on the grid | Move the cursor one column for each event. The terminal must send horizontal wheel events |
| Drag on a divider | Resize the panels |
| Drag on a header border | Resize the column width |
| Drag on the grid | Select cells |
| Click on a diagram box | Select the table |
| Drag on a diagram | Move the view |
| Wheel on a diagram | Move the view up and down, or to the side with a horizontal wheel |

A click also answers a dialog and presses the console bar buttons.
A click anywhere also removes the hint line message.
The result tab line acts as a divider between the editor and the results.
The `keytest` test did not record a drag.
With mouse capture active, the native terminal selection needs `Option` in iTerm.

## Terminal

1. Request only keyboard disambiguation, not the report of all keys.
2. Ignore events of lone modifier keys.
3. Treat `Event::Paste` as a paste.
4. Restore the terminal at exit, including after a panic.

## Configuration

### Edit shortcuts in Settings

1. Open Settings with `Ctrl+W ,`.
2. Select **Shortcuts**.
3. Select the **Global** or **Project** scope.
4. Select the keyboard mode.
5. Select a command from the list with `Up`/`Down` and `Enter`, or click its row.
6. Select an existing shortcut or **+ Add shortcut** with the side keys.
7. Select **Finish with: Enter** or **Mouse OK**.
8. Select **Record**.
9. Press the keys in order.
10. Press `Enter` or click **OK** to confirm the recording.
11. Select **Save** to write the selected scope and apply its shortcuts.

The list shows every configurable command and its shortcuts, including commands without assigned keys.
Use `Tab` to focus the list. Use the mouse wheel, `PageUp`/`PageDown`, or `Home`/`End` to move through it.
The note below the list shows the selected command's panel and full description.
**Back to commands** or `Esc` returns from the command editor without discarding its changes.

With **Mouse OK**, the recording includes `Enter`. Click **OK** to finish.
`Esc` or **Cancel recording** discards the recording and keeps Settings open.
**Cancel** closes Settings without a write.
The recorder accepts only key press events. It ignores key repeats and key releases.

**Remove shortcut** removes the selected application shortcut. The palette keeps the action when it is available.
Native Vim movements and operators keep their internal keys.
**Restore action** removes the action's override from the selected scope.
A project then uses the global keys. The global scope then uses the default keys.
Neither command changes the other scope or keyboard mode.

Settings shows conflicts and blocks **Save** until you correct them.
The message names the key and both actions.
The check includes sequence prefixes, both keyboard modes, and the combined global and project keys.
A panel can use a global key, because panel shortcuts have priority.
The editor region includes its application commands. Other internal Vim movements and operators stay in EdTUI.
Plain Vim keys retain their native editing behavior. Panel pages and the palette provide explicit functional actions.

### File format

The `preferences.keymap` field of `config.json` maps an action to a list of keys, per mode.

```json
{
  "version": 1,
  "preferences": {
    "keymap": {
      "conventional": { "run": ["ctrl+enter", "f5", "ctrl+w e"], "stop": [] },
      "vim": { "run": ["ctrl+enter", "f5", "f8 r s q"] }
    }
  }
}
```

A list replaces the default keys of the action, and an empty list removes the shortcut.
A sequence can have any number of keys separated by spaces, such as `ctrl+w e` and `f8 a b c`.
The application waits for the complete sequence. `Esc` or an unmatched key cancels it.
A plain character stays text when a text field has focus.
Two actions with the same key, in the same region, invalidate the configuration.
A key and a sequence that starts with it also invalidate the configuration.
An unknown action or key name also invalidates the configuration.
In that case, the invalid configuration rules of the plan apply.
