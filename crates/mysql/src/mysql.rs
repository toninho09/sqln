//! MySQL connector. The text protocol keeps the text of the database in each value.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use mysql_async::consts::{ColumnType, StatusFlags};
use mysql_async::prelude::Queryable;
use mysql_async::{ClientIdentity, Column, Conn, Error as DriverError, Opts, OptsBuilder, SslOpts};
use tokio::time::timeout;

use super::auto_tls::AutoTls;
use super::fix::network_fix;
use super::target::{Address, NetworkTarget, Tls};
use super::{
    Canceller, Capabilities, ConnectError, Connector, ErrorKind, ExecError, Fix, MAX_NOTICES,
    Namespace, ResetOutcome, Session, SessionContext, StatementEnd, TlsRefusal, TransactionCommand,
    TransactionState, describe, tls,
};
use crate::catalog::{
    Catalog, Columns, ForeignKey, KeyColumn, Relation, RelationKind, RelationRef, Schema,
    add_objects, group_foreign_keys,
};
use crate::grid::{Row, RowWriter, Value};
use crate::sink::{Flow, ResultSink};
use crate::sql::{Dialect, counts_rows, is_plain_read, quote_identifier};

/// `ER_QUERY_INTERRUPTED`: `KILL QUERY` stopped the statement.
const QUERY_INTERRUPTED: u16 = 1317;
/// The value of a binary column in the character set field of the protocol.
const BINARY_CHARSET: u16 = 63;
const TCP_KEEPALIVE: Duration = Duration::from_secs(30);
/// A statement with no effect. Its answer has the state of the transaction.
const STATUS_REQUEST: &str = "DO 0";
const CATALOG_SQL: &str = "\
    SELECT s.SCHEMA_NAME, t.TABLE_NAME, t.TABLE_TYPE \
    FROM information_schema.SCHEMATA s \
    LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME \
    WHERE s.SCHEMA_NAME NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys') \
    ORDER BY 1, 2";
/// The routines and triggers of each schema of `CATALOG_SQL`.
const OBJECTS_SQL: &str = "\
    SELECT ROUTINE_SCHEMA, ROUTINE_NAME, LOWER(ROUTINE_TYPE), '' \
    FROM information_schema.ROUTINES \
    WHERE ROUTINE_SCHEMA NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys') \
    UNION ALL \
    SELECT TRIGGER_SCHEMA, TRIGGER_NAME, 'trigger', EVENT_OBJECT_TABLE \
    FROM information_schema.TRIGGERS \
    WHERE TRIGGER_SCHEMA NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')";
const COLUMNS_SQL: &str = "\
    SELECT COLUMN_NAME FROM information_schema.COLUMNS \
    WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
const FOREIGN_KEYS_SQL: &str = "\
    SELECT CONSTRAINT_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, COLUMN_NAME, \
    REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
    WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
    ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION";
/// The keys of other tables that reach a relation, with the shape of `FOREIGN_KEYS_SQL`:
/// the column of the relation comes before the column of the other table.
const REFERENCING_KEYS_SQL: &str = "\
    SELECT CONSTRAINT_NAME, TABLE_SCHEMA, TABLE_NAME, REFERENCED_COLUMN_NAME, COLUMN_NAME \
    FROM information_schema.KEY_COLUMN_USAGE \
    WHERE REFERENCED_TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME = ? \
    ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION";
/// A session call that gets no answer in this time counts as lost.
const SESSION_LIMIT: Duration = Duration::from_secs(5);

fn ssl_options(tls_mode: &Tls) -> Result<Option<SslOpts>, ConnectError> {
    let Tls::Verify(files) = tls_mode else {
        return Ok(None);
    };
    let mut options = SslOpts::default();
    options = match &files.ca {
        Some(ca) => {
            tls::read_certificates(ca)?;
            options
                .with_root_certs(vec![ca.clone().into()])
                .with_disable_built_in_roots(true)
        }
        None => options.with_root_certs(
            tls::system_roots()
                .iter()
                .map(|root| root.to_vec().into())
                .collect(),
        ),
    };
    if let Some(client) = &files.client {
        let identity =
            ClientIdentity::new(client.certificate.clone().into(), client.key.clone().into());
        options = options.with_client_identity(Some(identity));
    }
    Ok(Some(options))
}

/// `ER_SECURE_TRANSPORT_REQUIRED`: the server refuses a connection without TLS.
const SECURE_TRANSPORT_REQUIRED: u16 = 3159;

/// Do not write this value in a log: its debug text shows the password.
fn connect_error(error: &mysql_async::Error, address: &Address) -> ConnectError {
    match error {
        // `1045` is a wrong password, and `1698` is an account that needs another one.
        mysql_async::Error::Server(server) if matches!(server.code, 1045 | 1698) => {
            ConnectError::refused_password(describe(error))
        }
        mysql_async::Error::Server(server) if server.code == SECURE_TRANSPORT_REQUIRED => {
            ConnectError::new(describe(error))
                .kind(error_kind(error))
                .tls_refused(TlsRefusal::ServerNeedsTls)
        }
        // The handshake of the server has no TLS flag.
        mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer) => {
            ConnectError::new(describe(error))
                .kind(error_kind(error))
                .tls_refused(TlsRefusal::ServerHasNoTls)
        }
        // `1049` is an unknown database. In a statement it is a name of the SQL instead.
        mysql_async::Error::Server(server) if server.code == 1049 => {
            ConnectError::new(describe(error))
                .kind(error_kind(error))
                .fix(Some(Fix::Database))
        }
        mysql_async::Error::Server(server) => ConnectError::new(describe(error))
            .kind(error_kind(error))
            .fix(code_fix(server.code)),
        _ => ConnectError::new(describe(error))
            .kind(error_kind(error))
            .fix(network_fix(error, address)),
    }
}

