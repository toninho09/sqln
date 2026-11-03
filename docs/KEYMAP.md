# SQLN — Navigation and shortcuts

## State of this document

**Defined:** this document is approved.
It details the navigation and the shortcuts of the [plan](INITIAL_PLAN.md).
The plan remains the reference for the functional rules.

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
| Main tabs | Open consoles and tables |
| Editor | SQL text of the console |
| Filter bar | `WHERE` and `ORDER BY` fields of the table view |
| Grid | Rows of the result or of the table |
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
| `r` | Read the configuration again |
| `?` | Open help |
| `:` | Open the command palette |

`Ctrl+W C` and `Ctrl+W R` do not ask for confirmation, like the commit and rollback buttons.
`Ctrl+W N` uses uppercase `N` and does not replace `Ctrl+W n`.
`Ctrl+W H` uses uppercase `H` and does not replace `Ctrl+W h`.
`Ctrl+W E` and `Ctrl+W A` use uppercase keys and do not replace `Ctrl+W e` and `Ctrl+W a`.
The console list offers the rename action with `r`.
The connection change asks for confirmation and does not run SQL. The rules are in section 12 of the plan.
The main tabs follow the opening order, without groups by type.
Closing the last console with an open table does not create another console tab.

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
| Quit | `Ctrl+W q` | Close the last tab |

`Ctrl+Enter` and `Shift+Enter` need keyboard enhancement in the terminal.
At startup, the application detects that feature and shows the available path in the hint bar.

The palette lists the actions available in the current state, with the key of each one.
Type to filter. `Up` and `Down` move the line, `Enter` runs, `Esc` closes, and a click runs the line.
An action without a key stays in the list. A navigator key appears only with the navigator in focus.
`Ctrl+P` has priority over the `MoveUp` of EdTUI during input in the Vim mode.

The comment action acts on the selection or on the cursor line, with `-- ` in SQL and `# ` in Redis.
A selection that ends at column 0 leaves that line out. If all lines have the comment, the action removes it.
A terminal without keyboard enhancement sends `Ctrl+/` as the byte of `Ctrl+7`, so both keys do the same action.
`Ctrl+/` is not in the `keytest` record. Use `Ctrl+W /` if the terminal does not deliver that key.

`Ctrl+C` does not end the application and does not stop the script.
With a selection, it copies. Without a selection, it does nothing.

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
| Save now | `Ctrl+S` | `Ctrl+S` |

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
| Show the sessions of the server | `S` | `S` |
| Generate SQL | `g` | `g` |
| Import a file to the table of the row | `I` | `I` |
| Copy the row name | `y` or `Ctrl+C` | `y` or `Ctrl+C` |
| Filter the tables | `/` or `Ctrl+F` | `/` or `Ctrl+F` |
| Clear the filter | `Esc` | `Esc` |

`Enter` on a connection selects the first open console of it or creates a console.
`n` opens the console list of the connection, with **+ New console** on the first line. Without saved consoles, `n` creates a console.
In the list, `Enter` picks the row, `Delete` (or `x` in the Vim mode) removes the script of the row, and `Esc` cancels.
`s` opens the list of scripts without connection, with the same keys.
On a schema, `Enter` opens or closes the row.
On a table or a view, `Enter` opens the table view, or selects its tab when it already exists.
`Right` and `Left` open and close the columns of a table.
`i` opens the structure of the row: table, view, column, schema, or connection.
In the grid of a table, `i` opens the structure of that table.
In the structure tab of a table, `[` and `]` switch the section. In the other table tabs, those keys change the page.
`S` opens the sessions of the server of the row. SQLite, Redis, and Valkey refuse it with a reason.
The hint line changes with the cursor line. The connection commands appear on the connection line, but the keys act on any line of it.
The labels say the object of the command, such as `n new console` and `a add connection`.
A command that does not fit leaves the line, and the key keeps acting.

`p` erases the saved password of the connection, with a confirmation. The hint appears only when a password is saved.
`g` opens the list of SQL templates of a table, a view, or the table of a column. The text goes to the cursor of the console of the connection and does not run. Redis has no templates.
`y` copies the row name, without quotes.

### Tree filter

The filter lists the tables and the views that match each word, by the relation name or the schema name. It reads only the in-memory catalogs and does not open connections.

| Action | Key in the field |
|---|---|
| Open the table of the cursor | `Enter` |
| Move between relations | `Up` and `Down` |
| Go to the filtered tree | `Tab` |
| Cancel and return the cursor | `Esc` |

The filter stays after you open a table. In the filtered tree, the navigator keys act, except `Left` and `Right`. `/` returns to the field, and `Esc` clears the filter with the cursor on the same row.
`Enter` or `Tab` with an empty field close the filter.

## Connection form

| Action | Key |
|---|---|
| Next field | `Tab`, `Down`, or `Enter` in a field |
| Previous field | `Shift+Tab` or `Up` |
| Change an option | `Left` and `Right`; in the Vim mode, also `h` and `l` |
| Toggle **Read only** | `Space`, `Left`, or `Right` |
| Press a button | `Enter` |
| Cancel | `Esc` |

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
`R` serves a structure, a key, and a sessions tab. A table tab uses **Apply**.
Without a selection, `Ctrl+C` copies the current cell.
The result that still receives rows does not close, and the **Messages** tab does not either.

In the value window and the row window, the keys are the same in both maps:

| Action | Key |
|---|---|
| Scroll | `Up` and `Down`, or `j` and `k` |
| Toggle the shape of a JSON value | `f` |
| Change row, in the row window | `Left` and `Right`, or `h` and `l` |
| Copy the visible text | `Ctrl+C` |
| Close | `Esc` or `Enter` |

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
| Previous and next page | `[` and `]` |

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
In the table view, the script and transaction actions have no effect, because the table has no script and no transaction.
Those actions are `F5`, `Shift+F5`, `Ctrl+W e`, `Ctrl+W s`, `Ctrl+W E`, `Ctrl+W A`, `Ctrl+W m`, `Ctrl+W C`, `Ctrl+W R`, and `Ctrl+W d`.
In the table grid, `x` and `Delete` also have no effect.
`Ctrl+W c` closes the table tab and frees its data.

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
| Click on the `EXPLORER` title | Give focus to the navigator |
| Click on the table header | Apply sorting: ascending, descending, and no order |
| Double click on a table | Open the table view, with two clicks on the same cell within 400 ms |
| Click on a bar field or button | Give focus to the field, or press the button |
| Click on **Previous** or **Next** | Change page in the table grid |
| Click on a row of the script question | Change the action of the row, or press the button |
| Wheel | Scroll lists, editor, and grid |
| Drag on a divider | Resize the panels |
| Drag on a header border | Resize the column width |
| Drag on the grid | Select cells |

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

The `keymap` field of `config.json` maps an action to a list of keys, per mode.

```json
{
  "keymap": {
    "conventional": { "run": ["ctrl+enter", "f5", "ctrl+w e"], "stop": [] },
    "vim": { "run": ["ctrl+enter", "f5"] }
  }
}
```

A list replaces the default keys of the action, and an empty list removes the shortcut.
A key can have two parts separated by a space, such as `ctrl+w e` and `g g`.
Two actions with the same key, in the same region, invalidate the configuration.
A key and a sequence that starts with it also invalidate the configuration.
An unknown action or key name also invalidates the configuration.
In that case, the invalid configuration rules of the plan apply.
