//! All interface text. This file keeps the text in one place for a later translation.

use crate::catalog::Group;
use crate::export::ExportFormat;
use crate::generate::Template;
use crate::import::ImportFormat;
use crate::import::form::Section;
use crate::input::{Action, SuggestionKeys};
use crate::structure::SessionSignal;

/// One command of the hint bar: a key and its effect. A note has an empty key.
pub type HintPart = (&'static str, &'static str);

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
pub const EDIT_REVIEW_HINT: &str = "F2/Ctrl+S apply · Up/Down scroll · Tab buttons · Esc cancel";

pub fn edit_pending(rows: usize, cells: usize) -> String {
    format!("Pending: {rows} rows, {cells} cells")
}

pub fn edit_production(table: &str, connection: &str) -> String {
    format!("Apply the reviewed changes to {table} on the production connection {connection}?")
}

pub const CLIPBOARD_UNAVAILABLE: &str = "Clipboard unavailable. Paste with the terminal.";
pub const COPY_SENT_TO_TERMINAL: &str = "Copy request sent to the terminal. No confirmation.";
pub const COPY_FAILED: &str = "Copy failed.";

pub const FIND_LABEL: &str = "Find:    ";
pub const REPLACE_LABEL: &str = "Replace: ";
pub const NO_MATCHES: &str = "no matches";

pub const HINT_FILTER: &[HintPart] = &[
    ("Enter", "apply"),
    ("Tab", "accept or next field"),
    ("Esc", "grid"),
    ("Ctrl+Space", "suggestions"),
];
pub const HINT_TREE_FILTER: &[HintPart] = &[
    ("Enter", "open table"),
    ("Up/Down", "move"),
    ("Tab", "tree"),
    ("Esc", "cancel"),
];
pub const HINT_GRID_SEARCH: &[HintPart] =
    &[("Enter", "next"), ("Up/Down", "move"), ("Esc", "close")];
/// The effect of `Enter` and of `Tab` when the key does not accept a suggestion.
pub const LIST_EDITOR_KEYS: (HintPart, HintPart) = (("Enter", "new line"), ("Tab", "indent"));
pub const LIST_FILTER_KEYS: (HintPart, HintPart) = (("Enter", "apply"), ("Tab", "next field"));

/// Lists the commands of an open suggestion list.
pub fn hint_list(keys: SuggestionKeys, (enter, tab): (HintPart, HintPart)) -> Vec<HintPart> {
    let (accept, other) = match keys {
        SuggestionKeys::Tab => ("Tab", Some(enter)),
        SuggestionKeys::Enter => ("Enter", Some(tab)),
        SuggestionKeys::Both => ("Tab/Enter", None),
    };
    let mut parts = vec![(accept, "accept"), ("Up/Down", "move")];
    parts.extend(other);
    parts.push(("Esc", "close list"));
    parts
}

pub fn suggestion_keys_name(keys: SuggestionKeys) -> &'static str {
    match keys {
        SuggestionKeys::Tab => "Tab",
        SuggestionKeys::Enter => "Enter",
        SuggestionKeys::Both => "Tab or Enter",
    }
}

pub fn theme_name(name: crate::store::config::ThemeName) -> &'static str {
    use crate::store::config::ThemeName;
    match name {
        ThemeName::Auto => "Auto",
        ThemeName::Astrodark => "Astrodark",
        ThemeName::Ansi => "Terminal colors",
        ThemeName::TokyoNight => "Tokyo Night",
        ThemeName::CatppuccinMocha => "Catppuccin Mocha",
        ThemeName::CatppuccinLatte => "Catppuccin Latte",
        ThemeName::Gruvbox => "Gruvbox",
        ThemeName::Nord => "Nord",
    }
}

pub fn connection_icon_name(icon: crate::store::config::ConnectionIcon) -> &'static str {
    use crate::store::config::ConnectionIcon;
    match icon {
        ConnectionIcon::Warning => "Warning",
        ConnectionIcon::Skull => "Skull",
        ConnectionIcon::Radiation => "Radiation",
        ConnectionIcon::Flag => "Flag",
        ConnectionIcon::Crown => "Crown",
        ConnectionIcon::Flask => "Flask",
        ConnectionIcon::Star => "Star",
        ConnectionIcon::Diamond => "Diamond",
        ConnectionIcon::Tools => "Tools",
        ConnectionIcon::Gear => "Gear",
        ConnectionIcon::Home => "Home",
        ConnectionIcon::Heart => "Heart",
        ConnectionIcon::Snowflake => "Snowflake",
        ConnectionIcon::Check => "Check",
    }
}

pub const HINT_SEARCH: &[HintPart] = &[
    ("Tab", "next field"),
    ("Enter", "next or replace"),
    ("Ctrl+A", "replace all"),
    ("Esc", "close"),
];
pub const HINT_PICKER: &[HintPart] = &[
    ("Up/Down", "move"),
    ("Enter", "choose"),
    ("r", "rename"),
    ("Del", "remove a script"),
    ("Esc", "cancel"),
];
pub const HINT_CHOOSE: &[HintPart] = &[("Up/Down", "move"), ("Enter", "choose"), ("Esc", "cancel")];
pub const HINT_NAME: &[HintPart] = &[("Enter", "saves"), ("Esc", "cancels")];
pub const HINT_SETTINGS_TEXT: &[HintPart] = &[
    ("Tab", "next"),
    ("Enter", "next"),
    ("Esc", "cancel"),
    ("", "An empty field removes the value"),
];
pub const HINT_SETTINGS_SECTIONS: &[HintPart] =
    &[("Up/Down", "section"), ("Tab", "next"), ("Esc", "cancel")];