/// The configuration that can correct a server error code. See "Server Error Message
/// Reference" of MySQL. `1130` is a host that the server does not accept, and `1290`
/// is a server with `read_only`.
fn code_fix(code: u16) -> Option<Fix> {
    match code {
        1130 => Some(Fix::ServerRule),
        1040 | 1203 => Some(Fix::PoolLimit),
        1044 | 1142 | 1143 | 1227 => Some(Fix::Privilege),
        1792 => Some(Fix::ReadOnly),
        1290 => Some(Fix::ServerReadOnly),
        _ => None,
    }
}

fn driver_options(target: &NetworkTarget) -> Result<Opts, ConnectError> {
    let mut builder = OptsBuilder::default()
        .user(Some(target.user.clone()))
        .pass(
            target
                .password
                .as_ref()
                .map(|password| password.expose().to_owned()),
        )
        .db_name(target.database.clone())
        .tcp_keepalive(Some(TCP_KEEPALIVE))
        // The driver can move to a local socket that the user did not ask for.
        .prefer_socket(false)
        .ssl_opts(ssl_options(&target.tls)?);
    builder = match &target.address {
        Address::Tcp { host, port } => builder.ip_or_hostname(host.clone()).tcp_port(*port),
        Address::Socket(path) => builder.socket(Some(path.display().to_string())),
    };
    Ok(builder.into())
}

pub struct MySqlConnector {
    target: NetworkTarget,
    auto_tls: AutoTls,
}

impl MySqlConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self {
            target,
            auto_tls: AutoTls::default(),
        }
    }
}

/// Opens one session with the resolved TLS mode of `target`.
async fn connect_once(target: NetworkTarget) -> Result<MySqlSession, ConnectError> {
    let options = driver_options(&target)?;
    let conn = Conn::new(options.clone())
        .await
        .map_err(|error| connect_error(&error, &target.address))?;
    let canceller = Arc::new(MySqlCanceller {
        options,
        connection: conn.id(),
    });
    Ok(MySqlSession {
        conn: Some(conn),
        canceller,
        database: target.database,
        transaction: TransactionState::None,
        running: false,
        status_is_current: true,
        connect_notice: None,
        trace: None,
    })
}

#[async_trait]
impl Connector for MySqlConnector {
    fn dialect(&self) -> Dialect {
        Dialect::MySql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Database,
            transactions: true,
            parameters: true,
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let (mut session, notice) = self.auto_tls.connect(&self.target, connect_once).await?;
        session.connect_notice = notice;
        Ok(Box::new(session))
    }
}

struct MySqlCanceller {
    options: Opts,
    connection: u32,
}

impl MySqlCanceller {
    async fn kill_query(
        &self,
        trace: Option<crate::trace::TraceContext>,
    ) -> Result<(), DriverError> {
        let mut conn = Conn::new(self.options.clone()).await?;
        let sql = format!("KILL QUERY {}", self.connection);
        crate::trace::observe(trace, &sql, conn.query_drop(&sql)).await?;
        conn.disconnect().await
    }
}

#[async_trait]
impl Canceller for MySqlCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.cancel_traced(None).await
    }

    async fn cancel_traced(&self, trace: Option<crate::trace::TraceContext>) -> Result<(), String> {
        match timeout(SESSION_LIMIT, self.kill_query(trace)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(describe(&error)),
            Err(_) => Err("The cancel request got no answer.".to_owned()),
        }
    }
}

/// The name of the column type for the agent, such as `int unsigned` or `varchar`.
/// The protocol type has no length, so `varchar(20)` gives `varchar`.
fn column_type(column: &Column) -> String {
    use ColumnType::*;
    let binary = column.character_set() == BINARY_CHARSET;
    let name = match column.column_type() {
        MYSQL_TYPE_TINY => "tinyint",
        MYSQL_TYPE_SHORT => "smallint",
        MYSQL_TYPE_INT24 => "mediumint",
        MYSQL_TYPE_LONG => "int",
        MYSQL_TYPE_LONGLONG => "bigint",
        MYSQL_TYPE_FLOAT => "float",
        MYSQL_TYPE_DOUBLE => "double",
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => "decimal",
        MYSQL_TYPE_YEAR => "year",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "date",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "time",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "datetime",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "timestamp",
        MYSQL_TYPE_BIT => "bit",
        MYSQL_TYPE_JSON => "json",
        MYSQL_TYPE_ENUM => "enum",
        MYSQL_TYPE_SET => "set",
        MYSQL_TYPE_GEOMETRY => "geometry",
        MYSQL_TYPE_NULL => "null",
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING if binary => "varbinary",
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => "varchar",
        MYSQL_TYPE_STRING if binary => "binary",
        MYSQL_TYPE_STRING => "char",
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB
            if binary =>
        {
            "blob"
        }
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => {
            "text"
        }
        _ => "unknown",
    };
    if is_number(column)
        && column
            .flags()
            .contains(mysql_async::consts::ColumnFlags::UNSIGNED_FLAG)
    {
        format!("{name} unsigned")
    } else {
        name.to_owned()
    }
}

fn is_number(column: &Column) -> bool {
    matches!(
        column.column_type(),
        ColumnType::MYSQL_TYPE_TINY
            | ColumnType::MYSQL_TYPE_SHORT
            | ColumnType::MYSQL_TYPE_INT24
            | ColumnType::MYSQL_TYPE_LONG
            | ColumnType::MYSQL_TYPE_LONGLONG
            | ColumnType::MYSQL_TYPE_FLOAT
            | ColumnType::MYSQL_TYPE_DOUBLE
            | ColumnType::MYSQL_TYPE_DECIMAL
            | ColumnType::MYSQL_TYPE_NEWDECIMAL
            | ColumnType::MYSQL_TYPE_YEAR
    )
}

