# Decisions — Interface

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Settings in the palette — 2026-11-27

The user asked to type a setting in `Ctrl+P`, such as `pool limit`, and open that field in its section.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Scope | The fields of the current connection form and the fields of the Settings dialog | Only the connection | User choice |
| Row | **Connection: <field> = <value> — <connection>** and **Setting: <field> = <value> — <section>**, after the commands and before the help pages. A secret field and the provider key show no value | No value | User choice |
| Current connection | The explorer row while the explorer has the focus, else the connection of the open tab. A connection without a saved definition has no rows | The connection of the console only | The same rule as the sessions and the locks |
| Connected connection | The restricted form opens. When it does not show the field, the focus goes to **Disconnect**, with the note "Disconnect to change <field>." | A question that disconnects; only the visible fields | User choice |
| Model | A palette row holds a field. No new `Action` and no new shortcut | One `Action` per field | The rule of the help pages. A new field needs no new command |
| Settings rows | The palette reads the files as the Settings dialog does. A configuration that the dialog cannot open gives no setting rows | Show rows that fail | The dialog gives the reason when the user opens it |

## Help pages in the palette — 2026-11-27

The user asked to find help from `Ctrl+P`. For example, the words of a Kafka statement find the help page that explains it.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Pages | The palette lists each page of the help as **Help: <page>**: the guide topics, **All keys**, and each key group, after the commands | Only the guide topics | User choice |
| Search | The filter of a help row reads the full text of the page: title, text, steps, and examples, or the keys of a group | Only the name and the title | User choice. `emit changes` finds **Help: Kafka** |
| Model | A palette row holds a command or a help page. The guide stays data, with no new `Action` and no new shortcut | One `Action` per topic | User choice. A new topic needs no new command |
| Open | A help row opens `F1` on its page. The rows show only when the help is available | A separate window | The help keeps its keys, filter, and copy |

## Connection order and folders — 2026-11-16

The user asked to change the order of the connections in the explorer and to group them in folders, in the most friendly way. The user chose the points below.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Folder model | Each connection has an optional `folder` name. A folder exists while it holds a connection. One level only | A `folders` list with empty folders; nested folders | User choice. Less code and fewer rules |
| Scopes | The global connections come first, then the project connections. Each folder belongs to one scope. A name in both scopes shows `(global)` or `(local)`, as connection names do. A move never crosses the scope, and the refusal says why | Folders that join both scopes; free order across scopes | A global item and a project item cannot change places, because each file keeps its own list |
| Storage | The order is the order of `connections` in `config.json`, and `folder` is a field of the entry. `version` stays 1. An earlier SQLN refuses a file with `folder` | `workspace.json` | User choice. The project file shares the order with the team. Same rule as `consumerGroup` |
| Keys | `navigatorMoveUp` and `navigatorMoveDown`: `Alt+Up` and `Alt+Down` in both maps, and also `K` and `J` in the Vim mode. `navigatorMoveToFolder`: `m` | `K` and `J` in both maps | User choice |
| Neighbors | A move swaps the item with the item above or below it at the same level: in its folder, or at the top level of its scope. A folder moves as one block. At an edge, the move refuses and names `m` and the mouse | A move that enters or leaves a folder | A predictable step. `m` and the mouse change the folder |
| Folder list | `m` lists **+ New folder**, **No folder** (for a connection in a folder), and the other folders of the scope. A connection that enters a folder goes to its end. **No folder** puts it after its folder | Free text only | One list for all cases |
| Mouse | A drag starts on a connection or folder row and begins when the pointer leaves that row, so a click and a double click stay as before. The target row shows `>`, and the status line says what the release does or why it refuses. A folder row takes a connection at its end. The row of its own folder takes a connection out of the folder, at the place of the folder, so a connection can leave a folder when the scope has no loose connection. The empty area below the last row takes the item to the end of its scope, at the top level: a connection leaves its folder, and a folder moves with its connections. While the pointer is there, the first empty row shows `> End of global` or `> End of project`. When the tree fills the panel, no empty area stays, and the row of the folder takes a connection out. Another row gives the item its place: above it when the item moves up, below it when the item moves down. A connection takes the folder of a connection row, and a folder takes the place of the top-level item of the row | Release always above the row | The last place stays reachable |
| Folder row | `e` renames the folder. A name that another folder of the scope uses joins the two folders. `Delete` removes the folder after a question, and its connections stay at its place without a folder. Other connection commands refuse with a reason | Rename refuses a used name | User choice for `e` and `Delete`. A join is the result that the name asks for |
| Open state | A folder starts open. The closed folders go to `closedFolders` of `workspace.json`, with the layout | Session only | User choice |
| Form | **Folder** in the General section, after **Name**, also for a connected connection. An empty field gives no folder | No form field | A connection can get a folder when it is created |
| Filter | The filtered tree shows no folder row and refuses a move with a reason | Folder rows in the filtered tree | The filter lists only the matches |

