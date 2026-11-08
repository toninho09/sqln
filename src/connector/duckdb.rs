//! DuckDB connector. `duckdb` is synchronous, so each session owns one thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use async_trait::async_trait;
use duckdb::arrow::array::Array;
use duckdb::arrow::datatypes::DataType;
use duckdb::arrow::util::display::array_value_to_string;
use duckdb::{AccessMode, Config, Connection, InterruptHandle, ToSql};
use tokio::sync::{Mutex, oneshot};

use super::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState,
};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use crate::command_log::{self, TraceContext};
use crate::exec::{Flow, ResultSink};
use crate::grid::{RowWriter, Value};
use crate::sql::{
    Dialect, TokenKind, changes_state, changes_without_result, counts_rows, is_plain_read,
    quote_identifier, tokenize,
};
use crate::ui::text;

const MAIN_SCHEMA: &str = "main";

/// Where a DuckDB database lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DuckdbSource {
    File(PathBuf),
    /// One database in memory. It lives while the pool holds its only session, and a
    /// disconnect or the end of the application loses it.
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuckdbTarget {
    pub source: DuckdbSource,
    pub allow_create: bool,
    /// The only way to make a DuckDB connection read-only. The engine applies it when the
    /// database opens, and a session cannot change it later.
    pub read_only: bool,
}

impl DuckdbTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            source: DuckdbSource::File(path.into()),
            allow_create: false,
            read_only: false,
        }
    }

    pub fn in_memory() -> Self {
        Self {
            source: DuckdbSource::Memory,
            allow_create: false,
            read_only: false,
        }
    }

    pub fn is_memory(&self) -> bool {
        matches!(self.source, DuckdbSource::Memory)
    }

    /// The path of the file of the source. A database in memory has the `:memory:` text,
    /// which never reaches the driver.
    pub fn path(&self) -> &Path {
        match &self.source {
            DuckdbSource::File(path) => path,
            DuckdbSource::Memory => Path::new(":memory:"),
        }
    }

    /// True when the file of the source exists. A database in memory has no file.
    pub fn source_exists(&self) -> bool {
        matches!(&self.source, DuckdbSource::File(path) if path.exists())
    }

    /// The text of the place of the source.
    pub fn file_display(&self) -> String {
        match &self.source {
            DuckdbSource::File(path) => path.display().to_string(),
            DuckdbSource::Memory => ":memory:".to_owned(),
        }
    }
}

/// Reads the text of a DuckDB file field. An empty text and `:memory:` open one database
/// in memory.
pub fn file_source(text: &str) -> DuckdbSource {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case(":memory:") {
        DuckdbSource::Memory
    } else {
        DuckdbSource::File(PathBuf::from(text))
    }
}

/// The suffixes that DuckDB reads as a file of another format. `Connection::open` of one of
/// them does not fail: it starts one database in memory, so a table of the session would
/// disappear at the end without a word. The user reads these files with `read_csv`,
/// `read_parquet`, or `read_json` from a database.
const EXTERNAL_SUFFIXES: &[&str] = &["csv", "tsv", "parquet", "json", "jsonl", "ndjson"];

/// Refuses a file of another format, and names the statement that reads it.
fn refuse_external_file(path: &Path) -> Result<(), ConnectError> {
    let Some(suffix) = path.extension().and_then(|suffix| suffix.to_str()) else {
        return Ok(());
    };
    let suffix = suffix.to_ascii_lowercase();
    if !EXTERNAL_SUFFIXES.contains(&suffix.as_str()) {
        return Ok(());
    }
    let reader = match suffix.as_str() {
        "parquet" => "read_parquet",
        "json" | "jsonl" | "ndjson" => "read_json",
        _ => "read_csv",
    };
    Err(ConnectError::new(format!(
        "{} is not a DuckDB database. Open a database file, and read this file with \
         SELECT * FROM {reader}('{}').",
        path.display(),
        path.display()
    )))
}

