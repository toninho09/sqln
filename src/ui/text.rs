//! All interface text. This file keeps the text in one place for a later translation.

use crate::input::Action;

pub const CLIPBOARD_UNAVAILABLE: &str = "Clipboard unavailable. Paste with the terminal.";
pub const COPY_SENT_TO_TERMINAL: &str = "Copy request sent to the terminal. No confirmation.";
pub const COPY_FAILED: &str = "Copy failed.";

pub const FIND_LABEL: &str = "Find:    ";
pub const REPLACE_LABEL: &str = "Replace: ";
pub const NO_MATCHES: &str = "no matches";

pub const HINT_EDITOR: &str = "F5 run · Shift+F5 script · Ctrl+Space suggestions · Ctrl+F find · Ctrl+W ? keys";
pub const HINT_RUNNING: &str = "Ctrl+W x stop · The editor stays available";
pub const HINT_GRID: &str = "Ctrl+W k editor · Ctrl+C copy · Enter value · Ctrl+F find · [ ] tabs · F1 keys";
pub const HINT_GRID_SEARCH: &str = "Enter next · Up/Down move · Esc close";
pub const HINT_LIST: &str = "Tab accept · Up/Down move · Enter new line · Esc close list";
pub const HINT_SEARCH: &str = "Tab next field · Enter next or replace · Ctrl+A replace all · Esc close";
pub const HINT_NAVIGATOR: &str = "Enter open · n console · a add · e edit · r refresh · D disconnect · Del remove";
pub const HINT_PREFIX: &str = "hjkl panel · n p tab · b nav · d schema · e s x run · m C R tx · c q close · ?";
pub const HINT_PICKER: &str = "Up/Down move · Enter choose · Esc cancel";
pub const HINT_FORM: &str = "Tab next · Left/Right change · Enter next or press · Esc cancel";
pub const HINT_DIALOG: &str = "Tab next option · Enter confirm · Esc cancel";
pub const HINT_OVERLAY: &str = "Up/Down scroll · Ctrl+C copy · Esc close";

pub const SMALL_TERMINAL: &str = "Small terminal";
pub const MINIMUM_SIZE: &str = "Minimum size";

pub const NO_CONNECTION: &str = "No database. Use Ctrl+W h and n in the navigator, or a to add a connection.";
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
pub const FORM_FILE_MISSING: &str = "The file does not exist. Save asks before it creates the file.";
pub const FORM_NAME_TAKEN: &str = "Another connection has this name.";
pub const FORM_NEEDS_NAME: &str = "The connection needs a name.";
pub const FORM_NEEDS_HOST: &str = "The connection needs a host.";
pub const FORM_NEEDS_USER: &str = "The connection needs a user.";
pub const FORM_NEEDS_FILE: &str = "SQLite needs a file.";
pub const FORM_URI_KIND: &str = "The connection text is for another database type.";
pub const FORM_POOL_ZERO: &str = "The pool limit must be 1 or more.";
pub const FORM_CLIENT_PAIR: &str = "A client certificate needs its file and its key.";
pub const FORM_TLS_FILES: &str = "TLS files need a connection with TLS.";
pub const FORM_CONNECTED: &str = "This connection is connected. Disconnect it to change access, TLS, or the pool.";
pub const MYSQL_CLIENT_KEY: &str = "MySQL needs an RSA client key in PKCS#1 form (BEGIN RSA PRIVATE KEY).";
pub const FIELD_KIND: &str = "Type";
pub const FIELD_NAME: &str = "Name";
pub const FIELD_ENTRY: &str = "Entry";
pub const FIELD_URI: &str = "URI";
pub const FIELD_HOST: &str = "Host or socket";
pub const FIELD_PORT: &str = "Port";
pub const FIELD_DATABASE: &str = "Database";
pub const FIELD_USER: &str = "User";
pub const FIELD_PASSWORD: &str = "Password";
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
pub const BUTTON_TEST: &str = "Test connection";
pub const BUTTON_SAVE: &str = "Save";
pub const BUTTON_SAVE_NAME: &str = "Save name";
pub const BUTTON_DISCONNECT: &str = "Disconnect";
pub const BUTTON_CANCEL: &str = "Cancel";
pub const VALUE_POSTGRES: &str = "PostgreSQL";
pub const VALUE_MYSQL: &str = "MySQL";
pub const VALUE_SQLITE: &str = "SQLite";
pub const VALUE_FIELDS: &str = "Fields";
pub const VALUE_URI: &str = "URI";
pub const VALUE_TLS_VERIFY: &str = "Verify the certificate";
pub const VALUE_TLS_OFF: &str = "Off";
pub const VALUE_DEFAULT: &str = "Default";
pub const VALUE_ON: &str = "On";
pub const VALUE_OFF: &str = "Off";
pub const TITLE_NEW_CONNECTION: &str = "New connection";
pub const TITLE_REMOVE_CONNECTION: &str = "Remove connection";
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