## Tab close and modal order — 2026-11-14

The user asked for simpler code to add tabs, windows, and grid commands. The change found two rules that the code applied in different ways, and the user chose one rule for each.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Close of the tab on screen | A table, diagram, or compare tab that closes on screen gives the screen to the tab at the same place of the tab row, whatever its kind. With an empty row, the explorer takes the focus | A closed table gives the screen to another table first | User choice. One rule for all tabs that are not consoles |
| Order of the windows | One order decides which window gets the keys, the mouse, the paste, and the hint, and which window is drawn on top: the question dialog, the password, the script conflicts, the palette, the history, the name, the file browser, the settings, the export, the cell edit, the import, the compare, the picker, the help, the text window, and the connection form | Different orders for keys, mouse, paste, and drawing | User choice. The window on top always gets the input. The conflicts question, which opens at any time, is on top of a form |

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
| Width | At 80 columns, the hint row of a connection keeps each command | Leave out `F1 help` | The row fits |

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
| Preferences conflict | The save of the preferences carries the hash of `config.json` when the dialog opened. A file that another program changed refuses the save, and the note asks to close and open the dialog again | Write the snapshot of the dialog over the file | User choice, 2026-11-27. The snapshot removed the change of another instance or of a hand edit without a message |
| Damage | A file that cannot load gives a note at the start, in Settings, and at each reload; the other files load | Block the configuration | A theme is not critical data, and one damaged file cannot stop the connections |
| Missing theme | The choice falls back to `auto`, with a note in the status bar | Keep the last colors or quit | The data is not critical, and the note names the theme and the fallback |
| External change | The poll hashes a manifest of the folder names and contents, and the existing reload question covers the themes | Reload without a question | The rule of `config.json` applies, and `ReloadConfig` already reloads both |
| Per-connection theme | The connection form cycles the built-in themes and the customs of both scopes | The connection picks its own scope only | The resolution is the merged catalog, and a project can share a theme that a global connection uses |

## Getting-started guide in the help — 2026-11-13

The user asked for a help screen for users who are lost. `F1` already showed only the key reference.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Kind | A getting-started guide with topics, short texts, and steps | Context help per panel; an interactive tour | User choice |
| Place | Guide pages inside `F1`, with no new shortcut | A new command; a link on the home screen only | User choice |
| Topics | Connect and open a console; run SQL and results; explorer and tables; Kafka SELECT; scripts, palette, and settings | More topics, such as AI | User choice. The Kafka topic was added later on the same day |
| First page | `F1` always opens on the first topic | Open on **All keys**; guide only without a connection | User choice |
| Sidebar | The window has a sidebar with one row per page: each topic, **All keys**, then each key group. It replaces the `Group:` row | Only the topics in the sidebar | User choice. One list for all pages. `Tab`, `Shift+Tab`, and a click open a page |
| Search | The filter acts on the open page. On a topic, it keeps the matching steps and examples, and the title and text stay | The filter hides the sidebar pages without a match | User choice. A key page never shows the guide text |
| Kafka topic | Always shown. It covers basic SELECT (`LIMIT`, `SCAN`, `PARTITION`, `START AT`), `WHERE` filters, `EMIT CHANGES`, and `TOPICS` and `PARTITIONS` | Only with a saved Kafka connection | User choice |
| Divider and title | The sidebar divider joins the border only where the border line shows | Cut the title | The help title is longer than the sidebar, and the join hid one letter |
| Keys of the steps | The effective key of the mode and the custom shortcuts. An explorer step starts with the focus key. A command without a key shows the palette path | Fixed key texts | The guide must stay correct with Vim mode and custom shortcuts |
| Labels | The title is `Help · <mode> · <panel>`. The hint row shows `F1 help`, and the welcome screen shows **Show help** | Keep `Keys` and `F1 keys` | `F1` no longer opens on the keys. The width does not change |
