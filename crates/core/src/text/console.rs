//! Consoles: runs, transactions, phases, syntax, parameters, and schemas.

pub const TITLE_RENAME: &str = "Rename console";
pub const NAME_EMPTY: &str = "The console needs a name.";
pub const NAME_TAKEN: &str = "Another console of this connection has this name.";
pub const CONSOLE_RENAMED: &str = "The console has a new name.";
pub const TITLE_SCHEMA: &str = "Schema";
pub const TITLE_DATABASE: &str = "Database";
pub const SCHEMA_DEFAULT: &str = "(default of the connection)";
pub const SCHEMA_LOADING: &str = "Reading the catalog.";
pub const LABEL_SCHEMA: &str = "Schema";
pub const LABEL_DATABASE: &str = "Database";
pub const DATABASE_LABEL: &str = "Database";
pub const NO_DATABASE_LABEL: &str = "No database";
pub const CURSOR_OUTSIDE: &str = "The cursor is outside a statement.";
pub const NO_STATEMENT: &str = "The text has no statement to run.";
pub const NO_SUBQUERY: &str =
    "No complete SELECT or WITH subquery at the cursor. Move the cursor inside a subquery.";
pub const SUBQUERY_UNAVAILABLE: &str = "Focus an idle SQL console to run a subquery.";

pub fn subquery_question(line: usize, sql: &str, warning: Option<&str>) -> String {
    let warning = warning
        .map(|message| format!("{message}\n\n"))
        .unwrap_or_default();
    format!(
        "{warning}Run only the subquery at line {line}?\n\n{sql}\n\nOuter aliases and CTE dependencies are not included."
    )
}
pub const EXECUTION_ACTIVE: &str = "The console has an execution in progress.";
pub const NO_TRANSACTION: &str = "No transaction is open.";
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
pub const PHASE_LIVE: &str = "Live";
pub const PHASE_STOPPING: &str = "Stopping";

pub const COPY_REFUSED: &str =
    "SQLN does not support COPY with STDIN or STDOUT. Use a file on the server.";
pub const LIVE_STOPPED: &str =
    "The stream stopped at the user request. The grid keeps the rows that it received.";
pub const STOPPED_AFTER_LAST: &str =
    "The execution stopped. The last statement ended before the stop request.";
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
pub const TITLE_TRANSACTION_ENDED: &str = "Transaction ended";
pub const BODY_RUNNING: &str =
    "Stop the execution and close? The statement in progress must end first.";
pub const BODY_TRANSACTION_CLOSE: &str =
    "This console has an open transaction. Choose an action before the close.";
pub const BODY_TRANSACTION_MODE: &str =
    "This console has an open transaction. Choose an action before the change to autocommit.";
pub const BODY_NO_ANSWER: &str = "The statement did not answer. If you close the connection, you lose the changes that are not committed.";
pub const BODY_UNKNOWN: &str = "The connection closed with an open transaction. The next statement can run outside that transaction.";
pub const OPTION_CREATE: &str = "Create file";
pub const OPTION_STOP_CLOSE: &str = "Stop and close";
pub const OPTION_COMMIT_CLOSE: &str = "Commit and close";
pub const OPTION_ROLLBACK_CLOSE: &str = "Rollback and close";
pub const OPTION_COMMIT_AUTO: &str = "Commit and use autocommit";
pub const OPTION_ROLLBACK_AUTO: &str = "Rollback and use autocommit";
pub const OPTION_KEEP_WAITING: &str = "Keep waiting";
pub const OPTION_CLOSE_CONNECTION: &str = "Close the connection";

pub const SYNTAX_TITLE: &str = "SQL syntax warnings";
pub const SYNTAX_CLEAN: &str = "No local syntax warnings. The database can still reject this SQL.";
pub const SYNTAX_WAITING: &str = "Syntax check pending. Wait after the last edit.";
pub const SYNTAX_UNAVAILABLE: &str =
    "Syntax warnings need a SQL console. Open a PostgreSQL, MySQL, SQLite, or DuckDB console.";
pub const SYNTAX_LIMITATION: &str = "Local parser warnings can include unsupported database syntax. Review the SQL. Run remains available.";

pub const PARAMETERS_TITLE: &str = "Parameters";
pub const PARAMETERS_TOGGLE: &str = "Collapse or expand parameters";
pub const PARAMETER_SET: &str = "Set";
pub const PARAMETER_UNSET: &str = "Unset";
pub const PARAMETER_TYPES: [&str; 4] = ["Text", "Number", "Boolean", "NULL"];

pub fn console_name(number: usize) -> String {
    format!("console-{number}")
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