/// Opens the one database of a connector. Every session clones this connection, so all
/// of them share one instance: a second `Connection::open` of the same file would make an
/// isolated instance, and a write of one would not reach the other.
fn open(target: &DuckdbTarget) -> Result<Connection, ConnectError> {
    let mode = if target.read_only {
        AccessMode::ReadOnly
    } else {
        AccessMode::ReadWrite
    };
    let config = Config::default()
        .access_mode(mode)
        .map_err(|error| ConnectError::new(error.to_string()))?;
    match &target.source {
        DuckdbSource::File(path) => {
            refuse_external_file(path)?;
            // The driver creates a missing file, so the permission must come first.
            if !path.exists() && !target.allow_create {
                return Err(ConnectError::new(format!(
                    "The DuckDB file does not exist: {}",
                    path.display()
                )));
            }
            Connection::open_with_flags(path, config)
                .map_err(|error| ConnectError::new(error.to_string()))
        }
        DuckdbSource::Memory => Connection::open_in_memory_with_flags(config)
            .map_err(|error| ConnectError::new(error.to_string())),
    }
}

pub struct DuckdbConnector {
    target: DuckdbTarget,
    /// The thread that owns the database. Every session clones the owner connection, so all
    /// of them share one instance: a second `Connection::open` of the same file would make
    /// an isolated instance, and a write of one would not reach the other.
    owner: Mutex<Option<Owner>>,
}

struct Owner {
    id: u64,
    requests: Sender<OwnerRequest>,
}

/// The owner that one call uses: its identity and its channel. A sender has no stable
/// comparison, so the identity is a number.
#[derive(Clone)]
struct OwnerHandle {
    id: u64,
    requests: Sender<OwnerRequest>,
}

/// Gives each owner thread its identity.
static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);

enum OwnerRequest {
    /// Returns a connection that shares the database of the owner.
    Clone(oneshot::Sender<Result<Connection, ConnectError>>),
}

impl DuckdbConnector {
    pub fn new(target: DuckdbTarget) -> Self {
        Self {
            target,
            owner: Mutex::new(None),
        }
    }

    /// Returns the owner that serves the calls, and starts its thread on the first call.
    /// The lock spans the open, so a second caller waits for the first one instead of
    /// opening the file again: two opens of one file race for its lock.
    async fn owner(&self) -> Result<OwnerHandle, ConnectError> {
        let mut owner = self.owner.lock().await;
        if let Some(owner) = owner.as_ref() {
            return Ok(OwnerHandle {
                id: owner.id,
                requests: owner.requests.clone(),
            });
        }
        let target = self.target.clone();
        let (requests, inbox) = channel();
        let (opened, wait_opened) = oneshot::channel();
        thread::Builder::new()
            .name("duckdb-owner".to_owned())
            .spawn(move || {
                let connection = match open(&target) {
                    Ok(connection) => connection,
                    Err(error) => {
                        let _ = opened.send(Err(error));
                        return;
                    }
                };
                let _ = opened.send(Ok(()));
                serve_owner(&connection, &inbox);
            })
            .map_err(|error| ConnectError::new(error.to_string()))?;
        // The first connection reports a failure of the open, and no owner stays then.
        wait_opened.await.map_err(|_| thread_stopped())??;
        let id = NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed);
        *owner = Some(Owner {
            id,
            requests: requests.clone(),
        });
        Ok(OwnerHandle { id, requests })
    }

    /// Returns a connection that shares the database of the connector. The owner thread
    /// is the only holder of the database, so a file clears a dead owner and starts a
    /// new one once. A database in memory dies with its thread: a restart would hand out
    /// an empty database without a word, so it keeps the failure.
    async fn connection(&self) -> Result<Connection, ConnectError> {
        let memory = self.target.is_memory();
        for _ in 0..2 {
            let handle = self.owner().await?;
            let (reply, answer) = oneshot::channel();
            if handle.requests.send(OwnerRequest::Clone(reply)).is_ok() {
                return answer.await.map_err(|_| thread_stopped())?;
            }
            if memory {
                return Err(thread_stopped());
            }
            let mut owner = self.owner.lock().await;
            // Another caller can start a new owner first. That one stays.
            if owner.as_ref().is_some_and(|entry| entry.id == handle.id) {
                *owner = None;
            }
        }
        Err(thread_stopped())
    }
}

/// The error of a DuckDB thread that no longer answers.
fn thread_stopped() -> ConnectError {
    ConnectError::new("The DuckDB thread stopped.".to_owned())
}

/// Runs in the owner thread. It ends when the connector drops the sender.
fn serve_owner(connection: &Connection, inbox: &Receiver<OwnerRequest>) {
    while let Ok(request) = inbox.recv() {
        match request {
            OwnerRequest::Clone(reply) => {
                let clone = connection
                    .try_clone()
                    .map_err(|error| ConnectError::new(error.to_string()));
                let _ = reply.send(clone);
            }
        }
    }
}

