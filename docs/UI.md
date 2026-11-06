# SQLN — Screen sketches

## State of the design

This document presents a visual proposal based on the [functional plan](INITIAL_PLAN.md).
The plan remains the reference for the confirmed decisions.

The values, names, and data below are fictional examples.
Sizes, spacing, and colors are still visual proposals.
The approved shortcuts are in `docs/KEYMAP.md`.
The user approved the flows and adjustments recorded in `docs/DECISIONS.md` on 2026-10-31.
That approval does not mean that all flows already exist in the code.
The final interface uses English; the Portuguese labels in these sketches were examples.
The application implements sections 1 to 4 and 6, and part of sections 5 and 8.
The form offers **Session only** and **System vault**. Without a vault, saving offers plain text after the warning.
The opening screen of section 1 is a list over the screen, not a panel with search.
Section 2 describes the current screen, in the style approved on 2026-10-31.
The other sketches show the content of each screen, not its exact drawing.
Windows have a rounded border, and buttons lose the brackets in themes with color.

## Local assistant controls

The palette offers **Agent sessions: allow or revoke access**, **Review agent changes**, **Stop all agent tasks**, and per-console direct editing.
In an agent tab, it also offers the session dialog of that tab and the task list of that session.
The MCP row stays above the status line while an access request or an edit proposal waits, or an agent task runs.
It does not capture editor input, and typing or temporary status messages do not clear it.

```text
 MCP  1 access request · 2 edit proposals  F8: sessions             [Review]
 MCP  1 task running                                       [Stop]
```

- The row shows counts with text, the effective shortcut, **Review**, and **Stop** while a task runs.
- A narrow row drops the shortcut first, then uses short counts such as `1 access · 2 edits`.
- **Review** opens the oldest access request, then the oldest proposal. The row never decides access.

A card at the bottom right of the main area, above the MCP row, shows the most urgent agent item.
The card does not take the focus or capture typing, and the editor text stays visible. One card shows at a time, in this order:

| Card | Text | Buttons |
|---|---|---|
| `MCP access request` | `client asks for access to SQLN.` A new request also rings the terminal bell once | **Review**, **Later** |
| `Edit proposal` | `client edits console-1: +2 −1 lines.` | **Review**, **Later** |
| `Agent task running` | `client: Query 3f2a9c1e · 12s`, after 3 seconds of work, so a short query shows no card | **Stop**, **Hide** |

```text
 ╭ MCP access request ───────────────────────────────╮
 │ claude-code asks for access to SQLN.              │
 │ F8: sessions                    [Review]  [Later] │
 ╰───────────────────────────────────────────────────╯
```

- The border and title use the warning color. The title is a text label for terminals without colors.
- **Review** opens the access request or the proposal of the card. **Stop** cancels the task of the card only.
- **Later** and **Hide** hide that item's card; the MCP row keeps the item. The next item's card then shows.
- A new item shows a card again, with its place, such as `(2 of 2)`. A remembered client shows no card and does not ring.
- An area narrower than 36 columns or lower than 6 rows shows the MCP row only.

The access request shows the client, `(reported, not verified)`, the short session identifier, and the request age.
Its options are **Reject**, **Allow**, **Allow all**, and **Full access**. Reject has the initial focus, and `Esc` selects it.
After a decision, the next request opens, with `(1 of N)` in the title while more remain.
The session list shows `Waiting for you`, the option of an allowed session, and `Rejected`, with the running tasks.
It also lists clients with remembered access and no live session, as `client  remembered  option`. Select one to **Forget** it.

A risky write of a session with **Full access** opens the run confirmation of a user run.
Its text starts with `MCP client asks to run this SQL.` **Cancel** has the initial focus.