/// Numbers and dates also report the binary character set, so the type comes first.
fn is_binary(column: &Column) -> bool {
    column.character_set() == BINARY_CHARSET
        && matches!(
            column.column_type(),
            ColumnType::MYSQL_TYPE_BIT
                | ColumnType::MYSQL_TYPE_TINY_BLOB
                | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
                | ColumnType::MYSQL_TYPE_LONG_BLOB
                | ColumnType::MYSQL_TYPE_BLOB
                | ColumnType::MYSQL_TYPE_VAR_STRING
                | ColumnType::MYSQL_TYPE_STRING
                | ColumnType::MYSQL_TYPE_VARCHAR
                | ColumnType::MYSQL_TYPE_GEOMETRY
        )
}

fn write_cell(row: &mut RowWriter, cell: Option<&mysql_async::Value>, column: &Column) {
    let bytes = match cell {
        None | Some(mysql_async::Value::NULL) => return row.push(Value::Null),
        Some(mysql_async::Value::Bytes(bytes)) => bytes.as_slice(),
        Some(other) => {
            let text = other.as_sql(true);
            return row.push(if is_number(column) {
                Value::Number(&text)
            } else {
                Value::Text(text.trim_matches('\''))
            });
        }
    };
    row.push(match std::str::from_utf8(bytes) {
        Ok(text) if !is_binary(column) && is_number(column) => Value::Number(text),
        Ok(text) if !is_binary(column) => Value::Text(text),
        _ => Value::binary(bytes),
    });
}

fn row_values(cells: &mut RowWriter, row: &mysql_async::Row) -> Row {
    let columns = row.columns_ref();
    for (index, column) in columns.iter().enumerate() {
        write_cell(cells, row.as_ref(index), column);
    }
    cells.finish()
}

/// Finds the text after `near` in a syntax error of the database.
fn syntax_offset(message: &str, sql: &str) -> Option<usize> {
    let (_, after) = message.split_once("near '")?;
    let (near, line) = after.rsplit_once("' at line ")?;
    if near.is_empty() {
        return Some(sql.len());
    }
    let line: usize = line.trim().parse().ok()?;
    let start: usize = sql
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    sql.get(start..)?.find(near).map(|found| start + found)
}

fn exec_error(error: &DriverError, sql: &str) -> ExecError {
    match error {
        DriverError::Server(server) => ExecError::with_kind(
            code_kind(server.code),
            format!("{} ({})", server.message, server.code),
        )
        .at(syntax_offset(&server.message, sql))
        .fix(code_fix(server.code)),
        other => ExecError::with_kind(error_kind(other), describe(other)),
    }
}

/// The kind of an error of the driver. A failure of the socket ends the connection.
fn error_kind(error: &DriverError) -> ErrorKind {
    match error {
        DriverError::Server(server) => code_kind(server.code),
        DriverError::Io(_) => ErrorKind::Connection,
        DriverError::Driver(mysql_async::DriverError::ConnectionClosed) => ErrorKind::Connection,
        _ => ErrorKind::Other,
    }
}

/// The kind of a server error code. See "Server Error Message Reference" of MySQL.
fn code_kind(code: u16) -> ErrorKind {
    match code {
        // Parse error, unknown table, column, database, or function, and a wrong value.
        1054 | 1064 | 1146 | 1049 | 1305 | 1149 | 1630 => ErrorKind::Syntax,
        // A missing privilege, and a write in a read-only transaction or server.
        1044 | 1142 | 1143 | 1227 | 1370 | 1792 | 1290 => ErrorKind::Permission,
        1045 | 1698 | 1251 => ErrorKind::Auth,
        QUERY_INTERRUPTED => ErrorKind::Canceled,
        // `max_execution_time` and a lock wait.
        3024 | 1205 => ErrorKind::Timeout,
        // Too many connections, out of memory, and a full table or disk.
        1040 | 1041 | 1037 | 1038 | 1114 | 1021 => ErrorKind::Resource,
        1235 => ErrorKind::Unsupported,
        // The server went away or lost the connection during the query.
        2006 | 2013 | 1927 | 1053 => ErrorKind::Connection,
        _ => ErrorKind::Other,
    }
}

fn server_code(error: &DriverError) -> Option<u16> {
    match error {
        DriverError::Server(server) => Some(server.code),
        _ => None,
    }
}

#[derive(Default)]
struct Outcome {
    has_columns: bool,
    changed: Option<u64>,
}

struct MySqlSession {
    trace: Option<crate::trace::TraceContext>,
    /// `Drop` takes this value. All other code finds it here.
    conn: Option<Conn>,
    canceller: Arc<MySqlCanceller>,
    /// The database of the connection. A cleanup returns to it.
    database: Option<String>,
    transaction: TransactionState,
    /// True from the start of a statement to its end. A closed future leaves it set.
    running: bool,
    /// False after an error, because an error packet has no transaction state.
    status_is_current: bool,
    /// The notice of the connection, such as a fallback to no TLS. The first statement
    /// shows it in **Messages**.
    connect_notice: Option<&'static str>,
}

impl MySqlSession {
    /// The reset of the server keeps the current database, so a `USE` needs this step.
    /// MySQL cannot leave a database, so a session without a database closes.
    async fn clean(&mut self) -> bool {
        if !matches!(
            crate::trace::observe(
                self.trace.clone(),
                "COM_RESET_CONNECTION",
                self.conn().reset()
            )
            .await,
            Ok(true)
        ) {
            return false;
        }
        let current: Option<Option<String>> = match crate::trace::observe(
            self.trace.clone(),
            "SELECT DATABASE()",
            self.conn().query_first("SELECT DATABASE()"),
        )
        .await
        {
            Ok(current) => current,
            Err(_) => return false,
        };
        let current = current.flatten();
        if current == self.database {
            return true;
        }
        let Some(database) = &self.database else {
            return false;
        };
        let sql = format!("USE {}", quote_identifier(database, Dialect::MySql));
        crate::trace::observe(self.trace.clone(), &sql, self.conn().query_drop(&sql))
            .await
            .is_ok()
    }
}