#[async_trait]
impl Connector for DuckdbConnector {
    fn dialect(&self) -> Dialect {
        Dialect::DuckDb
    }

    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities {
            // `main`, plus the schemas that a user creates.
            namespace: super::Namespace::Schema,
            parameters: true,
            // A table tab edits rows through a key, and the compare read that finds the
            // key is not part of this connector yet.
            data_edit: false,
            single_session: self.target.is_memory(),
            ..super::Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        self.connect_traced(None).await
    }

    async fn connect_traced(
        &self,
        trace: Option<TraceContext>,
    ) -> Result<Box<dyn Session>, ConnectError> {
        let memory = self.target.is_memory();
        let database = self.connection().await?;
        let (opened, wait_opened) = oneshot::channel();
        let (requests, inbox) = channel();
        let opening_trace = trace.clone();
        thread::Builder::new()
            .name("duckdb-session".to_owned())
            .spawn(move || {
                let _ = opened.send(Ok(database.interrupt_handle()));
                serve(&database, &inbox, opening_trace);
            })
            .map_err(|error| ConnectError::new(error.to_string()))?;
        let interrupt = wait_opened
            .await
            .map_err(|_| ConnectError::new("The DuckDB thread stopped.".to_owned()))??;
        Ok(Box::new(DuckdbSession {
            requests,
            interrupt: Arc::new(Interrupt(interrupt)),
            memory,
            read_only: self.target.read_only,
            trace,
        }))
    }
}

type Reply<T> = oneshot::Sender<Result<T, ExecError>>;

enum Request {
    Trace(Option<TraceContext>),
    Context(SessionContext, Reply<()>),
    Run(
        String,
        Vec<crate::sql::parameters::Parameter>,
        ResultSink,
        Reply<StatementEnd>,
    ),
    TransactionOpen(oneshot::Sender<TransactionState>),
    Control(TransactionCommand, Reply<()>),
    Catalog(Reply<Catalog>),
    Columns(String, String, Reply<Vec<String>>),
}

fn exec_error(error: duckdb::Error) -> ExecError {
    ExecError::new(error.to_string())
}

/// Runs in the session thread. It ends when the session drops its sender.
fn serve(conn: &Connection, inbox: &Receiver<Request>, mut trace: Option<TraceContext>) {
    // The connection cannot report its transaction state, so the thread keeps it.
    let mut transaction = TransactionState::None;
    while let Ok(request) = inbox.recv() {
        match request {
            Request::Trace(context) => trace = context,
            Request::Context(context, reply) => {
                let _ = reply.send(apply_context(conn, &context, &trace));
            }
            Request::Run(sql, parameters, sink, reply) => {
                let command = crate::sql::parameters::command(&sql, &parameters);
                let result = command_log::observe_blocking(&trace, command, || {
                    run(conn, &sql, &parameters, sink)
                });
                transaction = next_state(transaction, &sql, &result);
                let _ = reply.send(result);
            }
            Request::TransactionOpen(reply) => {
                let _ = reply.send(transaction);
            }
            Request::Control(command, reply) => {
                let sql = match command {
                    TransactionCommand::Begin => "BEGIN",
                    TransactionCommand::Commit => "COMMIT",
                    TransactionCommand::Rollback => "ROLLBACK",
                };
                let result = command_log::observe_blocking(&trace, sql, || conn.execute_batch(sql))
                    .map_err(exec_error);
                if result.is_ok() {
                    transaction = match command {
                        TransactionCommand::Begin => TransactionState::Open,
                        TransactionCommand::Commit | TransactionCommand::Rollback => {
                            TransactionState::None
                        }
                    };
                }
                let _ = reply.send(result);
            }
            Request::Catalog(reply) => {
                let _ = reply.send(read_catalog(conn, &trace).map_err(exec_error));
            }
            Request::Columns(schema, relation, reply) => {
                let _ =
                    reply.send(read_columns(conn, &schema, &relation, &trace).map_err(exec_error));
            }
        }
    }
}

/// Applies the schema of a console. `read_only` cannot change an open database, so a
/// writable connection keeps it in the runner, which refuses a changing statement.
fn apply_context(
    conn: &Connection,
    context: &SessionContext,
    trace: &Option<TraceContext>,
) -> Result<(), ExecError> {
    let sql = match &context.schema {
        Some(schema) => format!("SET schema = {}", quote_identifier(schema, Dialect::DuckDb)),
        None => "RESET schema".to_owned(),
    };
    command_log::observe_blocking(trace, &sql, || conn.execute_batch(&sql)).map_err(exec_error)
}

