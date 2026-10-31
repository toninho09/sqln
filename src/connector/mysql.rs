//! MySQL connector. The text protocol keeps the text of the database in each value.

use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use mysql_async::consts::{ColumnType, StatusFlags};
use mysql_async::prelude::Queryable;
use mysql_async::{ClientIdentity, Column, Conn, Error as DriverError, Opts, OptsBuilder, SslOpts};
use tokio::time::timeout;

use super::target::{Address, NetworkTarget, Tls};
use super::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState, describe, tls,
};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use crate::exec::{Flow, ResultSink};
use crate::grid::{Row, Value};
use crate::sql::{Dialect, counts_rows, is_plain_read};
use crate::ui::text;

/// `ER_QUERY_INTERRUPTED`: `KILL QUERY` stopped the statement.
const QUERY_INTERRUPTED: u16 = 1317;
/// The value of a binary column in the character set field of the protocol.
const BINARY_CHARSET: u16 = 63;
const TCP_KEEPALIVE_MS: u32 = 30_000;
/// A statement with no effect. Its answer has the state of the transaction.
const STATUS_REQUEST: &str = "DO 0";
const CATALOG_SQL: &str = "\
    SELECT s.SCHEMA_NAME, t.TABLE_NAME, t.TABLE_TYPE \
    FROM information_schema.SCHEMATA s \
    LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME \
    WHERE s.SCHEMA_NAME NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys') \
    ORDER BY 1, 2";
const COLUMNS_SQL: &str = "\
    SELECT COLUMN_NAME FROM information_schema.COLUMNS \
    WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
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
        check_client_key(&client.key)?;
        let identity =
            ClientIdentity::new(client.certificate.clone().into(), client.key.clone().into());
        options = options.with_client_identity(Some(identity));
    }
    Ok(Some(options))
}

/// The driver reads only RSA keys in PKCS#1 form, and its error does not say that.
fn check_client_key(path: &Path) -> Result<(), ConnectError> {
    let key = std::fs::read(path).map_err(|error| ConnectError::new(error.to_string()))?;
    let contains = |part: &[u8]| key.windows(part.len()).any(|window| window == part);
    if contains(b"-----BEGIN ") && !contains(b"-----BEGIN RSA PRIVATE KEY-----") {
        return Err(ConnectError::new(text::MYSQL_CLIENT_KEY.to_owned()));
    }
    Ok(())
}

/// Do not write this value in a log: its debug text shows the password.
fn connect_error(error: &mysql_async::Error) -> ConnectError {
    match error {
        // `1045` is a wrong password, and `1698` is an account that needs another one.
        mysql_async::Error::Server(server) if matches!(server.code, 1045 | 1698) => {
            ConnectError::refused_password(describe(error))
        }
        _ => ConnectError::new(describe(error)),
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
        .tcp_keepalive(Some(TCP_KEEPALIVE_MS))
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
}

impl MySqlConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self { target }
    }
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
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let options = driver_options(&self.target)?;
        let conn = Conn::new(options.clone())
            .await
            .map_err(|error| connect_error(&error))?;
        let canceller = Arc::new(MySqlCanceller {
            options,
            connection: conn.id(),
        });
        Ok(Box::new(MySqlSession {
            conn: Some(conn),
            canceller,
            database: self.target.database.clone(),
            transaction: TransactionState::None,
            running: false,
            status_is_current: true,
        }))
    }
}

struct MySqlCanceller {
    options: Opts,
    connection: u32,
}

impl MySqlCanceller {
    async fn kill_query(&self) -> Result<(), DriverError> {
        let mut conn = Conn::new(self.options.clone()).await?;
        conn.query_drop(format!("KILL QUERY {}", self.connection))
            .await?;
        conn.disconnect().await
    }
}

