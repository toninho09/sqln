//! PostgreSQL connector. It reads through the simple protocol, so each value keeps
//! the text of the database.

mod parameters;
mod text_io;

use std::future::poll_fn;
use std::io;
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpStream, UnixStream};
use tokio::time::timeout;
use tokio_postgres::config::SslMode;
use tokio_postgres::error::{DbError, ErrorPosition, SqlState};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::types::Type;
use tokio_postgres::{
    AsyncMessage, CancelToken, Client, Config, SimpleQueryMessage, SimpleQueryRow,
};
use tokio_postgres_rustls::MakeRustlsConnect;

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
use crate::sql::literal::text_literal;
use crate::sql::{
    Dialect, counts_rows, is_plain_read, quote_identifier, reads_or_writes_stdio,
    removes_prepared_statements,
};
use crate::ui::text;

/// The two clocks differ only inside a transaction. The simple protocol gives each
/// call its own statement time, and a failed transaction refuses this call.
const TRANSACTION_PROBE: &str = "SELECT transaction_timestamp() <> statement_timestamp()";
/// The steps of `DISCARD ALL` without `DEALLOCATE ALL`. The driver keeps the statements
/// of its type lookup in a cache, and `DEALLOCATE ALL` removes them from the server.
const CLEANUP_SQL: &str = "CLOSE ALL; SET SESSION AUTHORIZATION DEFAULT; RESET ALL; \
    UNLISTEN *; SELECT pg_advisory_unlock_all(); DISCARD PLANS; DISCARD TEMP; \
    DISCARD SEQUENCES;";
/// The statements of `PREPARE`. The statements of the driver are not in this list.
const USER_STATEMENTS_SQL: &str = "SELECT name FROM pg_prepared_statements WHERE from_sql";
const CATALOG_SQL: &str = "\
    SELECT n.nspname, c.relname, c.relkind \
    FROM pg_catalog.pg_namespace n \
    LEFT JOIN pg_catalog.pg_class c \
      ON c.relnamespace = n.oid AND c.relkind IN ('r', 'p', 'f', 'v', 'm') \
    WHERE n.nspname !~ '^pg_' AND n.nspname <> 'information_schema' \
    ORDER BY 1, 2";
/// The routines, sequences, triggers, and types of each schema of `CATALOG_SQL`. A member
/// of an extension stays out, and so do aggregates, window functions, and the internal
/// triggers of the keys: `pg_get_functiondef` refuses an aggregate.
const OBJECTS_SQL: &str = "\
    SELECT n.nspname::text, o.name, o.kind, o.detail \
    FROM ( \
      SELECT p.pronamespace AS space, p.proname::text AS name, \
        CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END AS kind, \
        pg_catalog.pg_get_function_identity_arguments(p.oid) AS detail, \
        'pg_catalog.pg_proc'::pg_catalog.regclass AS class, p.oid \
      FROM pg_catalog.pg_proc p WHERE p.prokind IN ('f', 'p') \
      UNION ALL \
      SELECT c.relnamespace, c.relname::text, 'sequence', '', \
        'pg_catalog.pg_class'::pg_catalog.regclass, c.oid \
      FROM pg_catalog.pg_class c WHERE c.relkind = 'S' \
      UNION ALL \
      SELECT c.relnamespace, t.tgname::text, 'trigger', c.relname::text, \
        'pg_catalog.pg_trigger'::pg_catalog.regclass, t.oid \
      FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid \
      WHERE NOT t.tgisinternal \
      UNION ALL \
      SELECT t.typnamespace, t.typname::text, \
        CASE t.typtype WHEN 'e' THEN 'enum' ELSE 'domain' END, '', \
        'pg_catalog.pg_type'::pg_catalog.regclass, t.oid \
      FROM pg_catalog.pg_type t WHERE t.typtype IN ('e', 'd') \
    ) o \
    JOIN pg_catalog.pg_namespace n ON n.oid = o.space \
    WHERE n.nspname !~ '^pg_' AND n.nspname <> 'information_schema' \
    AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d \
      WHERE d.classid = o.class AND d.objid = o.oid AND d.deptype = 'e')";
const COLUMNS_SQL: &str = "\
    SELECT a.attname::text \
    FROM pg_catalog.pg_attribute a \
    JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
    WHERE n.nspname = $1::text AND c.relname = $2::text AND a.attnum > 0 AND NOT a.attisdropped \
    ORDER BY a.attnum";