pub const MODE_AUTO: &str = "Auto";
pub const MODE_MANUAL: &str = "Manual";
pub const TRANSACTION_NONE: &str = "No transaction";
pub const TRANSACTION_OPEN: &str = "Transaction open";
pub const TRANSACTION_FAILED: &str = "Transaction failed";
pub const TRANSACTION_UNKNOWN: &str = "Transaction unknown";
pub const PHASE_IDLE: &str = "Idle";
pub const PHASE_WAITING: &str = "Waiting";
pub const PHASE_RUNNING: &str = "Running";
pub const PHASE_STOPPING: &str = "Stopping";

pub const COPY_REFUSED: &str = "SQLN does not support COPY with STDIN or STDOUT. Use a file on the server.";
pub const STOPPED_AFTER_LAST: &str = "The execution stopped. The last statement ended before the stop request.";
pub const STOPPED_BEFORE_SEND: &str = "The statement stopped before it reached the database.";
pub const COMMIT_OF_FAILED_TRANSACTION: &str = "The transaction has an error, and a commit cannot save it. Use rollback.";
pub const NOT_COMMITTED: &str = "The transaction is not committed.";
pub const DATABASE_ENDED_TRANSACTION: &str = "The database ended the transaction.";
pub const CANCELED_BEFORE_START: &str = "The execution stopped before it started. No SQL was sent.";

pub const TITLE_CREATE: &str = "Create database";
pub const TITLE_RUNNING: &str = "Execution in progress";
pub const TITLE_TRANSACTION: &str = "Open transaction";
pub const TITLE_NO_ANSWER: &str = "No answer from the database";
pub const TITLE_UNKNOWN: &str = "Transaction state unknown";
pub const TITLE_KEYS: &str = "Keys";
pub const BODY_RUNNING: &str = "Stop the execution and close? The statement in progress must end first.";
pub const BODY_TRANSACTION_CLOSE: &str = "This console has an open transaction. Choose an action before the close.";
pub const BODY_TRANSACTION_MODE: &str = "This console has an open transaction. Choose an action before the change to autocommit.";
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
        Action::NavigatorOpen => "Open a console, or open the row",
        Action::NavigatorNewConsole => "New console",
        Action::NavigatorRefresh => "Read the catalog again",
        Action::NavigatorDisconnect => "Disconnect",
        Action::NavigatorEdit => "Edit the connection",
        Action::NavigatorDelete => "Remove the connection",
        Action::NavigatorAdd => "Add a connection",
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
    }
}

pub fn keys_title(mode: &str, region: &str) -> String {
    format!(" {TITLE_KEYS} · {mode} · {region} ")
}

/// Writes one row of the help screen.
pub fn keys_row(keys: &str, action: Action) -> String {
    format!("{keys:<24}{}", action_text(action))
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

pub fn rows_read(rows: usize) -> String {
    plural(rows, "row")
}

pub fn row_cap_reached(rows: usize) -> String {
    format!("{} read. Row cap reached. The result can have more rows.", plural(rows, "row"))
}

pub fn budget_reached(rows: usize) -> String {
    format!("{} read. Data budget reached. Increase the limit or run less data.", plural(rows, "row"))
}

pub fn value_too_large(rows: usize) -> String {
    format!("{} read. A value is larger than the data budget, so the read stopped.", plural(rows, "row"))
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
    format!("Stopped before statement {}. The database did not receive it.", index + 1)
}

pub fn statement_timeout(index: usize) -> String {
    format!("Statement {}: the time limit ended. The database did not confirm the result.", index + 1)
}

pub fn statement_unknown(index: usize) -> String {
    format!("Statement {}: the connection closed. The result is unknown.", index + 1)
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

pub fn remove_connection(name: &str) -> String {
    format!("Remove the definition of {name}? Its open consoles stay, and they cannot run.")
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
  -h, --help             Show this text.
  -V, --version          Show the version.

Prototype options:
  --sqlite <file>        Open a SQLite database.
  --url <text>           Open postgres://user@host/db or mysql://user@host/db.
                         SQLN_PASSWORD gives the password.
  --read-only            Ask the database to refuse changes.
  --row-cap <rows>       Set the row cap of each result. The default is 1000.
  --sql-timeout <sec>    Set the time limit of each statement. 0 removes it.
  --vim                  Use the Vim keys.
  --dialect <name>       Use postgres, mysql, or sqlite, without a database.
  --no-catalog           Start without the sample catalog.
";
