//! Redis, Databricks, ClickHouse, libSQL, and DuckDB.

pub const FORM_REDIS_DATABASE: &str = "The Redis database must be a number.";
pub const FORM_REDIS_SCHEMAS: &str = "Each Redis database must be a number.";
pub const FORM_DATABRICKS_SOCKET: &str =
    "A Databricks connection needs the host name of the workspace, not a socket.";
pub const DATABRICKS_BAD_WAREHOUSE: &str =
    "The warehouse id must have only letters and digits. Copy it from the SQL warehouse page.";
pub const DATABRICKS_READ_ONLY: &str = "This connection is read only. Databricks keeps no session state, so SQLN refuses the write here.";
pub const DATABRICKS_NO_TRANSACTIONS: &str =
    "Databricks connections do not support transactions in SQLN. Run each statement alone.";
pub const DATABRICKS_NO_TOKEN: &str =
    "A Databricks connection needs a personal access token. Type the token in the password field.";
pub const DATABRICKS_REFUSED: &str =
    "Databricks refused the token. Make a new personal access token and try again.";
pub const DATABRICKS_NO_SUCH_WAREHOUSE: &str =
    "Databricks does not know this warehouse id. Copy the id from the SQL warehouse page.";
pub const DATABRICKS_CANCELED: &str = "Databricks canceled the statement.";
pub const DATABRICKS_CLOSED: &str =
    "Databricks closed the statement before SQLN read its result. Run it again.";
pub const DATABRICKS_BAD_ANSWER: &str = "Databricks sent an answer that SQLN cannot read.";
pub const DATABRICKS_UNSAFE_LINK: &str =
    "Databricks sent a result link without TLS. SQLN does not download it.";
pub fn databricks_status(status: u16) -> String {
    format!("Databricks answered with HTTP status {status}.")
}
pub fn databricks_chunk_too_large(bytes: u64, limit: usize) -> String {
    format!(
        "A result part of Databricks has {bytes} bytes, and the memory limit is {limit} bytes. Add LIMIT, or increase the memory limit."
    )
}
pub const DATABRICKS_NO_WAREHOUSE: &str =
    "A Databricks connection needs a warehouse id. Set warehouse=<id>.";
pub const CLICKHOUSE_NO_TRANSACTIONS: &str =
    "ClickHouse connections do not support transactions in SQLN. Run each statement alone.";
pub const CLICKHOUSE_REFUSED: &str =
    "ClickHouse refused the user or the password. Check them and try again.";
pub const CLICKHOUSE_CANCELED: &str = "ClickHouse canceled the statement.";
pub const CLICKHOUSE_BAD_ANSWER: &str = "ClickHouse sent an answer that SQLN cannot read.";
pub fn clickhouse_status(status: u16) -> String {
    format!("ClickHouse answered with HTTP status {status}.")
}
pub fn form_http_socket(source: &str) -> String {
    format!("A {source} connection needs a host name, not a socket.")
}
pub const LIBSQL_NO_DATABASE: &str =
    "A libSQL connection has no database. Each database has its own URL.";
pub const LIBSQL_READ_ONLY: &str = "This connection is read only. The libSQL server has no read-only session, so SQLN refuses the write here.";
pub const LIBSQL_REFUSED: &str =
    "The libSQL server refused the token. Make a new token and try again.";
pub const LIBSQL_BAD_ANSWER: &str = "The libSQL server sent an answer that SQLN cannot read.";
pub const LIBSQL_CANCELED: &str = "SQLN stopped the wait for the libSQL server. The server can complete the statement, and SQLN closed the session.";
pub const LIBSQL_STREAM_CLOSED: &str = "The libSQL server closed the session, because it was idle for too long. An open transaction ended without a commit. Run the statement again.";
pub fn libsql_status(status: u16) -> String {
    format!("The libSQL server answered with HTTP status {status}.")
}
pub fn answer_too_large(source: &str, limit: usize) -> String {
    format!(
        "An answer part of {source} has more than {limit} bytes, the memory limit. Add LIMIT, or increase the memory limit."
    )
}
pub const TLS_FALLBACK_NOTICE: &str =
    "The server does not support TLS. SQLN connected without TLS.";
pub const REDIS_SERVER_HAS_NO_TLS: &str = "The server answered without TLS.";
pub const REDIS_SERVER_NEEDS_TLS: &str = "The server did not answer without TLS.";
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
/// DuckDB fixes its access mode when the file opens, so a session refuses a write here.
pub const DUCKDB_READ_ONLY: &str = "This connection is read only. A DuckDB connection cannot change its access mode, so SQLN refuses the write here.";
pub const CONTINUOUS_NOT_LAST: &str = "A command that does not end must be the last command of the run. Move it to the end or run it alone.";
pub const CONTINUOUS_CONSOLE_ONLY: &str = "A command that does not end runs only in a console. Run it in a console and use Stop to end it.";
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
pub const REDIS_COLUMN_STREAM: &str = "stream";
pub const REDIS_COLUMN_RECEIVED_AT: &str = "received_at";
pub const REDIS_COLUMN_KIND: &str = "kind";
pub const REDIS_COLUMN_PATTERN: &str = "pattern";
pub const REDIS_COLUMN_CHANNEL: &str = "channel";
pub const REDIS_COLUMN_MESSAGE: &str = "message";
pub const REDIS_COLUMN_TIME: &str = "time";
pub const REDIS_COLUMN_DATABASE: &str = "database";
pub const REDIS_COLUMN_CLIENT: &str = "client";
pub const REDIS_COLUMN_COMMAND: &str = "command";
pub const REDIS_SYNC_STATUS: &str = "status";
pub const REDIS_SYNC_RDB: &str = "rdb";
pub const REDIS_SYNC_COMMAND: &str = "command";
pub const REDIS_LIVE_CLOSED: &str =
    "The server closed the connection of the stream. Run the command again.";
pub const REDIS_LIVE_BAD: &str =
    "The server sent a reply that SQLN cannot read. Run the command again.";

pub fn redis_subscribed(channel: &str, count: &str) -> String {
    format!("Subscribed to {channel}. Subscriptions: {count}.")
}

pub fn redis_unknown_type(kind: &str) -> String {
    format!("SQLN cannot show a value of type {kind}. Use the console.")
}
