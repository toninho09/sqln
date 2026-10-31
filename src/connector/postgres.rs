//! PostgreSQL connector. It reads through the simple protocol, so each value keeps
//! the text of the database.

use std::io;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpStream, UnixStream};
use tokio::time::timeout;
use tokio_postgres::config::SslMode;
use tokio_postgres::error::{ErrorPosition, SqlState};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::types::Type;
use tokio_postgres::{CancelToken, Client, Config, SimpleQueryMessage, SimpleQueryRow};
use tokio_postgres_rustls::MakeRustlsConnect;

use super::target::{Address, NetworkTarget, Tls};
use super::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState, describe, tls,
};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use crate::exec::{Flow, ResultSink};
use crate::grid::{Row, Value};
use crate::sql::{Dialect, counts_rows, is_plain_read, reads_or_writes_stdio};
use crate::ui::text;

/// The two clocks differ only inside a transaction. The simple protocol gives each
/// call its own statement time, and a failed transaction refuses this call.
const TRANSACTION_PROBE: &str = "SELECT transaction_timestamp() <> statement_timestamp()";
const CATALOG_SQL: &str = "\
    SELECT n.nspname, c.relname, c.relkind \
    FROM pg_catalog.pg_namespace n \
    LEFT JOIN pg_catalog.pg_class c \
      ON c.relnamespace = n.oid AND c.relkind IN ('r', 'p', 'f', 'v', 'm') \
    WHERE n.nspname !~ '^pg_' AND n.nspname <> 'information_schema' \
    ORDER BY 1, 2";
const COLUMNS_SQL: &str = "\
    SELECT a.attname::text \
    FROM pg_catalog.pg_attribute a \
    JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
    WHERE n.nspname = $1::text AND c.relname = $2::text AND a.attnum > 0 AND NOT a.attisdropped \
    ORDER BY a.attnum";
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
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let tls = self.tls()?;
        let (client, connection) = self
            .config()
            .connect(tls.clone())
            .await
            .map_err(|error| connect_error(&error))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
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
        }))
    }
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
    async fn send_on<S>(&self, stream: S, host: &str) -> Result<(), String>
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
        self.token
            .cancel_query_raw(stream, connect)
            .await
            .map_err(|error| describe(&error))
    }

    async fn send(&self) -> Result<(), String> {
        match &self.address {
            Address::Tcp { host, port } => {
                let stream = TcpStream::connect((host.as_str(), *port))
                    .await
                    .map_err(|error| error.to_string())?;
                self.send_on(stream, host).await
            }
            Address::Socket(folder) => {
                let path = folder.join(format!(".s.PGSQL.{DEFAULT_PORT}"));
                let stream = UnixStream::connect(path)
                    .await
                    .map_err(|error| error.to_string())?;
                self.send_on(stream, "").await
            }
        }
    }
}