pub const HINT_SUGGESTIONS: &[HintPart] = &[("Ctrl+Space", "suggestions")];
pub const HINT_FIND: &[HintPart] = &[("Ctrl+F", "find")];
pub const HINT_EDITOR_STAYS: &[HintPart] = &[("", "The editor stays available")];
pub const HINT_CONFLICTS: &[HintPart] = &[
    ("Up/Down", "move"),
    ("Left/Right", "choose"),
    ("Enter", "next"),
    ("Esc", "decide later"),
];
/// The application adds the key that reads the file again.
pub const HINT_CONFIG_ERROR: &[HintPart] = &[("", "Configuration error. Fix the file, then")];
pub const HINT_FORM: &[HintPart] = &[
    ("Tab", "next"),
    ("Left/Right", "change"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_FORM_TEXT: &[HintPart] = &[("Tab", "next"), ("Enter", "next"), ("Esc", "cancel")];
pub const HINT_FORM_LIST: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "choose"),
    ("Esc", "close list"),
];
pub const HINT_FORM_BUTTON: &[HintPart] = &[("Tab", "next"), ("Enter", "press"), ("Esc", "cancel")];
pub const HINT_FORM_SCHEMAS: &[HintPart] = &[
    ("Tab", "next"),
    ("Ctrl+O", "list"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_MARK_LIST: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Space", "mark"),
    ("Enter", "done"),
    ("Esc", "cancel"),
];
pub const HINT_FORM_PATH: &[HintPart] = &[
    ("Tab", "next"),
    ("Ctrl+O", "browse"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_BROWSER: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "open"),
    ("Left", "up"),
    ("~", "home"),
    (".", "hidden"),
    ("Esc", "cancel"),
];
pub const HINT_EXPORT_FORMATS: &[HintPart] = &[
    ("Up/Down", "format"),
    ("Tab", "next"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_IMPORT_SECTIONS: &[HintPart] = &[
    ("Up/Down", "section"),
    ("Tab", "next"),
    ("Enter", "next"),
    ("Esc", "cancel"),
];
pub const HINT_IMPORT_COLUMNS: &[HintPart] = &[
    ("Up/Down", "column"),
    ("Left/Right", "file column"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_IMPORT_PREVIEW: &[HintPart] = &[
    ("Up/Down", "rows"),
    ("Left/Right", "columns"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_DIALOG: &[HintPart] = &[
    ("Tab", "next option"),
    ("Enter", "confirm"),
    ("Esc", "cancel"),
];
pub const HINT_HELP: &[HintPart] = &[
    ("Type", "search"),
    ("Tab", "group"),
    ("Up/Down", "scroll"),
    ("PgUp/PgDn", "page"),
    ("Esc", "close"),
];
pub const HELP_SEARCH: &str = "Type a command or shortcut to filter";
pub const HELP_ALL: &str = "All";
pub const HELP_GROUP: &str = "Group";
pub const HELP_GROUP_KEYS: &str = "Tab / Shift+Tab";

pub const HINT_OVERLAY: &[HintPart] =
    &[("Up/Down", "scroll"), ("Ctrl+C", "copy"), ("Esc", "close")];
pub const HINT_OVERLAY_ROW: &[HintPart] = &[
    ("Left/Right", "row"),
    ("Up/Down", "scroll"),
    ("Ctrl+C", "copy"),
    ("Esc", "close"),
];
pub const HINT_OVERLAY_JSON: &[HintPart] = &[
    ("f", "switch form"),
    ("Up/Down", "scroll"),
    ("Ctrl+C", "copy"),
    ("Esc", "close"),
];

/// Writes a key of the key map for the hint bar: `ctrl+w e` becomes `Ctrl+W e`.
pub fn key_label(key: &str) -> String {
    let token = |token: &str| {
        let modified = token.len() > 1 && token.contains('+');
        let parts: Vec<String> = token
            .split('+')
            .map(|part| match part {
                "delete" => "Del".to_owned(),
                // A letter holds its case alone, because `n` and `N` are two keys.
                _ if part.chars().count() == 1 && modified => part.to_uppercase(),
                _ if part.chars().count() == 1 => part.to_owned(),
                _ => {
                    let mut chars = part.chars();
                    chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(chars).collect::<String>()
                    })
                }
            })
            .collect();
        parts.join("+")
    };
    key.split_whitespace()
        .map(token)
        .collect::<Vec<_>>()
        .join(" ")
}

pub const HINT_PREFIX: &[HintPart] = &[("Ctrl+W", "…"), ("Tab", "explorer"), ("Esc", "cancel")];
pub const HINT_SEQUENCE: &[HintPart] = &[("", "Press the next key"), ("Esc", "cancel")];
pub const HINT_RECORD_KEYS: &[HintPart] = &[
    ("Enter", "confirm"),
    ("OK", "confirm"),
    ("Esc", "cancel recording"),
];
pub const HINT_RECORD_MOUSE: &[HintPart] = &[("OK", "confirm"), ("Esc", "cancel recording")];
pub const HINT_EXPLORER_MENU: &[HintPart] = &[("Ctrl+W Tab", "…"), ("Esc", "cancel")];
pub const HINT_COMMANDS: &[HintPart] = &[("Ctrl+W", "commands")];
pub const HINT_PREFIX_MENU: &[HintPart] = &[("Ctrl+W", "menu")];
pub const HINT_PALETTE: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "run"),
    ("Esc", "cancel"),
];

pub const HINT_HISTORY: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "insert"),
    ("Esc", "cancel"),
];

pub const TITLE_EXPLORER: &str = "EXPLORER";
pub const FORM_GENERAL: &str = "General";
pub const FORM_ADDRESS: &str = "Address";
pub const FORM_CREDENTIALS: &str = "Credentials";
pub const FORM_SECURITY: &str = "Security";
pub const FORM_OPTIONS: &str = "Options";
pub const TITLE_PALETTE: &str = "Commands";
pub const TITLE_PREFIX: &str = "Ctrl+W";
pub const TITLE_EXPLORER_MENU: &str = "Ctrl+W · Explorer";
pub const NO_COMMANDS: &str = "No command matches";

pub const STATUS_EDITOR: &str = "EDITOR";
pub const STATUS_NORMAL: &str = "NORMAL";
pub const STATUS_INSERT: &str = "INSERT";
pub const STATUS_VISUAL: &str = "VISUAL";
pub const STATUS_RESULTS: &str = "RESULTS";
pub const STATUS_EXPLORER: &str = "EXPLORER";
pub const STATUS_FILTER: &str = "FILTER";
pub const STATUS_TABLE: &str = "TABLE";
pub const STATUS_DIAGRAM: &str = "DIAGRAM";
pub const STATUS_HOME: &str = "HOME";

pub const GROUP_PANELS: &str = "Panels";
pub const GROUP_TABS: &str = "Tabs";
pub const GROUP_RUN: &str = "Run";
pub const GROUP_TRANSACTION: &str = "Transaction";
pub const GROUP_CONNECTION: &str = "Connection";
pub const GROUP_APPLICATION: &str = "Application";
pub const GROUP_CONSOLE: &str = "Console";
pub const GROUP_ROW: &str = "Row";
pub const MENU_FOCUS: &str = "Move the focus";
pub const MENU_NAVIGATOR: &str = "Toggle the explorer";
pub const MENU_NEXT_TAB: &str = "Next tab";
pub const MENU_PREVIOUS_TAB: &str = "Previous tab";
pub const MENU_RENAME: &str = "Rename the tab";
pub const MENU_CLOSE_TAB: &str = "Close the tab";
pub const MENU_RUN: &str = "Run the statement";
pub const MENU_SCRIPT: &str = "Run the script";
pub const MENU_EXPLORER: &str = "Explorer commands";
pub const MENU_CONTEXT: &str = "Panel commands";
pub const MENU_OPEN: &str = "Open";
pub const MENU_EXPAND: &str = "Expand";
pub const MENU_COLLAPSE: &str = "Collapse/parent";
pub const MENU_CLEAR_FILTER: &str = "Clear filter";
pub const MENU_LOAD_GRID: &str = "Load grid";
pub const COMMAND_LOAD_GRID: &str = "Load the grid";
pub const COMMAND_OPEN_CONSOLE: &str = "Open the console";
pub const COMMAND_OPEN_DATA: &str = "Open the data";
pub const COMMAND_OPEN_DDL: &str = "Open the DDL in a new console";
pub const COMMAND_TOGGLE_ROW: &str = "Open or close the row";
pub const GROUP_SELECTION_VIEW: &str = "Selection and view";
pub const GROUP_TABLE_RESULTS: &str = "Table and results";
pub const MENU_SESSIONS: &str = "Sessions";
pub const MENU_NEW_CONSOLE: &str = "New console";
pub const MENU_SCRIPTS: &str = "Scripts";
pub const MENU_REFRESH: &str = "Read the catalog";
pub const MENU_RENAME_CONSOLE: &str = "Rename the console";
pub const MENU_REMOVE_CONSOLE: &str = "Remove the console";
pub const MENU_EDIT_CONNECTION: &str = "Edit connection";
pub const MENU_ADD_CONNECTION: &str = "Add a connection";
pub const MENU_REMOVE_CONNECTION: &str = "Remove connection";
pub const MENU_DISCONNECT: &str = "Disconnect";
pub const MENU_DELETE_PASSWORD: &str = "Delete the password";
pub const MENU_STRUCTURE: &str = "Show the structure";
pub const MENU_DIAGRAM: &str = "ER diagram";
pub const MENU_COMPARE: &str = "Compare";
pub const MENU_GENERATE: &str = "Generate SQL";
pub const MENU_IMPORT: &str = "Import a file";
pub const MENU_COPY_NAME: &str = "Copy the name";
pub const MENU_FILTER: &str = "Filter the tables";
pub const MENU_EXPLAIN: &str = "Show the plan";
pub const MENU_EXPLAIN_ANALYZE: &str = "Plan with a run";
pub const MENU_STOP: &str = "Stop";
pub const MENU_MODE: &str = "Auto or manual";
pub const MENU_COMMIT: &str = "Commit";
pub const MENU_ROLLBACK: &str = "Rollback";
pub const MENU_FORMAT: &str = "Format the SQL";
pub const MENU_COMMENT: &str = "Comment the rows";
pub const MENU_HISTORY: &str = "Query history";
pub const MENU_SCHEMA: &str = "Schema or database";
pub const MENU_CHANGE: &str = "Change connection";
pub const MENU_SETTINGS: &str = "Settings";
pub const MENU_RELOAD: &str = "Reload the config";
pub const MENU_COMMANDS: &str = "Commands";
pub const MENU_KEYS: &str = "All keys";
pub const MENU_QUIT: &str = "Quit";

pub const WELCOME_NO_CONNECTION: &str = "No connection yet";
pub const WELCOME_WRITE: &str = "Write SQL here";
pub const WELCOME_ADD: &str = "Add a connection";
pub const WELCOME_RUN: &str = "Run the statement";
pub const WELCOME_SCRIPT: &str = "Run the script";
pub const WELCOME_SUGGESTIONS: &str = "Show suggestions";
pub const WELCOME_SUGGESTIONS_KEY: &str = "Ctrl+Space";
pub const WELCOME_COMMANDS: &str = "Show all commands";
pub const WELCOME_KEYS: &str = "Show all keys";

pub const HOME_NO_CONSOLE: &str = "No console is open";
pub const HOME_EXPLORER: &str = "Show the explorer";
pub const HOME_CONSOLE: &str = "Open a console";
pub const HOME_SCRIPTS: &str = "Scripts without a connection";
pub const HOME_SETTINGS: &str = "Open the settings";
pub const HOME_QUIT: &str = "Quit SQLN";
/// The version under the logo of the home screen.
pub const HOME_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The name of the application in large letters, for the home screen. Only ASCII, so
/// each terminal shows it.
pub const HOME_LOGO: [&str; 5] = [
    r"  ____   ___  _     _   _ ",
    r" / ___| / _ \| |   | \ | |",
    r" \___ \| | | | |   |  \| |",
    r"  ___) | |_| | |___| |\  |",
    r" |____/ \__\_\_____|_| \_|",
];

/// Writes the name and the version for the first screen.
pub fn welcome_title() -> String {
    format!("SQLN {}", env!("CARGO_PKG_VERSION"))
}

pub fn pool_usage(busy: usize, limit: usize) -> String {
    format!("Pool {busy}/{limit}")
}

pub fn data_usage(used_mib: usize, limit_mib: usize) -> String {
    format!("Data {used_mib}/{limit_mib} MiB")
}

pub const SMALL_TERMINAL: &str = "Small terminal";
pub const MINIMUM_SIZE: &str = "Minimum size";

pub const NO_CONNECTION: &str =
    "No database. Use Ctrl+W h and n in the navigator, or a to add a connection.";
pub const NO_CONNECTIONS_ROW: &str = "No connections";
pub const NO_TREE_MATCH: &str = "No match";
pub const LOADED_CATALOGS_ONLY: &str = "Loaded catalogs only";
pub const ADD_CONNECTION_ROW: &str = "Press a to add one";
pub const NOTE_LOADING: &str = "(loading)";
pub const NOTE_FAILED: &str = "(failed)";
pub const MARK_VIEW: &str = "(view)";
/// Returns the name of a group of the explorer.
pub fn group_name(group: Group) -> &'static str {
    match group {
        Group::Routines => "Routines",
        Group::Sequences => "Sequences",
        Group::Triggers => "Triggers",
        Group::Types => "Types",
    }
}
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
pub const DISCONNECTED: &str = "Disconnected.";
pub const SESSION_IN_USE: &str = "A session is still in use. It closes at its end.";
pub const NOT_CONNECTED: &str = "This connection has no open session.";
pub const FORM_TESTING: &str = "Testing the connection.";
pub const SCHEMA_LIST_READING: &str = "Reading the names.";
pub const NO_SCHEMA_MATCH: &str = "No name has this text.";
pub const FORM_TEST_OK: &str = "The connection works.";
pub fn form_file_missing(path: &str) -> String {
    format!("The file does not exist. Save asks before it creates the file: {path}")
}
pub const FORM_NO_CURRENT_FOLDER: &str =
    "SQLN cannot read the current folder. Write the full path of the file.";
pub const FORM_NAME_TAKEN: &str = "Another connection has this name.";
pub const FORM_NEEDS_NAME: &str = "The connection needs a name.";
pub const FORM_NEEDS_HOST: &str = "The connection needs a host.";
pub const FORM_NEEDS_USER: &str = "The connection needs a user.";
pub const FORM_NEEDS_FILE: &str = "SQLite needs a file.";
pub const FORM_REDIS_DATABASE: &str = "The Redis database must be a number.";
pub const FORM_REDIS_SCHEMAS: &str = "Each Redis database must be a number.";
pub const REDIS_SOCKET_TLS: &str = "A Unix socket does not use TLS. Turn off TLS.";
pub const REDIS_NO_SOCKET: &str = "This system has no Unix sockets.";
pub const REDIS_NO_STOP: &str = "The server cannot stop this command.";
pub const REDIS_BAD_QUOTES: &str = "The command has a quote that does not close.";
pub const REDIS_NO_COMMAND: &str = "The line has no command.";
pub const REDIS_NO_KEY: &str = "The key does not exist.";
pub const REDIS_BAD_PAGE: &str = "SQLN.VALUE needs a database, a key, an offset, and a count.";
pub const REDIS_BAD_REPLY: &str = "The server sent a reply that SQLN cannot read.";
pub const REDIS_NO_TRANSACTIONS: &str = "Redis has no manual transactions. Use MULTI in a script.";
pub const REDIS_READ_ONLY: &str =
    "This connection is read only, and the server does not mark this command as a read.";
pub const REDIS_CONTINUOUS: &str =
    "SQLN cannot show a continuous stream, so it refuses this command.";
pub const REDIS_IDENTITY: &str =
    "This command changes the login of a shared session, so SQLN refuses it.";
pub const REDIS_NO_REPLY: &str =
    "This command stops the answers of the server, so SQLN refuses it.";
pub const REDIS_COLUMN_VALUE: &str = "value";
pub const REDIS_COLUMN_FIELD: &str = "field";
pub const REDIS_COLUMN_MEMBER: &str = "member";
pub const REDIS_COLUMN_SCORE: &str = "score";
pub const REDIS_COLUMN_ID: &str = "id";
pub const REDIS_COLUMN_FIELDS: &str = "fields";
pub const FORM_URI_KIND: &str = "The connection text is for another database type.";
pub const FORM_POOL_ZERO: &str = "The pool limit must be 1 or more.";
pub const FORM_CLIENT_PAIR: &str = "A client certificate needs its file and its key.";
pub const FORM_TLS_FILES: &str = "TLS files need a connection with TLS.";
pub const FORM_CONNECTED: &str =
    "This connection is connected. Disconnect it to change access, TLS, or the pool.";
