# Decisions — Editor

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Horizontal scrollbar in the console editor

The user requests a horizontal scrollbar with the same appearance rules as the vertical scrollbar.

- A line wider than the text area shows a horizontal bar below the editor text.
- Both bars use the existing theme styles. The horizontal bar shares the editor renderer with the cell editor.
- The bar uses Unicode display width and the editor tab width. Text width is cached for each revision.
- The existing horizontal wheel moves the view one character per event and keeps the cursor inside it.
- Keyboard movement, selection, copy, editing, and execution keep their existing behavior.
- An editor with only one row keeps its text row. Inline reviews keep their existing controls and layout.

The editor already handles horizontal wheel events before this change.
This entry replaces the editor exclusion in **Horizontal wheel on the grid — 2026-11-03**.

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