/// `unnest` has no schema here: only that form reads two arrays in parallel.
const FOREIGN_KEYS_SQL: &str = "\
    SELECT k.conname::text, rn.nspname::text, r.relname::text, a.attname::text, t.attname::text \
    FROM pg_catalog.pg_constraint k \
    JOIN pg_catalog.pg_class c ON c.oid = k.conrelid \
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
    JOIN pg_catalog.pg_class r ON r.oid = k.confrelid \
    JOIN pg_catalog.pg_namespace rn ON rn.oid = r.relnamespace \
    CROSS JOIN LATERAL unnest(k.conkey, k.confkey) WITH ORDINALITY AS u(source, target, place) \
    JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.source \
    JOIN pg_catalog.pg_attribute t ON t.attrelid = k.confrelid AND t.attnum = u.target \
    WHERE k.contype = 'f' AND n.nspname = $1::text AND c.relname = $2::text \
    ORDER BY k.conname, u.place";
/// The keys of other tables that reach a relation, with the shape of `FOREIGN_KEYS_SQL`:
/// the column of the relation comes before the column of the other table. A key that a
/// partition copies from its parent stays out.
const REFERENCING_KEYS_SQL: &str = "\
    SELECT k.conname::text, n.nspname::text, c.relname::text, t.attname::text, a.attname::text \
    FROM pg_catalog.pg_constraint k \
    JOIN pg_catalog.pg_class c ON c.oid = k.conrelid \
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
    JOIN pg_catalog.pg_class r ON r.oid = k.confrelid \
    JOIN pg_catalog.pg_namespace rn ON rn.oid = r.relnamespace \
    CROSS JOIN LATERAL unnest(k.confkey, k.conkey) WITH ORDINALITY AS u(target, source, place) \
    JOIN pg_catalog.pg_attribute t ON t.attrelid = k.confrelid AND t.attnum = u.target \
    JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.source \
    WHERE k.contype = 'f' AND k.conparentid = 0 AND rn.nspname = $1::text AND r.relname = $2::text \
    ORDER BY n.nspname, c.relname, k.conname, u.place";
/// A session call that gets no answer in this time counts as lost.
const SESSION_LIMIT: Duration = Duration::from_secs(5);
/// The port in the name of a Unix socket file.
const DEFAULT_PORT: u16 = 5432;

pub struct PostgresConnector {
    target: NetworkTarget,
}

impl PostgresConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self { target }
    }

    fn config(&self) -> Config {
        let target = &self.target;
        let mut config = Config::new();
        config.user(&target.user).application_name("sqln");
        config
            .keepalives(true)
            .keepalives_idle(Duration::from_secs(30))
            .keepalives_interval(Duration::from_secs(10))
            .keepalives_retries(3);
        if let Some(password) = &target.password {
            config.password(password.expose());
        }
        if let Some(database) = &target.database {
            config.dbname(database);
        }
        match &target.address {
            Address::Tcp { host, port } => {
                config.host(host).port(*port);
            }
            Address::Socket(path) => {
                config.host_path(path);
            }
        }
        // `Prefer`, the driver default, accepts a server without TLS.
        config.ssl_mode(match target.tls {
            Tls::Disabled => SslMode::Disable,
            Tls::Verify(_) => SslMode::Require,
        });
        config
    }

    /// `SslMode::Disable` never uses the value that this function returns without TLS.
    fn tls(&self) -> Result<MakeRustlsConnect, ConnectError> {
        let files = match &self.target.tls {
            Tls::Verify(files) => tls::client_config(files)?,
            Tls::Disabled => tls::unused_client_config()?,
        };
        Ok(MakeRustlsConnect::new(files))
    }
}

/// Reads one row of `OBJECTS_SQL`. A missing cell gives an empty text.
fn object_row(row: Vec<Option<String>>) -> [String; 4] {
    let mut cells = row.into_iter().map(Option::unwrap_or_default);
    std::array::from_fn(|_| cells.next().unwrap_or_default())
}

fn connect_error(error: &tokio_postgres::Error) -> ConnectError {
    let Some(database) = error.as_db_error() else {
        return ConnectError::new(describe(error));
    };
    let message = format!("{} ({})", database.message(), database.code().code());
    // `28000` also covers a rule of `pg_hba.conf`, which a new password does not change.
    if *database.code() == SqlState::INVALID_PASSWORD {
        ConnectError::refused_password(message)
    } else {
        ConnectError::new(message)
    }
}

