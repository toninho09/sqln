//! All interface text. This file keeps the text in one place for a later translation.

use crate::input::{Action, SuggestionKeys};

/// One command of the hint bar: a key and its effect. A note has an empty key.
pub type HintPart = (&'static str, &'static str);

pub const CLIPBOARD_UNAVAILABLE: &str = "Clipboard unavailable. Paste with the terminal.";
pub const COPY_SENT_TO_TERMINAL: &str = "Copy request sent to the terminal. No confirmation.";
pub const COPY_FAILED: &str = "Copy failed.";

pub const FIND_LABEL: &str = "Find:    ";
pub const REPLACE_LABEL: &str = "Replace: ";
pub const NO_MATCHES: &str = "no matches";
pub const UNDO_TOO_LARGE: &str =
    "This edit is too large for the undo history. It has no undo step.";

pub const HINT_FILTER: &[HintPart] = &[
    ("Enter", "apply"),
    ("Tab", "accept or next field"),
    ("Esc", "grid"),
    ("Ctrl+Space", "suggestions"),
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
pub const HINT_SETTINGS: &[HintPart] = &[
    ("Tab", "next"),
    ("Left/Right", "change"),
    ("", "An empty field removes the value"),
    ("Esc", "cancel"),
];
pub const HINT_SETTINGS_SECTIONS: &[HintPart] =
    &[("Up/Down", "section"), ("Tab", "next"), ("Esc", "cancel")];
pub const HINT_CTRL_SPACE: &[HintPart] = &[("Ctrl+Space", "suggestions"), ("Ctrl+F", "find")];
pub const HINT_EDITOR_STAYS: &[HintPart] = &[("", "The editor stays available")];
pub const HINT_CONFLICTS: &[HintPart] = &[
    ("Up/Down", "move"),
    ("Left/Right", "choose"),
    ("Enter", "next"),
    ("Esc", "decide later"),
];
pub const HINT_CONFIG_ERROR: &[HintPart] = &[
    ("", "Configuration error. Fix the file, then"),
    ("Ctrl+W r", "loads it again"),
];
pub const HINT_FORM: &[HintPart] = &[
    ("Tab", "next"),
    ("Left/Right", "change"),
    ("Enter", "next or press"),
    ("Esc", "cancel"),
];
pub const HINT_DIALOG: &[HintPart] = &[
    ("Tab", "next option"),
    ("Enter", "confirm"),
    ("Esc", "cancel"),
];
pub const HINT_OVERLAY: &[HintPart] =
    &[("Up/Down", "scroll"), ("Ctrl+C", "copy"), ("Esc", "close")];

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

pub const HINT_PREFIX: &[HintPart] = &[("Ctrl+W", "…"), ("Esc", "cancel")];
pub const HINT_PALETTE: &[HintPart] = &[
    ("", "Write to filter"),
    ("Up/Down", "move"),
    ("Enter", "run"),
    ("Esc", "cancel"),
];

pub const TITLE_EXPLORER: &str = "EXPLORER";
pub const FORM_GENERAL: &str = "GENERAL";
pub const FORM_ADDRESS: &str = "ADDRESS";
pub const FORM_CREDENTIALS: &str = "CREDENTIALS";
pub const FORM_SECURITY: &str = "SECURITY";
pub const FORM_OPTIONS: &str = "OPTIONS";
pub const TITLE_PALETTE: &str = "Commands";
pub const TITLE_PREFIX: &str = "Ctrl+W";
pub const NO_COMMANDS: &str = "No command matches";

pub const STATUS_EDITOR: &str = "EDITOR";
pub const STATUS_NORMAL: &str = "NORMAL";
pub const STATUS_INSERT: &str = "INSERT";
pub const STATUS_VISUAL: &str = "VISUAL";
pub const STATUS_RESULTS: &str = "RESULTS";
pub const STATUS_EXPLORER: &str = "EXPLORER";
pub const STATUS_FILTER: &str = "FILTER";
pub const STATUS_TABLE: &str = "TABLE";

pub const GROUP_PANELS: &str = "Panels";
pub const GROUP_TABS: &str = "Tabs";
pub const GROUP_RUN: &str = "Run";
pub const GROUP_TRANSACTION: &str = "Transaction";
pub const GROUP_CONNECTION: &str = "Connection";
pub const GROUP_APPLICATION: &str = "Application";
pub const MENU_FOCUS: &str = "Move the focus";
pub const MENU_NAVIGATOR: &str = "Toggle the explorer";
pub const MENU_NEXT_TAB: &str = "Next tab";
pub const MENU_PREVIOUS_TAB: &str = "Previous tab";
pub const MENU_RENAME: &str = "Rename the tab";
pub const MENU_CLOSE_TAB: &str = "Close the tab";
pub const MENU_RUN: &str = "Run the statement";
pub const MENU_SCRIPT: &str = "Run the script";
pub const MENU_STOP: &str = "Stop";
pub const MENU_MODE: &str = "Auto or manual";
pub const MENU_COMMIT: &str = "Commit";
pub const MENU_ROLLBACK: &str = "Rollback";
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
pub const ADD_CONNECTION_ROW: &str = "Press a to add one";
pub const NOTE_LOADING: &str = "(loading)";
pub const NOTE_FAILED: &str = "(failed)";
pub const MARK_VIEW: &str = "(view)";
pub const NO_SCHEMA_CHOICE: &str = "This database has no schema or database choice.";
pub const SCHEMA_BUSY: &str = "End the execution and the transaction before the change.";
pub const NO_DEFINITION: &str = "This connection has no definition to edit.";
pub const NO_SELECTED_CONNECTION: &str = "Select a connection.";
pub const DISCONNECTED: &str = "Disconnected.";
pub const SESSION_IN_USE: &str = "A session is still in use. It closes at its end.";
pub const NOT_CONNECTED: &str = "This connection has no open session.";
pub const FORM_TESTING: &str = "Testing the connection.";
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
pub const MYSQL_CLIENT_KEY: &str =
    "MySQL needs an RSA client key in PKCS#1 form (BEGIN RSA PRIVATE KEY).";
pub const FIELD_KIND: &str = "Type";
pub const FIELD_NAME: &str = "Name";
pub const FIELD_SCOPE: &str = "Scope";
pub const FIELD_ENTRY: &str = "Entry";
pub const FIELD_URI: &str = "URI";
pub const FIELD_HOST: &str = "Host or socket";
pub const FIELD_PORT: &str = "Port";
pub const FIELD_DATABASE: &str = "Database";
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
pub const FIELD_CONNECT: &str = "Connect limit (seconds)";
pub const FIELD_SQL_TIMEOUT: &str = "SQL limit (s, 0 = none)";
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
pub const NO_OTHER_CONNECTION: &str = "No other connection is available.";
pub const SETTINGS_SAVED: &str = "The settings are saved.";
pub const SECTION_GENERAL: &str = "General";
pub const SECTION_EDITOR: &str = "Editor";
pub const SECTION_RESULTS: &str = "Results";
pub const SECTION_APPEARANCE: &str = "Appearance";
pub const FIELD_KEYBOARD: &str = "Keyboard";
pub const FIELD_REOPEN_TABS: &str = "Reopen tabs";
pub const FIELD_SUGGESTION_KEYS: &str = "Autocomplete";
pub const FIELD_PAGE_SIZE: &str = "Page size";
pub const FIELD_ROW_CAP: &str = "Row cap";
pub const FIELD_BUDGET: &str = "Data (MiB)";
pub const FIELD_THEME: &str = "Default theme";
pub const FIELD_CONNECTION_THEME: &str = "Theme";
pub const FIELD_CONNECTION_ICON: &str = "Icon";

pub fn settings_number(name: &str) -> String {
    format!("{name} needs a number of 1 or more.")
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
         The results, the messages, and the schema choice of the console end."
    )
}
pub const TITLE_SCHEMA: &str = "Schema";
pub const TITLE_DATABASE: &str = "Database";
pub const SCHEMA_DEFAULT: &str = "(default of the connection)";
pub const SCHEMA_LOADING: &str = "Reading the catalog.";
pub const CONTROL_RUN: &str = "Run";
pub const CONTROL_SCRIPT: &str = "Script";
pub const CONTROL_STOP: &str = "Stop";
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
pub const EXECUTION_ACTIVE: &str = "The console has an execution in progress.";
pub const NO_TRANSACTION: &str = "No transaction is open.";
pub const RESULT_READING: &str = "This result receives rows. Ctrl+W x stops the script.";
pub const COPIED: &str = "Copied.";
pub const NO_MESSAGES: &str = "No messages.";
pub const MESSAGES_TAB: &str = "Messages";

