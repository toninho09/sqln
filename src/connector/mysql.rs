//! MySQL connector. The text protocol keeps the text of the database in each value.

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
    Canceller, Capabilities, ConnectError, Connector, ExecError, MAX_NOTICES, Namespace,
    ResetOutcome, Session, SessionContext, StatementEnd, TransactionCommand, TransactionState,
    describe, tls,
};
use crate::catalog::{
    Catalog, Columns, ForeignKey, KeyColumn, Relation, RelationKind, RelationRef, Schema,
    add_objects, group_foreign_keys,
};
use crate::exec::{Flow, ResultSink};
use crate::grid::{Row, RowWriter, Value};
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
            parameters: true,
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
            trace: None,
        }))
    }
}

struct MySqlCanceller {
    options: Opts,
    connection: u32,
}

impl MySqlCanceller {
    async fn kill_query(
        &self,
        trace: Option<crate::command_log::TraceContext>,
    ) -> Result<(), DriverError> {
        let mut conn = Conn::new(self.options.clone()).await?;
        let sql = format!("KILL QUERY {}", self.connection);
        crate::command_log::observe(trace, &sql, conn.query_drop(&sql)).await?;
        conn.disconnect().await
    }
}

#[async_trait]
impl Canceller for MySqlCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.cancel_traced(None).await
    }

    async fn cancel_traced(
        &self,
        trace: Option<crate::command_log::TraceContext>,
    ) -> Result<(), String> {
        match timeout(SESSION_LIMIT, self.kill_query(trace)).await {
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

#[derive(Default)]
struct Outcome {
    has_columns: bool,
    changed: Option<u64>,
}

struct MySqlSession {
    trace: Option<crate::command_log::TraceContext>,
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
        if !matches!(
            crate::command_log::observe(
                self.trace.clone(),
                "COM_RESET_CONNECTION",
                self.conn().reset()
            )
            .await,
            Ok(true)
        ) {
            return false;
        }
        let current: Option<Option<String>> = match crate::command_log::observe(
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
        crate::command_log::observe(self.trace.clone(), &sql, self.conn().query_drop(&sql))
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
    fn conn(&mut self) -> &mut Conn {
        self.conn
            .as_mut()
            .expect("only the drop of the session takes the connection")
    }

    async fn read(&mut self, sql: &str, sink: &mut ResultSink) -> Result<Outcome, ExecError> {
        crate::command_log::observe(self.trace.clone(), sql, self.read_untraced(sql, sink)).await
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
                sink.set_columns(names).await;
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
            crate::command_log::observe(
                self.trace
                    .as_ref()
                    .map(crate::command_log::TraceContext::internal),
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
                crate::command_log::observe(
                    self.trace
                        .as_ref()
                        .map(crate::command_log::TraceContext::internal),
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
    fn set_trace_context(&mut self, trace: Option<crate::command_log::TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        crate::command_log::canceller(self.canceller.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        if context.read_only {
            let sql = "SET SESSION TRANSACTION READ ONLY";
            crate::command_log::observe(self.trace.clone(), sql, self.conn().query_drop(sql))
                .await
                .map_err(|error| exec_error(&error, sql))?;
        }
        if let Some(schema) = &context.schema {
            let sql = format!("USE {}", quote_identifier(schema, Dialect::MySql));
            crate::command_log::observe(self.trace.clone(), &sql, self.conn().query_drop(&sql))
                .await
                .map_err(|error| exec_error(&error, &sql))?;
        }
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.running = true;
        let read = self.read(sql, &mut sink).await;
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
        let read = crate::command_log::observe(self.trace.clone(), command, async {
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
            crate::command_log::observe(self.trace.clone(), sql, self.conn().query_drop(sql)).await;
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
                crate::command_log::observe(self.trace.clone(), "COM_PING", self.conn().ping())
            )
            .await,
            Ok(Ok(()))
        )
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let rows: Vec<(String, Option<String>, Option<String>)> = crate::command_log::observe(
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
                });
            }
        }
        let objects: Vec<(String, String, String, String)> = crate::command_log::observe(
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
            crate::command_log::observe(self.trace.clone(), sql, self.conn().query_first(sql))
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
        let command = crate::command_log::limited(format_args!(
            "{COLUMNS_SQL}\nParameters: schema={schema:?}, relation={relation:?}"
        ));
        crate::command_log::observe(
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
    /// Reads the keys of a key query: the name, the schema and the name of the other
    /// table, the column of the relation, and the column of the other table.
    async fn read_keys(
        &mut self,
        sql: &str,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        let command = crate::command_log::limited(format_args!(
            "{sql}\nParameters: schema={schema:?}, relation={relation:?}"
        ));
        let rows: Vec<(String, String, String, String, String)> = crate::command_log::observe(
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
