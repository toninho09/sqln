//! All interface text. This file keeps the text in one place for a later translation.

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
pub const HINT_PREFIX: &str = "e run · s script · x stop · m mode · C commit · R rollback · c close · ? keys";
pub const HINT_DIALOG: &str = "Tab next option · Enter confirm · Esc cancel";
pub const HINT_OVERLAY: &str = "Up/Down scroll · Ctrl+C copy · Esc close";

pub const SMALL_TERMINAL: &str = "Small terminal";
pub const MINIMUM_SIZE: &str = "Minimum size";

pub const NO_CONNECTION: &str = "No database. Start with --sqlite <file> or --url <text>.";
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

pub const KEYS_SCREEN: &str = "\
F5 or Ctrl+Enter      Run the selection or the statement
Shift+F5, Shift+Enter Run the script
Ctrl+W e, Ctrl+W s    Run, run the script
Ctrl+W x              Stop the script
Ctrl+W m              Autocommit or manual
Ctrl+W C, Ctrl+W R    Commit, rollback
Ctrl+W j, Ctrl+W k    Results, editor
Ctrl+W c              Close
Ctrl+Space            Suggestions
Ctrl+F                Find and replace
Ctrl+Z, Ctrl+Y        Undo, redo
Enter in the grid     Open the value
Ctrl+C in the grid    Copy as TSV
Ctrl+F in the grid    Find in the loaded rows (Vim: /)
< and > in the grid   Change the column width
= in the grid         Use the automatic column width
x in the grid         Close the result
[ and ] in the grid   Previous and next result";

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