An edit proposal opens as an [inline review](#inline-review) in its console. The tab of a console with a pending proposal starts with `± `.

## Inline review

AI responses and agent edit proposals show in the editor, as in an IDE.

An AI insertion shows ghost text at the requested place, in the hint style. The text of the console does not change before acceptance. A dedicated row after each ghost text holds its buttons; a ghost has no bar. Several answers can wait at once, each on its own line.
The suggestion list offers two AI rows: **Complete with AI…** and, above it, **Complete with AI with a request…**. The request row opens a modal of one line; `Enter` sends the request with the completion, `Esc` cancels, and an empty input sends a plain completion.
While an answer runs, the requested line shows an animated mark (`|`, `/`, `-`, `\`) in the first column of the gutter, so the user sees each anchor while editing other lines. The ghost replaces the mark when the answer shows.

```text
 1   SELECT 42
     [Tab Accept] [Esc Reject]
 2   FROM t
     [Tab Accept] [Esc Reject]
```

`Tab` decides the ghost at the line of the cursor, at its anchor. `Esc` ends the request of that line, or rejects its ghost. Each ghost stays until the user decides: keys, clicks, commands, cursor movement, tab changes, and schema refreshes keep it, and the editor stays editable. An edit of the anchor line closes only the ghost of that line, in silence; an edit on another line moves the anchor with the text. Any undo takes every ghost. A new request on a line replaces the answer of that line, and a request on a free line adds one, within the configured limit. `Run` executes the text without the ghosts, and the ghosts stay. The tab of the console shows `±N` with the count of the ghosts that wait.

An agent proposal shows an inline diff. During a diff, the AI requests of the console end and new requests refuse; the ghosts that wait stay hidden and return when the review closes. Removed lines have `-` and the error color. Added lines have `+` and the success color. Changed words are bold and underlined. The `-` and `+` marks stay in terminals without colors.

```text
 12   SELECT id, name
 13 - FROM users
 14 - WHERE active
    + FROM users u
    + WHERE u.active = true
      [a Accept] [r Reject]
 15   ORDER BY id;
 1/2 changes · MCP claude [A Accept all] [R Reject all] [Esc Later]
```

- Each pending change has its **Accept** and **Reject** buttons on the row after it, and they decide that change. The bar on the last row of the editor shows the place of the current change, the origin, and the buttons for all changes.
- A narrow row or bar drops the labels of the last buttons first, then shortens the text before the buttons. The keys stay.
- No button has the focus, and `Enter` decides nothing.
- An accepted change goes into the text at once, as one undo step. **Accept all** is one undo step. After a decision, the next pending change becomes current.
- The editor is read only during the review. A key that edits or runs the text shows the reason and the review keys in the status bar. Global keys, `Ctrl+W`, and the palette still work.
- `Esc` is **Later**: the review closes, and the proposal and its decisions stay pending.
- After the last decision, the editor stays. The card shows the next proposal.
- Left/Right and the horizontal wheel move the view sideways; PageUp/PageDown and the wheel move it up and down.
- A click on a change makes it current. A click on a button acts.
- The `Ctrl+W` panel menu, the palette, the hint row, and `F1` list the review commands.
- A change of the text from outside the review, such as a reload or a direct edit of an agent, closes the review with the reason.

The status bar shows `MCP n`, `MCP n all` with **Allow all**, or `MCP n full` with **Full access**. It goes before the transaction and execution state.
An agent tab opens when the user allows access, without a focus change, and stands at the right end of the tab row, before **Quit**.
When the row is full, it scrolls as usual, and agent tabs stay after the other tabs.
The close of the last agent tab of a session with access asks **Close the agent tab**, with **Cancel** (initial focus) and **Revoke and close**.

```text
 console-1 X  console-2 X                      MCP · claude [3f2a9c1e] X  [Quit]
```

Tab labels use `MCP ·` for agent consoles and analyses, `MCP off ·` without access, `Direct ·` for direct edits, and `SQL ·` for SQL that the agent placed and the user did not open yet.
Without placed SQL, an agent console has no editor: its Messages and result grids fill the console, with **Stop**, **Tasks N**, and **Access: level**.
With placed SQL, an editor appears above them. The bar holds **Run**, **New console**, **Clear**, **Stop**, **Commit** and **Rollback** for an open transaction, **Tasks N**, and **Access: level**. The title row adds `SQL from agent`. There is no mode button: the tab runs in Auto mode.
A narrow bar drops **Access** first, then **Tasks**. The palette keeps both commands.

```text
 [Run]  [New console]  [Clear]  [Stop]  [Tasks 1]  [Access: Allow]
 1   CREATE TABLE orders_archive (id INTEGER PRIMARY KEY);
─ Result 1 ─ Messages ───────────────────────────────────
```

- **Access: level** shows `Allow`, `Allow all`, `Full access`, or `off`. It opens the session dialog: **Keep access** has the initial focus, then the other levels and **Revoke**.
- **Tasks N** counts the active tasks of the session on all its connections. It is disabled at zero. It opens a list:

```text
 ╭ Agent tasks ─────────────────────────╮
 │ Query    3f2a9c1e  running  12s      │
 │ DDL read 9b01aa42  validating  4s    │
 ╰──────────────────────────────────────╯
```

- A selected task opens a dialog with the client, the task, and its state. **Keep running** has the initial focus. **Stop** cancels that task only; access stays.
- A task that ended before the selection shows a status message and opens no dialog.
- The Messages of the agent tab record the session's actions: runs and edits of user consoles, proposals and their decision, discarded proposals, opened tabs, access changes, and revocation.
See [MCP](MCP.md) for the operation and persistence rules.

## 1. Startup and console opening

If `.sqln/` is not found in the current directory or the upper ones, ask about its creation.
Accept creates the project in the current directory. Refuse keeps only the global scope.

```text
┌─ Create project ────────────────────────────────────┐
│ No .sqln/ folder was found.                          │
│ Create a project in the current directory?           │
│                                                      │
│ [Create project]  [Use only global settings]         │
└──────────────────────────────────────────────────────┘
```

At startup, the application reopens the tabs of the last session, in the same order and with the same active tab.
The **Reopen tabs** option of the Settings dialog (`reopenTabs` in `config.json`) turns that off. The default is on.

`sqln file.db` and `sqln postgres://user@host/database` open a connection for this session only.
In that case, the saved tabs do not open, the **Create project** question does not appear, and nothing goes to the files.
The project uses the list of its `workspace.json`. Without a project, the application uses the global list.
No tab runs SQL or opens a connection at startup.
A console without text does not come back, because it has no file. A tab whose connection left the form also does not come back.
A console that another instance uses stays closed, and the file list does not change at exit.

Current implementation: `n` in the navigator opens a list over the screen, with the title `Consoles of <connection> · <scope>`.
The first line, **+ New console**, creates a console. The other lines show the name, the age of the last change, and the `(open)` mark.
The list has no search field and no buttons. `Enter` picks, `Delete` removes, and `Esc` cancels.
`s` in the navigator opens the **Scripts without a connection** list, which also shows the scope of each script.

```text
┌─ Connections ───────┬─ Consoles of app (local) ──────────────┐
│ app (local)         │ Search: [                             ]│
│ app (global)        │                                        │
│ warehouse           │ console-1                today, 10:30  │
│                     │ investigate-orders       yesterday, 16:20 │
│                     │                                        │
│                     │ [Open]  [Create new console]           │
└─────────────────────┴────────────────────────────────────────┘
```

The search field and the time presentation are design proposals.
The filtering by connection and the reuse offer are already defined.

Opening an already open console selects its existing tab.
Creating another console generates an automatic name, which can change later.
`Ctrl+W N` renames the current console; the list also offers the action.
Refuse empty or repeated names in the same connection, without changing identity, files, connection, or scope.

`Ctrl+W a` opens the connection change or attach of the console.
Block during an execution or an open transaction.
Show the current connection, the new connection, and the destination folder before the confirmation.
Save before the change; a failure cancels the change.
Keep text, name, cursor, scroll, and the Messages history; clear results and database or schema selection.
Do not run or convert the SQL automatically.

## 2. Console view

Editor and results appear together.
The user can drag the divider and collapse the connection navigator.

```text
▎ ◐ 12.345s  console-1 ×   ✓  orders ×   ✗  users (table) ×
──────────────────────────────────────────────────────────────────────
 EXPLORER        │ app › public › console-1
 ▾ ● ⛁ app  pg   │ ▶ Run  ▶▶ Script  ■ Stop  ◐ Auto  ✓ Commit  ↩ Rollback  ◇ public
   ▾ ◇ public    │  1 ✓ SELECT id, name                               ▐
     ▸ ▦ users   │  2   FROM public.users;                            │
     ▸ ◫ v_sales │  3                                                 │
                 │─ Result 1 ─ Result 2 ─ Messages ─────────────────────
                 │    │ id │ name   │ active
                 │  1 │  1 │ Ana    │ true                             ▐
                 │  2 │  2 │ Bruno  │ true                             │
                 │ 1,000 rows · row cap reached
 EDITOR  app  public  Auto · No transaction · Idle  Pool 0/5 · Data 1/256 MiB  3:14
 F5 run  Shift+F5 script  Ctrl+Space suggestions  Ctrl+F find  F1 keys
```

With `"preferences": { "icons": "nerd" }` in `config.json`, the database logo comes before the name, and `pg` leaves the line.
That set changes all symbols of the interface and needs a Nerd Font.
In iTerm, install the Nerd Font Bundle at `Settings > Profiles > Text > Manage Special Exceptions`. The text keeps the profile font.

The line under the tabs and the panel title appear only with 30 lines or more.
At 80 x 24, the editor and the grid share 18 lines.

**Execution state in the tabs:** a colored badge stays at the start of the tab.

| Badge | State | Without color (`NO_COLOR`) |
|---|---|---|
| `◐ 12.345s` | Running, with the time. The mark spins | `\| 12.345s` |
| `✓` | Finished | `ok` |
| `✗` | Error, timeout, or connection loss | `ERR` |
| `■` | Stopped by the user | `stop` |

The badge of the result stays until the user leaves the tab where they saw it. A tab that ends off-screen keeps the badge until a visit.
The next execution changes the badge. Consoles and tables use the same badges.

**Time:** the status bar shows `Running 12.345s` during the execution and `Idle (last 2.440s)` after.
The time has three decimal places, and the screen updates every 100 ms only during an execution. Long times use `1m 05.123s` and `1h 02m 05.123s`.
The time starts with the execution, so the wait for a password does not count.

**Parameters panel:** when the editor text holds `:name` parameters, a panel appears between the editor and the result tabs, up to five field rows plus a title and an error row.
Each row shows the name, the type (`Text`, `Number`, `Boolean`, `NULL`), the value input, and `Set` or `Unset`.
The title holds the collapse state, the count, and the effective `Ctrl+W P` shortcut; a click on the title collapses the panel to one row.
The wheel moves the selected field. A field without a value at a run shows the error style and takes the focus.
The panel needs no rows when the connection offers no parameters, as Redis and Valkey.

**Tabs:** each tab has `×` at the end. `NO_COLOR` shows `X`, and Nerd Font shows its symbol.
A click on `×` or on the column after it closes the tab, with the questions of `Ctrl+W c`.
Closing another tab does not change the tab on screen. Closing the last tab shows the home screen.
A tab cut by the line does not show `×`. A click on it opens the whole tab.
**Quit:** the right end of the tab row holds the `Quit` button, in the style of the console buttons (`[Quit]` without color). A click quits as `Ctrl+W q`: a running statement or an open transaction asks first. The button is also on the home screen, and the tabs stop one column before it.

**Hint line:** each key uses the style of the keys of the `Ctrl+W` menu, and the effect uses the hint color.
Two spaces separate the commands. Without color, the key is bold and the effect is dim.
The line lists only the commands that act on the screen state, from the most relevant to the least.
A command that does not fit leaves the line whole, so the screen never shows a cut key. `F1` lists all keys.
The normal hint row reserves the effective palette shortcut. It adds more commands as the terminal gets wider.
The line stays one row high. Its order does not depend on usage history.
`Ctrl+W Space` opens the bound commands of the current panel; `Ctrl+W Tab` opens the explorer page.

| State | First commands |
|---|---|
| Editor | End the transaction, run, script, results, find, suggestions, format, and history |
| Editor without connection | Choose the connection and go to the navigator |
| Editor running | Stop |
| Results | Stop, end the transaction, clear selection, copy, value, find, row, editor, and export |
| **Messages** | Return to the editor and switch tab |
| Grid of a table | Stop, clear selection, load, copy, value, find, `WHERE`, sort, row, follow, more, and pages |
| Connection line | Open console, new console, expand/collapse, refresh, edit, and filter |
| **Consoles** line | Expand or collapse, and new console |
| Console line | Open, rename, remove, and return to the **Consoles** line |
| Schema line | Expand, structure, new console, and refresh |
| Table or view line | Open data, structure, columns, refresh, filter, copy name, and SQL/DDL |
| Column line | Return to the table, structure, new console, and refresh |
| Group line | Expand or collapse |
| Object line | DDL, and return to the group |

- `commit` and `rollback` appear only with an open transaction. After an error in the transaction, only `rollback` appears.
- `disc` appears only with an open session. `pwd` appears only on a connection that uses a password.
- Copy, open the value, and search appear only with rows in the grid.
- `more` appears only with more rows to read. `page` appears only with more than one page.
- In the forms, the line changes with the field: text, option, path, or button.
- In the console list, `rename` and `remove` appear only on a script row.
- During an export, the line shows the stop key before the progress.
- During an import, the line shows the rows, the errors, the part of the file, and the stop key.

**Objects:** under a schema, the tree shows a group for each kind of object after the tables and views: `Routines (n)`, `Sequences (n)`, `Triggers (n)`, and `Types (n)`. An empty group does not appear, and each group starts closed.
A routine of PostgreSQL shows its arguments, such as `total(x integer)`, so two overloads have two lines. A trigger shows its table, such as `audit (orders)`.

```text
  ▾ public
    ▦ customers
    ▦ orders
    ▾ Routines (2)
      ƒ total(x integer)
      ƒ total(x text)
    ▸ Triggers (1)
```

**DDL:** the text goes to a new console of the connection, with the schema of the object, and nothing runs. The first line names the kind, the object, and the connection:

```text
-- Function public.total(x integer) · shop
CREATE OR REPLACE FUNCTION public.total(x integer)
...;
```

A text that SQLN writes from the catalog has a second line: `-- SQLN wrote this text from the catalog. The text of the server can differ.` An error of the read goes to the status line, and no console opens.

**Tree filter:** the field takes the first line of the navigator, and the status line shows `FILTER` during typing.
The filter also finds objects by name, under their schema and group. `Enter` on an object in the filtered tree opens its DDL.
Without a result, the tree shows `No match` and `Loaded catalogs only`.

**Database notices:** a PostgreSQL `NOTICE` or a MySQL warning enters **Messages** with the statement number. The tab shows `Messages (n)`, and the active tab does not change.

**Transaction age:** the status bar shows `Transaction open 3m` from 1 min. At 5 min or more, the text uses the error color.

**Bell:** an execution of `bellAfterSeconds` or more rings the terminal BEL at the end. A Stop does not ring.

**Messages:** one message takes the hint line for 8 s.
A key, a paste, or a click removes the message before that. The pointer and the wheel do not remove.
The credential wait stays on the line while the wait lasts, and ends with it.
A write that keeps failing repeats the alert on each new attempt.
The invalid configuration hint stays until `Ctrl+W r` loads a valid file.

**Focus:** the status bar label names the place of the keys: `EDITOR`, `RESULTS`, `EXPLORER`, `FILTER`, or `TABLE`.
In the Vim mode, the editor shows `NORMAL`, `INSERT`, or `VISUAL`.
The title of the focused panel uses the highlight color.
The current line of the editor and the grid appears only in the focused panel.

**Status bar:** the `PROD` badge, the transaction, and the execution never leave the bar.

**Run confirmation:** an `UPDATE` or `DELETE` statement without `WHERE` opens **Confirm the run**, with focus on **Cancel**.
A production connection opens the same dialog for any change. The window shows the count and the first risky statement.
A read-only connection does not ask, because the database refuses the write.

**Formatting:** `Ctrl+W f` formats the selection or the statement under the cursor, in one undo step.

**Comment:** `Ctrl+W /` or `Ctrl+/` comments the selection or the cursor line, in one undo step. The cursor and the selection stay on the same characters, so the same key undoes the change.

Without space, the bar removes in this order: position, data, pool, and schema. Then the connection name shortens.
An open transaction uses the warning color. A failed or unknown transaction uses the error color.

**Grid:** the row number counts the loaded rows.
Click a row number in a result or table grid to select the whole row, including hidden columns.
Copy uses the selected row. A cell click replaces the selection; `Esc` clears it.
The border of a header accepts the drag on the three columns between two grid columns.

**Scroll bars:** the editor and the grid reserve the last column.
The navigator uses its last column only with excess rows.

The indicators stay in the free column, between the row number and the text:

| Symbol | State |
|---|---|
| `●` | Running |
| `✓` | Success |
| `⋯` | Cap or budget reached |
| `✗` | Error |
| `—` | Not run |
| `■` | Stopped |
| `◷` | Timeout |
| `?` | Unknown result |

When the database reports the position of an error, the editor underlines the token and keeps the cursor.
The mark leaves with the next edit, and an edit during the execution prevents the mark.

This example shows a finished execution, with two statements.
The second statement reached the row cap and ended the read.
The connection already returned to the pool, and the user walks the rows without a new query.
Do not show final success of a statement before its read ends.

Control rules:

- **Run:** uses the selection or, without a selection, the statement under the cursor.
- **Whole script:** runs the full text, stopping at the first error.
- **Stop script:** interrupts the sequence without closing the console, erasing the text, or resolving the transaction automatically.
- Do not send the next statements after accepting the stop; do not promise immediate interruption at the server.
- Stopping an active statement sends the cancellation and waits 5 seconds for the response.
- Without a response, close the connection; with an open transaction, ask for confirmation before closing.
- After closing, indicate that the result of the statement is unknown.
- The database or schema selector applies to each execution, even with another physical connection. In PostgreSQL, the chosen schema comes first in `search_path`, and the default path of the session stays after it.
- Accepting a new run clears the result tabs of that console, without erasing the text or resolving the transaction.
- The clearing happens even when the new run needs to wait for a connection; a blocked attempt does not clear the results.
- Each result reads rows up to the cap, without waiting for a user action.
- With **Limit in SQL** on (the default, in Settings > Results), a plain read without its own limit goes to the database with `LIMIT` with the cap and one row. **Messages** shows `Statement n: SQLN added LIMIT <rows> to this statement.` as information, and `Messages (n)` does not count it.
- A result without rows shows the columns and the zero-row indication.
- At the cap, report that the result may have more rows.
- To see more rows, raise the cap and run the command again.
- Block another execution, mode switch, and commit or rollback buttons while the execution is pending or active.
- Switch the mode through the bar selector or the shortcut.

The default SQL timeout is 30 seconds, configurable, with an option to run without a deadline.
It does not replace the connection deadline and does not limit the pool wait.
Count the deadline per statement, subtracting the pool wait and including the row read.

After the execution, an open transaction keeps its connection reserved.
In that state, the user can start another execution or resolve the transaction through the available controls.

## 3. Table view

Double click on a table opens a tab in the same strip of the consoles.
Opening the same table again selects that tab, without duplicating.

```text
┌─ Connections ──────┬─ console-1 ─ [users (table)] ──────────────────────────────────────┐
│ app (local)        │ app_db.public.users                                                │
│   public           │ WHERE    [active = true                                          ] │
│     users          │ ORDER BY [                                                       ] │
│     orders         │ [✓ Apply filters]  [✕ Clear filters]  [↓ Load more]  [■ Stop]            │
│ app (global)       ├─────┬──────────┬───────────────────────────────────────────────────│
│ warehouse          │ id  │ name     │ active                                            │
│                    │ 101 │ Ana      │ true                                              │
│                    │ 102 │ Bruno    │ true                                              │
│                    │ ...                                                                │
│                    │ 200 rows loaded          [◀ Previous]  Page 2 of 2+  [▶ Next]      │
│                    │ No ORDER BY: rows can repeat or be missing between pages.          │
├────────────────────┴────────────────────────────────────────────────────────────────────┤
│ App (local) · Grid data: 1/256 MiB                                                     │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

The example shows a filter that the user already applied.
Keep filters and sorting only while that tab stays open.
Closing the tab discards those values; reopening starts without filters or sorting.
A table tab of the last session comes back without rows, without a filter, and without sorting.
The first selection of the tab loads the rows. A tab that already starts on screen shows `Enter loads the rows.` and waits for `Enter` or **Apply filters**, so the opening does not connect or ask for a password.
A new table does not receive automatic sorting, not even by primary key.

Bar rules:

- Typing does not run queries.
- Refuse a filter with another statement.
- `Ctrl+W x` stops a table query in progress.
- `Tab` accepts an autocomplete suggestion. The **Autocomplete** option changes that key to `Enter` or to both.
- `Enter` applies the fields unless **Autocomplete** uses `enter` or `both`. Then it accepts a suggestion first.
- `[Apply filters]` has the same effect as `Enter`.
- `[Clear filters]` clears the fields and applies the query without a filter or sorting.
- With pending cell edits, the bar adds **Apply changes**, **Review**, and **Discard**. A narrow bar hides the last buttons; the keys still work.
- A click on the header applies sorting at once, keeping the `WHERE` already applied.
- Do not apply a `WHERE` draft as an effect of the header click.
- Invalid filter or sorting leave the grid empty, with the error and the fields available for correction.

Current implementation, from top to bottom: title, `WHERE` field, `ORDER BY` field, buttons
**Apply filters**, **Clear filters**, the pending-edit buttons, **Load more**, and **Stop**, grid, summary, and one warning line.
The title appears only with 30 lines or more. The pool and data use stays in the status bar.
The tab label is `name (table)` or `name (view)`.
The strip mixes consoles, tables, diagrams, and compares in opening order.
Closing the last console with another tab open selects a remaining tab, without creating an empty console.
The summary shows the loaded rows and the state.
The `F` key follows a foreign key of the row of the cursor. With one key, it goes there at once. With more keys, a list opens:

```text
┌ Follow a key ──────────────┐
│ → customers (id)           │
│ ← items (order_id)         │
│ ← payments (order_id)      │
└────────────────────────────┘
```

`→` marks a key that leaves the table, and `←` marks a key of another table that reads the row. The keys of the column of the cursor come first, and a key with `NULL` in the row stays out.
The choice opens the tab of the other table with a filter such as `"customer_id" = 1`. An open tab of that table gets the focus and the new filter, keeps its `ORDER BY`, and reads the first page. A busy tab refuses.
The keys of a table arrive with its columns. When they are not in memory, `F` reads them first and then acts.
The `c` key counts the rows of the applied filter, and the summary starts to show `100 of 12,345 rows loaded`. The key only acts when more rows exist to load.
The count uses the deadline, Stop, the pool queue, and the password of a page, and does not change the grid. **Apply filters** and **Clear filters** clear the total, and sorting keeps the total.
The warning line shows the risk of the unstable order, or the message of the last query.
The error of a first page takes the grid area.
A click on the header sorts in ascending order, then descending, and then removes the order.

Scrolling and navigation walk the already loaded data.
More rows need **Load more** or **Next**.
With more than one page, the line under the grid shows **Previous**, `Page 2 of 3+`, and **Next**. The `+` says that the table may have more rows.
Each page is a slice of the accumulated rows, and the button puts the first row of the page at the top of the grid.
**Next** shows an already loaded page without a query. If the page did not come yet, **Next** loads the page and then shows it.
**Previous** never queries the database. The page number follows the cursor.
The line prioritizes the load state. Without space, the buttons use the short form `◀ 2/3+ ▶`, and the `more rows available` note leaves first. A long state takes the buttons off the line, and `[` and `]` keep working.
Loading a page makes a new query, without keeping a read open between pages.
**Load more** keeps the current cursor and scroll.
**Load more** may wait for a free connection.
Without a unique sort, rows can repeat or go missing between pages.
Show that warning when no sorting is applied.
The global cap of 256 MiB can suspend new loads, without erasing the existing rows.
If a value goes above the limit, report the reason without representing it as null, empty, or truncated content without a warning.
The copy follows the TSV format confirmed in section 10 of the plan, without headers.

### Grid editing

Only keyed, writable SQL table tabs offer cell editing.
`F2` opens a modal value editor; `Enter` still opens the read-only value window.
The modal uses the selected keyboard mode. Global navigation and run shortcuts do not reach the application.
`Tab` walks the editor and the Keep value, Set NULL, and Cancel buttons.
`F2` or `Ctrl+S` keeps the value locally. In Vim insert mode, `Esc` first enters normal mode.
Changed cells show a `*`, a different style, and underline. The summary names pending rows and cells.

`Ctrl+S` or `Ctrl+R` opens a read-only, scrollable review of old values, new values, and SQL.
The review has Apply and Cancel buttons. `F2` or `Ctrl+S` applies; production asks for confirmation.
Errors keep local values visible. Conflicts require discard and reload before a new edit.
A commit without a confirmed response shows Commit result unknown and blocks submission.
Discard does not claim to undo a commit.

Tab switches preserve edits. Page changes, reload, filters, sorting, copy, export, and import require resolution.
Closing a dirty tab or exiting offers Apply, Discard, and Cancel.
An active edit operation blocks immediate close.
After a confirmed commit, the first page reloads with applied fields; draft text stays unchanged.
Cursor and selection return by primary key when their rows remain on that page.
All modal controls fit an 80 × 24 terminal and support the mouse.

### Structure tab

`i` on a navigator row, or in the grid of a table, opens the structure tab.
The tab label is `name (structure)`. Opening the same structure again selects the tab.
The table tab and the structure tab stay open together.

```text
│ [console-1] [users (table)] [users (structure)]                                        │
│ app · users · structure                                                                │
│ [✓ Reload]  [↓ Load more]  [■ Stop]                                                    │
│ ─ Columns ─ Indexes ─ Foreign keys ──────────────────────────────────────────────────  │
│ name    type     nullable  default  key                                                │
│ id      integer  no                 PK                                                 │
│ email   text     no                                                                    │
│ active  boolean  yes       true                                                        │
```

| Navigator row | Content |
|---|---|
| Table, view, or column | Three sections: columns, indexes, and foreign keys |
| Schema | All the columns of the schema, without sections |
| Connection | All the columns of the schemas the navigator shows, without sections |

| Grid | Columns |
|---|---|
| Table columns | `name`, `type`, `nullable`, `default`, `key` |
| Indexes | `name`, `columns`, `unique` |
| Foreign keys | `name`, `columns`, `references`, `on_update`, `on_delete` |
| Schema or connection | `schema`, `relation`, `name`, `type`, `nullable`, `default` |

- The title of a connection shows `all schemas` in the place of the name.
- The tab has no `WHERE` and `ORDER BY` fields, and a click on the header does not sort.
- `[` and `]`, or a click on the section name, switch the section. The switch rereads the grid.
- On a schema or a connection, `[` and `]` change the page.
- **Reload** rereads the grid. DDL does not update the tab on its own.
- The grid uses the pagination, the budget, the search, the copy, and the export of the table tab.
- The export has no target table, as in a console.
- The tab does not come back in the next session.
- Redis and Valkey have no structure. `i` shows the reason in the status bar.

### Diagram tab

`d` on a navigator row, or `Ctrl+W D` in any panel, opens the ER diagram tab. `Ctrl+W D` uses the row with the focus in the navigator, the table of a table tab, or the current schema of a console. The tab label is `name (diagram)`. Opening the same diagram again selects the tab.

```text
│ [console-1] [main (diagram)]                                                           │
│ shop.db · main · diagram                                                               │
│┌ customers ───────┐           ┌ orders ────────────────┐                               │
││ ◆ id     INTEGER │◀──────┐   │ ◆ id           INTEGER │◀─┐                            │
││   name   TEXT    │       └───┤ ◇ customer_id  INTEGER │  │    ┌ order_items ────────┐ │
│└──────────────────┘           │   total        NUMERIC │  └────┤ ◆ order_id  INTEGER │ │
│                               └────────────────────────┘       └─────────────────────┘ │
│┌ employees ────────────┐                                                               │
││ ◆ id          INTEGER │◀─┐                                                            │
││ ◇ manager_id  INTEGER ├──┘                                                            │
│└───────────────────────┘                                                               │
│ 4 tables · 3 keys                                                                      │
```

| Navigator row | Content |
|---|---|
| Schema | All the tables of the schema |
| Table or column | The table, and the tables that its foreign keys join, one step away |
| Connection | The tables of the current schema |

- A box has the table name on its top border and one row per column: the key mark, the name, and the type. `◆` marks the primary key and `◇` marks a foreign key. Without color, the marks are `PK` and `FK`.
- A line joins the row of the foreign key column to the row of the column that it reads. The arrow stands at the table that the key reads.
- The parent of a key stands on the left of its child. A cycle turns one key to the other side. A key to the same table makes a loop on the right border.
- The tables without keys stand in rows under the others.
- A table of another schema is a box with `schema.table` only, in a dim style.
- The selected box has the color of the focus, and so do its lines.
- Views, and the partitions of a PostgreSQL table, stay out.
- The row under the picture shows the count of tables and keys, `keys only`, and `partial` when a limit stopped the read.
- **Reload** (`R`) reads the diagram again. DDL does not update the tab on its own.
- The tab does not come back in the next session.
- Redis and Valkey have no diagram. `d` shows the reason in the status bar.

### Compare tab

`c` on a navigator row, or `Ctrl+W v` in any panel, opens the **Compare** dialog. `Ctrl+W v` uses the row with the focus in the navigator, the table of a table tab, the focus of a diagram, the current schema of a console, or the sides of a compare tab. The source comes from the row, and the target is the next connection with the same schema and table. The first field chooses **Structure** or **Data**: a table row starts with **Data**, and a connection or a schema row starts with **Structure**. An empty table compares the whole schema. A data compare needs a table. The script makes the target equal to the source.

A connection, a schema, and a table field show a list below the field when they get the focus. The text filters the list: each word must be in the name, with no case. A connection or a schema has a search text. A table field filters with its own text, and the table list holds only tables. `Enter` or `Tab` choose the marked name and move to the next field, so a connection leads to its schemas, and a schema to its tables. A click on a row also chooses it. `Esc` closes the list and keeps the value. A table can keep a name that the list does not hold: `Esc`, then `Tab`.

```text
╭─ Compare ────────────────────────────────────────────────────────────╮
│                                                                      │
│   Compare                < Structure >                               │
│ > Source connection      < shop >                                    │
│   Source schema          < public >                                  │
│   Source table                                                       │
│   Target connection      < shop-staging >                            │
│   Target schema          < public >                                  │
│   Target table                                                       │
│   Removals as statements [ ]                                         │
│                                                                      │
│   The script makes the target equal to the source. An empty table    │
│   compares the schema.                                               │
│   [ Compare ]  [ Cancel ]                                            │
╰──────────────────────────────────────────────────────────────────────╯
```

The tab label is `name (compare)`. Opening the same compare again selects the tab and reads both sides again.

```text
│ [console-1] [public (compare)]                                                         │
│ shop · public → shop-staging · public · compare                                        │
│ Change │ Object  │ Table  │ Name  │ Source             │ Target                         │
│ create │ table   │ items  │ items │ 3 columns          │ NULL                           │
│ drop   │ table   │ old    │ old   │ NULL               │ 1 column                       │
│ alter  │ column  │ owners │ name  │ text NOT NULL      │ character varying(5)           │
│ 3 differences                                                                          │
```

- The grid has the search, the copy, the row window, and the export of the other grids. It holds each difference, so **All rows** reads nothing.
- `S` opens the script in a new console of the target and does not run it. `Y` copies it.
- A removal stays a comment unless **Removals as statements** is on.
- Between two dialects, the grid shows the differences of names, `NULL` rules, keys, and indexes, and the row under the grid says `no script: two dialects`.
- **Reload** (`R`) reads both sides again.
- The tab does not come back in the next session.

A data compare has the label `name (data)`. The grid has a `Change` column and the columns of the source table.

```text
│ shop · public.prices → shop-staging · public.prices · data compare                      │
│ Change            │ id │ name     │ price │ note                                       │
│ update name, note │ 2  │ old name │ 3.00  │ x                                          │
│   target          │ 2  │ name     │ 3.00  │ NULL                                       │
│ delete            │ 4  │ gone     │ 9.00  │ NULL                                       │
│ insert            │ 3  │ new      │ 1.00  │ it's                                       │
│ 3 differences                                                                          │
```

- The rows match by the primary key of the source. A table without a primary key, a target without the table, and a target without a column of the source refuse with the reason.
- An update takes two rows: the source row with the names of the changed columns, and the target row under it.
- The script has one statement per row in one transaction. A `DELETE` stays a comment unless **Removals as statements** is on.
- A source table that does not fit in the data budget ends the compare with the reason, and no result.
- The script uses the literals of the target, so a data compare also works between two dialects.

### Export

`e` in the grid, or **Export the result to a file** in the palette, opens the modal. The export covers the active result and generates a file.

```text
╭─ Export ───────┬─────────────────────────────────────────────────────╮
│ > CSV          │   Rows             < All rows >                     │
│   TSV          │   File             sers-2026-10-31-1530.csv [...]  │
│   JSON         │   Header           < On >                           │
│   JSON Lines   │   Delimiter        < , >                            │
│   SQL INSERT   │   NULL text        < Empty >                        │
│                │   UTF-8 BOM        < Off >                          │
│                │                                                     │
│                │   [Export]  [Cancel]                                │
╰────────────────┴─────────────────────────────────────────────────────╯
```

| Range | What it writes |
|---|---|
| **Selection** | The selected rows and columns |
| **Loaded rows** | The rows of the grid |
| **All rows** | All the rows and columns, with a new read. The table uses the applied filter and order |

The modal opens at **Selection**, when it exists. Otherwise, it opens at **All rows**, when available, and last at **Loaded rows**.
**All rows** disappears when the grid already has everything.
In the console, **All rows** needs a plain read and a command in the execution, or an open transaction. In the other cases, the modal explains the reason.
Redis and Valkey offer only the loaded rows, without SQL INSERT.

| Format | Options |
|---|---|
| CSV | Header, delimiter, NULL text, and BOM |
| TSV | Header, NULL text, and BOM |
| JSON and JSON Lines | None |
| SQL INSERT | Table, one per row or grouped, and rows per `INSERT` |

The modal has 72 columns and grows to 140 on a larger screen, with 8 free columns. The height does not change, because it already shows all the fields.
The **File** field accepts `~` and a path relative to the current folder, and creates the missing folders. The extension follows the format until the user edits the path.
An existing file asks for **Replace** or **Cancel**.
The `...` button and `Ctrl+O` open the file explorer.

After **Export**, the modal closes, the tab becomes busy, and the hint line shows the written rows. Each tab can have one export, and the modal refuses a destination in use.
**All rows** has no time limit, and **Stop** cancels. The grid keeps the result.
The write uses `<name>.tmp` and renames at the end. After **Stop** or an error, a window offers **Delete** and **Keep**. Without written rows, the temporary file disappears without a question.

### Import

`I` in the navigator or in a table grid, or the import action of the palette, opens the modal. The navigator first opens the table. The modal reads the columns of the table first, so the rows of the grid stay.

```text
╭ Import ────────┬──────────────────────────────────────────────────────────╮
│                │   Import to people                                       │
│   File         │   Column                Type              File column    │
│   Format       │   id                    INTEGER           < ID >         │
│ > Columns      │   name                  TEXT              < Name >       │
│   Options      │   age                   INTEGER           < age >        │
│   Preview      │   photo                 BLOB              < ignore >     │
│                │                                                          │
│                │   [Import]  [Cancel]                                     │
╰────────────────┴──────────────────────────────────────────────────────────╯
```

The modal has 78 columns and grows to 140 on a larger screen, with 8 free columns.
The sections are **File**, **Format**, **Columns**, **Options**, and **Preview**.
**File** reads the path when the focus leaves the field. The name of the file and its first line choose the format and the delimiter, and the user can change them.
**Columns** starts with a mapping by name, or by position in a file without a header. A table column with **ignore** keeps its default.
**Options** starts with **All or nothing** in each dialog. **Log errors** adds the log path, with the `...` button and `Ctrl+O`, and the error limit.
**Preview** shows the first 50 rows with the current choices, and a row with a problem shows its reason.

After **Import**, the modal closes, the tab becomes busy, and the hint line shows the progress. **Stop** ends the import.
At the end, a window shows the inserted rows, the rows with errors, and the path of the log. The tab then reads the table again.
A connection marked as production asks before the import, with the file, the table, and the connection.

### File explorer

The path fields have a `...` button. The button and `Ctrl+O` open this window above the modal:

```text
╭ Choose a file ───────────────────────────────────────────────╮
│ /home/ana/projects/store                                     │
│ >                                                            │
│ Use this folder                                              │
│ ../                                                          │
│ docs/                                                        │
│ src/                                                         │
│ Cargo.toml                                                   │
│ store.db                                                     │
╰──────────────────────────────────────────────────────────────╯
```

The window stays 8 columns narrower than the modal, and the list shows 14 to 35 rows, according to the screen height.
The fields are **File** of Export, **File** and **Log file** of Import, **File** of SQLite, **CA file**, **Client certificate**, and **Client key**.
The window opens in the folder of the field path. Without it, it opens in the closest existing folder, and with an empty field it opens in the current folder.
Folders come first, and the order ignores case. All files appear, except the hidden ones. A filter that starts with `.` shows the hidden ones.
`Enter` on a folder enters it. `Enter` on a file fills the field with the absolute path and closes the window. `Esc` closes the window, and the field does not change.

**Use this folder** appears only in Export, **Log file** of Import, and SQLite, because those fields accept a new file. The row changes the folder and keeps the name of the field. With an empty field, it receives the folder, and the user types the name.
In Export, the extension keeps following the format after **Use this folder**, and stops following after picking a file.
Closing the tab or exiting erases the temporary file.

### Redis or Valkey key

In the navigator, each numbered database shows up to 1,000 keys. `(partial list)` beside the database marks an incomplete list.
`Enter` on a key opens the value in a tab with the label `<key> (key)`.
The tab uses the grid, the pages, and **Load more**, without `WHERE`, `ORDER BY`, **Clear filters**, and sorting.
**Reload** reads the value again.
In the console, each line is one command, as in `redis-cli`. A `#` at the start of the line makes a comment.

## 4. Connection form

Offer form and URI input, with the same credential policy.
With an active project, select project by default and allow choosing global.
Without an active project, offer only global.
The example below considers an active project.

```text
╭ New connection ┬──────────────────────────────────────────────────────────╮
│                │                                                          │
│ > General      │   GENERAL                                                │
│   Address      │                                                          │
│   Credentials  │ > Type                     < PostgreSQL >                │
│   Security     │   Name                     app                           │
│   Options      │   Scope                    < Project >                   │
│                │   Entry                    < Fields >                    │
│                │                                                          │
│                │   [Test connection]  [Save]  [Cancel]                    │
╰────────────────┴──────────────────────────────────────────────────────────╯
```

The fields change according to the type. SQLite uses a file, not an in-memory database.

The form has a sidebar, as the Settings dialog has. Each section shows its own fields:

| Section | Fields |
|---|---|
| General | Type, Name, Scope of a new connection with a project, Entry of a network source |
| Address | Host, Port, and Database; or the URI; or the SQLite file |
| Credentials | User, Password, and Credential |
| Security | TLS, CA file, client certificate, and client key |
| Options | Schemas (Databases in MySQL and Redis), SQLite foreign keys and busy limit, Pool, Read only, Production, Connect limit, SQL limit, Idle limit, Theme, and Icon |

A section without fields stays out of the sidebar, so SQLite has no Credentials and no Security.
The buttons stay at the bottom of each section.
All sections keep the height of the largest section, so the window does not move. The width starts at 76 columns, grows to 140 columns on a larger screen, and leaves 8 columns free.
The form of a connected connection also has the sidebar: General holds the name, and Options holds the schemas, the theme, the icon, and the production mark.
A save or a test with a bad value opens the section of the field and puts the focus on it.
**Idle limit** closes a session that waits in the pool for that time, with 300 seconds as the default and 0 for no limit. A console with an open transaction keeps its session.
**Schemas** holds the schemas, or the databases, that the connection shows, with commas between them. Empty shows all. `Ctrl+O` or the `...` button reads the names from the server and opens a list with a mark on each row. The current schema always appears. A connection with a list shows `(filtered)` in the explorer.

The focused field has a mark at the start of the line, a highlighted label, and its own background.
Without color, the mark is `>`, and the label is bold.
The form note ends on the line above the buttons. A long note takes the two lines of the area.
A test success uses the success color. A progress note uses the warning color, and an error uses the error color.
Resolve relative paths of `config.json` according to the scope: project root or global configuration folder.
In the form, a typed SQLite file starts from the current directory, and the connection saves the full path.
A saved and unchanged path continues as it is in the file, and the test uses the folder of the scope.

The form suggests disabled TLS on `localhost`, `127.0.0.0/8`, `::1`, and Unix socket, and verification on the others.
The **Read only** field is applied by the database and prevents accidents.

The **Production** field shows `PROD` in the status bar, including with `NO_COLOR`.
It also asks for confirmation before an execution that is not a plain read.
The field changes with the active connection, like the name and the theme.

Require a required and unique name per scope, without case distinction, including on rename.
Allow equal names between global and project, with an origin mark when there is a collision.

Require a disconnect before changing access data, TLS, or the pool limit of a connected definition.
A URI does not authorize saving a password in the project configuration.
Do not show passwords in examples, error messages, or public representations of the connection.

## 5. Confirmations and errors

### Open transaction

When closing a console with an open transaction, offer commit, rollback, or cancel of the close.
At application exit, ask separately for each console.
Run the choice before presenting the question of the next console.
Cancelling later does not undo already finished operations.

When switching to autocommit with an open transaction, use the same three decisions to finish or cancel the switch.
Do not select commit as an automatic consequence of the switch request.

If the commit or rollback of the close fails, keep the console open and show the error.
Offer a retry, when the original session and the transaction state allow it, or cancel the close.
Do not repeat the operation automatically or use another connection to try to finish the previous transaction.
If the connection is lost without confirming the result, indicate that the result is unknown.
During exit, resolve the current failure before asking about another console.

### Connection

- Connect on first use, without opening connections at application startup.
- Offer the **Disconnect** action on each connected definition.
- Before the first connection to a destination on this machine, show host, port, user, and TLS.
- Ask again when one of those values changes.
- After losing an open transaction, mark the state as unknown and require confirmation before the next execution.

### Reading and closing

- Closing an active console asks for confirmation to stop the sequence after the current statement finishes.
- Do not allow closing a result while its statement still reads rows; use **Stop script**.
- Do not promise immediate cancellation of the command in progress at the server.

### File and configuration

- Deleting a saved console or a connection needs confirmation of the affected items.
- Closing a tab does not delete its saved script.
- Removing a connection keeps its scripts on disk, without cascade deletion or automatic reattach by name.
- Show those scripts in the Scripts without a connection list, allowing open and edit without running SQL.
- Require the choice and confirmation of a connection before running; attaching does not run SQL automatically.
- If the attach changes the scope, report the new file location before finishing.
- A save failure keeps the text in memory and shows an alert.
- Closing with unwritten changes needs explicit confirmation.
- An external change of the script needs a choice between reload and overwrite.
- An invalid JSON configuration shows file and error position, without unlocking database operations until the fix.
- The user fixes that file outside the application and uses **Reload**.
- An unreadable `workspace.json` uses the initial state and shows a non-blocking warning.
- The action for a removed script uses the label **Close and discard the text**.

### Credentials

Use the vault as the main storage.
Only the absence of a vault allows offering global plain-text storage, with explicit authorization.
A locked vault or a vault with an error does not enable that exception.
With the vault locked or with an error, offer **Try again**, **Use password in this session**, and **Cancel**.
Treat an unknown failure as an error, never as an absence.
Confirm that distinction in real tests before releasing plain text as an alternative.
A session password stays in memory, per connection, until the application closes.
If the write fails, keep the password in memory, report that it was not saved, and offer an explicit retry.
When a vault starts to exist, ask for confirmation to migrate and verify the write before offering to remove the global password.
A migration failure does not authorize erasing the previous password.

Use stable identities when moving a project.
Ask for confirmation before reusing credentials on copies or when copy and folder change cannot be told apart.
On another computer without the credential, ask for the password, without carrying secrets through the project.
Changing the destination does not authorize sending the previous password automatically.
Removing a connection does not erase its password from the vault; deleting that password needs a separate confirmation.
Warn that other copies of the project may use the same credential.

Ask for the password before the first use that needs it, including test and catalog read.
Start with session-only use. Saving needs an explicit choice.
The password prompt, the trust confirmation, and the vault failure recovery already exist in the code.
The password stays masked, and the save option starts unchecked.
The form allows session or vault; testing a typed password does not write it or replace the password of the saved connection.
A new project path needs confirmation also when testing or saving an existing definition.

#### Credential flows

These flows follow approved rules.

| Flow | Required content | Security rule |
|---|---|---|
| Global plain text | Warning before authorization, unchecked option, and identification of `credentials.json` in the global folder | Offer only with a confirmed absence of vault; never write the secret into the project |
| Migration | Confirmation to migrate and, after the check, the offer to remove the global password | Keep the source on failure and report the incomplete migration |
| Refused password | Prompt for another password once and suspension notice after the second refusal; during suspension, each explicit action opens the prompt | Do not confuse refusal with network, TLS, or SQL failure; do not try to connect on its own during suspension |
| Credential deletion | Separate confirmation, with the connection identified and a warning about other project copies | Removing the connection does not authorize erasing the secret |

The current screen tests do not validate the native dialogs of Keychain or Secret Service.
The [vault validation](INITIAL_PLAN.md#validation-of-the-real-vaults) separates recorded checks from pending desktop validation.

## 6. Navigation and focus

The conventional mode is the default. The Vim mode also acts on the interface navigation.
The mouse works in both modes.

Contexts that need separate handling:

| Context | Behavior |
|---|---|
| Navigator | Select connection or object; open table by double click |
| Main tabs | Switch between already open consoles and tables |
| Editor | Edit SQL; `Tab` accepts suggestions and `Enter` breaks the line, unless another choice in **Autocomplete**; run only through an explicit action |
| Filters | `Tab` completes; `Enter` applies |
| Results | Select and copy data; navigate only in the loaded rows |
| Dialog | Resolve the question without triggering execution commands in the background |

Autocomplete offers schemas, tables, views, columns, aliases, CTEs, and dialect keywords.
Local lists also provide built-in types, functions with signatures, column options, and query clauses.
User-defined function completion stays out of scope.
Without a catalog available, suggest the keywords of the dialect.
The focus, tab, and execution shortcuts are defined in [KEYMAP.md](KEYMAP.md).
Navigation shortcuts do not capture characters during text input.
Data, names, and external errors must not be interpreted as terminal control sequences.

## 7. Return to the console

Reopening a console recovers its text, cursor, and scroll.
Do not recover grid data or run SQL automatically.
If the console is already open, only select the existing tab.
If another instance uses it, block its opening in this instance.

## 8. Complementary sketches

### Open manual transaction

The **Messages** tab keeps the command history of each open console in memory.
It shows the commands sent to the database, their results, notices, and errors.
A new execution replaces result grids, but it keeps this history.
A disconnect or a connection change also keeps the history and its connection labels.
Closing the console or the application discards it. SQLN does not save this history.

Commands include the effective SQL, with added limits and EXPLAIN prefixes, and parameters shown separately.
Internal commands include metadata reads, context, transaction control, cleanup, and health checks.
Operations without SQL show their technical names. Redis arguments use safe byte escapes.
Authentication and operations hidden inside drivers do not enter the history.
Import operations do not enter the history, including their setup and cleanup.

Commands of a console stay in that console, even while another tab is active.
Shared operations go only to the active SQL console of the same connection.
The destination stays fixed from the start of the operation. Without a matching console, SQLN discards the shared record.
Settings > Results > **System commands** (`showSystemCommands`) controls the visibility of Internal and Shared records.
The default is Off. The setting applies to existing records after Save, without deleting them or stopping capture.
Hidden records still use the history budget. The global and project scopes use the same precedence as other settings.
SQL from the editor and its errors stay visible.

Messages shows each command as a compact block, without a box.
The heading shows the command number, connection, and origin, with a separator rule.
SQL uses the same syntax styles as the editor, with the dialect of the recorded command.
Blank rows separate the heading, SQL, and result.
The result line combines the status, row count when available, and execution time in milliseconds.
Success, warning, error, and interrupted results have different marks and styles.
Notices and other details stay below the result. SQL keeps its syntax colors after an error.
Plain themes use text marks, and narrow views wrap the blocks without changing the SQL.

```text
── #24 · app_db · User ─────────────────

SELECT id, name
FROM users;

✓ Completed · 12 rows · 8.420 ms
```

Each completed or interrupted command keeps its execution time, including after text truncation.
The clock starts after the start record enters the event queue and stops before the completion record enters it.
Pool waiting and delivery of those history events do not enter the command time.

Internal records do not select the Messages tab or increase its warning count.
The warning count continues to describe the current execution.

Each console has a 10 MiB history budget, including stored text and record structures.
SQLN removes the oldest complete records when needed and shows a removal marker.
A command too large for one record has an explicit truncation marker; the database still receives its full text.
The view follows new records only while the scroll is at the end.
Reading older records keeps the current position, unless those records leave the budget.
With the pointer over Messages, the mouse wheel or touchpad scrolls three rows per event, without a click first.
This does not move the editor or change keyboard focus. Horizontal wheel events do not move Messages.
The scroll stops at each edge, and reversing the wheel moves it immediately.
Returning to the end resumes following new records.

```text
┌─ console-1 ──────────────────────────────────────────────┐
│ Schema: [public]                                         │
│ [Run] [Whole script] [Stop script]                       │
│ Mode: [Manual] · Open transaction · [Commit] [Rollback]  │
├──────────────────────────────────────────────────────────┤
│ 1 ✓ UPDATE users SET active = false WHERE id = 7;        │
├──────────────────────────────────────────────────────────┤
│ Messages                                                 │
│ 1 row changed. Transaction not confirmed yet.            │
└──────────────────────────────────────────────────────────┘
```

### Pool wait

**Stop script** cancels the wait without sending SQL to the database.

```text
┌─ console-2 ──────────────────────────────────────────────┐
│ Waiting for connection · Pool busy: 5/5                  │
│ The SQL has not reached the database yet.                │
│ [Stop script]                                            │
└──────────────────────────────────────────────────────────┘
```

### Cancellation

The first sketch shows the 5-second wait for the statement response.
The second appears only with an open transaction.

```text
┌─ Stop script ────────────────────────────────────────────┐
│ Cancellation sent. Waiting for the database response.    │
│ Time: 3 of 5 seconds                                     │
└──────────────────────────────────────────────────────────┘
```

```text
┌─ Close connection ───────────────────────────────────────┐
│ The database did not answer the cancellation.            │
│ Closing the connection ends the open transaction.        │
│ The uncommitted changes will be lost.                    │
│                                                          │
│ [Close connection]  [Keep waiting]                       │
└──────────────────────────────────────────────────────────┘
```

### Budget exhausted

In the consoles, the budget ends the read like the row cap.
In the table view, **Load more** stays unavailable until space exists.

```text
┌─ Grid data ──────────────────────────────────────────────┐
│ Budget reached: 256/256 MiB                              │
│ The loaded rows were kept.                               │
│ Close tabs or raise the limit to load more rows.         │
└──────────────────────────────────────────────────────────┘
```

### SQLite form

If the file does not exist, ask before creating it.

```text
┌─ Connection ─────────────────────────────────────────────┐
│ Type:         [SQLite]                                   │
│ Name:         [local]                                    │
│ Scope:        [Project]  (can change to Global)          │
│ File:         [data/app.db]                              │
│ foreign_keys: [SQLite default]                           │
│ Lock:         deadline [5] seconds                       │
│ Pool:         [5]                                        │
│                                                          │
│ [Test connection]  [Save]  [Cancel]                      │
└──────────────────────────────────────────────────────────┘
```

### URI input

A password in the URI follows the credential rules and does not stay in the project configuration.

```text
┌─ Connection ─────────────────────────────────────────────┐
│ Type:    [PostgreSQL]                                    │
│ Name:    [app]                                           │
│ Scope:   [Project]  (can change to Global)               │
│ Input:   [URI]                                           │
│ URI:     [postgres://app_user@db.example.test/app_db]    │
│                                                          │
│ [Test connection]  [Save]  [Cancel]                      │
└──────────────────────────────────────────────────────────┘
```

### Edit of a connected connection

Access data, TLS, and the pool limit need a disconnect.

```text
┌─ Edit connection: app (local) ───────────────────────────┐
│ This connection is connected.                            │
│ Disconnect to change access, TLS, or the pool limit.     │
│                                                          │
│ Name: [app]                                              │
│                                                          │
│ [Disconnect]  [Save name]  [Cancel]                      │
└──────────────────────────────────────────────────────────┘
```

### Destination confirmation

This question appears before the first connection to a destination on this machine, according to section 3 of the plan.
Show destination, user, and TLS; start with focus on **Cancel**.
The confirmation already exists in the code. A new path can show **Confirm project path**.
Cancel prevents sending the operation that awaits those credentials.

```text
┌─ Confirm destination ────────────────────────────────────┐
│ Host:    db.example.test                                 │
│ Port:    5432                                            │
│ User:    app_user                                        │
│ TLS:     Verify certificate                              │
│                                                          │
│ [Confirm]  [Cancel]                                      │
└──────────────────────────────────────────────────────────┘
```

### Password prompt

Start with session-only use, with the save option unchecked.
The option text changes according to the vault.
Without a vault, the option says that the global storage uses **plain text** and needs authorization.
A locked vault or a vault with an error does not allow that alternative.

```text
┌─ Password: app (local) ──────────────────────────────────┐
│ User:    app_user                                        │
│ Password:  [••••••••]                                    │
│                                                          │
│ [ ] Save in the system vault                             │
│                                                          │
│ [Connect]  [Cancel]                                      │
└──────────────────────────────────────────────────────────┘
```

### Scripts without a connection

The execution stays blocked until the choice and confirmation of a connection.

```text
┌─ Scripts without a connection ───────────────────────────┐
│ investigate-orders         yesterday, 16:20              │
│ monthly-report             10/12, 09:15                  │
│                                                          │
│ [Open]  [Attach to a connection]                         │
└──────────────────────────────────────────────────────────┘
```

### Settings

Open with `Ctrl+W ,`.
The window has 72 columns and grows to 140 on a larger screen, with 8 free columns.
The command list in **Shortcuts** grows to 30 rows. Other sections keep a fixed height.
The lists, the conflicts, **Rename console**, the password, and the questions grow to 100 columns. The lists and the conflicts also grow to 30 rows. The cell detail and the help grow to 140 columns.
The project writes only the values it declares.
Offer the global or project scope and the fields below.
Show the values inherited from global and allow removing a project definition to use the global one.
**Save** writes the changes of all sections. **Cancel** changes nothing.
The sidebar groups fields in **General**, **Editor**, **Results**, **Appearance**, **MCP**, **Autocomplete**, and **Shortcuts**. The panel shows one section at a time.
The scope and the buttons stay outside the sections, and appear in all of them.
An error note appears above the buttons, even when the field is in another section.
A new section needs a row in the `SECTIONS` table, and the window grows with the largest section.
Use **Shortcuts** to record, remove, or restore command bindings per keyboard mode and scope.
Settings checks conflicts before saving. Hand edits of `preferences.keymap` remain supported.
When the keyboard mode changes, keep text, cursor, and scroll.
The switch happens in the editor itself, so the selection and the history continue, including on reload.

```text
╭─ Settings ─────┬─────────────────────────────────────────────────────╮
│                │                                                     │
│ ▌ General      │   Scope         < Project >                         │
│   Editor       │                                                     │
│   Results      │   GENERAL                                           │
│   Appearance   │   Keyboard      < from global: Vim >                │
│                │   Reopen tabs   < Default: On >                     │
│                │                                                     │
│                │                                                     │
│                │                                                     │
│                │                                                     │
│                │   [Save]  [Cancel]                                  │
│                │                                                     │
╰────────────────┴─────────────────────────────────────────────────────╯
```

The **Editor** section includes **Autocomplete** and **Undo limit**, with 64 MiB per console as the conventional undo default.
The **Results** section includes **Limit in SQL**, on by default, and **System commands**, off by default.
**System commands** only filters the Messages view.

The **MCP** section has the MCP fields. See [MCP settings](MCP.md#mcp-settings).
Below the fields, a help text shows the Claude Code command and a reference to `docs/MCP.md`. SQLN never changes the files of an MCP client.

The **Appearance** section has the **Default theme** field.
The choice changes the colors at once, before saving. `Esc` and **Cancel** bring back the saved theme.
In the global scope, a theme defined in the project keeps priority.

The connection form has the **Theme** field, which defines a theme only for that connection.
The **Default** value uses the default theme. The open tab decides the colors of the whole screen: the screen uses the theme of the connection of the tab, and a tab without its own theme uses the default theme.
That field also changes the colors before saving. It works with the active connection and does not close the session or the transaction.
While Settings is open, the screen shows the chosen default theme, even in a tab with its own theme.

The **Icon** field changes the icon of the connection in the navigator. The **Default** value uses the database icon.
The icons are `⚠ ☠ ☢ ⚑ ♛ ⚗ ★ ◆ ⚒ ⚙ ⌂ ♥ ❄ ✔`, and the name in the file is the English name in lowercase, such as `skull`.
The chosen icon stays before the name, and the database mark (`pg`, or the logo with the Nerd Font) stays after it:

```text
 ▸ ● ☠ prod  pg
```

That field also works with the active connection and appears in all icon sets, including with `NO_COLOR`.

| Theme | Value in `theme` | Use |
|---|---|---|
| Auto | `auto` | Astrodark with truecolor; terminal colors in the other cases |
| Astrodark | `astrodark` | Neutral dark |
| Terminal colors | `ansi` | Colors of the terminal profile, without truecolor |
| Tokyo Night | `tokyo-night` | Bluish dark |
| Catppuccin Mocha | `catppuccin-mocha` | Violet dark |
| Catppuccin Latte | `catppuccin-latte` | Light |
| Gruvbox | `gruvbox` | Warm dark |
| Nord | `nord` | Bluish gray |

`NO_COLOR` has priority over any theme.

### Close with an open transaction

The initial focus stays on **Cancel close**, the option with no effect on the database.
There is no commit as a default answer.

```text
┌─ Close console-1 ────────────────────────────────────────┐
│ This console has an open transaction.                    │
│                                                          │
│ [Commit and close]  [Rollback and close]                 │
│ [Cancel close]                                           │
└──────────────────────────────────────────────────────────┘
```

### Detail window

`Enter` in the grid opens the full value. The window is read-only.

```text
┌─ users.bio · row 2 ──────────────────────────────────────┐
│ Full text of the selected value, with automatic          │
│ line wrap and scroll.                                    │
│                                                          │
│ 1,240 characters · text                                  │
│ [Copy]  [Close]                                          │
└──────────────────────────────────────────────────────────┘
```

A valid JSON object or array of up to 1 MiB opens formatted, and `f` switches between the formatted text and the original.
The formatting only changes spaces: the keys keep the order, and the numbers keep the digits.
`Ctrl+C` copies the visible shape, and the grid `Ctrl+C` copies the original value.
The **Format JSON** option of the Settings dialog chooses the initial shape. The default is on.

### Row window

`r` in the grid opens the row of the cursor vertically. The window is read-only.

```text
╭─ row 3 of 100 loaded ───────────╮
│ id         : 3                  │
│ name       : Ana Souza          │
│ bio        : long text that     │
│              wraps aligned      │
│ deleted_at : NULL               │
╰─────────────────── 12 columns ──╯
```

`Left` and `Right` change row, without reading another page, and the grid cursor follows the row.
The window keeps the scroll, so the same columns stay in view when comparing two rows.
Each value shows up to 4,096 columns and ends with `…` at the cut. `Enter` in the grid shows the whole value.
The JSON stays compact in this window. `f` toggles formatting only in the cell value window.

### Help screen

`F1` shows the effective shortcuts of the active mode and the focused panel.
The reference groups the panel keys, execution, transactions, connections, tabs, panels, and application commands.
Empty groups do not appear. The key column has a separate style. Long keys and descriptions wrap without cuts.

Type words to filter by group, command, or shortcut. Each word must match, without case sensitivity.
`Tab` and `Shift+Tab` select a group, or **All**. The filter stays when the group changes.
`Up` and `Down` scroll one row. `PageUp` and `PageDown` scroll one page. The mouse wheel also scrolls.
`Ctrl+Home` and `Ctrl+End` go to the start and end. `Home` and `End` move the search cursor.
`Ctrl+U` clears the filter. `Ctrl+C` copies the reference. `Esc`, `Enter`, and `F1` close it without running commands.
The footer shows the matching command count and the visible row range. A scrollbar shows the position.

```text
╭─ Keys · Conventional · Editor ──────────────────────────╮
│ > Type a command or shortcut to filter                  │
│ Group: All · Tab / Shift+Tab                            │
│ Editor (5)                                             │
│ Ctrl+Space                Suggestions                  │
│ Ctrl+F                    Find and replace             │
│ Ctrl+Z, Ctrl+Y            Undo, redo                    │
│ Tab                       Accept the suggestion        │
│ Esc                       Close the list               │
│                                                        │
│ Run (8)                                                │
│ ctrl+enter, f5, ctrl+w e   Run the selection or the     │
│                           statement                    │
╰───────────────────────────── 42 commands · 1–10/56 ─────╯
```

### Prefix menu

`Ctrl+W` opens this window above the status bar. The window shows the effective keys of the `keymap`.
The rows change with the panel of the cursor, and a group without rows leaves the window. `docs/KEYMAP.md` has the rule.
`Space` opens the functional commands of the current panel, with its effective shortcuts.
The title names the panel and, when applicable, the console or table. `Tab` keeps the explicit explorer page.
An unmatched key or `Esc` closes the page without editing text. Unassigned commands remain available in the palette.

```text
╭─ Ctrl+W ─────────────────────────────────────────────────────────╮
│ Panels                Run                    Connection          │
│ hjkl Move the focus   e    Run the statement d    Schema or da…  │
│ b    Toggle the expl… s    Run the script    a    Change conn…   │
╰──────────────────────────────────────────────────────────────────╯
```

### Query history

`Ctrl+W H` opens the **Query history** window, with the statements of the console connection, the newest first.
Each row shows success or failure, the age, and the start of the SQL. The typed words filter the list.
`Enter` inserts the statement at the cursor and does not run it. The **Query history** option of the Settings dialog turns the history off.

### Command palette

`Ctrl+P` opens this window. Each word can appear in the label, shortcut, action name, or technical aliases.
The list includes available functional editor, grid, diagram, compare, and application commands, without duplicate actions.
Stop and transaction actions take priority, then panel actions. Unavailable actions stay out of the list.
Navigator row actions require navigator focus. Add connection and scripts without a connection remain global.
Actions name their target where needed. A panel shortcut appears only where that shortcut works.
The palette includes Apply filters and Clear filters for table filters without adding default shortcuts.
The window has 64 columns and 12 list rows, and grows to 100 columns and 30 rows on a larger screen. It stays on top.

```text
╭─ Commands ───────────────────────────────────────────╮
│ > conn                                               │
│ Change the connection of the console        Ctrl+W a │
│ Add a connection                                     │
│ Scripts without a connection                         │
╰──────────────────────────────────────────────────────╯
```

### Home screen

The screen appears when no tab is open: after the close of the last tab, and at a start
without a connection, without a tab of the last session, and without a database on the
command line. With a connection, the start opens a console of the first connection.
The explorer gets the focus, so `a`, `n`, `s`, and `Enter` act at once.
The screen takes no text, and the commands of a tab have no effect. `Ctrl+W q` quits.

```text
 EXPLORER               │
 ▸ one  sq              │                          ____   ___  _     _   _
                        │                         / ___| / _ \| |   | \ | |
                        │                         \___ \| | | | |   |  \| |
                        │                          ___) | |_| | |___| |\  |
                        │                         |____/ \__\_\_____|_| \_|
                        │                                   0.1.2
                        │                            No console is open
                        │
                        │                 Ctrl+W h a  Add a connection
                        │                 Ctrl+W h n  Open a console
                        │                 Ctrl+W h s  Scripts without a connection
                        │                   Ctrl+W ,  Open the settings
                        │                     Ctrl+P  Show all commands
                        │                         F1  Show all keys
                        │                   Ctrl+W q  Quit SQLN
```

Without a connection, the screen shows **No connection yet**, without the **Open a console** row.
With the explorer hidden, the first row is `Ctrl+W b` **Show the explorer**, the status line shows `HOME`,
and `Ctrl+W b` gives the focus to the explorer again. An area too short for the logo shows `SQLN 0.1.2` instead.

### Welcome screen

The screen appears over an editor without text. It keeps no state and captures no keys.
The first typed character removes the screen.

```text
           SQLN 0.1.0
           Write SQL here

                   F5  Run the statement
             Shift+F5  Run the script
           Ctrl+Space  Show suggestions
               Ctrl+P  Show all commands
                   F1  Show all keys
```

Without a connection, the screen shows **No connection yet** and the path `Ctrl+W h a`.

### Small terminal

The application keeps the state and draws again when the size is enough.

```text
┌─ SQLN ───────────────────────────────────────────────────┐
│ Small terminal: 52 x 14                                  │
│ Minimum size: 80 x 24                                    │
│ Executions and transactions stay active.                 │
└──────────────────────────────────────────────────────────┘
```

### Batch external changes

Each row needs one choice. The saving of the listed files stays paused.

```text
┌─ Scripts changed outside SQLN ───────────────────────────┐
│ investigate-orders         [Reload] [Overwrite]          │
│ monthly-report             [Reload] [Overwrite]          │
│ console-1 (removed)        [Recreate] [Close]            │
│                                                          │
│ [Apply]  [Decide later]                                  │
└──────────────────────────────────────────────────────────┘
```

### Search and replace in the editor

`Ctrl+F` opens the bar. `Tab` moves to the replace field.
The bar keys are in [KEYMAP.md](KEYMAP.md).

```text
┌─ console-1 ──────────────────────────────────────────────┐
│ Find:       [users            ]  3 of 12                 │
│ Replace:    [people           ]  [Replace] [All]         │
├──────────────────────────────────────────────────────────┤
│ 2 SELECT id FROM users;                                  │
│ 3 SELECT name FROM users;                                │
└──────────────────────────────────────────────────────────┘
```