#[async_trait]
impl Canceller for PostgresCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.requested.store(true, Ordering::SeqCst);
        match timeout(SESSION_LIMIT, self.send()).await {
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
fn bytea_value(text: &str) -> Value {
    let decoded = text
        .strip_prefix("\\x")
        .filter(|hex| hex.len().is_multiple_of(2))
        .and_then(|hex| {
            (0..hex.len() / 2)
                .map(|index| u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok())
                .collect::<Option<Box<[u8]>>>()
        });
    decoded.map_or_else(|| Value::Text(text.into()), Value::Binary)
}

fn row_values(row: &SimpleQueryRow, kinds: &[ColumnKind]) -> Result<Row, ExecError> {
    let mut cells = Vec::with_capacity(row.len());
    for index in 0..row.len() {
        let text = row
            .try_get(index)
            .map_err(|error| ExecError::new(describe(&error)))?;
        cells.push(match (text, kinds.get(index)) {
            (None, _) => Value::Null,
            (Some(text), Some(ColumnKind::Number)) => Value::Number(text.into()),
            (Some(text), Some(ColumnKind::Binary)) => bytea_value(text),
            (Some(text), _) => Value::Text(text.into()),
        });
    }
    Ok(cells.into_boxed_slice())
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

fn exec_error(error: &tokio_postgres::Error, sql: &str) -> ExecError {
    let Some(database) = error.as_db_error() else {
        return ExecError::new(describe(error));
    };
    let mut message = format!("{} ({})", database.message(), database.code().code());
    if let Some(detail) = database.detail() {
        message = format!("{message}\n{detail}");
    }
    if let Some(hint) = database.hint() {
        message = format!("{message}\n{hint}");
    }
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

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

struct PostgresSession {
    client: Client,
    canceller: Arc<PostgresCanceller>,
    transaction: TransactionState,
}

impl PostgresSession {
    /// Asks for the column types before the statement runs. A statement that fails
    /// here fails in the same way in the simple protocol, and it must not run twice:
    /// the second error would hide the first one in a transaction.
    async fn column_kinds(&self, sql: &str) -> Result<Vec<ColumnKind>, ExecError> {
        if self.transaction == TransactionState::Failed {
            return Ok(Vec::new());
        }
        match self.client.prepare(sql).await {
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
        let stream = self
            .client
            .simple_query_raw(sql)
            .await
            .map_err(|error| exec_error(&error, sql))?;
        let mut stream = pin!(stream);
        let mut outcome = Outcome::default();
        let mut storing = true;
        let mut stopped_at_cap = false;
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
                    let flow = sink.push(row_values(&row, kinds)?).await;
                    storing = flow == Flow::Continue;
                    let free = self.transaction == TransactionState::None;
                    if flow == Flow::Stop && free && is_plain_read(sql, Dialect::Postgres) {
                        stopped_at_cap = self.canceller.cancel().await.is_ok();
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
        match timeout(SESSION_LIMIT, self.client.simple_query(TRANSACTION_PROBE)).await {
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
        let messages = self
            .client
            .simple_query(sql)
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
    fn canceller(&self) -> Arc<dyn Canceller> {
        self.canceller.clone()
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        let mut sql = String::new();
        if context.read_only {
            sql.push_str("SET default_transaction_read_only = on;");
        }
        if let Some(schema) = &context.schema {
            sql.push_str(&format!("SET search_path TO {};", quoted(schema)));
        }
        if sql.is_empty() {
            return Ok(());
        }
        self.client
            .batch_execute(&sql)
            .await
            .map_err(|error| exec_error(&error, &sql))
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        if reads_or_writes_stdio(sql, Dialect::Postgres) {
            return Err(ExecError::new(text::COPY_REFUSED));
        }
        self.canceller.requested.store(false, Ordering::SeqCst);
        let kinds = self.column_kinds(sql).await?;
        if self.canceller.requested.load(Ordering::SeqCst) {
            return Err(ExecError::new(text::STOPPED_BEFORE_SEND));
        }
        let read = self.read(sql, &kinds, &mut sink).await;
        let (rows, limit) = sink.finish().await;
        read.map(|outcome| StatementEnd {
            rows,
            has_columns: outcome.has_columns,
            changed: outcome.changed,
            limit,
        })
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
        self.client
            .batch_execute(sql)
            .await
            .map_err(|error| exec_error(&error, sql))?;
        if command == TransactionCommand::Begin {
            self.transaction = TransactionState::Open;
        }
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        match timeout(SESSION_LIMIT, self.client.batch_execute("DISCARD ALL")).await {
            Ok(Ok(())) => {
                // `DISCARD ALL` removes the statement of the driver cache.
                self.client.clear_type_cache();
                ResetOutcome::Clean
            }
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
                    relations: Vec::new(),
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
                });
            }
        }
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
        let rows = self
            .client
            .query(COLUMNS_SQL, &[&schema, &relation])
            .await
            .map_err(|error| exec_error(&error, COLUMNS_SQL))?;
        rows.iter()
            .map(|row| {
                row.try_get(0)
                    .map_err(|error| ExecError::new(describe(&error)))
            })
            .collect()
    }
}
