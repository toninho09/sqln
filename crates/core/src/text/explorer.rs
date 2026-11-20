//! The explorer: folders, moves, and definitions.

pub const NO_CONNECTIONS_ROW: &str = "No connections";
pub const NO_TREE_MATCH: &str = "No match";
pub const LOADED_CATALOGS_ONLY: &str = "Loaded catalogs only";
pub const ADD_CONNECTION_ROW: &str = "Press a to add one";
pub const NOTE_LOADING: &str = "(loading)";
pub const NOTE_FAILED: &str = "(failed)";
pub const MARK_VIEW: &str = "(view)";
pub const NOT_FOR_OBJECT: &str = "This command does not act on this object.";
pub const DEFINITION_MISSING: &str =
    "The object is not in the database. Read the catalog again with r.";
pub const DEFINITION_PRIVILEGE: &str =
    "The server did not give the text. Check the privileges of the user.";
pub const DEFINITION_STOPPED: &str = "The read of the DDL stopped.";
pub const DEFINITION_WRITTEN: &str =
    "-- SQLN wrote this text from the catalog. The text of the server can differ.";
pub const NO_SCHEMA_CHOICE: &str = "This database has no schema or database choice.";
pub const SCHEMA_BUSY: &str = "End the execution and the transaction before the change.";
pub const NO_DEFINITION: &str = "This connection has no definition to edit.";
pub const NO_SELECTED_CONNECTION: &str = "Select a connection.";
pub const MOVE_FOLDER_EDGE: &str =
    "This connection is at the edge of its folder. Press m or drag it to leave the folder.";
pub const MOVE_SCOPE_EDGE: &str =
    "This item is at the edge of its scope. The global and the project connections stay apart.";
pub const MOVE_OTHER_SCOPE: &str =
    "An item cannot move to the other scope. Each scope keeps its connections in its own file.";
pub const MOVE_NOT_SAVED: &str = "Only a saved connection can move.";
pub const MARK_DROP: &str = ">";
pub const DROP_OUTSIDE: &str = "Release on a row of the explorer to move the item.";

pub fn drop_into(item: &str, folder: &str) -> String {
    format!("Release to move {item} into the folder {folder}.")
}

/// The row below the tree that takes an item to the end of its scope.
pub fn drop_end_row(global: bool) -> &'static str {
    if global {
        "End of global"
    } else {
        "End of project"
    }
}

pub fn drop_end(item: &str, global: bool) -> String {
    let scope = if global { "global" } else { "project" };
    format!("Release to move {item} to the end of the {scope} connections.")
}

pub fn drop_out(item: &str, folder: &str) -> String {
    format!("Release to move {item} out of the folder {folder}.")
}

pub fn drop_at(item: &str, target: &str) -> String {
    format!("Release to move {item} to the place of {target}.")
}
pub const MOVE_FILTERED: &str =
    "The filtered tree cannot change the order. Clear the filter first.";

pub fn arrange_failed(error: &str) -> String {
    format!("The order was not saved: {error}. Fix the file and try again.")
}
pub const NOT_FOR_FOLDER: &str =
    "This command acts on a connection. Open the folder and select a connection.";
pub const TITLE_NEW_FOLDER: &str = "New folder";
pub const TITLE_RENAME_FOLDER: &str = "Rename folder";
pub const TITLE_REMOVE_FOLDER: &str = "Remove folder";
pub const OPTION_REMOVE_FOLDER: &str = "Remove folder";
pub const FOLDER_NEW: &str = "+ New folder";
pub const FOLDER_NONE: &str = "No folder";

pub fn title_move_to_folder(name: &str) -> String {
    format!("Move {name} to")
}

pub fn remove_folder(name: &str) -> String {
    format!(
        "Remove the folder {name}? Its connections stay at the same place, without a folder. No connection is removed."
    )
}

pub const MARK_PRODUCTION: &str = "PROD";