#[async_trait]
impl Connector for PostgresConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Schema,
            transactions: true,
            parameters: true,
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let tls = self.tls()?;
        let enabled = Arc::new(AtomicBool::new(false));
        let notices = Arc::new(Mutex::new(Notices::default()));
        let config = self.config();
        let client = if matches!(self.target.tls, Tls::Disabled) {
            match &self.target.address {
                Address::Tcp { host, port } => {
                    let socket = TcpStream::connect((host.as_str(), *port))
                        .await
                        .map_err(|error| ConnectError::new(error.to_string()))?;
                    socket
                        .set_nodelay(true)
                        .map_err(|error| ConnectError::new(error.to_string()))?;
                    let keepalive = socket2::TcpKeepalive::new()
                        .with_time(Duration::from_secs(30))
                        .with_interval(Duration::from_secs(10))
                        .with_retries(3);
                    socket2::SockRef::from(&socket)
                        .set_tcp_keepalive(&keepalive)
                        .map_err(|error| ConnectError::new(error.to_string()))?;
                    let (client, connection) = config
                        .connect_raw(
                            text_io::TextIo::new(socket, enabled.clone()),
                            tokio_postgres::NoTls,
                        )
                        .await
                        .map_err(|error| connect_error(&error))?;
                    drive(connection, notices.clone());
                    client
                }
                Address::Socket(folder) => {
                    let socket =
                        UnixStream::connect(folder.join(format!(".s.PGSQL.{DEFAULT_PORT}")))
                            .await
                            .map_err(|error| ConnectError::new(error.to_string()))?;
                    let (client, connection) = config
                        .connect_raw(
                            text_io::TextIo::new(socket, enabled.clone()),
                            tokio_postgres::NoTls,
                        )
                        .await
                        .map_err(|error| connect_error(&error))?;
                    drive(connection, notices.clone());
                    client
                }
            }
        } else {
            let (client, connection) = config
                .connect(text_io::TextTls {
                    inner: tls.clone(),
                    enabled: enabled.clone(),
                })
                .await
                .map_err(|error| connect_error(&error))?;
            drive(connection, notices.clone());
            client
        };
        enabled.store(true, Ordering::SeqCst);
        let canceller = Arc::new(PostgresCanceller {
            token: client.cancel_token(),
            address: self.target.address.clone(),
            tls,
            requested: AtomicBool::new(false),
        });
        Ok(Box::new(PostgresSession {
            client,
            canceller,
            transaction: TransactionState::None,
            notices,
            statements_lost: false,
            trace: None,
        }))
    }
}