#[async_trait]
impl Canceller for MySqlCanceller {
    async fn cancel(&self) -> Result<(), String> {
        match timeout(SESSION_LIMIT, self.kill_query()).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(describe(&error)),
            Err(_) => Err("The cancel request got no answer.".to_owned()),
        }
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

fn cell_value(cell: Option<&mysql_async::Value>, column: &Column) -> Value {
    let bytes = match cell {
        None | Some(mysql_async::Value::NULL) => return Value::Null,
        Some(mysql_async::Value::Bytes(bytes)) => bytes.as_slice(),
        // The text protocol sends bytes. This line keeps any other form readable.
        Some(other) => return Value::Text(other.as_sql(true).into()),
    };
    match std::str::from_utf8(bytes) {
        Ok(text) if !is_binary(column) && is_number(column) => Value::Number(text.into()),
        Ok(text) if !is_binary(column) => Value::Text(text.into()),
        _ => Value::Binary(bytes.into()),
    }
}

fn row_values(row: &mysql_async::Row) -> Row {
    let columns = row.columns_ref();
    (0..row.len())
        .map(|index| cell_value(row.as_ref(index), &columns[index]))
        .collect()
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
        DriverError::Server(server) => ExecError {
            message: format!("{} ({})", server.message, server.code),
            offset: syntax_offset(&server.message, sql),
        },
        other => ExecError::new(describe(other)),
    }
}

fn server_code(error: &DriverError) -> Option<u16> {
    match error {
        DriverError::Server(server) => Some(server.code),
        _ => None,
    }
}

fn quoted(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

#[derive(Default)]
struct Outcome {
    has_columns: bool,
    changed: Option<u64>,
}

struct MySqlSession {
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
}

impl MySqlSession {
    /// The reset of the server keeps the current database, so a `USE` needs this step.
    /// MySQL cannot leave a database, so a session without a database closes.
    async fn clean(&mut self) -> bool {
        if !matches!(self.conn().reset().await, Ok(true)) {
            return false;
        }
        let current: Option<Option<String>> =
            match self.conn().query_first("SELECT DATABASE()").await {
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
        let sql = format!("USE {}", quoted(database));
        self.conn().query_drop(sql).await.is_ok()
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
    fn conn(&mut self) -> &mut Conn {
        self.conn
            .as_mut()
            .expect("only the drop of the session takes the connection")
    }

    async fn read(&mut self, sql: &str, sink: &mut ResultSink) -> Result<Outcome, ExecError> {
        let free = self.transaction == TransactionState::None;
        let canceller = self.canceller.clone();
        let mut result = self
            .conn()
            .query_iter(sql)
            .await
            .map_err(|error| exec_error(&error, sql))?;
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
                sink.set_columns(names).await;
            }
            let mut storing = true;
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
                let flow = sink.push(row_values(&row)).await;
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

    /// Reads the state of the last answer. A successful statement needs no call to
    /// the server, so `SHOW WARNINGS` and `ROW_COUNT()` work on the next statement,
    /// and `SET TRANSACTION` still applies to the next transaction.
    async fn observed_state(&mut self) -> TransactionState {
        if !self.status_is_current {
            match timeout(SESSION_LIMIT, self.conn().query_drop(STATUS_REQUEST)).await {
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
    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        if context.read_only {
            let sql = "SET SESSION TRANSACTION READ ONLY";
            self.conn()
                .query_drop(sql)
                .await
                .map_err(|error| exec_error(&error, sql))?;
        }
        if let Some(schema) = &context.schema {
            let sql = format!("USE {}", quoted(schema));
            self.conn()
                .query_drop(&sql)
                .await
                .map_err(|error| exec_error(&error, &sql))?;
        }
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.running = true;
        let read = self.read(sql, &mut sink).await;
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
        let done = self.conn().query_drop(sql).await;
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
        matches!(timeout(SESSION_LIMIT, self.conn().ping()).await, Ok(Ok(())))
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let rows: Vec<(String, Option<String>, Option<String>)> = self
            .conn()
            .query(CATALOG_SQL)
            .await
            .map_err(|error| exec_error(&error, CATALOG_SQL))?;
        let mut schemas: Vec<Schema> = Vec::new();
        for (name, relation, kind) in rows {
            if schemas.last().is_none_or(|schema| schema.name != name) {
                schemas.push(Schema {
                    name,
                    relations: Vec::new(),
                    partial: false,
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
                });
            }
        }
        let sql = "SELECT DATABASE()";
        let current: Option<Option<String>> = self
            .conn()
            .query_first(sql)
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
        self.conn()
            .exec(COLUMNS_SQL, (schema, relation))
            .await
            .map_err(|error| exec_error(&error, COLUMNS_SQL))
    }
}