/// The presentation of a column. DuckDB reports each result column with an Arrow type.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnKind {
    Text,
    Number,
    Binary,
}

fn column_kind(data_type: &DataType) -> ColumnKind {
    match data_type {
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView => ColumnKind::Binary,
        DataType::FixedSizeBinary(_) => ColumnKind::Binary,
        other if other.is_numeric() => ColumnKind::Number,
        _ => ColumnKind::Text,
    }
}

/// Writes a cell from the Arrow array of its column. The text comes from Arrow, so a
/// list, a struct, a timestamp, and every other type keep the text of the database.
fn write_cell(cells: &mut RowWriter, column: &dyn Array, row: usize, kind: ColumnKind) {
    // A column of the type `Null` holds NULL values without a null bit.
    if column.is_null(row) || matches!(column.data_type(), DataType::Null) {
        return cells.push(Value::Null);
    }
    let Ok(text) = array_value_to_string(column, row) else {
        return cells.push(Value::Null);
    };
    match kind {
        ColumnKind::Number => cells.push(Value::Number(&text)),
        ColumnKind::Text => cells.push(Value::Text(&text)),
        // Arrow writes a binary value as hexadecimal digits.
        ColumnKind::Binary => {
            let bytes = decode_hex(&text);
            cells.push(Value::binary(&bytes));
        }
    }
}

/// Reads the hexadecimal text of Arrow back into the bytes of the value.
fn decode_hex(text: &str) -> Vec<u8> {
    let digits = text.as_bytes();
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    let value = |byte: u8| -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    };
    for pair in digits.as_chunks::<2>().0 {
        match (value(pair[0]), value(pair[1])) {
            (Some(high), Some(low)) => bytes.push(high << 4 | low),
            _ => return Vec::new(),
        }
    }
    bytes
}

/// Reads the state of the transaction. The driver reports `is_autocommit` as a constant,
/// so the session keeps the state. DuckDB accepts no statement inside a transaction that
/// an error aborted, and only a rollback ends it.
fn next_state(
    state: TransactionState,
    sql: &str,
    result: &Result<StatementEnd, ExecError>,
) -> TransactionState {
    match result {
        Err(_) => match state {
            TransactionState::Open => TransactionState::Failed,
            other => other,
        },
        Ok(_) => match transaction_word(sql) {
            Some(Control::Begin) => TransactionState::Open,
            Some(Control::End) => TransactionState::None,
            None => state,
        },
    }
}

enum Control {
    Begin,
    End,
}

/// Returns the transaction command that a statement runs.
fn transaction_word(sql: &str) -> Option<Control> {
    let tokens = tokenize(sql, Dialect::DuckDb);
    let word = tokens
        .iter()
        .find(|token| token.kind != TokenKind::Comment)
        .filter(|token| token.kind == TokenKind::Word)
        .map(|token| &sql[token.start()..token.end()])
        .unwrap_or("");
    match word.to_ascii_uppercase().as_str() {
        "BEGIN" | "START" => Some(Control::Begin),
        "COMMIT" | "END" | "ROLLBACK" | "ABORT" => Some(Control::End),
        _ => None,
    }
}

fn parameter_value(parameter: &crate::sql::parameters::Parameter) -> Box<dyn ToSql> {
    use crate::sql::parameters::Kind;
    match parameter.kind {
        Kind::Null => Box::new(duckdb::types::Null),
        Kind::Boolean => Box::new(parameter.value == "true"),
        // A text value in a number keeps its digits: the binding of a text would make the
        // database compare text with the type of the column, and a large integer loses
        // precision on the way. A number out of the range of `i128` keeps its text.
        Kind::Number => match parameter.value.parse::<i128>() {
            Ok(number) => Box::new(number),
            Err(_) => match parameter.value.parse::<f64>() {
                Ok(number) => Box::new(number),
                Err(_) => Box::new(parameter.value.clone()),
            },
        },
        Kind::Text => Box::new(parameter.value.clone()),
    }
}