pub const FIELD_KIND: &str = "Type";
pub const FIELD_NAME: &str = "Name";
pub const FIELD_SCOPE: &str = "Scope";
pub const FIELD_ENTRY: &str = "Entry";
pub const FIELD_URI: &str = "URI";
pub const FIELD_HOST: &str = "Host or socket";
pub const FIELD_PORT: &str = "Port";
pub const FIELD_DATABASE: &str = "Database";
pub const FIELD_SCHEMAS: &str = "Schemas";
pub const FIELD_DATABASES: &str = "Databases";
pub const FIELD_USER: &str = "User";
pub const FIELD_PASSWORD: &str = "Password";
pub const FIELD_CREDENTIAL: &str = "Credential";
pub const VALUE_VAULT: &str = "System vault";
pub const VALUE_SESSION: &str = "Session only";
pub const TITLE_TRUST: &str = "Confirm destination";
pub const TITLE_PROJECT_TRUST: &str = "Confirm project path";
pub const TITLE_VAULT: &str = "System vault";
pub const TITLE_PASSWORD: &str = "Password";
pub const OPTION_CONFIRM_DESTINATION: &str = "Confirm";
pub const OPTION_SESSION: &str = "Use for this session";
pub const SAVE_IN_VAULT: &str = "Save in the system vault";
pub const BUTTON_CONNECT: &str = "Connect";
pub const AUTH_WAIT: &str = "Preparing credentials. The interface remains available.";
pub const AUTH_PROJECT_LABEL: &str = "Project";
pub const AUTH_CANCELLED: &str = "Credential request canceled. No SQL starts from this request.";
pub fn delete_password_question(name: &str) -> String {
    format!(
        "Delete the saved password of {name}?\n\nOther copies of the project can use the same password."
    )
}

pub const AUTH_CHANGED: &str = "The connection or form changed. Start the operation again.";
pub const AUTH_SAVE_FAILED: &str =
    "The password is not saved. It stays in memory. Retry explicitly, or use it for this session.";
pub const AUTH_REFERENCE_FAILED: &str = "The vault holds the password, but SQLN cannot save its reference. Retry, or use it for this session.";
pub const AUTH_PLAIN_WARNING: &str = "Without a vault, SQLN can save the password in credentials.json, in the global folder. That file stores it in plain text.";
pub const AUTH_PASSWORD_ACCEPTED: &str =
    "The database accepts the password. Start the operation again.";
pub const AUTH_REFUSED_HINT: &[HintPart] = &[
    ("", "The database refused the last password"),
    ("Enter", "connects"),
    ("Esc", "cancels"),
];
pub const AUTH_SUSPENDED: &str = "The database refused the password again. SQLN stops new attempts on this connection.\nThe next action of yours asks for a password. A password that the database accepts, or a change to the connection, ends this state.";
pub const TITLE_PASSWORD_REFUSED: &str = "Password refused";
pub const TITLE_MIGRATION: &str = "Move the password to the vault";
pub const TITLE_DELETE_PASSWORD: &str = "Delete saved password";
pub const MIGRATION_QUESTION: &str = "This computer has a system vault now, and credentials.json holds this password in plain text.\nMove it to the vault?";
pub const MIGRATION_VERIFIED: &str = "The vault holds the password, and SQLN read it back.\nRemove the plain text copy from credentials.json?";
pub const MIGRATION_INCOMPLETE: &str =
    "The migration is not complete. The plain text password stays in credentials.json.";
pub const OPTION_MIGRATE: &str = "Move to the vault";
pub const OPTION_KEEP_PLAIN: &str = "Keep the plain text copy";
pub const OPTION_REMOVE_PLAIN: &str = "Remove the plain text copy";
pub const OPTION_SAVE_PLAIN: &str = "Save in plain text";
pub const OPTION_DELETE_PASSWORD: &str = "Delete password";
pub const OPTION_KEEP_PASSWORD: &str = "Keep password";
pub const SAVE_IN_PLAIN_TEXT: &str = "Save in plain text in credentials.json";
pub const PASSWORD_DELETED: &str = "The saved password is deleted.";
pub const PASSWORD_NOT_FOUND: &str = "This connection has no saved password.";
pub const AUTH_TRUST_BODY: &str = "Confirm this destination before SQLN sends credentials. A new project path also needs confirmation.";
pub const AUTH_PASSWORD_HINT: &[HintPart] = &[
    ("Enter", "connects"),
    ("Tab", "changes field"),
    ("Esc", "cancels"),
];
pub const FIELD_TLS: &str = "TLS";
pub const FIELD_CA: &str = "CA file";
pub const FIELD_CLIENT_CERTIFICATE: &str = "Client certificate";
pub const FIELD_CLIENT_KEY: &str = "Client key";
pub const FIELD_FILE: &str = "File";
pub const FIELD_FOREIGN_KEYS: &str = "foreign_keys";
pub const FIELD_BUSY: &str = "Lock limit (seconds)";
pub const FIELD_POOL: &str = "Pool limit";
pub const FIELD_READ_ONLY: &str = "Read only";
pub const FIELD_PRODUCTION: &str = "Production";
pub const FIELD_CONNECT: &str = "Connect limit (seconds)";
pub const FIELD_SQL_TIMEOUT: &str = "SQL limit (s, 0 = none)";
pub const FIELD_IDLE_TIMEOUT: &str = "Idle limit (s, 0 = none)";
pub const BUTTON_TEST: &str = "Test connection";
pub const BUTTON_SAVE: &str = "Save";
pub const BUTTON_DISCONNECT: &str = "Disconnect";
pub const BUTTON_CANCEL: &str = "Cancel";
pub const VALUE_POSTGRES: &str = "PostgreSQL";
pub const VALUE_MYSQL: &str = "MySQL";
pub const VALUE_REDIS: &str = "Redis/Valkey";
pub const VALUE_SQLITE: &str = "SQLite";
pub const VALUE_SCOPE_PROJECT: &str = "Project";
pub const VALUE_SCOPE_GLOBAL: &str = "Global";
pub const VALUE_FIELDS: &str = "Fields";
pub const VALUE_URI: &str = "URI";
pub const VALUE_TLS_VERIFY: &str = "Verify the certificate";
pub const VALUE_TLS_OFF: &str = "Off";
pub const VALUE_DEFAULT: &str = "Default";
pub const VALUE_ON: &str = "On";
pub const VALUE_OFF: &str = "Off";
pub fn on_off(value: bool) -> &'static str {
    if value { VALUE_ON } else { VALUE_OFF }
}
pub fn default_value(value: &str) -> String {
    format!("{VALUE_DEFAULT}: {value}")
}
pub const TITLE_NEW_CONNECTION: &str = "New connection";
pub const TITLE_REMOVE_CONNECTION: &str = "Remove connection";
pub const TITLE_RENAME: &str = "Rename console";
pub const TITLE_SETTINGS: &str = "Settings";
pub const TITLE_CHANGE_CONNECTION: &str = "Change connection";
pub const OPTION_CHANGE: &str = "Change connection";
pub const NAME_EMPTY: &str = "The console needs a name.";
pub const NAME_TAKEN: &str = "Another console of this connection has this name.";
pub const CONSOLE_RENAMED: &str = "The console has a new name.";
pub const CONNECTION_CHANGE_BUSY: &str = "End the execution and the transaction before the change.";
pub const CONNECTION_CHANGE_PENDING: &str =
    "The connection change is in progress. Try again after it completes.";
pub const NO_OTHER_CONNECTION: &str = "No other connection is available.";
pub const SETTINGS_SAVED: &str = "The settings are saved.";
pub const TITLE_BROWSER: &str = "Choose a file";
pub const BUTTON_BROWSE: &str = "...";
pub const BROWSER_USE_FOLDER: &str = "Use this folder";
pub const BROWSER_EMPTY: &str = "No matching items";
pub const BROWSER_BAD_PATH: &str = "A text field cannot hold this path.";

pub fn browser_unreadable(reason: &str) -> String {
    format!("Cannot read the folder: {reason}")
}

pub const TITLE_EXPORT: &str = "Export";
pub const TITLE_EXPORT_EXISTS: &str = "File exists";
pub const TITLE_EXPORT_PART: &str = "Incomplete export";
pub const BUTTON_EXPORT: &str = "Export";
pub const OPTION_EXPORT_REPLACE: &str = "Replace";
pub const OPTION_EXPORT_KEEP: &str = "Keep";
pub const OPTION_EXPORT_DELETE: &str = "Delete";
pub const FIELD_EXPORT_ROWS: &str = "Rows";
pub const FIELD_EXPORT_FILE: &str = "File";
pub const FIELD_EXPORT_HEADER: &str = "Header";
pub const FIELD_EXPORT_DELIMITER: &str = "Delimiter";
pub const FIELD_EXPORT_NULL: &str = "NULL text";
pub const FIELD_EXPORT_BOM: &str = "UTF-8 BOM";
pub const FIELD_EXPORT_TABLE: &str = "Table";
pub const FIELD_EXPORT_GROUPING: &str = "INSERT";
pub const FIELD_EXPORT_BATCH: &str = "Rows per INSERT";
pub const VALUE_EXPORT_ALL_ROWS: &str = "All rows";
pub const VALUE_EXPORT_ONE_ROW: &str = "One per row";
pub const VALUE_EXPORT_GROUPED: &str = "Grouped";
pub const VALUE_EXPORT_NULL_EMPTY: &str = "Empty";
pub const VALUE_EXPORT_NULL_WORD: &str = "NULL";
pub const EXPORT_NO_FILE: &str = "Write the path of the file.";
pub const EXPORT_IS_FOLDER: &str = "This path is a folder. Write a file name.";
pub const EXPORT_NO_TABLE: &str = "Write the table of the INSERT statements.";
pub const EXPORT_IN_USE: &str = "Another export writes to this file.";
pub const EXPORT_ALL_READ: &str =
    "All rows needs a plain read. This statement can change data, so it does not run again.";
pub const EXPORT_ALL_SCRIPT: &str =
    "All rows needs one statement. Run this statement alone, then export.";
pub const EXPORT_ALL_SOURCE: &str =
    "All rows is not available for this source. Load more rows first.";