/// The drop of a `Conn` starts a task that reads the statement to its end, with the
/// socket open. Without a Tokio runtime on the thread, the drop only closes the socket.
impl Drop for MySqlSession {
    fn drop(&mut self) {
        if self.running
            && let Some(conn) = self.conn.take()
        {
            let _ = thread::Builder::new().spawn(move || drop(conn));
        }
    }
}

impl MySqlSession {
    #[expect(
        clippy::expect_used,
        reason = "only the drop of the session takes the connection"
    )]
    fn conn(&mut self) -> &mut Conn {
        self.conn
            .as_mut()
            .expect("only the drop of the session takes the connection")
    }

    async fn read(&mut self, sql: &str, sink: &mut ResultSink) -> Result<Outcome, ExecError> {
        crate::trace::observe(self.trace.clone(), sql, self.read_untraced(sql, sink)).await
    }

    async fn read_untraced(
        &mut self,
        sql: &str,
        sink: &mut ResultSink,
    ) -> Result<Outcome, ExecError> {
        let free = self.transaction == TransactionState::None;
        let canceller = self.canceller();
        let result = self
            .conn()
            .query_iter(sql)
            .await
            .map_err(|error| exec_error(&error, sql))?;
        Self::consume(result, sql, sink, free, canceller).await
    }

    async fn consume<P: mysql_async::prelude::Protocol>(
        mut result: mysql_async::QueryResult<'_, '_, P>,
        sql: &str,
        sink: &mut ResultSink,
        free: bool,
        canceller: Arc<dyn Canceller>,
    ) -> Result<Outcome, ExecError> {
        let mut outcome = Outcome::default();
        let mut stopped_at_cap = false;
        loop {
            let columns = result.columns().unwrap_or_else(|| Vec::new().into());
            if !columns.is_empty() {
                outcome.has_columns = true;
                let names = columns
                    .iter()
                    .map(|column| column.name_str().into_owned())
                    .collect();
                let types = columns
                    .iter()
                    .map(|column| Some(column_type(column)))
                    .collect();
                sink.set_typed_columns(names, types).await;
            }
            let mut storing = true;
            let mut writer = RowWriter::default();
            loop {
                let row = match result.next().await {
                    Ok(Some(row)) => row,
                    Ok(None) => break,
                    // The cancellation of the row cap is not an error of the statement.
                    Err(error)
                        if stopped_at_cap && server_code(&error) == Some(QUERY_INTERRUPTED) =>
                    {
                        break;
                    }
                    Err(error) => return Err(exec_error(&error, sql)),
                };
                if !storing {
                    continue;
                }
                let flow = sink.push(row_values(&mut writer, &row)).await;
                storing = flow == Flow::Continue;
                if flow == Flow::Stop && free && is_plain_read(sql, Dialect::MySql) {
                    stopped_at_cap = canceller.cancel().await.is_ok();
                }
            }
            sink.close_result().await;
            if columns.is_empty() && counts_rows(sql, Dialect::MySql) {
                outcome.changed = Some(result.affected_rows());
            }
            if result.is_empty() {
                return Ok(outcome);
            }
        }
    }

    /// Reads the warnings of the last statement. The cleanup of the session removes them,
    /// so a later run cannot read them. This call makes `ROW_COUNT()` return -1 in the
    /// next statement. `SET TRANSACTION` has no warnings, so this call never follows it.
    async fn send_warnings(&mut self, sql: &str, sink: &mut ResultSink) {
        let count = usize::from(self.conn().get_warnings());
        let shows = sql
            .split_whitespace()
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("SHOW"));
        if count == 0 || shows {
            return;
        }
        let request = format!("SHOW WARNINGS LIMIT {MAX_NOTICES}");
        let rows = timeout(
            SESSION_LIMIT,
            crate::trace::observe(
                self.trace
                    .as_ref()
                    .map(crate::trace::TraceContext::internal),
                &request,
                self.conn().query(&request),
            ),
        )
        .await;
        let Ok(Ok(rows)) = rows else { return };
        let rows: Vec<(String, u32, String)> = rows;
        if rows.is_empty() {
            return;
        }
        let more = count.saturating_sub(rows.len());
        let texts = rows
            .into_iter()
            .map(|(level, code, message)| format!("{level}: {message} ({code})"))
            .collect();
        sink.notices(texts, more).await;
    }

    /// Reads the state of the last answer. A successful statement needs no call to
    /// the server, so `SHOW WARNINGS` and `ROW_COUNT()` work on the next statement,
    /// and `SET TRANSACTION` still applies to the next transaction.
    async fn observed_state(&mut self) -> TransactionState {
        if !self.status_is_current {
            match timeout(
                SESSION_LIMIT,
                crate::trace::observe(
                    self.trace
                        .as_ref()
                        .map(crate::trace::TraceContext::internal),
                    STATUS_REQUEST,
                    self.conn().query_drop(STATUS_REQUEST),
                ),
            )
            .await
            {
                Ok(Ok(())) => self.status_is_current = true,
                _ => return TransactionState::Unknown,
            }
        }
        let open = self.conn().last_ok_packet().is_some_and(|answer| {
            answer
                .status_flags()
                .contains(StatusFlags::SERVER_STATUS_IN_TRANS)
        });
        if open {
            TransactionState::Open
        } else {
            TransactionState::None
        }
    }
}