fn run(
    conn: &Connection,
    sql: &str,
    parameters: &[crate::sql::parameters::Parameter],
    mut sink: ResultSink,
) -> Result<StatementEnd, ExecError> {
    let mut statement = conn.prepare(sql).map_err(exec_error)?;
    let values: Vec<Box<dyn ToSql>> = parameters.iter().map(parameter_value).collect();
    // The prepared statement tells the shape, but DuckDB answers every row change with
    // one `Count` column, also under `WITH`: a change without `RETURNING` goes through
    // `execute`, because the `Count` row belongs to the statement, not to the user.
    if statement.column_count() == 0 || changes_without_result(sql, Dialect::DuckDb) {
        let bound = values.iter().map(|value| value.as_ref() as &dyn ToSql);
        let changed = statement
            .execute(duckdb::params_from_iter(bound))
            .map_err(exec_error)?;
        let changed = counts_rows(sql, Dialect::DuckDb).then_some(changed as u64);
        let (rows, limit) = sink.finish_blocking();
        return Ok(StatementEnd {
            rows,
            has_columns: false,
            changed,
            limit,
        });
    }
    let bound = values.iter().map(|value| value.as_ref() as &dyn ToSql);
    // The stream keeps the result on the server side, so the row cap stops a large read
    // before the client holds every row.
    let batches = statement
        .stream_arrow(duckdb::params_from_iter(bound))
        .map_err(exec_error)?;
    let schema = batches.get_schema();
    let kinds: Vec<ColumnKind> = schema
        .fields()
        .iter()
        .map(|field| column_kind(field.data_type()))
        .collect();
    sink.set_columns_blocking(schema.fields().iter().map(|f| f.name().clone()).collect());
    let read_only = is_plain_read(sql, Dialect::DuckDb);
    let mut cells = RowWriter::default();
    let mut storing = true;
    let mut stopped = false;
    for batch in batches {
        let batch = batch.map_err(exec_error)?;
        if !storing {
            continue;
        }
        for row in 0..batch.num_rows() {
            for (index, kind) in kinds.iter().enumerate() {
                write_cell(&mut cells, batch.column(index).as_ref(), row, *kind);
            }
            if sink.push_blocking(cells.finish()) != Flow::Continue {
                // A read ends in the client. A change that returns rows runs to its end,
                // because only the database can finish it.
                if read_only {
                    stopped = true;
                    break;
                }
                storing = false;
            }
        }
        if stopped {
            break;
        }
    }
    let (rows, limit) = sink.finish_blocking();
    Ok(StatementEnd {
        rows,
        has_columns: true,
        changed: None,
        limit,
    })
}

fn read_catalog(conn: &Connection, trace: &Option<TraceContext>) -> duckdb::Result<Catalog> {
    // The current schema is the one of the session, which a console changes with
    // `SET schema`, as PostgreSQL reads it.
    let current: Option<String> =
        command_log::observe_blocking(trace, "SELECT current_schema()", || {
            conn.query_row("SELECT current_schema()", [], |row| row.get(0))
        })?;
    // `duckdb_schemas` lists one row per catalog, so `main` repeats. The two schemas of
    // DuckDB itself stay out.
    let sql = "SELECT DISTINCT schema_name FROM duckdb_schemas() \
               WHERE schema_name NOT IN ('information_schema', 'pg_catalog') \
               ORDER BY 1";
    let names: Vec<String> = command_log::observe_blocking(trace, sql, || {
        let mut statement = conn.prepare(sql)?;
        let names = statement.query_map([], |row| row.get(0))?;
        names.collect()
    })?;

    let mut schemas = Vec::with_capacity(names.len());
    for name in names {
        let literal = crate::sql::quote_literal(&name, Dialect::DuckDb);
        let sql = format!(
            "SELECT table_name, 'table' FROM duckdb_tables() WHERE schema_name = {literal} \
             UNION ALL \
             SELECT view_name, 'view' FROM duckdb_views() \
             WHERE schema_name = {literal} AND NOT internal \
             ORDER BY 1"
        );
        let relations = command_log::observe_blocking(trace, &sql, || {
            let mut statement = conn.prepare(&sql)?;
            let relations = statement.query_map([], |row| {
                let kind: String = row.get(1)?;
                Ok(Relation {
                    name: row.get(0)?,
                    kind: if kind == "view" {
                        RelationKind::View
                    } else {
                        RelationKind::Table
                    },
                    columns: Columns::NotLoaded,
                    foreign_keys: Vec::new(),
                    referencing: Vec::new(),
                })
            })?;
            relations.collect()
        })?;
        schemas.push(Schema {
            name,
            relations,
            ..Schema::default()
        });
    }
    Ok(Catalog {
        schemas,
        current_schema: Some(current.unwrap_or_else(|| MAIN_SCHEMA.to_owned())),
    })
}