pub const EXPORT_RESULT_CLOSED: &str = "The result closed before the end of the export.";
pub const EXPORT_DELETED: &str = "The incomplete file is deleted.";
pub const EXPORT_STOPPED: &str = "The export stopped before the first row.";
pub const TITLE_IMPORT: &str = "Import";
pub const TITLE_IMPORT_DONE: &str = "Import complete";
pub const TITLE_IMPORT_FAILED: &str = "Import not complete";
pub const TITLE_IMPORT_PRODUCTION: &str = "Import to production";
pub const BUTTON_IMPORT: &str = "Import";
pub const FIELD_IMPORT_FORMAT: &str = "Format";
pub const FIELD_IMPORT_ENCODING: &str = "Encoding";
pub const FIELD_IMPORT_MODE: &str = "On failure";
pub const FIELD_IMPORT_BATCH: &str = "Rows per batch";
pub const FIELD_IMPORT_LOG: &str = "Log file";
pub const FIELD_IMPORT_LIMIT: &str = "Error limit";
pub const VALUE_IMPORT_UTF8: &str = "UTF-8";
pub const VALUE_IMPORT_WINDOWS_1252: &str = "Windows-1252";
pub const VALUE_IMPORT_ALL: &str = "All or nothing";
pub const VALUE_IMPORT_BATCH: &str = "By batch";
pub const VALUE_IMPORT_LOG: &str = "Log errors";
pub const VALUE_IMPORT_IGNORE: &str = "ignore";
pub const VALUE_IMPORT_NULL: &str = "NULL";
pub const IMPORT_HEAD_COLUMN: &str = "Column";
pub const IMPORT_HEAD_TYPE: &str = "Type";
pub const IMPORT_HEAD_SOURCE: &str = "File column";
pub const IMPORT_NO_FILE: &str = "Write the path of the file.";
pub const IMPORT_READING: &str = "The dialog still reads the file.";
pub const IMPORT_READING_LINE: &str = "Reading the file.";
pub const IMPORT_CHOOSE_FILE: &str = "Choose a file. Press Tab to read the path.";
pub const IMPORT_NO_COLUMNS: &str = "The file has no columns.";
pub const IMPORT_NO_MAPPING: &str = "Choose a file column for one table column or more.";
pub const IMPORT_JSON_FORMAT: &str = "JSON files have no format options.";
pub const IMPORT_STOPPED: &str = "The import stopped.";
pub const IMPORT_NOT_UTF8: &str = "This row is not valid UTF-8. Try Windows-1252.";
pub const IMPORT_NOT_OBJECT: &str = "This value is not a JSON object.";
pub const IMPORT_NOT_ARRAY: &str = "A JSON file must hold an array of objects.";
pub const IMPORT_JSON_OPEN: &str = "The JSON file ends before its array ends.";
pub const IMPORT_LOST: &str = "The connection stopped. The last batch has an unknown state.";
pub const IMPORT_TASK_FAILED: &str = "The import task stopped.";
pub const SECTION_GENERAL: &str = "General";
pub const SECTION_EDITOR: &str = "Editor";
pub const SECTION_RESULTS: &str = "Results";
pub const SECTION_APPEARANCE: &str = "Appearance";
pub const SECTION_SHORTCUTS: &str = "Shortcuts";
pub const FIELD_SHORTCUT_MODE: &str = "Key mode";
pub const SHORTCUT_COMMANDS: &str = "Commands";
pub const SHORTCUT_KEYS: &str = "Shortcuts";
pub const SHORTCUT_UNASSIGNED: &str = "Not assigned";
pub const SHORTCUT_BACK: &str = "Back to commands";
pub const HINT_SHORTCUT_COMMANDS: &[HintPart] = &[
    ("Up/Down", "command"),
    ("Enter", "edit"),
    ("Tab", "next"),
    ("Esc", "cancel"),
];
pub const HINT_SHORTCUT_EDIT: &[HintPart] = &[
    ("Tab", "next"),
    ("Left/Right", "change"),
    ("Enter", "choose"),
    ("Esc", "back to commands"),
];
pub const FIELD_SHORTCUT_BINDING: &str = "Shortcut";
pub const FIELD_SHORTCUT_FINISH: &str = "Finish with";
pub const SHORTCUT_ADD: &str = "+ Add shortcut";
pub const SHORTCUT_MOUSE_OK: &str = "Mouse OK";
pub const SHORTCUT_RECORD: &str = "Record";
pub const SHORTCUT_REMOVE: &str = "Remove shortcut";
pub const SHORTCUT_RESTORE: &str = "Restore action";
pub const SHORTCUT_OK: &str = "OK";
pub const SHORTCUT_CANCEL: &str = "Cancel recording";
pub const SHORTCUT_EMPTY: &str = "Press at least one key.";
pub const SHORTCUT_FINISH_FIRST: &str = "Confirm or cancel the recording first.";
pub const SHORTCUT_RECORD_KEYS: &str =
    "Press the keys. Enter or a click on OK confirms. Esc cancels.";
pub const SHORTCUT_RECORD_MOUSE: &str =
    "Press the keys, including Enter. Click OK to confirm. Esc cancels.";
pub const SHORTCUT_RESTORED: &str = "The action uses its inherited keys. Save to apply.";

pub fn shortcut_region(region: crate::input::Region) -> &'static str {
    match region {
        crate::input::Region::Global => VALUE_SCOPE_GLOBAL,
        crate::input::Region::Editor => REGION_EDITOR,
        crate::input::Region::Navigator => REGION_NAVIGATOR,
        crate::input::Region::Grid => REGION_GRID,
        crate::input::Region::Diagram => REGION_DIAGRAM,
    }
}
pub const FIELD_KEYBOARD: &str = "Keyboard";
pub const FIELD_REOPEN_TABS: &str = "Reopen tabs";
pub const FIELD_QUERY_HISTORY: &str = "Query history";
pub const TITLE_GENERATE: &str = "Generate SQL";
pub const NO_COLUMNS_TO_LIST: &str = "This relation has no known columns.";
/// Returns the row of a SQL template in its list.
pub fn template_name(template: Template) -> &'static str {
    match template {
        Template::SelectAll => "SELECT *",
        Template::SelectColumns => "SELECT with the columns",
        Template::Insert => "INSERT",
        Template::Update => "UPDATE",
        Template::Delete => "DELETE",
    }
}
pub const TEMPLATE_DDL: &str = "DDL";
pub const TITLE_FOLLOW: &str = "Follow a key";
pub const MENU_FOLLOW: &str = "Follow a key";
/// The marks of a key in the list of `F`: a key that leaves the table, and a key of
/// another table that reaches it.
pub const FOLLOW_LEAVING: &str = "→";
pub const FOLLOW_REACHING: &str = "←";
pub const FOLLOW_NONE: &str = "The row has no key with a value to follow.";
pub const FOLLOW_REFUSED: &str = "Key navigation acts on the rows of a table.";
pub const FIELD_FORMAT_JSON: &str = "Format JSON";
pub const FIELD_SHOW_SYSTEM_COMMANDS: &str = "System commands";
pub const FIELD_BELL_AFTER: &str = "Bell seconds";
pub const FIELD_SUGGESTION_KEYS: &str = "Autocomplete";
pub const FIELD_UNDO_LIMIT: &str = "Undo (MiB)";
pub const FIELD_PAGE_SIZE: &str = "Page size";
pub const FIELD_ROW_CAP: &str = "Row cap";
pub const FIELD_CAP_IN_SQL: &str = "Limit in SQL";
pub const FIELD_BUDGET: &str = "Data (MiB)";
pub const FIELD_THEME: &str = "Default theme";
pub const FIELD_CONNECTION_THEME: &str = "Theme";
pub const FIELD_CONNECTION_ICON: &str = "Icon";

pub fn export_format_name(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "CSV",
        ExportFormat::Tsv => "TSV",
        ExportFormat::Json => "JSON",
        ExportFormat::JsonLines => "JSON Lines",
        ExportFormat::SqlInsert => "SQL INSERT",
    }
}

pub fn export_selection(rows: usize, columns: usize) -> String {
    format!("Selection ({rows} × {columns})")
}

pub fn export_loaded(rows: usize) -> String {
    format!("Loaded rows ({rows})")
}

pub fn export_delimiter(symbol: char) -> String {
    format!("{symbol}")
}

pub fn export_exists(path: &str) -> String {
    format!("{path} exists. Replace this file?")
}

pub fn export_progress(rows: usize, path: &str, stop: Option<&str>) -> String {
    let progress = format!("Export: {rows} rows written to {path}");
    match stop {
        Some(key) => format!("{key} stops. {progress}"),
        None => progress,
    }
}

pub fn export_done(rows: usize, path: &str) -> String {
    format!("Exported {rows} rows to {path}.")
}

pub fn import_format_name(format: ImportFormat) -> &'static str {
    match format {
        ImportFormat::Csv => "CSV",
        ImportFormat::Tsv => "TSV",
        ImportFormat::Json => "JSON",
        ImportFormat::JsonLines => "JSON Lines",
    }
}

pub fn import_section_name(section: Section) -> &'static str {
    match section {
        Section::File => "File",
        Section::Format => "Format",
        Section::Columns => "Columns",
        Section::Options => "Options",
        Section::Preview => "Preview",
    }
}

pub fn import_title(table: &str) -> String {
    format!("Import to {table}")
}

pub fn import_summary(columns: usize, mapped: usize, table: usize) -> String {
    format!("{columns} file columns. {mapped} of {table} table columns get values.")
}

pub fn import_failed(reason: &str) -> String {
    format!("The import failed: {reason}")
}

pub fn import_closed(rows: usize) -> String {
    format!("The tab closed. The import saved {rows} rows.")
}

pub fn import_progress(rows: usize, errors: usize, percent: u64, stop: Option<&str>) -> String {
    let stop = stop.map_or_else(String::new, |key| format!(" {key} stops."));
    format!("Importing: {rows} rows, {errors} errors, {percent}% of the file.{stop}")
}

pub fn import_production(path: &str, table: &str, connection: &str) -> String {
    format!("Import {path} to the table {table} of the production connection {connection}?")
}

/// The end of an import.
pub struct ImportReport<'a> {
    pub rows: usize,
    pub errors: usize,
    pub ignored: u64,
    pub log: Option<&'a str>,
    pub failure: Option<&'a str>,
}

pub fn import_report(report: &ImportReport<'_>) -> String {
    let mut body = format!("Rows inserted: {}.", report.rows);
    if report.errors > 0 {
        body.push_str(&format!("\nRows with errors: {}.", report.errors));
    }
    if report.ignored > 0 {
        let ignored = report.ignored;
        body.push_str(&format!(
            "\n{ignored} objects had keys that are not columns."
        ));
    }
    if let Some(log) = report.log {
        body.push_str(&format!("\nLog: {log}"));
    }
    if let Some(failure) = report.failure {
        body.push_str(&format!("\n{failure}"));
    }
    body
}

pub fn import_open_quote(line: u64) -> String {
    format!("The quotes that start at line {line} have no end.")
}

pub fn import_field_count(count: usize, width: usize) -> String {
    format!("This row has {count} fields. The file has {width} columns.")
}

/// Returns the name of a column of a file without a header.
pub fn import_column(number: usize) -> String {
    format!("column_{number}")
}

pub fn import_json_after(line: u64) -> String {
    format!("Line {line} has text after the end of the JSON array.")
}

pub fn import_json_syntax(line: u64) -> String {
    format!("Line {line} does not continue the JSON array.")
}

pub fn import_nul(column: &str) -> String {
    format!("The value of {column} holds a NUL byte.")
}

pub fn import_base64(column: &str) -> String {
    format!("The value of {column} is not valid Base64.")
}

pub fn import_bad_row(line: u64, reason: &str) -> String {
    format!("Line {line}: {reason}")
}

pub fn import_bad_lot(first: u64, last: u64, reason: &str) -> String {
    format!("The batch of lines {first} to {last} failed: {reason}")
}

pub fn import_error_limit(limit: usize) -> String {
    format!("The import stopped after {limit} errors.")
}

pub fn import_log_failed(reason: &str) -> String {
    format!("The log was not written: {reason}")
}

pub fn export_failed(reason: &str) -> String {
    format!("The export failed: {reason}")
}

pub fn export_choices_failed(reason: &str) -> String {
    format!("The export choices are not saved: {reason}")
}

/// Asks about the rows of an export that did not end. `failure` holds the reason.
pub fn export_part(rows: usize, failure: Option<&str>, replaces: bool) -> String {
    let start = failure.map_or_else(
        || format!("The export stopped after {rows} rows."),
        |reason| format!("The export failed after {rows} rows: {reason}"),
    );
    let question = if replaces {
        "Keep replaces the old file."
    } else {
        "Keep this incomplete file?"
    };
    format!("{start} {question}")
}

pub fn settings_number(name: &str) -> String {
    format!("{name} needs a number of 1 or more.")
}
pub fn settings_seconds(name: &str) -> String {
    format!("{name} needs a number of seconds. The value 0 stops the bell.")
}

pub fn from_global(value: &str) -> String {
    format!("from global: {value}")
}