fn drive<S, T>(mut connection: tokio_postgres::Connection<S, T>, notices: Arc<Mutex<Notices>>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(message) = poll_fn(|context| connection.poll_message(context)).await {
            match message {
                Ok(AsyncMessage::Notice(notice)) => {
                    if let Ok(mut notices) = notices.lock() {
                        notices.push(with_detail(
                            format!("{}: {}", notice.severity(), notice.message()),
                            &notice,
                        ));
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
}

/// Reads to the end of the connection after its shutdown. The driver returns after
/// it writes the cancel request, and the server handles the request some time later.
/// The server closes the connection after that work, so the end of this stream proves
/// that the request cannot reach a statement that starts later.
struct UntilClosed<S> {
    stream: S,
    shut_down: bool,
}

impl<S: AsyncRead + Unpin> AsyncRead for UntilClosed<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(context, buffer)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for UntilClosed<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(context, data)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.shut_down {
            ready!(Pin::new(&mut self.stream).poll_shutdown(context))?;
            self.shut_down = true;
        }
        let mut space = [0; 64];
        loop {
            let mut buffer = ReadBuf::new(&mut space);
            match ready!(Pin::new(&mut self.stream).poll_read(context, &mut buffer)) {
                Ok(()) if buffer.filled().is_empty() => return Poll::Ready(Ok(())),
                Ok(()) => {}
                // The server can reset the connection when it closes it.
                Err(_) => return Poll::Ready(Ok(())),
            }
        }
    }
}

struct PostgresCanceller {
    token: CancelToken,
    address: Address,
    tls: MakeRustlsConnect,
    /// A request between the two steps of a statement reaches no running statement.
    requested: AtomicBool,
}

impl PostgresCanceller {
    async fn send_on<S>(
        &self,
        stream: S,
        host: &str,
        trace: Option<crate::command_log::TraceContext>,
    ) -> Result<(), String>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let stream = UntilClosed {
            stream,
            shut_down: false,
        };
        let mut tls = self.tls.clone();
        let connect =
            <MakeRustlsConnect as MakeTlsConnect<UntilClosed<S>>>::make_tls_connect(&mut tls, host)
                .map_err(|error| error.to_string())?;
        crate::command_log::observe(
            trace,
            "PostgreSQL CancelRequest",
            self.token.cancel_query_raw(stream, connect),
        )
        .await
        .map_err(|error| describe(&error))
    }

    async fn send(&self, trace: Option<crate::command_log::TraceContext>) -> Result<(), String> {
        match &self.address {
            Address::Tcp { host, port } => {
                let stream = TcpStream::connect((host.as_str(), *port))
                    .await
                    .map_err(|error| error.to_string())?;
                self.send_on(stream, host, trace).await
            }
            Address::Socket(folder) => {
                let path = folder.join(format!(".s.PGSQL.{DEFAULT_PORT}"));
                let stream = UnixStream::connect(path)
                    .await
                    .map_err(|error| error.to_string())?;
                self.send_on(stream, "", trace).await
            }
        }
    }
}

#[async_trait]
impl Canceller for PostgresCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.cancel_traced(None).await
    }

    async fn cancel_traced(
        &self,
        trace: Option<crate::command_log::TraceContext>,
    ) -> Result<(), String> {
        self.requested.store(true, Ordering::SeqCst);
        match timeout(SESSION_LIMIT, self.send(trace)).await {
            Ok(result) => result,
            Err(_) => Err("The cancel request got no answer.".to_owned()),
        }
    }
}

/// The presentation of a column. The simple protocol does not report the column types.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnKind {
    Text,
    Number,
    Binary,
}

fn column_kind(column: &Type) -> ColumnKind {
    match *column {
        Type::INT2
        | Type::INT4
        | Type::INT8
        | Type::FLOAT4
        | Type::FLOAT8
        | Type::NUMERIC
        | Type::OID => ColumnKind::Number,
        Type::BYTEA => ColumnKind::Binary,
        _ => ColumnKind::Text,
    }
}

/// Reads the `\x00ff` form. The `escape` form of `bytea_output` stays as text.
fn write_bytea(row: &mut RowWriter, text: &str) {
    let decoded = text
        .strip_prefix("\\x")
        .filter(|hex| hex.len().is_multiple_of(2))
        .and_then(|hex| {
            (0..hex.len() / 2)
                .map(|index| u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok())
                .collect::<Option<Box<[u8]>>>()
        });
    row.push(decoded.as_deref().map_or(Value::Text(text), Value::binary));
}

fn row_values(
    cells: &mut RowWriter,
    row: &SimpleQueryRow,
    kinds: &[ColumnKind],
) -> Result<Row, ExecError> {
    for index in 0..row.len() {
        let text = row
            .try_get(index)
            .map_err(|error| ExecError::new(describe(&error)))?;
        match (text, kinds.get(index)) {
            (None, _) => cells.push(Value::Null),
            (Some(text), Some(ColumnKind::Number)) => cells.push(Value::Number(text)),
            (Some(text), Some(ColumnKind::Binary)) => write_bytea(cells, text),
            (Some(text), _) => cells.push(Value::Text(text)),
        }
    }
    Ok(cells.finish())
}

/// The database counts characters from 1, and the editor uses bytes from 0.
/// An error at the end of the text points past the last character.
fn byte_offset(sql: &str, position: u32) -> Option<usize> {
    let index = usize::try_from(position).ok()?.checked_sub(1)?;
    sql.char_indices()
        .map(|(offset, _)| offset)
        .chain([sql.len()])
        .nth(index)
}

/// Adds the detail and the hint of a message of the database.
fn with_detail(mut message: String, database: &DbError) -> String {
    if let Some(detail) = database.detail() {
        message = format!("{message}\n{detail}");
    }
    if let Some(hint) = database.hint() {
        message = format!("{message}\n{hint}");
    }
    message
}