#[async_trait]
impl Session for MySqlSession {
    fn set_trace_context(&mut self, trace: Option<crate::trace::TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        crate::trace::canceller(self.canceller.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        if context.read_only {
            let sql = "SET SESSION TRANSACTION READ ONLY";
            crate::trace::observe(self.trace.clone(), sql, self.conn().query_drop(sql))
                .await
                .map_err(|error| exec_error(&error, sql))?;
        }
        if let Some(schema) = &context.schema {
            let sql = format!("USE {}", quote_identifier(schema, Dialect::MySql));
            crate::trace::observe(self.trace.clone(), &sql, self.conn().query_drop(&sql))
                .await
                .map_err(|error| exec_error(&error, &sql))?;
        }
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.running = true;
        let read = self.read(sql, &mut sink).await;
        self.finish_read(sql, read, sink).await
    }

    async fn run_parameters(
        &mut self,
        sql: &str,
        parameters: &[crate::sql::parameters::Parameter],
        mut sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        use crate::sql::parameters::Kind;
        let values: Vec<_> = parameters
            .iter()
            .map(|parameter| match parameter.kind {
                Kind::Null => mysql_async::Value::NULL,
                Kind::Boolean => mysql_async::Value::Int(i64::from(parameter.value == "true")),
                Kind::Text | Kind::Number => {
                    mysql_async::Value::Bytes(parameter.value.as_bytes().to_vec())
                }
            })
            .collect();
        self.running = true;
        let command = crate::sql::parameters::command(sql, parameters);
        let read = crate::trace::observe(self.trace.clone(), command, async {
            let free = self.transaction == TransactionState::None;
            let canceller = self.canceller();
            let result = self
                .conn()
                .exec_iter(sql, values)
                .await
                .map_err(|error| exec_error(&error, sql))?;
            Self::consume(result, sql, &mut sink, free, canceller).await
        })
        .await;
        self.finish_read(sql, read, sink).await
    }

    async fn transaction_state(&mut self) -> TransactionState {
        self.transaction = self.observed_state().await;
        self.transaction
    }

    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError> {
        let sql = match command {
            TransactionCommand::Begin => "BEGIN",
            TransactionCommand::Commit => "COMMIT",
            TransactionCommand::Rollback => "ROLLBACK",
        };
        let done =
            crate::trace::observe(self.trace.clone(), sql, self.conn().query_drop(sql)).await;
        self.status_is_current = done.is_ok();
        done.map_err(|error| exec_error(&error, sql))?;
        if command == TransactionCommand::Begin {
            self.transaction = TransactionState::Open;
        }
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        match timeout(SESSION_LIMIT, self.clean()).await {
            Ok(true) => ResetOutcome::Clean,
            _ => ResetOutcome::Discard,
        }
    }

    async fn is_alive(&mut self) -> bool {
        matches!(
            timeout(
                SESSION_LIMIT,
                crate::trace::observe(self.trace.clone(), "COM_PING", self.conn().ping())
            )
            .await,
            Ok(Ok(()))
        )
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let rows: Vec<(String, Option<String>, Option<String>)> = crate::trace::observe(
            self.trace.clone(),
            CATALOG_SQL,
            self.conn().query(CATALOG_SQL),
        )
        .await
        .map_err(|error| exec_error(&error, CATALOG_SQL))?;
        let mut schemas: Vec<Schema> = Vec::new();
        for (name, relation, kind) in rows {
            if schemas.last().is_none_or(|schema| schema.name != name) {
                schemas.push(Schema {
                    name,
                    ..Schema::default()
                });
            }
            if let (Some(schema), Some(relation)) = (schemas.last_mut(), relation) {
                schema.relations.push(Relation {
                    name: relation,
                    kind: match kind.as_deref() {
                        Some("BASE TABLE") | None => RelationKind::Table,
                        Some(_) => RelationKind::View,
                    },
                    columns: Columns::NotLoaded,
                    foreign_keys: Vec::new(),
                    referencing: Vec::new(),
                    notes: Vec::new(),
                });
            }
        }
        let objects: Vec<(String, String, String, String)> = crate::trace::observe(
            self.trace.clone(),
            OBJECTS_SQL,
            self.conn().query(OBJECTS_SQL),
        )
        .await
        .map_err(|error| exec_error(&error, OBJECTS_SQL))?;
        let objects = objects
            .into_iter()
            .map(|(schema, name, kind, detail)| [schema, name, kind, detail]);
        add_objects(&mut schemas, objects);
        let sql = "SELECT DATABASE()";
        let current: Option<Option<String>> =
            crate::trace::observe(self.trace.clone(), sql, self.conn().query_first(sql))
                .await
                .map_err(|error| exec_error(&error, sql))?;
        Ok(Catalog {
            schemas,
            current_schema: current.flatten(),
        })
    }

    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        let command =
            crate::limited::limited(super::relation_command(COLUMNS_SQL, schema, relation));
        crate::trace::observe(
            self.trace.clone(),
            command,
            self.conn().exec(COLUMNS_SQL, (schema, relation)),
        )
        .await
        .map_err(|error| exec_error(&error, COLUMNS_SQL))
    }

    async fn read_foreign_keys(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        self.read_keys(FOREIGN_KEYS_SQL, schema, relation).await
    }

    async fn read_referencing_keys(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        self.read_keys(REFERENCING_KEYS_SQL, schema, relation).await
    }
}

impl MySqlSession {
    /// Ends a read: the warnings of the server, the rows of the sink, and the state of
    /// a statement that just ended.
    async fn finish_read(
        &mut self,
        sql: &str,
        read: Result<Outcome, ExecError>,
        mut sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        if let Some(notice) = self.connect_notice.take() {
            sink.notices(vec![notice.to_owned()], 0).await;
        }
        if read.is_ok() {
            self.send_warnings(sql, &mut sink).await;
        }
        let (rows, limit) = sink.finish().await;
        self.running = false;
        self.status_is_current = read.is_ok();
        read.map(|outcome| StatementEnd {
            rows,
            has_columns: outcome.has_columns,
            changed: outcome.changed,
            limit,
        })
    }
}