pub const LABEL_WHERE: &str = "WHERE";
pub const LABEL_ORDER_BY: &str = "ORDER BY";
pub const BUTTON_APPLY: &str = "Apply";
pub const BUTTON_CLEAR: &str = "Clear";
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
pub const TABLE_STOPPING: &str = "stopping";
pub const TABLE_BUDGET: &str = "data budget reached; close a tab to free space";
pub const TABLE_VALUE_TOO_LARGE: &str = "a value is larger than the data budget";
pub const TABLE_NO_ORDER: &str = "No ORDER BY: pages can repeat or skip rows.";
pub const BUTTON_RELOAD: &str = "Reload";
pub const NOTE_PARTIAL_KEYS: &str = "(partial list)";
pub const REGION_TABLE: &str = "Table";
pub const REGION_FILTER: &str = "Filter";

pub const MODE_AUTO: &str = "Auto";
pub const MODE_MANUAL: &str = "Manual";
pub const TRANSACTION_NONE: &str = "No transaction";
pub const TRANSACTION_OPEN: &str = "Transaction open";
pub const TRANSACTION_FAILED: &str = "Transaction failed";
pub const TRANSACTION_UNKNOWN: &str = "Transaction unknown";
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

pub const FILTER_KEYS: &str = "\
Enter                 Apply the fields
Tab                   Accept the suggestion, or the next field
Shift+Tab             Next field
Ctrl+Space            Suggestions
Esc                   Close the list, or go to the grid";