fn exec_error(error: &tokio_postgres::Error, sql: &str) -> ExecError {
    let Some(database) = error.as_db_error() else {
        return ExecError::new(describe(error));
    };
    let message = format!("{} ({})", database.message(), database.code().code());
    let message = with_detail(message, database);
    let offset = match database.position() {
        Some(ErrorPosition::Original(position)) => byte_offset(sql, *position),
        _ => None,
    };
    ExecError { message, offset }
}

#[derive(Default)]
struct Outcome {
    has_columns: bool,
    changed: Option<u64>,
}

/// Writes the `search_path` of a console schema. The schema comes first, and the default
/// path of the session stays after it, so the types and functions of an extension stay
/// visible, such as `geography` of PostGIS in `public`. With `only`, the path holds only
/// the schema. The cleanup of the session restores the default path.
fn schema_path_sql(schema: &str, only: bool) -> String {
    let first = quote_identifier(schema, Dialect::Postgres);
    if only {
        return format!("SET search_path TO {first};");
    }
    let first = text_literal(&first, Dialect::Postgres);
    format!(
        "SELECT pg_catalog.set_config('search_path', pg_catalog.concat_ws(', ', {first}, \
         NULLIF(pg_catalog.current_setting('search_path'), '')), false);"
    )
}

/// The notices of the statement in progress. The task of the connection writes them.
/// The server sends a notice before the end of its statement, and that task handles the
/// messages in order, so `run` finds each notice when the statement ends.
#[derive(Default)]
struct Notices {
    texts: Vec<String>,
    /// The notices after `MAX_NOTICES`.
    more: usize,
}

impl Notices {
    fn push(&mut self, text: String) {
        if self.texts.len() < MAX_NOTICES {
            self.texts.push(text);
        } else {
            self.more += 1;
        }
    }
}

struct PostgresSession {
    client: Client,
    trace: Option<crate::command_log::TraceContext>,
    canceller: Arc<PostgresCanceller>,
    transaction: TransactionState,
    notices: Arc<Mutex<Notices>>,
    /// True after a statement that can remove the prepared statements of the driver.
    /// The driver cache then names statements that the server does not have.
    statements_lost: bool,
}

impl PostgresSession {
    /// Removes the notices of the session. They stay out of **Messages** when the
    /// statement starts empty, as after the cleanup of a session.
    fn take_notices(&self) -> Notices {
        self.notices
            .lock()
            .map(|mut notices| std::mem::take(&mut *notices))
            .unwrap_or_default()
    }

