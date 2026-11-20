//! The connection form, credentials, trust, and connection changes.

use super::{HintPart, count, plural};

pub const FORM_GENERAL: &str = "General";
pub const FORM_ADDRESS: &str = "Address";
pub const FORM_CREDENTIALS: &str = "Credentials";
pub const FORM_SECURITY: &str = "Security";
pub const FORM_OPTIONS: &str = "Options";
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
pub const FORM_URI_KIND: &str = "The connection text is for another database type.";
pub const FORM_POOL_ZERO: &str = "The pool limit must be 1 or more.";
pub const FORM_CLIENT_PAIR: &str = "A client certificate needs its file and its key.";
pub const FORM_TLS_FILES: &str = "TLS files need a connection with TLS.";
pub const FORM_CONNECTED: &str =
    "This connection is connected. Disconnect it to change access, TLS, or the pool.";
pub const FIELD_KIND: &str = "Type";
pub const FIELD_NAME: &str = "Name";
pub const FIELD_FOLDER: &str = "Folder";
pub const FIELD_SCOPE: &str = "Scope";
pub const FIELD_ENTRY: &str = "Entry";
pub const FIELD_URI: &str = "URI";
pub const FIELD_HOST: &str = "Host or socket";
pub const FIELD_PORT: &str = "Port";
pub const FIELD_DATABASE: &str = "Database";
pub const FIELD_CATALOG: &str = "Catalog";
pub const FIELD_WAREHOUSE: &str = "Warehouse";
pub const FIELD_TOKEN: &str = "Token";
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
pub const FIELD_SQL_TIMEOUT: &str = "Statement (s, 0 = none)";
pub const FIELD_IDLE_TIMEOUT: &str = "Idle limit (s, 0 = none)";
pub const BUTTON_TEST: &str = "Test connection";
pub const BUTTON_SAVE: &str = "Save";
pub const BUTTON_DISCONNECT: &str = "Disconnect";
pub const VALUE_POSTGRES: &str = "PostgreSQL";
pub const VALUE_MYSQL: &str = "MySQL";
pub const VALUE_REDIS: &str = "Redis/Valkey";
pub const VALUE_KAFKA: &str = "Kafka";
pub const VALUE_DATABRICKS: &str = "Databricks";
pub const VALUE_SQLITE: &str = "SQLite";
pub const VALUE_DUCKDB: &str = "DuckDB";
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
pub const TITLE_CHANGE_CONNECTION: &str = "Change connection";
pub const OPTION_CHANGE: &str = "Change connection";
pub const CONNECTION_CHANGE_BUSY: &str = "End the execution and the transaction before the change.";
pub const CONNECTION_CHANGE_PENDING: &str =
    "The connection change is in progress. Try again after it completes.";
pub const NO_OTHER_CONNECTION: &str = "No other connection is available.";

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

pub fn value_too_large(rows: usize) -> String {
    format!(
        "{} read. A value is larger than the data budget, so the read stopped.",
        plural(rows, "row")
    )
}

pub fn form_needs_number(label: &str) -> String {
    format!("{label} needs a number.")
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

pub fn value_title(column: &str, row: usize) -> String {
    format!(" {column} · row {} ", count(row + 1))
}

pub fn value_footer(chars: usize, kind: &str) -> String {
    format!(" {} · {kind} ", plural(chars, "character"))
}

pub const VALUE_TEXT: &str = "text";
pub const VALUE_JSON: &str = "JSON";
pub const VALUE_JSON_ORIGINAL: &str = "JSON, original";
pub const VALUE_NUMBER: &str = "number";
pub const VALUE_BINARY: &str = "binary";
pub const VALUE_NULL: &str = "NULL";