pub const EDITOR_KEYS: &str = "\
Ctrl+Space            Suggestions
Ctrl+F                Find and replace
Ctrl+Z, Ctrl+Y        Undo, redo
Tab                   Accept the suggestion
Esc                   Close the list";

pub fn action_text(action: Action) -> &'static str {
    match action {
        Action::Run => "Run the selection or the statement",
        Action::RunScript => "Run the script",
        Action::Stop => "Stop the script",
        Action::ToggleMode => "Autocommit or manual",
        Action::Commit => "Commit",
        Action::Rollback => "Rollback",
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
        Action::NavigatorScripts => "Scripts without a connection",
        Action::NavigatorDeletePassword => "Delete the saved password",
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
        Action::GridFind => "Find in the loaded rows",
        Action::GridNarrow => "Make the column narrow",
        Action::GridWiden => "Make the column wide",
        Action::GridAutoWidth => "Use the automatic column width",
        Action::GridCloseResult => "Close the result",
        Action::GridPreviousResult => "Previous result",
        Action::GridNextResult => "Next result",
        Action::GridFocusFilter => "Edit the WHERE filter",
        Action::GridFocusOrder => "Edit the ORDER BY text",
        Action::GridSortColumn => "Sort by the column",
        Action::GridLoadMore => "Load more rows",
    }
}

/// Writes the name of an action in the command list.
pub fn command_text(action: Action) -> &'static str {
    match action {
        Action::Help => "Show the keys",
        action => action_text(action),
    }
}

pub fn keys_title(mode: &str, region: &str) -> String {
    format!(" {TITLE_KEYS} · {mode} · {region} ")
}

/// Writes one row of the help screen.
pub fn keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", action_text(action))
}

/// Writes a row of the list of keys of a table. The result keys move between the
/// pages of the table.
pub fn table_keys_row(keys: &str, action: Action) -> String {
    match action {
        Action::GridPreviousResult => format!("{keys:<24}Previous page of rows"),
        Action::GridNextResult => format!("{keys:<24}Next page of rows"),
        _ => keys_row(keys, action),
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

pub fn rows_loaded(rows: usize) -> String {
    format!("{} loaded", plural(rows, "row"))
}

pub fn table_label(name: &str, kind: &str) -> String {
    format!("{name} ({kind})")
}

pub fn table_title(connection: &str, schema: &str, relation: &str, kind: &str) -> String {
    format!("{connection} · {schema}.{relation} · {kind}")
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

pub const VALUE_TEXT: &str = "text";
pub const VALUE_NUMBER: &str = "number";
pub const VALUE_BINARY: &str = "binary";
pub const VALUE_NULL: &str = "NULL";

pub const HELP: &str = "\
sqln: terminal database client

Usage: sqln [options]

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
    format!("{} loads the rows.", key.unwrap_or(BUTTON_APPLY))
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