fn read_columns(
    conn: &Connection,
    schema: &str,
    relation: &str,
    trace: &Option<TraceContext>,
) -> duckdb::Result<Vec<String>> {
    let sql = "SELECT column_name FROM duckdb_columns() \
               WHERE schema_name = ?2 AND table_name = ?1 \
               ORDER BY column_index";
    command_log::observe_blocking(
        trace,
        format_args!("{sql}\nParameters: ?1={relation:?}, ?2={schema:?}"),
        || {
            let mut statement = conn.prepare(sql)?;
            let names = statement.query_map((relation, schema), |row| row.get(0))?;
            names.collect()
        },
    )
}

struct Interrupt(Arc<InterruptHandle>);

#[async_trait]
impl Canceller for Interrupt {
    async fn cancel(&self) -> Result<(), String> {
        self.cancel_traced(None).await
    }
    async fn cancel_traced(&self, trace: Option<TraceContext>) -> Result<(), String> {
        command_log::observe(trace, "duckdb_interrupt", async {
            self.0.interrupt();
            Ok(())
        })
        .await
    }
}

struct DuckdbSession {
    trace: Option<TraceContext>,
    requests: Sender<Request>,
    interrupt: Arc<Interrupt>,
    /// True when the database of the session lives in memory, so the session stays.
    memory: bool,
    /// The read-only choice of the connection arrives at each reservation. A DuckDB
    /// connection cannot change its access mode, so a write is refused here.
    read_only: bool,
}

impl DuckdbSession {
    async fn ask<T>(
        &self,
        request: Request,
        answer: oneshot::Receiver<Result<T, ExecError>>,
    ) -> Result<T, ExecError> {
        self.requests
            .send(request)
            .map_err(|_| ExecError::new("The DuckDB thread stopped."))?;
        answer
            .await
            .map_err(|_| ExecError::new("The DuckDB thread stopped."))?
    }

    /// Refuses a statement that changes data on a read-only connection. A connection
    /// that opened its file with `access_mode = READ_ONLY` already refuses it.
    fn refuse_write(&self, sql: &str) -> Result<(), ExecError> {
        if self.read_only && changes_state(sql, Dialect::DuckDb) {
            return Err(ExecError::new(text::DUCKDB_READ_ONLY));
        }
        Ok(())
    }
}

#[async_trait]
impl Session for DuckdbSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.trace = trace.clone();
        let _ = self.requests.send(Request::Trace(trace));
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        command_log::canceller(self.interrupt.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.read_only = context.read_only;
        let (reply, answer) = oneshot::channel();
        self.ask(Request::Context(context.clone(), reply), answer)
            .await
    }

    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.refuse_write(sql)?;
        let (reply, answer) = oneshot::channel();
        self.ask(
            Request::Run(sql.to_owned(), Vec::new(), sink, reply),
            answer,
        )
        .await
    }

    async fn run_parameters(
        &mut self,
        sql: &str,
        parameters: &[crate::sql::parameters::Parameter],
        sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        self.refuse_write(sql)?;
        let (reply, answer) = oneshot::channel();
        self.ask(
            Request::Run(sql.to_owned(), parameters.to_vec(), sink, reply),
            answer,
        )
        .await
    }

    async fn transaction_state(&mut self) -> TransactionState {
        let (reply, answer) = oneshot::channel();
        if self.requests.send(Request::TransactionOpen(reply)).is_err() {
            return TransactionState::Unknown;
        }
        answer.await.unwrap_or(TransactionState::Unknown)
    }

    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError> {
        let (reply, answer) = oneshot::channel();
        self.ask(Request::Control(command, reply), answer).await
    }

    /// A new connection is the only cleanup that removes temporary tables. A database
    /// in memory lives in this session, so the session stays and the cleanup keeps it.
    async fn reset(&mut self) -> ResetOutcome {
        if self.memory {
            ResetOutcome::Clean
        } else {
            ResetOutcome::Discard
        }
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let (reply, answer) = oneshot::channel();
        self.ask(Request::Catalog(reply), answer).await
    }

    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        let (reply, answer) = oneshot::channel();
        let request = Request::Columns(schema.to_owned(), relation.to_owned(), reply);
        self.ask(request, answer).await
    }
}