pub fn keyboard_name(name: crate::store::config::KeyboardName) -> String {
    match name {
        crate::store::config::KeyboardName::Conventional => MODE_CONVENTIONAL.to_owned(),
        crate::store::config::KeyboardName::Vim => MODE_VIM.to_owned(),
    }
}

pub fn change_connection(
    name: &str,
    current: &str,
    target: &str,
    folder: &str,
    moves: bool,
) -> String {
    let files = if moves {
        "The files move to"
    } else {
        "The files stay in"
    };
    format!(
        "Move {name} from {current} to {target}?\n\n{files} {folder}.\n\
         SQLN saves the text first, and it runs no SQL.\n\
         The results and the schema choice end. The Messages history stays."
    )
}
pub const TITLE_SCHEMA: &str = "Schema";
pub const TITLE_DATABASE: &str = "Database";
pub const SCHEMA_DEFAULT: &str = "(default of the connection)";
pub const SCHEMA_LOADING: &str = "Reading the catalog.";
pub const CONTROL_RUN: &str = "Run";
pub const CONTROL_SCRIPT: &str = "Script";
pub const CONTROL_STOP: &str = "Stop";
/// The button at the right end of the main tabs. It quits as `Ctrl+W q`.
pub const CONTROL_QUIT: &str = "Quit";
pub const CONTROL_COMMIT: &str = "Commit";
pub const CONTROL_ROLLBACK: &str = "Rollback";
pub const LABEL_SCHEMA: &str = "Schema";
pub const LABEL_DATABASE: &str = "Database";
pub const REGION_EDITOR: &str = "Editor";
pub const REGION_NAVIGATOR: &str = "Navigator";
pub const REGION_GRID: &str = "Results";
pub const MODE_CONVENTIONAL: &str = "Conventional";
pub const MODE_VIM: &str = "Vim";
pub const DATABASE_LABEL: &str = "Database";
pub const NO_DATABASE_LABEL: &str = "No database";
pub const CURSOR_OUTSIDE: &str = "The cursor is outside a statement.";
pub const NO_STATEMENT: &str = "The text has no statement to run.";
pub const PLAN_REFUSED: &str = "This source has no query plan.";
pub const PLAN_ANALYZE_REFUSED: &str = "SQLite cannot run a statement inside a plan.";
pub const PLAN_ONE_STATEMENT: &str = "A plan needs one statement. Select only one.";
pub const PLAN_HAS_EXPLAIN: &str = "The statement has EXPLAIN already. Use Run, or remove it.";
pub const PLAN_UNREADABLE: &str =
    "SQLN cannot show this plan as a tree. The grid shows the text of the database.";
pub const PLAN_ANALYZE_RUNS: &str =
    "The database runs this statement to measure it. Its changes can stay.";
pub const PLAN_TAB: &str = "Plan";
pub const PLAN_NODE: &str = "Node";
pub const PLAN_OBJECT: &str = "Object";
pub const PLAN_SHARE: &str = "Share";
pub const PLAN_COST: &str = "Cost";
pub const PLAN_ROWS: &str = "Rows";
pub const PLAN_TIME: &str = "Time (ms)";
pub const PLAN_ACTUAL_ROWS: &str = "Actual rows";
pub const PLAN_LOOPS: &str = "Loops";
pub const PLAN_DETAIL: &str = "Detail";
pub const EXECUTION_ACTIVE: &str = "The console has an execution in progress.";
pub const NO_TRANSACTION: &str = "No transaction is open.";
pub const RESULT_READING: &str = "This result receives rows. Ctrl+W x stops the script.";
pub const COPIED: &str = "Copied.";
pub const NO_MESSAGES: &str = "No messages.";
pub const MESSAGES_TAB: &str = "Messages";
/// Returns the label of the tab. The number says that a warning waits on that tab.
pub fn messages_tab(warnings: usize) -> String {
    match warnings {
        0 => MESSAGES_TAB.to_owned(),
        warnings => format!("{MESSAGES_TAB} ({warnings})"),
    }
}
pub fn more_notices(count: usize) -> String {
    format!("{count} more notices.")
}
pub fn limit_added(rows: usize) -> String {
    format!("SQLN added LIMIT {rows} to this statement.")
}
pub const SECTION_COLUMNS: &str = "Columns";
pub const SECTION_INDEXES: &str = "Indexes";
pub const SECTION_FOREIGN_KEYS: &str = "Foreign keys";
pub const ALL_SCHEMAS: &str = "all schemas";
pub const STRUCTURE_REFUSED: &str = "This source has no structure. Keys have no columns.";
pub const SESSIONS_REFUSED: &str = "SQLN lists sessions only on PostgreSQL and MySQL.";
pub const DIAGRAM_REFUSED: &str = "This source has no diagram. Keys have no columns.";
pub const DIAGRAM_NO_SCHEMA: &str =
    "The connection has no current schema. Use the row of a schema.";
pub const DIAGRAM_VIEW: &str = "A view has no keys. Use the row of its schema.";
pub const DIAGRAM_CONSOLE: &str = "A console has no tables. Use the row of a schema or a table.";
pub const DIAGRAM_EMPTY: &str = "This schema has no tables.";
pub const DIAGRAM_STOPPED: &str = "The read stopped. The diagram did not change.";
pub const DIAGRAM_PARTIAL: &str = "partial: a limit stopped the read";
pub const DIAGRAM_KEYS_ONLY: &str = "keys only";
pub const COMPARE_REFUSED: &str = "This source has no tables to compare.";
pub const COMPARE_CONSOLE: &str = "A console has no tables. Use the row of a schema or a table.";
pub const COMPARE_VIEW: &str = "A view has no keys to compare. Use the row of a table.";
pub const COMPARE_NO_CONNECTION: &str = "Add a database connection first.";
pub const COMPARE_TABLE_ON_BOTH: &str = "Write the source table too, or clear the target table.";
pub const COMPARE_SAME_SIDES: &str = "The target is the source. Choose another target.";
pub const COMPARE_LIMITED: &str =
    "A limit stopped the read. The compare needs each row, so it shows nothing.";
pub const COMPARE_STOPPED: &str = "The read stopped. The compare did not change.";
pub const COMPARE_BUDGET: &str = "The data budget is full. Close a result and read again.";
pub const COMPARE_EQUAL: &str = "No differences. The target is equal to the source.";
pub const COMPARE_NO_SCRIPT: &str = "No script: the target has no differences.";
pub const COMPARE_NO_SCRIPT_DIALECTS: &str = "No script: the two sources have different dialects.";
pub const COMPARE_NOT_READ: &str = "The compare has no result yet.";
pub const COMPARE_LOADING: &str = "loading…";
pub const COMPARE_TWO_DIALECTS: &str = "no script: two dialects";
pub const COMPARE_COLUMNS: [&str; 6] = ["Change", "Object", "Table", "Name", "Source", "Target"];
pub const COMPARE_CREATE: &str = "create";
pub const COMPARE_DROP: &str = "drop";
pub const COMPARE_ALTER: &str = "alter";
pub const COMPARE_TABLE: &str = "table";
pub const COMPARE_COLUMN: &str = "column";
pub const COMPARE_PRIMARY_KEY: &str = "primary key";
pub const COMPARE_UNIQUE: &str = "unique key";
pub const COMPARE_FOREIGN_KEY: &str = "foreign key";
pub const COMPARE_CHECK: &str = "check";
pub const COMPARE_INDEX: &str = "index";
pub const COMPARE_NO_KEY: &str = "The source table has no primary key, so its rows have no match.";
pub const COMPARE_DATA_BUDGET: &str =
    "The source table does not fit in the data budget. The compare shows nothing.";
pub const COMPARE_DATA_TABLE: &str = "A data compare needs a source table.";
pub const COMPARE_CHANGE: &str = "Change";
pub const COMPARE_INSERT: &str = "insert";
pub const COMPARE_DELETE: &str = "delete";
pub const COMPARE_TARGET_ROW: &str = "  target";
pub const KIND_STRUCTURE: &str = "Structure";
pub const KIND_DATA: &str = "Data";
pub const FIELD_COMPARE_KIND: &str = "Compare";
pub const COMPARE_DATA_HINT: &str =
    "The script makes the rows of the target table equal to the rows of the source table.";
pub const TITLE_COMPARE: &str = "Compare";
pub const BUTTON_COMPARE: &str = "Compare";
pub const FIELD_COMPARE_SOURCE: &str = "Source connection";
pub const FIELD_COMPARE_SOURCE_SCHEMA: &str = "Source schema";
pub const FIELD_COMPARE_SOURCE_TABLE: &str = "Source table";
pub const FIELD_COMPARE_TARGET: &str = "Target connection";
pub const FIELD_COMPARE_TARGET_SCHEMA: &str = "Target schema";
pub const FIELD_COMPARE_TARGET_TABLE: &str = "Target table";
pub const FIELD_COMPARE_REMOVALS: &str = "Removals as statements";
pub const COMPARE_HINT: &str =
    "The script makes the target equal to the source. An empty table compares the schema.";
pub const SESSIONS_READ_ONLY: &str = "A read-only connection cannot change a session.";
pub const SESSION_SIGNAL_SENT: &str = "The database accepted the request.";
pub const SESSION_GONE: &str = "The session ended before the request.";
pub const TITLE_CANCEL_QUERY: &str = "Cancel the query";
pub const TITLE_END_SESSION: &str = "End the session";
pub const OPTION_CANCEL_QUERY: &str = "Cancel the query";
pub const OPTION_END_SESSION: &str = "End the session";

pub const LABEL_WHERE: &str = "WHERE";
pub const LABEL_ORDER_BY: &str = "ORDER BY";
pub const BUTTON_APPLY: &str = "Apply";
pub const BUTTON_CLEAR: &str = "Clear";
pub const BUTTON_APPLY_FILTERS: &str = "Apply filters";
pub const BUTTON_CLEAR_FILTERS: &str = "Clear filters";
pub const BUTTON_REVIEW: &str = "Review";
pub const BUTTON_DISCARD: &str = "Discard";
pub const BUTTON_LOAD_MORE: &str = "Load more";
pub const BUTTON_PREVIOUS: &str = "Previous";
pub const BUTTON_NEXT: &str = "Next";
/// Writes the place of a page. The total stays open while the table can have more rows.
pub fn table_page(page: usize, pages: usize, more: bool) -> String {
    let open = if more { "+" } else { "" };
    format!("Page {page} of {pages}{open}")
}

