//! Table edits and auto-refresh.

use super::count;

pub const EDIT_TITLE: &str = "Edit cell";
pub const EDIT_REVIEW: &str = "Review table changes";
pub const EDIT_REVIEW_AGAIN: &str =
    "Pending changes stay in their table. Select that table and open Review again.";
pub const EDIT_APPLY: &str = "Apply changes";
pub const EDIT_KEEP: &str = "Keep value";
pub const EDIT_SET_NULL: &str = "Set NULL";
pub const EDIT_DISCARD: &str = "Discard changes";
pub const EDIT_BEFORE: &str = "Before";
pub const EDIT_AFTER: &str = "After";
pub const EDIT_UNCERTAIN_LABEL: &str = "Commit result unknown";
pub const EDIT_NO_CELL: &str = "No cell is selected. Open a table and select a value.";
pub const EDIT_UNAVAILABLE: &str = "This table does not support editing, or the connection is read-only. Use SQL on a writable table.";
pub const EDIT_MANUAL_TRANSACTION: &str = "Resolve the open transaction before editing table data.";
pub const EDIT_NO_KEY: &str =
    "Editing needs a primary key with non-NULL values. Use SQL for this table.";
pub const EDIT_PROTECTED: &str =
    "This column cannot be edited. Select a non-key, non-generated column.";
pub const EDIT_BINARY: &str = "Binary values do not support editing. Use SQL for this column.";
pub const EDIT_NOT_NULL: &str = "This column does not accept NULL. Enter a value.";
pub const EDIT_SCHEMA_CHANGED: &str =
    "The table definition changed. Discard changes and reload the table.";
pub const EDIT_CONFLICT: &str =
    "A row changed or no longer exists. Discard changes and reload the table.";
pub const EDIT_ENGINE: &str =
    "Grid editing requires an InnoDB table on MySQL. Use SQL for this table.";
pub const EDIT_BUDGET: &str =
    "The changes exceed the data budget. Free another result or discard changes.";
pub const EDIT_NO_CHANGES: &str = "No changes can be applied. Edit a value first.";
pub const EDIT_STOPPED: &str =
    "The operation stops before a confirmed commit. Check the connection and retry.";
pub const EDIT_COMMIT_UNKNOWN: &str = "The commit result is unknown. Verify the database before any new write. Do not repeat this operation.";
pub const EDIT_KEEP_PENDING: &str = "Pending values stay in the grid.";
pub const EDIT_RESOLVE: &str = "Apply or discard pending changes before this operation.";
pub const EDIT_SAVED: &str = "Changes saved. The table reloads the current database values.";
pub const EDIT_CLOSE_QUESTION: &str = "Apply or discard pending changes before closing this table?";
pub const EDIT_DISCARD_QUESTION: &str =
    "Discard all pending changes? This does not undo a database commit.";
pub const EDIT_CELL_HINT: &str = "F2/Ctrl+S keep locally · Tab buttons · Esc cancel";
pub fn selected_characters(number: usize) -> String {
    let unit = if number == 1 {
        "character"
    } else {
        "characters"
    };
    format!("{} {unit} selected", count(number))
}

pub const EDIT_REVIEW_HINT: &str = "F2/Ctrl+S apply · Up/Down scroll · Tab buttons · Esc cancel";
pub const AUTO_REFRESH_TITLE: &str = "Auto-refresh";
pub const AUTO_REFRESH_NEEDS_RUN: &str =
    "Run a statement first: auto-refresh repeats the last run of this console.";
pub const AUTO_REFRESH_READ_ONLY: &str =
    "Auto-refresh repeats reads only: the last run can change data.";

/// The name of one choice of the auto-refresh dialog.
pub fn auto_refresh_choice(seconds: Option<u64>) -> &'static str {
    match seconds {
        None => "Off",
        Some(1) => "1s",
        Some(2) => "2s",
        Some(5) => "5s",
        Some(10) => "10s",
        Some(30) => "30s",
        _ => "60s",
    }
}

/// The body of the auto-refresh dialog.
pub fn auto_refresh_body(current: Option<u64>) -> String {
    let state = match current {
        Some(seconds) => format!(
            "The current interval is {}.",
            auto_refresh_choice(Some(seconds))
        ),
        None => "Auto-refresh is off.".to_owned(),
    };
    format!(
        "{state} A console repeats its last run and a table reads its page again. Only reads repeat."
    )
}

/// The label of the state on screen, such as `auto 5s`.
pub fn auto_refresh_label(seconds: u64) -> String {
    format!("auto {seconds}s")
}

/// The status note when a failed execution turns the refresh off.
pub fn auto_refresh_stopped(message: Option<&str>) -> String {
    match message {
        Some(reason) => format!("Auto-refresh stopped: {reason}"),
        None => "Auto-refresh stopped.".to_owned(),
    }
}
pub const EDIT_ROW_DELETED: &str =
    "This row is marked for delete. Toggle the mark to edit its cells.";
pub const EDIT_ROW_ADDED: &str =
    "Row added below. Edit the cells, then review and apply the changes.";
pub const EDIT_ROW_MARKED: &str =
    "Row marked for delete. Review and apply, or discard the changes.";
pub const EDIT_ROW_UNMARKED: &str = "Delete mark removed from the row.";
pub const EDIT_INSERT_REMOVED: &str = "Pending insert row removed. No statement runs for it.";
pub const EDIT_UPDATE_ROW: &str = "Update";
pub const EDIT_DELETE_ROW: &str = "Delete";
pub const EDIT_INSERT_ROW: &str = "Insert";

pub fn edit_pending(updates: usize, deletes: usize, inserts: usize) -> String {
    let mut parts = Vec::new();
    let mut push = |count: usize, one: &'static str, many: &'static str| {
        if count > 0 {
            parts.push(format!("{count} {}", if count == 1 { one } else { many }));
        }
    };
    push(updates, "update", "updates");
    push(deletes, "delete", "deletes");
    push(inserts, "insert", "inserts");
    if parts.is_empty() {
        parts.push("no changes".to_owned());
    }
    format!("Pending: {}", parts.join(", "))
}

pub fn edit_insert_needs(column: &str) -> String {
    format!("Column {column} needs a value, because it rejects NULL and has no default.")
}

pub fn edit_production(table: &str, connection: &str) -> String {
    format!("Apply the reviewed changes to {table} on the production connection {connection}?")
}

pub fn edit_connection_title(name: &str) -> String {
    format!("Edit connection: {name}")
}