impl MySqlSession {
    /// Reads the keys of a key query: the name, the schema and the name of the other
    /// table, the column of the relation, and the column of the other table.
    async fn read_keys(
        &mut self,
        sql: &str,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        let command = crate::limited::limited(super::relation_command(sql, schema, relation));
        let rows: Vec<(String, String, String, String, String)> = crate::trace::observe(
            self.trace.clone(),
            command,
            self.conn().exec(sql, (schema, relation)),
        )
        .await
        .map_err(|error| exec_error(&error, sql))?;
        let pairs = rows.into_iter().map(|row| KeyColumn {
            key: row.0,
            target: RelationRef {
                schema: row.1,
                relation: row.2,
            },
            column: row.3,
            target_column: row.4,
        });
        Ok(group_foreign_keys(pairs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{DatabricksOptions, Engine, KafkaOptions, Secret, TlsFiles};
    use mysql_common::io::ParseBuf;
    use mysql_common::proto::MyDeserialize;
    use std::path::PathBuf;

    /// The character set of the text columns: `utf8_general_ci`.
    const TEXT_CHARSET: u16 = 33;
    /// A certificate of a test CA. It signs nothing and holds no secret.
    const CA_CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\n\
        MIIBbjCCARSgAwIBAgIUb8NlWpCwO1tWQ2B5KyumFzpXWp8wCgYIKoZIzj0EAwIw\n\
        HDEaMBgGA1UEAwwRc3FsbiB1bml0IHRlc3QgQ0EwIBcNMjYxMTIwMDMyMDUxWhgP\n\
        MjEyNjEwMjcwMzIwNTFaMBwxGjAYBgNVBAMMEXNxbG4gdW5pdCB0ZXN0IENBMFkw\n\
        EwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEo3CNXM2EKLkCgHXBjT7xNYfE6UVnu62L\n\
        zlKpKr2x4PkYIkz+DiSX1KUOkkRy9I0ekfCKzJ57hjdY34iLQh12DqMyMDAwHQYD\n\
        VR0OBBYEFLrXFwhSEAa62s5GtGg2OayfhNXRMA8GA1UdEwEB/wQFMAMBAf8wCgYI\n\
        KoZIzj0EAwIDSAAwRQIgNMwTUbzR/U1koSrjzlQmBKh+ch6n7uxW9oe1x3KRTm8C\n\
        IQCTEA2r9zd+lhetaFrcG08XzsWSAyjHV78aNAUAugov7g==\n\
        -----END CERTIFICATE-----";

    /// Builds a target of the tests: a MySQL connection with the given address and TLS.
    fn target(address: Address, tls: Tls) -> NetworkTarget {
        NetworkTarget {
            engine: Engine::MySql,
            address,
            user: "user".to_owned(),
            password: Some(Secret::new("secret")),
            database: Some("shop".to_owned()),
            tls,
            kafka: KafkaOptions::default(),
            databricks: DatabricksOptions::default(),
        }
    }

    /// A server error of the tests, as the server sends it.
    fn server_error(code: u16, message: &str) -> DriverError {
        DriverError::Server(mysql_async::ServerError {
            code,
            state: "42000".to_owned(),
            message: message.to_owned(),
        })
    }

    /// A failure of the socket of the connection.
    fn io_error() -> DriverError {
        DriverError::Io(mysql_async::IoError::Io(std::io::Error::other("closed")))
    }

    /// Builds a column of a result from the bytes of its definition, which the server
    /// sends the same way since MySQL 4.1: the catalog, five strings, and fixed fields.
    fn column(kind: ColumnType, charset: u16) -> Column {
        let mut bytes = Vec::new();
        for part in ["def", "", "", "", "c", "c"] {
            bytes.push(u8::try_from(part.len()).unwrap());
            bytes.extend_from_slice(part.as_bytes());
        }
        bytes.push(0x0c);
        bytes.extend_from_slice(&charset.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(u8::from(kind));
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&[0, 0]);
        Column::deserialize((), &mut ParseBuf(&bytes)).unwrap()
    }

    /// Writes one value of the driver in a row and returns the row, ready to read.
    fn written(column: &Column, value: Option<&mysql_async::Value>) -> Row {
        let mut writer = RowWriter::default();
        write_cell(&mut writer, value, column);
        writer.finish()
    }

    fn address() -> Address {
        Address::Tcp {
            host: "127.0.0.1".to_owned(),
            port: 3306,
        }
    }

    #[test]
    fn configuration_codes_give_their_fix() {
        let cases = [
            (1049, Some(Fix::Database)),
            (1130, Some(Fix::ServerRule)),
            (1040, Some(Fix::PoolLimit)),
            (1203, Some(Fix::PoolLimit)),
            (1142, Some(Fix::Privilege)),
            (1792, Some(Fix::ReadOnly)),
            (1290, Some(Fix::ServerReadOnly)),
            (1064, None),
        ];
        for (code, fix) in cases {
            let error = connect_error(&server_error(code, ""), &address());
            assert_eq!(error.fix, fix, "code {code}");
        }
        // In a statement, `1049` is a name of the SQL, not the database of the connection.
        assert_eq!(exec_error(&server_error(1049, ""), "USE x").fix, None);
        assert_eq!(
            exec_error(&server_error(1792, ""), "INSERT").fix,
            Some(Fix::ReadOnly)
        );
    }

    #[test]
    fn a_refused_port_asks_for_the_port() {
        let refused = DriverError::Io(mysql_async::IoError::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused,
        )));
        assert_eq!(connect_error(&refused, &address()).fix, Some(Fix::Port));
        assert_eq!(connect_error(&io_error(), &address()).fix, None);
    }

    #[test]
    fn server_codes_give_their_kind() {
        let cases = [
            (1054, ErrorKind::Syntax),
            (1064, ErrorKind::Syntax),
            (1146, ErrorKind::Syntax),
            (1049, ErrorKind::Syntax),
            (1142, ErrorKind::Permission),
            (1045, ErrorKind::Auth),
            (1317, ErrorKind::Canceled),
            (3024, ErrorKind::Timeout),
            (1040, ErrorKind::Resource),
            (1235, ErrorKind::Unsupported),
            (2006, ErrorKind::Connection),
            (9999, ErrorKind::Other),
        ];
        for (code, kind) in cases {
            assert_eq!(code_kind(code), kind, "code {code}");
            assert_eq!(error_kind(&server_error(code, "")), kind, "code {code}");
        }
    }

    #[test]
    fn a_failure_of_the_socket_or_of_the_driver_ends_the_connection() {
        assert_eq!(error_kind(&io_error()), ErrorKind::Connection);
        let closed = DriverError::Driver(mysql_async::DriverError::ConnectionClosed);
        assert_eq!(error_kind(&closed), ErrorKind::Connection);
        let mixed = DriverError::Driver(mysql_async::DriverError::MixedParams);
        assert_eq!(error_kind(&mixed), ErrorKind::Other);
    }

    #[test]
    fn a_wrong_password_is_refused_and_keeps_the_text_of_the_driver() {
        for code in [1045, 1698] {
            let error = connect_error(&server_error(code, "Access denied for 'user'"), &address());
            assert!(error.password_refused, "code {code}");
            assert_eq!(error.kind, ErrorKind::Auth, "code {code}");
            assert!(error.message.contains("Access denied"), "{}", error.message);
        }
        let error = connect_error(&server_error(1064, "Syntax error"), &address());
        assert!(!error.password_refused);
        assert_eq!(error.kind, ErrorKind::Syntax);
        let error = connect_error(&io_error(), &address());
        assert!(!error.password_refused);
        assert_eq!(error.kind, ErrorKind::Connection);
    }

    #[test]
    fn a_syntax_error_carries_its_offset_in_the_statement() {
        let sql = "SELECT FORM t";
        let error = exec_error(
            &server_error(
                1064,
                "You have an error in your SQL syntax; near 'FORM' at line 1",
            ),
            sql,
        );
        assert_eq!(error.offset, Some(7));
        assert_eq!(error.kind, ErrorKind::Syntax);
        assert!(error.message.ends_with("(1064)"), "{}", error.message);
        let error = exec_error(&io_error(), sql);
        assert_eq!(error.offset, None);
        assert_eq!(error.kind, ErrorKind::Connection);
    }

    #[test]
    fn the_offset_follows_the_line_of_the_error_and_counts_bytes() {
        assert_eq!(
            syntax_offset("near 'FORM' at line 1", "SELECT FORM t"),
            Some(7)
        );
        // An error on a later line starts after the bytes of the earlier lines.
        let sql = "SELECT '–' AS a\n  FORM t\n  WHERE a = 1";
        assert_eq!(
            syntax_offset("near 'FORM' at line 2", sql),
            sql.find("FORM")
        );
        assert_eq!("SELECT '–'\n".len(), 13);
        // An empty place is the end of the statement.
        assert_eq!(syntax_offset("near '' at line 3", sql), Some(sql.len()));
    }

    #[test]
    fn a_message_without_a_place_gives_no_offset() {
        assert_eq!(syntax_offset("Connection reset", "SELECT 1"), None);
        assert_eq!(syntax_offset("near 'FORM' somewhere", "SELECT 1"), None);
        assert_eq!(syntax_offset("near 'FORM' at line x", "SELECT 1"), None);
        // The text of the error is not on that line of the statement.
        assert_eq!(syntax_offset("near 'FORM' at line 1", "SELECT 1"), None);
        // A line beyond the statement has no place either.
        assert_eq!(syntax_offset("near 'SELECT' at line 9", "SELECT 1"), None);
    }

    #[test]
    fn only_a_server_error_has_a_code() {
        assert_eq!(server_code(&server_error(1317, "")), Some(1317));
        assert_eq!(server_code(&io_error()), None);
    }

    #[test]
    fn a_tcp_target_becomes_the_options_of_the_driver() {
        let target = target(
            Address::Tcp {
                host: "db".to_owned(),
                port: 3307,
            },
            Tls::Disabled,
        );
        let options = driver_options(&target).unwrap();
        assert_eq!(options.ip_or_hostname(), "db");
        assert_eq!(options.tcp_port(), 3307);
        assert_eq!(options.user(), Some("user"));
        assert_eq!(options.pass(), Some("secret"));
        assert_eq!(options.db_name(), Some("shop"));
        assert_eq!(options.socket(), None);
        assert_eq!(options.tcp_keepalive(), Some(TCP_KEEPALIVE));
        // The driver can move to a local socket that the user did not ask for.
        assert!(!options.prefer_socket());
        assert_eq!(options.ssl_opts(), None);
    }

    #[test]
    fn a_socket_target_without_a_database_stays_in_the_options() {
        let mut target = target(
            Address::Socket(PathBuf::from("/tmp/mysql.sock")),
            Tls::Disabled,
        );
        target.database = None;
        target.password = None;
        let options = driver_options(&target).unwrap();
        assert_eq!(options.socket(), Some("/tmp/mysql.sock"));
        assert_eq!(options.db_name(), None);
        assert_eq!(options.pass(), None);
    }

    #[test]
    fn tls_off_gives_no_ssl_options() {
        assert!(ssl_options(&Tls::Disabled).unwrap().is_none());
    }

    #[test]
    fn without_a_ca_file_the_system_roots_verify_the_server() {
        let files = TlsFiles {
            ca: None,
            client: None,
        };
        let options = ssl_options(&Tls::Verify(files)).unwrap().unwrap();
        assert!(!options.disable_built_in_roots());
        assert!(!options.root_certs().is_empty());
    }

    #[test]
    fn a_missing_ca_file_fails_with_its_path() {
        let files = TlsFiles {
            ca: Some(PathBuf::from("/no/such/sqln-ca.pem")),
            client: None,
        };
        let error = ssl_options(&Tls::Verify(files)).unwrap_err();
        assert!(
            error.message.contains("/no/such/sqln-ca.pem"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_ca_file_replaces_the_roots_of_the_system() {
        let path = std::env::temp_dir().join(format!("sqln-mysql-ca-{}.pem", std::process::id()));
        std::fs::write(&path, CA_CERTIFICATE).unwrap();
        let files = TlsFiles {
            ca: Some(path.clone()),
            client: None,
        };
        let options = ssl_options(&Tls::Verify(files)).unwrap().unwrap();
        assert!(options.disable_built_in_roots());
        assert_eq!(options.root_certs().len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_connector_speaks_mysql() {
        let connector = MySqlConnector::new(target(
            Address::Tcp {
                host: "db".to_owned(),
                port: 3306,
            },
            Tls::Disabled,
        ));
        assert_eq!(connector.dialect(), Dialect::MySql);
        let capabilities = connector.capabilities();
        assert_eq!(capabilities.namespace, Namespace::Database);
        assert!(capabilities.transactions);
        assert!(capabilities.parameters);
    }

    #[test]
    fn numbers_and_binaries_come_from_the_column_type() {
        assert!(is_number(&column(
            ColumnType::MYSQL_TYPE_LONGLONG,
            BINARY_CHARSET
        )));
        assert!(is_number(&column(
            ColumnType::MYSQL_TYPE_NEWDECIMAL,
            TEXT_CHARSET
        )));
        assert!(is_number(&column(
            ColumnType::MYSQL_TYPE_YEAR,
            TEXT_CHARSET
        )));
        assert!(!is_number(&column(
            ColumnType::MYSQL_TYPE_VARCHAR,
            TEXT_CHARSET
        )));
        // Numbers also report the binary character set, so the type comes first.
        assert!(!is_binary(&column(
            ColumnType::MYSQL_TYPE_LONGLONG,
            BINARY_CHARSET
        )));
        assert!(is_binary(&column(
            ColumnType::MYSQL_TYPE_VAR_STRING,
            BINARY_CHARSET
        )));
        assert!(is_binary(&column(
            ColumnType::MYSQL_TYPE_BLOB,
            BINARY_CHARSET
        )));
        assert!(!is_binary(&column(
            ColumnType::MYSQL_TYPE_BLOB,
            TEXT_CHARSET
        )));
    }

    #[test]
    fn null_values_stay_null() {
        let column = column(ColumnType::MYSQL_TYPE_VARCHAR, TEXT_CHARSET);
        for value in [None, Some(mysql_async::Value::NULL)] {
            assert_eq!(written(&column, value.as_ref()).get(0), Some(Value::Null));
        }
    }

    #[test]
    fn numbers_render_as_numbers_only_in_number_columns() {
        let number = column(ColumnType::MYSQL_TYPE_LONGLONG, TEXT_CHARSET);
        let value = Some(mysql_async::Value::Int(5));
        assert_eq!(
            written(&number, value.as_ref()).get(0),
            Some(Value::Number("5"))
        );
        let text = column(ColumnType::MYSQL_TYPE_VARCHAR, TEXT_CHARSET);
        assert_eq!(
            written(&text, value.as_ref()).get(0),
            Some(Value::Text("5"))
        );
        // A date quotes its SQL text, which the trim of the quotes removes.
        let date = Some(mysql_async::Value::Date(2024, 1, 2, 3, 4, 5, 0));
        assert_eq!(
            written(&text, date.as_ref()).get(0),
            Some(Value::Text("2024-01-02 03:04:05"))
        );
    }

    #[test]
    fn bytes_follow_the_column_type_and_the_encoding() {
        let text = column(ColumnType::MYSQL_TYPE_VARCHAR, TEXT_CHARSET);
        let bytes = Some(mysql_async::Value::Bytes(b"abc".to_vec()));
        assert_eq!(
            written(&text, bytes.as_ref()).get(0),
            Some(Value::Text("abc"))
        );
        let number = column(ColumnType::MYSQL_TYPE_LONG, TEXT_CHARSET);
        let digits = Some(mysql_async::Value::Bytes(b"123".to_vec()));
        assert_eq!(
            written(&number, digits.as_ref()).get(0),
            Some(Value::Number("123"))
        );
        // A binary column keeps the bytes, with a text that reads as text.
        let binary = column(ColumnType::MYSQL_TYPE_BLOB, BINARY_CHARSET);
        match written(&binary, bytes.as_ref()).get(0) {
            Some(Value::Binary(value)) => {
                assert_eq!(value.text(), Some("abc"));
                assert_eq!(value.bytes(), b"abc".as_slice());
            }
            other => panic!("expected binary text, got {other:?}"),
        }
        // Bytes that are not text stay bytes.
        let broken = Some(mysql_async::Value::Bytes(vec![0xff, 0xfe]));
        match written(&text, broken.as_ref()).get(0) {
            Some(Value::Binary(value)) => {
                assert_eq!(value.text(), None);
                assert_eq!(value.bytes(), &[0xff, 0xfe][..]);
            }
            other => panic!("expected binary bytes, got {other:?}"),
        }
    }
}