/// Writes the place of a page for a narrow row.
pub fn table_page_short(page: usize, pages: usize, more: bool) -> String {
    let open = if more { "+" } else { "" };
    format!("{page}/{pages}{open}")
}
pub const TABLE_TEXT_REFUSED: &str = "The WHERE and ORDER BY text must stay in one statement. Remove a semicolon, an open quote, or an open comment.";
pub const TABLE_BUSY: &str = "The table has a load in progress. Ctrl+W x stops it.";
pub const TABLE_NO_MORE: &str = "The table has no more rows to load.";
pub const TABLE_BLOCKED: &str = "The data budget is full. Close a tab to free space.";
pub const TABLE_STOPPED: &str = "The load stopped. The rows already loaded stay.";
pub const TABLE_TIMEOUT: &str = "The time limit ended. The database did not confirm the result.";
pub const TABLE_CLOSED: &str = "The connection closed. The result is unknown.";
pub const TABLE_CANCELED: &str = "The load stopped before it started. No SQL was sent.";
pub const TABLE_MORE: &str = "more rows available";
pub const TABLE_NO_MORE_ROWS: &str = "no more rows";
pub const TABLE_WAITING: &str = "waiting for a connection";
pub const TABLE_LOADING: &str = "loading";
pub const TABLE_COUNTING: &str = "counting";
pub const TABLE_STOPPING: &str = "stopping";
pub const TABLE_BUDGET: &str = "data budget reached; close a tab to free space";
pub const TABLE_VALUE_TOO_LARGE: &str = "a value is larger than the data budget";
pub const TABLE_NO_ORDER: &str = "No ORDER BY: pages can repeat or skip rows.";
pub const BUTTON_RELOAD: &str = "Reload";
pub const NOTE_PARTIAL_KEYS: &str = "(partial list)";
/// The connection shows only the schemas of its list.
pub const NOTE_FILTERED: &str = "(filtered)";
pub const REGION_TABLE: &str = "Table";
pub const REGION_DIAGRAM: &str = "Diagram";
pub const REGION_COMPARE: &str = "Compare";
pub const REGION_FILTER: &str = "Filter";

pub const MARK_PRODUCTION: &str = "PROD";
pub const MODE_AUTO: &str = "Auto";
pub const MODE_MANUAL: &str = "Manual";
pub const TRANSACTION_NONE: &str = "No transaction";
pub const TRANSACTION_OPEN: &str = "Transaction open";
pub const TRANSACTION_FAILED: &str = "Transaction failed";
pub const TRANSACTION_UNKNOWN: &str = "Transaction unknown";
/// Writes the age of a transaction in whole minutes: `3m` or `1h 05m`. A transaction
/// of less than one minute has no text.
pub fn transaction_age(age: std::time::Duration) -> Option<String> {
    let minutes = age.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => None,
        (0, minutes) => Some(format!("{minutes}m")),
        (hours, minutes) => Some(format!("{hours}h {minutes:02}m")),
    }
}

/// Writes the time of a run with three decimals: `12.345s`, `1m 05.123s`, or
/// `1h 02m 05.123s`.
pub fn elapsed(time: std::time::Duration) -> String {
    let seconds = time.as_secs();
    let millis = time.subsec_millis();
    match seconds {
        0..=59 => format!("{seconds}.{millis:03}s"),
        60..=3599 => format!("{}m {:02}.{millis:03}s", seconds / 60, seconds % 60),
        _ => format!(
            "{}h {:02}m {:02}.{millis:03}s",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ),
    }
}

pub fn last_run(time: std::time::Duration) -> String {
    format!("(last {})", elapsed(time))
}

pub const PHASE_IDLE: &str = "Idle";
pub const PHASE_WAITING: &str = "Waiting";
pub const PHASE_RUNNING: &str = "Running";
pub const PHASE_STOPPING: &str = "Stopping";

pub const COPY_REFUSED: &str =
    "SQLN does not support COPY with STDIN or STDOUT. Use a file on the server.";
pub const STOPPED_AFTER_LAST: &str =
    "The execution stopped. The last statement ended before the stop request.";
pub const STOPPED_BEFORE_SEND: &str = "The statement stopped before it reached the database.";
pub const COMMIT_OF_FAILED_TRANSACTION: &str =
    "The transaction has an error, and a commit cannot save it. Use rollback.";
pub const NOT_COMMITTED: &str = "The transaction is not committed.";
pub const DATABASE_ENDED_TRANSACTION: &str = "The database ended the transaction.";
pub const CANCELED_BEFORE_START: &str = "The execution stopped before it started. No SQL was sent.";

pub const TITLE_CREATE: &str = "Create database";
pub const TITLE_RUNNING: &str = "Execution in progress";
pub const TITLE_TRANSACTION: &str = "Open transaction";
pub const TITLE_NO_ANSWER: &str = "No answer from the database";
pub const TITLE_UNKNOWN: &str = "Transaction state unknown";
pub const TITLE_KEYS: &str = "Keys";
pub const BODY_RUNNING: &str =
    "Stop the execution and close? The statement in progress must end first.";
pub const BODY_TRANSACTION_CLOSE: &str =
    "This console has an open transaction. Choose an action before the close.";
pub const BODY_TRANSACTION_MODE: &str =
    "This console has an open transaction. Choose an action before the change to autocommit.";
pub const BODY_NO_ANSWER: &str = "The statement did not answer. If you close the connection, you lose the changes that are not committed.";
pub const BODY_UNKNOWN: &str = "The connection closed with an open transaction. The next statement can run outside that transaction.";
pub const OPTION_CANCEL: &str = "Cancel";
pub const OPTION_CREATE: &str = "Create file";
pub const OPTION_STOP_CLOSE: &str = "Stop and close";
pub const OPTION_COMMIT_CLOSE: &str = "Commit and close";
pub const OPTION_ROLLBACK_CLOSE: &str = "Rollback and close";
pub const OPTION_COMMIT_AUTO: &str = "Commit and use autocommit";
pub const OPTION_ROLLBACK_AUTO: &str = "Rollback and use autocommit";
pub const OPTION_KEEP_WAITING: &str = "Keep waiting";
pub const OPTION_CLOSE_CONNECTION: &str = "Close the connection";
pub const OPTION_CONTINUE: &str = "Run";
pub const OPTION_RETRY: &str = "Try again";
pub const OPTION_REMOVE: &str = "Remove";

pub const FILTER_KEYS: &[(&str, &str)] = &[
    ("Enter", "Apply the filters"),
    ("Tab", "Accept the suggestion, or the next field"),
    ("Shift+Tab", "Next field"),
    ("Ctrl+Space", "Suggestions"),
    ("Esc", "Close the list, or go to the grid"),
];

pub const EDITOR_KEYS: &[(&str, &str)] =
    &[("Tab", "Accept the suggestion"), ("Esc", "Close the list")];

pub const PARAMETERS_TITLE: &str = "Parameters";
pub const PARAMETERS_TOGGLE: &str = "Collapse or expand parameters";
pub const HINT_PARAMETERS: &[HintPart] = &[
    ("Tab", "field"),
    ("↑/↓", "parameter"),
    ("Enter", "set"),
    ("Esc", "editor"),
];
pub const PARAMETER_SET: &str = "Set";
pub const PARAMETER_UNSET: &str = "Unset";
pub const PARAMETER_TYPES: [&str; 4] = ["Text", "Number", "Boolean", "NULL"];

pub fn action_text(action: Action) -> &'static str {
    match action {
        Action::AgentSessions => "Agent sessions: allow or revoke access",
        Action::AgentReview => "Review agent changes",
        Action::AgentDirect => "Toggle agent direct editing for this console",
        Action::Run => "Run the selection or the statement",
        Action::RunScript => "Run the script",
        Action::Explain => "Show the plan of the statement",
        Action::ExplainAnalyze => "Run the statement and show its plan",
        Action::Stop => "Stop the script",
        Action::ToggleMode => "Autocommit or manual",
        Action::Commit => "Commit",
        Action::Rollback => "Rollback",
        Action::FormatSql => "Format the SQL",
        Action::FocusParameters => "Focus query parameters",
        Action::ToggleParameters => "Collapse or expand query parameters",
        Action::ToggleComment => "Comment or uncomment the rows",
        Action::OpenHistory => "Open the query history",
        Action::OpenSessions => "Show the sessions of the server",
        Action::ExplorerCommands => "Show the commands of the explorer",
        Action::ContextCommands => "Show the commands of the current panel",
        Action::EditorSave => "Save the console now",
        Action::EditorFind => "Find in the editor",
        Action::EditorReplace => "Find and replace in the editor",
        Action::EditorSuggestions => "Open the suggestions",
        Action::EditorUndo => "Undo the last edit",
        Action::EditorRedo => "Redo the last edit",
        Action::EditorSelectAll => "Select all the text",
        Action::EditorCopy => "Copy the selected text",
        Action::EditorCut => "Cut the selected text",
        Action::EditorPaste => "Paste into the editor",
        Action::Help => "Open this screen",
        Action::OpenPalette => "Open the commands",
        Action::CloseTab => "Close the tab",
        Action::Quit => "Quit",
        Action::NextTab => "Next tab",
        Action::PreviousTab => "Previous tab",
        Action::ToggleNavigator => "Hide or show the navigator",
        Action::ChooseSchema => "Choose the schema or database",
        Action::FocusLeft => "Go to the navigator",
        Action::FocusRight => "Leave the navigator",
        Action::FocusUp => "Go to the editor",
        Action::FocusDown => "Go to the results",
        Action::NavigatorUp => "Previous row",
        Action::NavigatorDown => "Next row",
        Action::NavigatorCollapse => "Close, or go to the parent",
        Action::NavigatorExpand => "Open",
        Action::NavigatorOpen => "Open a console or a table, or open the row",
        Action::NavigatorNewConsole => "New console",
        Action::NavigatorRefresh => "Read the catalog again",
        Action::NavigatorDisconnect => "Disconnect",
        Action::NavigatorEdit => "Edit the connection",
        Action::NavigatorDelete => "Remove the connection",
        Action::NavigatorAdd => "Add a connection",
        Action::RenameConsole => "Rename the console",
        Action::ChangeConnection => "Change the connection of the console",
        Action::OpenSettings => "Open the settings",
        Action::GridExport => "Export the result to a file",
        Action::GridImport => "Import a file to this table",
        Action::GridEditCell => "Edit the selected cell",
        Action::GridSetNull => "Set the selected cell to NULL",
        Action::GridReviewEdits => "Review pending table changes",
        Action::GridSaveEdits => "Review and apply pending table changes",
        Action::GridDiscardEdits => "Discard pending table changes",
        Action::NavigatorImport => "Import a file to the table of the row",
        Action::NavigatorScripts => "Scripts without a connection",
        Action::NavigatorDeletePassword => "Delete the saved password",
        Action::NavigatorStructure => "Show the structure of the row",
        Action::NavigatorSessions => "Show the sessions of the server",
        Action::NavigatorFilter => "Filter the tables of the explorer",
        Action::NavigatorClearFilter => "Clear the filter of the explorer",
        Action::NavigatorGenerate => "Generate SQL or the DDL of the row",
        Action::NavigatorCopyName => "Copy the name of the row",
        Action::GridStructure => "Show the structure of the table",
        Action::GridReload => "Read the grid again",
        Action::GridCancelQuery => "Cancel the query of the session",
        Action::GridEndSession => "End the session",
        Action::GridFollowKey | Action::FollowKey => "Follow a key of the row",
        Action::ReloadConfig => "Load the configuration again",
        Action::GridUp => "Up",
        Action::GridDown => "Down",
        Action::GridLeft => "Left",
        Action::GridRight => "Right",
        Action::GridPageUp => "Previous page",
        Action::GridPageDown => "Next page",
        Action::GridFirstRow => "First row",
        Action::GridLastRow => "Last row",
        Action::GridFirstColumn => "First column",
        Action::GridLastColumn => "Last column",
        Action::GridSelectAll => "Select all",
        Action::GridToggleSelection => "Start or end the selection",
        Action::GridClearSelection => "Clear the selection",
        Action::GridCopy => "Copy as TSV",
        Action::GridOpenValue => "Open the value",
        Action::GridOpenRow => "Open the row",
        Action::GridCountRows => "Count the rows",
        Action::GridFind => "Find in the loaded rows",
        Action::GridNarrow => "Make the column narrow",
        Action::GridWiden => "Make the column wide",
        Action::GridAutoWidth => "Use the automatic column width",
        Action::GridCloseResult => "Close the result",
        Action::GridPreviousResult => "Previous result",
        Action::GridNextResult => "Next result",
        Action::GridApply => "Apply the WHERE and ORDER BY fields",
        Action::GridClearFilters => "Clear the filters and reload the table",
        Action::GridFocusFilter => "Edit the WHERE filter",
        Action::GridFocusOrder => "Edit the ORDER BY text",
        Action::GridSortColumn => "Sort by the column",
        Action::GridLoadMore => "Load more rows",
        Action::NavigatorDiagram => "Show the ER diagram of the row",
        Action::OpenDiagram => "Show the ER diagram of the tab or the row",
        Action::OpenCompare => "Compare the tab or the row with another source",
        Action::DiagramUp => "Select the table above",
        Action::DiagramDown => "Select the table below",
        Action::DiagramLeft => "Select the table on the left",
        Action::DiagramRight => "Select the table on the right",
        Action::DiagramPanUp => "Move the view up",
        Action::DiagramPanDown => "Move the view down",
        Action::DiagramPanLeft => "Move the view to the left",
        Action::DiagramPanRight => "Move the view to the right",
        Action::DiagramPageUp => "Move the view up one page",
        Action::DiagramPageDown => "Move the view down one page",
        Action::DiagramOpen => "Open the data of the table",
        Action::DiagramStructure => "Show the structure of the table",
        Action::DiagramCopy => "Copy the diagram as Mermaid text",
        Action::DiagramReload => "Read the diagram again",
        Action::DiagramKeysOnly => "Show only the key columns, or all",
        Action::NavigatorCompare => {
            "Compare the structure or the data of the row with another source"
        }
        Action::CompareScript => "Open the sync script in a new console",
        Action::CompareCopy => "Copy the sync script",
    }
}