    /// Asks for the column types before the statement runs. A statement that fails
    /// here fails in the same way in the simple protocol, and it must not run twice:
    /// the second error would hide the first one in a transaction. After a loss of the
    /// driver statements, the type lookup fails, so all columns are text.
    async fn column_kinds(&self, sql: &str) -> Result<Vec<ColumnKind>, ExecError> {
        if self.transaction == TransactionState::Failed || self.statements_lost {
            return Ok(Vec::new());
        }
        let command =
            crate::command_log::limited(format_args!("PostgreSQL Parse/Describe (prepare)\n{sql}"));
        match crate::command_log::observe(
            self.trace
                .as_ref()
                .map(crate::command_log::TraceContext::internal),
            command,
            self.client.prepare(sql),
        )
        .await
        {
            Ok(statement) => Ok(statement
                .columns()
                .iter()
                .map(|column| column_kind(column.type_()))
                .collect()),
            Err(error) if error.as_db_error().is_some() => Err(exec_error(&error, sql)),
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn read(
        &self,
        sql: &str,
        kinds: &[ColumnKind],
        sink: &mut ResultSink,
    ) -> Result<Outcome, ExecError> {
        crate::command_log::observe(
            self.trace.clone(),
            sql,
            self.read_untraced(sql, kinds, sink),
        )
        .await
    }

    async fn read_untraced(
        &self,
        sql: &str,
        kinds: &[ColumnKind],
        sink: &mut ResultSink,
    ) -> Result<Outcome, ExecError> {
        let stream = self
            .client
            .simple_query_raw(sql)
            .await
            .map_err(|error| exec_error(&error, sql))?;
        let mut stream = pin!(stream);
        let mut outcome = Outcome::default();
        let mut storing = true;
        let mut stopped_at_cap = false;
        let mut writer = RowWriter::default();
        loop {
            let message = match stream.try_next().await {
                Ok(Some(message)) => message,
                Ok(None) => return Ok(outcome),
                // The cancellation of the row cap is not an error of the statement.
                Err(error) if stopped_at_cap && error.code() == Some(&SqlState::QUERY_CANCELED) => {
                    return Ok(outcome);
                }
                Err(error) => return Err(exec_error(&error, sql)),
            };
            match message {
                SimpleQueryMessage::RowDescription(columns) => {
                    outcome.has_columns = true;
                    let names = columns
                        .iter()
                        .map(|column| column.name().to_owned())
                        .collect();
                    sink.set_columns(names).await;
                }
                SimpleQueryMessage::Row(row) if storing => {
                    let flow = sink.push(row_values(&mut writer, &row, kinds)?).await;
                    storing = flow == Flow::Continue;
                    let free = self.transaction == TransactionState::None;
                    if flow == Flow::Stop && free && is_plain_read(sql, Dialect::Postgres) {
                        stopped_at_cap = self.canceller().cancel().await.is_ok();
                    }
                }
                SimpleQueryMessage::CommandComplete(rows)
                    if !outcome.has_columns && counts_rows(sql, Dialect::Postgres) =>
                {
                    outcome.changed = Some(rows);
                }
                _ => {}
            }
        }
    }

    async fn probe(&self) -> TransactionState {
        if self.client.is_closed() {
            return TransactionState::Unknown;
        }
        match timeout(
            SESSION_LIMIT,
            crate::command_log::observe(
                self.trace
                    .as_ref()
                    .map(crate::command_log::TraceContext::internal),
                TRANSACTION_PROBE,
                self.client.simple_query(TRANSACTION_PROBE),
            ),
        )
        .await
        {
            Ok(Ok(messages)) => {
                let open = messages.iter().any(|message| match message {
                    SimpleQueryMessage::Row(row) => matches!(row.try_get(0), Ok(Some("t"))),
                    _ => false,
                });
                if open {
                    TransactionState::Open
                } else {
                    TransactionState::None
                }
            }
            Ok(Err(error)) if error.code() == Some(&SqlState::IN_FAILED_SQL_TRANSACTION) => {
                TransactionState::Failed
            }
            _ => TransactionState::Unknown,
        }
    }

    async fn text_rows(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>, ExecError> {
        let messages = crate::command_log::observe(
            self.trace
                .as_ref()
                .map(crate::command_log::TraceContext::internal),
            sql,
            self.client.simple_query(sql),
        )
        .await
        .map_err(|error| exec_error(&error, sql))?;
        let mut rows = Vec::new();
        for message in messages {
            if let SimpleQueryMessage::Row(row) = message {
                let cells = (0..row.len())
                    .map(|index| row.try_get(index).map(|text| text.map(str::to_owned)));
                rows.push(
                    cells
                        .collect::<Result<_, _>>()
                        .map_err(|error| ExecError::new(describe(&error)))?,
                );
            }
        }
        Ok(rows)
    }
}

#[async_trait]
impl Session for PostgresSession {
    fn set_trace_context(&mut self, trace: Option<crate::command_log::TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        crate::command_log::canceller(self.canceller.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        let mut sql = String::new();
        if context.read_only {
            sql.push_str("SET default_transaction_read_only = on;");
        }
        if let Some(schema) = &context.schema {
            sql.push_str(&schema_path_sql(schema, context.only_schema));
        }
        if sql.is_empty() {
            return Ok(());
        }
        crate::command_log::observe(self.trace.clone(), &sql, self.client.batch_execute(&sql))
            .await
            .map_err(|error| exec_error(&error, &sql))
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        if reads_or_writes_stdio(sql, Dialect::Postgres) {
            return Err(ExecError::new(text::COPY_REFUSED));
        }
        self.canceller.requested.store(false, Ordering::SeqCst);
        self.take_notices();
        self.statements_lost |= removes_prepared_statements(sql, Dialect::Postgres);
        let kinds = self.column_kinds(sql).await?;
        if self.canceller.requested.load(Ordering::SeqCst) {
            return Err(ExecError::new(text::STOPPED_BEFORE_SEND));
        }
        let read = self.read(sql, &kinds, &mut sink).await;
        let notices = self.take_notices();
        sink.notices(notices.texts, notices.more).await;
        let (rows, limit) = sink.finish().await;
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
        sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        self.run_bound(sql, parameters, sink).await
    }

    async fn transaction_state(&mut self) -> TransactionState {
        self.transaction = self.probe().await;
        self.transaction
    }

    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError> {
        // In this state, the database answers `COMMIT` with a rollback and no error.
        if command == TransactionCommand::Commit && self.transaction == TransactionState::Failed {
            return Err(ExecError::new(text::COMMIT_OF_FAILED_TRANSACTION));
        }
        let sql = match command {
            TransactionCommand::Begin => "BEGIN",
            TransactionCommand::Commit => "COMMIT",
            TransactionCommand::Rollback => "ROLLBACK",
        };
        crate::command_log::observe(self.trace.clone(), sql, self.client.batch_execute(sql))
            .await
            .map_err(|error| exec_error(&error, sql))?;
        if command == TransactionCommand::Begin {
            self.transaction = TransactionState::Open;
        }
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        if self.statements_lost {
            return ResetOutcome::Discard;
        }
        let cleanup = async {
            let names = self.text_rows(USER_STATEMENTS_SQL).await.ok()?;
            let mut sql: String = names
                .into_iter()
                .filter_map(|row| row.into_iter().next().flatten())
                .map(|name| format!("DEALLOCATE {};", quote_identifier(&name, Dialect::Postgres)))
                .collect();
            sql.push_str(CLEANUP_SQL);
            crate::command_log::observe(self.trace.clone(), &sql, self.client.batch_execute(&sql))
                .await
                .ok()
        };
        match timeout(SESSION_LIMIT, cleanup).await {
            Ok(Some(())) => ResetOutcome::Clean,
            _ => ResetOutcome::Discard,
        }
    }

    async fn is_alive(&mut self) -> bool {
        !self.client.is_closed()
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let mut schemas: Vec<Schema> = Vec::new();
        for row in self.text_rows(CATALOG_SQL).await? {
            let mut cells = row.into_iter();
            let name = cells.next().flatten().unwrap_or_default();
            let relation = cells.next().flatten();
            let kind = cells.next().flatten();
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
                        Some("v" | "m") => RelationKind::View,
                        _ => RelationKind::Table,
                    },
                    columns: Columns::NotLoaded,
                    foreign_keys: Vec::new(),
                    referencing: Vec::new(),
                });
            }
        }
        let objects = self.text_rows(OBJECTS_SQL).await?;
        add_objects(&mut schemas, objects.into_iter().map(object_row));
        let current = self.text_rows("SELECT current_schema()").await?;
        let current_schema = current
            .into_iter()
            .next()
            .and_then(|row| row.into_iter().next())
            .flatten();
        Ok(Catalog {
            schemas,
            current_schema,
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
        let rows = crate::command_log::observe(
            self.trace.clone(),
            command,
            self.client.query(COLUMNS_SQL, &[&schema, &relation]),
        )
        .await
        .map_err(|error| exec_error(&error, COLUMNS_SQL))?;
        rows.iter()
            .map(|row| {
                row.try_get(0)
                    .map_err(|error| ExecError::new(describe(&error)))
            })
            .collect()
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

impl PostgresSession {
    /// Reads the keys of a key query: the name, the schema and the name of the other
    /// table, the column of the relation, and the column of the other table.
    async fn read_keys(
        &self,
        sql: &str,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        let command = crate::command_log::limited(format_args!(
            "{sql}\nParameters: schema={schema:?}, relation={relation:?}"
        ));
        let rows = crate::command_log::observe(
            self.trace.clone(),
            command,
            self.client.query(sql, &[&schema, &relation]),
        )
        .await
        .map_err(|error| exec_error(&error, sql))?;
        let pairs = rows.iter().map(|row| {
            Ok(KeyColumn {
                key: row.try_get(0)?,
                target: RelationRef {
                    schema: row.try_get(1)?,
                    relation: row.try_get(2)?,
                },
                column: row.try_get(3)?,
                target_column: row.try_get(4)?,
            })
        });
        let pairs: Result<Vec<KeyColumn>, tokio_postgres::Error> = pairs.collect();
        pairs
            .map(group_foreign_keys)
            .map_err(|error| ExecError::new(describe(&error)))
    }
}