/// Writes the name of an action in the command list.
pub fn command_text(action: Action) -> &'static str {
    match action {
        Action::Help => "Show the keys",
        action => action_text(action),
    }
}

/// Keeps panel command names short enough for the minimum terminal.
pub fn panel_command_text(action: Action) -> &'static str {
    use Action::*;
    match action {
        EditorSave => "Save now",
        EditorFind => "Find",
        EditorReplace => "Replace",
        EditorSuggestions => "Suggestions",
        EditorUndo => "Undo",
        EditorRedo => "Redo",
        EditorSelectAll | GridSelectAll => "Select all",
        EditorCopy | GridCopy => "Copy",
        EditorCut => "Cut",
        EditorPaste => "Paste",
        GridToggleSelection => "Start/end selection",
        GridClearSelection => "Clear selection",
        GridOpenValue => "Open value",
        GridOpenRow => "Open row",
        GridFind => "Find",
        GridNarrow => "Narrow column",
        GridWiden => "Widen column",
        GridAutoWidth => "Automatic width",
        GridCloseResult => "Close result",
        GridPreviousResult => "Previous",
        GridNextResult => "Next",
        GridApply => "Apply filters",
        GridClearFilters => "Clear filters",
        GridFocusFilter => "WHERE filter",
        GridFocusOrder => "ORDER BY",
        GridSortColumn => "Sort column",
        GridLoadMore => "Load more",
        GridCountRows => "Count rows",
        GridExport => "Export",
        GridImport => "Import",
        GridEditCell => "Edit cell",
        GridSetNull => "Set NULL",
        GridReviewEdits => "Review changes",
        GridSaveEdits => "Apply changes",
        GridDiscardEdits => "Discard changes",
        GridStructure => "Structure",
        GridReload | DiagramReload => "Reload",
        GridFollowKey => "Follow key",
        GridCancelQuery => "Cancel query",
        GridEndSession => "End session",
        CompareScript => "Open sync script",
        CompareCopy => "Copy sync script",
        DiagramOpen => "Open data",
        DiagramStructure => "Structure",
        DiagramCopy => "Copy Mermaid",
        DiagramKeysOnly => "Key/all columns",
        _ => action_text(action),
    }
}

/// Adds technical search terms without changing the visible command name.
pub fn command_aliases(action: Action) -> &'static str {
    match action {
        Action::Explain | Action::ExplainAnalyze => "explain query plan analyze",
        Action::EditorFind | Action::EditorReplace | Action::GridFind => "search find replace",
        Action::EditorSuggestions => "autocomplete completion suggestions",
        Action::GridCopy => "copy cell selection clipboard tsv",
        Action::NavigatorRefresh => "refresh reload catalog rename console",
        Action::GridFocusFilter => "where filter",
        Action::GridFocusOrder | Action::GridSortColumn => "order by sort",
        Action::NavigatorGenerate => "sql ddl create select insert update delete",
        Action::DiagramCopy => "mermaid diagram export",
        Action::GridReload | Action::DiagramReload => "refresh reload",
        _ => "",
    }
}

pub fn command_result_target(console: &str, result: usize) -> String {
    format!("{console} · result {result}")
}

pub fn keys_title(mode: &str, region: &str) -> String {
    format!(" {TITLE_KEYS} · {mode} · {region} ")
}

pub fn help_footer(commands: usize, first: usize, last: usize, total: usize) -> String {
    format!(" {} · {first}–{last}/{total} ", plural(commands, "command"))
}

/// Writes one row of the help screen.
pub fn keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", action_text(action))
}

/// Writes a row of the list of keys of a table. The result keys move between the
/// pages of the table.
pub fn table_keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", grid_action_text(action, true, false))
}

/// Writes a row of the list of keys of a structure with sections. The result keys
/// move between the sections.
pub fn section_keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", grid_action_text(action, true, true))
}

/// Names the page keys for the grid that receives them.
pub fn grid_action_text(action: Action, table: bool, sections: bool) -> &'static str {
    match (table, sections, action) {
        (true, true, Action::GridPreviousResult) => "Previous section",
        (true, true, Action::GridNextResult) => "Next section",
        (true, false, Action::GridPreviousResult) => "Previous page of rows",
        (true, false, Action::GridNextResult) => "Next page of rows",
        _ => action_text(action),
    }
}

pub fn count(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

fn plural(number: usize, noun: &str) -> String {
    let ending = if number == 1 { "" } else { "s" };
    format!("{} {noun}{ending}", count(number))
}

pub fn search_count(position: usize, total: usize, partial: bool) -> String {
    match (total, partial) {
        (0, false) => NO_MATCHES.to_owned(),
        (_, false) => format!("{} of {}", count(position), count(total)),
        (_, true) => format!("{} of {}+", count(position), count(total)),
    }
}

pub fn result_tab(number: usize) -> String {
    format!("Result {number}")
}

/// Writes the row under the grid of a plan. `top` is the node with the largest share.
pub fn plan_summary(
    nodes: usize,
    cost: Option<&str>,
    time: Option<&str>,
    top: Option<(&str, f64)>,
) -> String {
    let mut parts = vec![plural(nodes, "node")];
    parts.extend(cost.map(|cost| format!("cost {cost}")));
    parts.extend(time.map(|time| format!("{time} ms")));
    parts.extend(top.map(|(name, share)| format!("most: {name} {:.0}%", share * 100.0)));
    parts.join(" · ")
}

pub fn rows_loaded(rows: usize) -> String {
    format!("{} loaded", plural(rows, "row"))
}

pub fn rows_of_total(rows: usize, total: usize) -> String {
    format!("{} of {} loaded", count(rows), plural(total, "row"))
}

pub fn table_label(name: &str, kind: &str) -> String {
    format!("{name} ({kind})")
}

pub fn table_title(connection: &str, schema: &str, relation: &str, kind: &str) -> String {
    format!("{connection} · {schema}.{relation} · {kind}")
}

pub fn structure_title(connection: &str, name: &str) -> String {
    format!("{connection} · {name} · structure")
}

pub fn compare_columns(count: usize) -> String {
    plural(count, "column")
}

pub fn compare_label(name: &str) -> String {
    format!("{name} (compare)")
}

pub fn compare_title(source: &str, target: &str) -> String {
    format!("{source} → {target} · compare")
}

pub fn compare_summary(differences: usize) -> String {
    plural(differences, "difference")
}

pub fn data_compare_label(name: &str) -> String {
    format!("{name} (data)")
}

pub fn data_compare_title(source: &str, target: &str) -> String {
    format!("{source} → {target} · data compare")
}

pub fn compare_no_target_table(table: &str) -> String {
    format!("The target has no table {table}. Compare the structure first.")
}

pub fn compare_missing_column(column: &str) -> String {
    format!("The target has no column {column}. Compare the structure first.")
}

pub fn compare_update(columns: &str) -> String {
    format!("update {columns}")
}

pub fn data_compare_header(source: &str, target: &str, differences: usize) -> String {
    format!(
        "-- SQLN data compare\n-- Source: {source}\n-- Target: {target}\n-- {}",
        plural(differences, "difference")
    )
}

pub fn compare_wait_catalog(connection: &str) -> String {
    format!("Wait for the schemas of {connection}.")
}

pub fn compare_no_table(table: &str) -> String {
    format!("The source has no table {table}.")
}

pub fn compare_script_header(source: &str, target: &str, differences: usize) -> String {
    format!(
        "-- SQLN structure compare\n-- Source: {source}\n-- Target: {target}\n-- {}",
        plural(differences, "difference")
    )
}

pub fn diagram_label(name: &str) -> String {
    format!("{name} (diagram)")
}

pub fn diagram_title(connection: &str, name: &str) -> String {
    format!("{connection} · {name} · diagram")
}

/// Writes the row under a diagram.
pub fn diagram_summary(tables: usize, keys: usize) -> String {
    format!("{} · {}", plural(tables, "table"), plural(keys, "key"))
}

/// Writes the title of the explorer page, with the row that its commands act on.
pub fn explorer_menu_title(row: Option<&str>) -> String {
    match row {
        Some(row) => format!("{TITLE_EXPLORER_MENU} · {row}"),
        None => TITLE_EXPLORER_MENU.to_owned(),
    }
}

pub fn sessions_title(connection: &str) -> String {
    format!("{connection} · sessions")
}

/// Writes the question before a signal. An open transaction of that session rolls back.
pub fn session_signal_body(signal: SessionSignal, session: u64, user: &str, query: &str) -> String {
    let effect = match signal {
        SessionSignal::CancelQuery => "The query stops, and the session stays open.",
        SessionSignal::EndSession => {
            "The session closes, and the database undoes its open transaction."
        }
    };
    format!("Session {session} of {user}.\n{effect}\n\n{query}")
}

pub fn redis_unknown_type(kind: &str) -> String {
    format!("SQLN cannot show a value of type {kind}. Use the console.")
}

pub fn rows_read(rows: usize) -> String {
    plural(rows, "row")
}

pub fn row_cap_reached(rows: usize) -> String {
    format!(
        "{} read. Row cap reached. The result can have more rows.",
        plural(rows, "row")
    )
}

pub fn budget_reached(rows: usize) -> String {
    format!(
        "{} read. Data budget reached. Increase the limit or run less data.",
        plural(rows, "row")
    )
}

pub fn value_too_large(rows: usize) -> String {
    format!(
        "{} read. A value is larger than the data budget, so the read stopped.",
        plural(rows, "row")
    )
}

pub fn statement_changed(index: usize, rows: u64) -> String {
    let rows = usize::try_from(rows).unwrap_or(usize::MAX);
    format!("Statement {}: {} changed.", index + 1, plural(rows, "row"))
}

pub fn statement_done(index: usize) -> String {
    format!("Statement {}: done.", index + 1)
}

pub fn statement_error(index: usize, message: &str) -> String {
    format!("Statement {}: {message}", index + 1)
}

pub fn statement_stopped(index: usize) -> String {
    format!("Statement {}: stopped.", index + 1)
}

pub fn stopped_before(index: usize) -> String {
    format!(
        "Stopped before statement {}. The database did not receive it.",
        index + 1
    )
}

pub fn statement_timeout(index: usize) -> String {
    format!(
        "Statement {}: the time limit ended. The database did not confirm the result.",
        index + 1
    )
}

pub fn statement_unknown(index: usize) -> String {
    format!(
        "Statement {}: the connection closed. The result is unknown.",
        index + 1
    )
}

pub fn console_name(number: usize) -> String {
    format!("console-{number}")
}

pub fn form_needs_number(label: &str) -> String {
    format!("{label} needs a number.")
}

pub fn edit_connection_title(name: &str) -> String {
    format!("Edit connection: {name}")
}

pub fn remove_connection(name: &str, file: Option<&str>) -> String {
    let place = file.map_or_else(String::new, |file| format!(" from {file}"));
    format!(
        "Remove the definition of {name}{place}? Its open consoles stay, and they cannot run. The scripts of the connection stay on disk, in the list of scripts without a connection."
    )
}

pub fn connection_in_use(console: &str) -> String {
    format!("{console} has an execution or a transaction. End it first.")
}

pub fn console_question(console: &str, question: &str) -> String {
    format!("{console}: {question}")
}

pub fn create_database(path: &str) -> String {
    format!("The file does not exist: {path}\nCreate it?")
}

pub fn transaction_failed(message: &str) -> String {
    format!("The transaction command failed: {message}")
}

pub fn value_title(column: &str, row: usize) -> String {
    format!(" {column} · row {} ", count(row + 1))
}

pub fn value_footer(chars: usize, kind: &str) -> String {
    format!(" {} · {kind} ", plural(chars, "character"))
}

pub fn row_title(row: usize, loaded: usize) -> String {
    format!(" row {} of {} loaded ", count(row + 1), count(loaded))
}

pub fn row_footer(columns: usize) -> String {
    format!(" {} ", plural(columns, "column"))
}

pub const VALUE_TEXT: &str = "text";
pub const VALUE_JSON: &str = "JSON";
pub const VALUE_JSON_ORIGINAL: &str = "JSON, original";
pub const VALUE_NUMBER: &str = "number";
pub const VALUE_BINARY: &str = "binary";
pub const VALUE_NULL: &str = "NULL";

pub const HELP: &str = "\
sqln: terminal database client

Usage: sqln [options] [target]
       sqln mcp

Assistant:
  mcp                   Serve MCP over stdio for open SQLN instances. Select an
                         instance and approve access in its terminal interface.

Target:
  A SQLite file, or a URI such as postgres://user@host/name. Without a password
  in the URI, SQLN asks for it. A password in a URI stays in the shell history.
  The connection of a target lasts for this session, and SQLN saves nothing of it.

Options:
  --project <folder>     Use the project in this folder. Without it, SQLN uses
                         the nearest .sqln folder above the working folder.
  --log <file>           Write a log of events, without SQL or values.
  -h, --help             Show this text.
  -V, --version          Show the version.

Development options:
  --dialect <name>       Use postgres, mysql, sqlite, or redis in a console without
                         a connection.

The settings and the connections are in config.json, in the global folder
(~/.config/sqln, or $XDG_CONFIG_HOME/sqln) and in the .sqln folder of the project.
";

pub const AGENT_CONSOLE_TEMPORARY: &str =
    "Agent consoles are temporary. Copy SQL to a user console to save it.";
pub const ONE_TARGET: &str = "Give one file or one URI.";

pub const TITLE_HISTORY: &str = "Query history";
pub const NO_HISTORY: &str = "This connection has no history.";
pub const NO_HISTORY_MATCH: &str = "No statement has this text.";

pub fn history_failed(message: &str) -> String {
    format!("The query history did not save: {message}")
}

pub const FORMAT_UNCHANGED: &str = "The SQL has this format already.";
pub const FORMAT_REFUSED: &str = "SQLN cannot format this text. Close its string or comment.";

pub const TITLE_CONFIRM_RUN: &str = "Confirm the run";
pub const OPTION_RUN: &str = "Run";

/// The risks of one run, for the question before it starts.
pub struct RunRisk<'a> {
    /// The name of a production connection.
    pub production: Option<&'a str>,
    pub changes: usize,
    pub no_where: usize,
    pub total: usize,
    /// The start of the first statement with a risk.
    pub first: &'a str,
}

pub fn run_risk(risk: &RunRisk) -> String {
    let RunRisk {
        production,
        changes,
        no_where,
        total,
        first,
    } = risk;
    let mut lines = Vec::new();
    if let Some(name) = production {
        lines.push(format!("{name} is a production connection."));
        lines.push(format!(
            "Statements that can change data: {changes} of {total}."
        ));
    }
    if *no_where > 0 {
        lines.push(format!(
            "Statements without WHERE: {no_where} of {total}. They change all rows."
        ));
    }
    lines.push(String::new());
    lines.push((*first).to_owned());
    lines.join("\n")
}

/// The question of the command line, before the interface starts.
pub fn password_prompt(user: &str) -> String {
    format!("Password for {user} (Enter for none): ")
}

// Files: the configuration, the project, and the saved consoles.
pub const TITLE_CREATE_PROJECT: &str = "Create project";
pub const OPTION_CREATE_PROJECT: &str = "Create project";
pub const OPTION_GLOBAL_ONLY: &str = "Global settings only";
pub const TITLE_CONFIG_ERROR: &str = "Invalid configuration";
pub const TITLE_CONFIG_CHANGED: &str = "Configuration changed";
pub const BODY_CONFIG_CHANGED: &str = "Another program changed a configuration file. Open sessions stay as they are. Load the configuration again?";
pub const OPTION_CLOSE: &str = "Close";
pub const OPTION_RELOAD: &str = "Reload";
pub const OPTION_IGNORE: &str = "Ignore";
pub const CONFIG_BLOCKED: &str =
    "The configuration has an error. Fix the file, then press Ctrl+W r.";
pub const CONFIG_RELOADED: &str = "Configuration loaded again.";
pub const NO_STORE: &str = "This session has no configuration files.";
pub const NO_PROJECT: &str = "This session has no project. Create a .sqln folder first.";
pub const PROJECT_CREATED: &str = "Project created.";
pub const SAVED: &str = "Saved.";
pub const NOTHING_TO_SAVE: &str = "This console has no text to save.";
pub const NO_PLACE: &str = "The console has no connection, so it has no place to save.";
pub const TITLE_UNSAVED: &str = "Text not saved";
pub const OPTION_CLOSE_UNSAVED: &str = "Close without saving";
pub const OPTION_CLOSE_DISCARD: &str = "Close and discard the text";
pub const WORKSPACE_UNREADABLE: &str =
    "SQLN cannot read workspace.json. The layout and the cursors start again.";
pub const AGE_NOW: &str = "just now";
pub const MARK_OPEN: &str = " (open)";
/// The node above the consoles of a connection in the navigator.
pub const NODE_CONSOLES: &str = "Consoles";
pub const NEW_CONSOLE_ROW: &str = "+ New console";
pub const NO_ORPHANS: &str = "No scripts without a connection.";
pub const TITLE_ORPHANS: &str = "Scripts without a connection";
pub const TITLE_ASSOCIATE: &str = "Choose a connection";
pub const TITLE_ASSOCIATE_CONFIRM: &str = "Associate script";
pub const OPTION_ASSOCIATE: &str = "Associate";
pub const ASSOCIATE_SAVE_FIRST: &str = "The script has a save problem. Resolve it, then try again.";
pub const TITLE_REMOVE_CONSOLE: &str = "Remove console";
pub const SCRIPT_OPEN: &str = "Close the console before you remove its script.";
pub const SCRIPT_BUSY: &str = "Another SQLN instance uses this script.";
pub const SCRIPT_REMOVED: &str = "Script removed.";
pub const TITLE_CONFLICTS: &str = "Scripts changed outside SQLN";
pub const STATE_CHANGED: &str = "changed";
pub const STATE_REMOVED: &str = "removed";
pub const CHOICE_NONE: &str = "choose";
pub const CHOICE_RELOAD: &str = "Reload";
pub const CHOICE_OVERWRITE: &str = "Overwrite";
pub const CHOICE_RECREATE: &str = "Recreate";
pub const CHOICE_CLOSE: &str = "Close";
pub const BUTTON_LATER: &str = "Decide later";
pub const CONFLICT_CHOOSE: &str = "Choose an action for each script.";

pub fn create_project(folder: &str) -> String {
    format!("SQLN did not find a .sqln folder. Create a project in {folder}?")
}

pub fn project_failed(message: &str) -> String {
    format!("Cannot create the project: {message}")
}

pub fn config_error(message: &str, file: &str) -> String {
    format!(
        "{message}\n\nFile: {file}\n\nSQLN does not connect to a database until the file is correct. Fix the file, then use Reload."
    )
}

pub fn save_failed(name: &str, message: &str) -> String {
    format!("Cannot save {name}: {message} The text stays in the editor.")
}

pub fn unsaved_failed(name: &str, message: &str) -> String {
    format!(
        "SQLN cannot save {name}: {message}\n\nClose the console and lose the text that no file holds?"
    )
}

pub fn unsaved_changed(name: &str) -> String {
    format!(
        "Another program changed the script of {name}. SQLN did not save the text.\n\nClose the console and lose the text that no file holds?"
    )
}

pub fn unsaved_removed(name: &str) -> String {
    format!(
        "Another program removed the script of {name}. SQLN did not save the text.\n\nClose the console and lose the text that no file holds?"
    )
}

pub fn age_minutes(minutes: u64) -> String {
    format!("{minutes} min ago")
}

pub fn age_hours(hours: u64) -> String {
    format!("{hours} h ago")
}

pub fn age_days(days: u64) -> String {
    format!("{days} d ago")
}

pub fn consoles_title(connection: &str, scope: &str) -> String {
    format!("Consoles of {connection} · {scope}")
}

/// Tells how to load a table tab of the last session. A key that the user removed
/// leaves the button.
pub fn table_pending(key: Option<&str>) -> String {
    format!("{} loads the rows.", key.unwrap_or(BUTTON_RELOAD))
}

pub fn script_in_use(name: &str) -> String {
    format!("{name} is open in another SQLN instance.")
}

pub fn script_failed(name: &str, message: &str) -> String {
    format!("Cannot use the script {name}: {message}")
}

pub fn remove_script(name: &str, folder: &str) -> String {
    format!(
        "Remove the script {name}? SQLN deletes its .sql file and its .json file in {folder}. This action cannot be undone."
    )
}

pub fn associate_same(name: &str, connection: &str, folder: &str) -> String {
    format!(
        "Associate {name} with {connection}?\n\nThe files stay in {folder}.\nSQLN does not run any SQL."
    )
}

pub fn associate_move(name: &str, connection: &str, scope: &str, folder: &str) -> String {
    format!(
        "Associate {name} with {connection}?\n\nThe connection has the {scope} scope. SQLN moves the files to {folder}.\nSQLN does not run any SQL."
    )
}

pub fn associated(connection: &str) -> String {
    format!("The script now uses {connection}. Run it again to execute.")
}
