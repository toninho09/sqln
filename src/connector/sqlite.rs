//! SQLite connector. `rusqlite` is synchronous, so each session owns one thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, InterruptHandle, OpenFlags};
use tokio::sync::oneshot;

use super::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState,
};
use crate::catalog::{
    Catalog, Columns, ForeignKey, KeyColumn, Relation, RelationKind, RelationRef, Schema,
    add_objects, group_foreign_keys,
};
use crate::command_log::{self, TraceContext};
use crate::exec::{Flow, ResultSink};
use crate::grid::{RowWriter, Value};
use crate::sql::Dialect;

const MAIN_SCHEMA: &str = "main";

/// Where a SQLite database lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqliteSource {
    File(PathBuf),
    /// One database in memory. It lives while the pool holds its only session, and a
    /// disconnect or the end of the application loses it.
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteTarget {
    pub source: SqliteSource,
    pub allow_create: bool,
    /// `None` keeps the default of SQLite.
    pub foreign_keys: Option<bool>,
    pub busy_timeout: Duration,
}

impl SqliteTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            source: SqliteSource::File(path.into()),
            allow_create: false,
            foreign_keys: None,
            busy_timeout: Duration::from_secs(5),
        }
    }

    pub fn in_memory() -> Self {
        Self {
            source: SqliteSource::Memory,
            allow_create: false,
            foreign_keys: None,
            busy_timeout: Duration::from_secs(5),
        }
    }

    pub fn is_memory(&self) -> bool {
        matches!(self.source, SqliteSource::Memory)
    }

    /// The path of the file of the source. A database in memory has the `:memory:`
    /// text, which never reaches the driver.
    pub fn path(&self) -> &Path {
        match &self.source {
            SqliteSource::File(path) => path,
            SqliteSource::Memory => Path::new(":memory:"),
        }
    }

    /// True when the file of the source exists. A database in memory has no file.
    pub fn source_exists(&self) -> bool {
        matches!(&self.source, SqliteSource::File(path) if path.exists())
    }

    /// The text of the place of the source.
    pub fn file_display(&self) -> String {
        match &self.source {
            SqliteSource::File(path) => path.display().to_string(),
            SqliteSource::Memory => ":memory:".to_owned(),
        }
    }
}

/// Reads the text of a SQLite file field. `:memory:` opens one database in memory.
pub fn file_source(text: &str) -> SqliteSource {
    if text.trim().eq_ignore_ascii_case(":memory:") {
        SqliteSource::Memory
    } else {
        SqliteSource::File(PathBuf::from(text))
    }
}

/// Refuses the text forms that open a database in memory.
pub fn validate_path(path: &Path) -> Result<(), ConnectError> {
    let text = path.to_string_lossy();
    let lower = text.trim().to_lowercase();
    let in_memory = lower.is_empty()
        || lower == ":memory:"
        || lower.starts_with("file:")
        || lower.contains("mode=memory");
    if in_memory {
        return Err(ConnectError::new(format!(
            "SQLite needs a file. This path is not valid: {text}"
        )));
    }
    Ok(())
}

fn open(target: &SqliteTarget, trace: &Option<TraceContext>) -> Result<Connection, ConnectError> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if target.allow_create || target.is_memory() {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let failed = |error: rusqlite::Error| ConnectError::new(error.to_string());
    let conn = match &target.source {
        SqliteSource::File(path) => {
            validate_path(path)?;
            Connection::open_with_flags(path, flags).map_err(failed)?
        }
        SqliteSource::Memory => Connection::open_in_memory_with_flags(flags).map_err(failed)?,
    };
    conn.busy_timeout(target.busy_timeout).map_err(failed)?;
    if let Some(enabled) = target.foreign_keys {
        let sql = format!(
            "PRAGMA foreign_keys = {}",
            if enabled { "ON" } else { "OFF" }
        );
        command_log::observe_blocking(trace, &sql, || conn.execute_batch(&sql)).map_err(failed)?;
    }
    Ok(conn)
}

pub struct SqliteConnector {
    target: SqliteTarget,
}

impl SqliteConnector {
    pub fn new(target: SqliteTarget) -> Self {
        Self { target }
    }
}

#[async_trait]
impl Connector for SqliteConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    fn capabilities(&self) -> super::Capabilities {
        super::Capabilities {
            parameters: true,
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
        let target = self.target.clone();
        let (opened, wait_opened) = oneshot::channel();
        let (requests, inbox) = channel();
        let opening_trace = trace.clone();
        thread::Builder::new()
            .name("sqlite-session".to_owned())
            .spawn(move || match open(&target, &opening_trace) {
                Ok(conn) => {
                    let _ = opened.send(Ok(conn.get_interrupt_handle()));
                    serve(&conn, &inbox, opening_trace);
                }
                Err(error) => {
                    let _ = opened.send(Err(error));
                }
            })
            .map_err(|error| ConnectError::new(error.to_string()))?;
        let interrupt = wait_opened
            .await
            .map_err(|_| ConnectError::new("The SQLite thread stopped.".to_owned()))??;
        Ok(Box::new(SqliteSession {
            requests,
            interrupt: Arc::new(Interrupt(interrupt)),
            memory,
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
    TransactionOpen(oneshot::Sender<bool>),
    Control(TransactionCommand, Reply<()>),
    Catalog(Reply<Catalog>),
    Columns(String, String, Reply<Vec<String>>),
    ForeignKeys(String, String, Reply<Vec<ForeignKey>>),
    ReferencingKeys(String, String, Reply<Vec<ForeignKey>>),
}

fn exec_error(error: rusqlite::Error) -> ExecError {
    let offset = match &error {
        rusqlite::Error::SqlInputError { offset, .. } => usize::try_from(*offset).ok(),
        _ => None,
    };
    ExecError {
        message: error.to_string(),
        offset,
    }
}

/// Runs in the session thread. It ends when the session drops its sender.
fn serve(conn: &Connection, inbox: &Receiver<Request>, mut trace: Option<TraceContext>) {
    while let Ok(request) = inbox.recv() {
        match request {
            Request::Trace(context) => trace = context,
            Request::Context(context, reply) => {
                let sql = format!(
                    "PRAGMA query_only = {}",
                    if context.read_only { "ON" } else { "OFF" }
                );
                let _ = reply.send(
                    command_log::observe_blocking(&trace, &sql, || conn.execute_batch(&sql))
                        .map_err(exec_error),
                );
            }
            Request::Run(sql, parameters, sink, reply) => {
                let command = crate::sql::parameters::command(&sql, &parameters);
                let _ = reply.send(command_log::observe_blocking(&trace, command, || {
                    run(conn, &sql, &parameters, sink)
                }));
            }
            Request::TransactionOpen(reply) => {
                let _ = reply.send(!conn.is_autocommit());
            }
            Request::Control(command, reply) => {
                let sql = match command {
                    TransactionCommand::Begin => "BEGIN",
                    TransactionCommand::Commit => "COMMIT",
                    TransactionCommand::Rollback => "ROLLBACK",
                };
                let _ = reply.send(
                    command_log::observe_blocking(&trace, sql, || conn.execute_batch(sql))
                        .map_err(exec_error),
                );
            }
            Request::Catalog(reply) => {
                let _ = reply.send(read_catalog(conn, &trace).map_err(exec_error));
            }
            Request::Columns(schema, relation, reply) => {
                let _ =
                    reply.send(read_columns(conn, &schema, &relation, &trace).map_err(exec_error));
            }
            Request::ForeignKeys(schema, relation, reply) => {
                let keys = read_keys(conn, FOREIGN_KEYS_SQL, &schema, &relation, &trace);
                let _ = reply.send(keys.map_err(exec_error));
            }
            Request::ReferencingKeys(schema, relation, reply) => {
                let keys = read_keys(conn, REFERENCING_KEYS_SQL, &schema, &relation, &trace);
                let _ = reply.send(keys.map_err(exec_error));
            }
        }
    }
}

fn write_value(row: &mut RowWriter, cell: ValueRef<'_>) {
    match cell {
        ValueRef::Null => row.push(Value::Null),
        ValueRef::Integer(number) => row.push(Value::Number(&number.to_string())),
        ValueRef::Real(number) => row.push(Value::Number(&format!("{number:?}"))),
        // A text value is not always valid UTF-8, so the bytes keep the data.
        ValueRef::Text(bytes) => row.push(match std::str::from_utf8(bytes) {
            Ok(text) => Value::Text(text),
            Err(_) => Value::binary(bytes),
        }),
        ValueRef::Blob(bytes) => row.push(Value::binary(bytes)),
    }
}

/// Reads the rows. The sink keeps the rows of a read that fails in the middle.
fn read_rows(
    statement: &mut rusqlite::Statement<'_>,
    read_only: bool,
    sink: &mut ResultSink,
) -> Result<(), ExecError> {
    let column_count = statement.column_count();
    let mut storing = true;
    let mut cells = RowWriter::default();
    let mut rows = statement.raw_query();
    while let Some(row) = rows.next().map_err(exec_error)? {
        if !storing {
            continue;
        }
        for index in 0..column_count {
            write_value(&mut cells, row.get_ref(index).map_err(exec_error)?);
        }
        if sink.push_blocking(cells.finish()) != Flow::Continue {
            // A read ends in the client, and it needs no cancellation here.
            if read_only {
                break;
            }
            // Only the database ends a change, so the loop reads the other rows.
            storing = false;
        }
    }
    Ok(())
}

fn run(
    conn: &Connection,
    sql: &str,
    parameters: &[crate::sql::parameters::Parameter],
    mut sink: ResultSink,
) -> Result<StatementEnd, ExecError> {
    use crate::sql::parameters::Kind;
    let mut statement = conn.prepare(sql).map_err(exec_error)?;
    for (index, parameter) in parameters.iter().enumerate() {
        let value = match parameter.kind {
            Kind::Null => rusqlite::types::Value::Null,
            Kind::Boolean => rusqlite::types::Value::Integer(i64::from(parameter.value == "true")),
            Kind::Text | Kind::Number => rusqlite::types::Value::Text(parameter.value.clone()),
        };
        statement
            .raw_bind_parameter(index + 1, value)
            .map_err(exec_error)?;
    }
    let read_only = statement.readonly();
    let has_columns = statement.column_count() > 0;
    if has_columns {
        sink.set_columns_blocking(statement.column_names().map(str::to_owned).collect());
    }
    let read = read_rows(&mut statement, read_only, &mut sink);
    drop(statement);

    let changed = (!read_only && !has_columns).then(|| conn.changes());
    let (rows, limit) = sink.finish_blocking();
    read.map(|()| StatementEnd {
        rows,
        has_columns,
        changed,
        limit,
    })
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn read_catalog(conn: &Connection, trace: &Option<TraceContext>) -> rusqlite::Result<Catalog> {
    let sql = "SELECT name FROM pragma_database_list WHERE name <> 'temp'";
    let names: Vec<String> = command_log::observe_blocking(trace, sql, || {
        let mut names = conn.prepare(sql)?;
        names
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()
    })?;

    let mut schemas = Vec::with_capacity(names.len());
    let mut objects = Vec::new();
    for name in names {
        let sql = format!(
            "SELECT name, type FROM {}.sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name",
            quoted(&name),
        );
        let relations = command_log::observe_blocking(trace, &sql, || {
            let mut statement = conn.prepare(&sql)?;
            statement
                .query_map([], |row| {
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
                })?
                .collect::<rusqlite::Result<_>>()
        })?;
        let sql = format!(
            "SELECT name, tbl_name FROM {}.sqlite_schema WHERE type = 'trigger'",
            quoted(&name),
        );
        let triggers: Vec<[String; 4]> = command_log::observe_blocking(trace, &sql, || {
            let mut statement = conn.prepare(&sql)?;
            statement
                .query_map([], |row| {
                    Ok([name.clone(), row.get(0)?, "trigger".to_owned(), row.get(1)?])
                })?
                .collect::<rusqlite::Result<_>>()
        })?;
        schemas.push(Schema {
            name,
            relations,
            ..Schema::default()
        });
        objects.extend(triggers);
    }
    add_objects(&mut schemas, objects);
    Ok(Catalog {
        schemas,
        current_schema: Some(MAIN_SCHEMA.to_owned()),
    })
}

fn read_columns(
    conn: &Connection,
    schema: &str,
    relation: &str,
    trace: &Option<TraceContext>,
) -> rusqlite::Result<Vec<String>> {
    let sql = "SELECT name FROM pragma_table_info(?1, ?2)";
    command_log::observe_blocking(
        trace,
        format_args!("{sql}\nParameters: ?1={relation:?}, ?2={schema:?}"),
        || {
            let mut statement = conn.prepare(sql)?;
            statement
                .query_map([relation, schema], |row| row.get(0))?
                .collect()
        },
    )
}

/// Reads the keys of a relation. A key without target columns points to the primary key
/// of its table, and it stays out of the list.
/// The keys that leave a relation. A key without target columns reads the primary key of
/// its target.
const FOREIGN_KEYS_SQL: &str = "\
    SELECT f.id, f.\"table\", f.\"from\", coalesce(f.\"to\", p.name) \
    FROM pragma_foreign_key_list(?1, ?2) f \
    LEFT JOIN pragma_table_info(f.\"table\", ?2) p ON f.\"to\" IS NULL AND p.pk = f.seq + 1 \
    WHERE coalesce(f.\"to\", p.name) IS NOT NULL \
    ORDER BY f.id, f.seq";

/// The keys of other tables that reach a relation, with the shape of `FOREIGN_KEYS_SQL`:
/// the column of the relation comes before the column of the other table.
const REFERENCING_KEYS_SQL: &str = "\
    SELECT f.id, t.name, coalesce(f.\"to\", p.name), f.\"from\" \
    FROM pragma_table_list t \
    JOIN pragma_foreign_key_list(t.name, t.schema) f \
    LEFT JOIN pragma_table_info(?1, ?2) p ON f.\"to\" IS NULL AND p.pk = f.seq + 1 \
    WHERE t.schema = ?2 AND t.type = 'table' AND f.\"table\" = ?1 COLLATE NOCASE \
      AND coalesce(f.\"to\", p.name) IS NOT NULL \
    ORDER BY t.name, f.id, f.seq";

/// Reads the keys of a key query: the number, the other table, the column of the
/// relation, and the column of the other table. A key stays in its schema.
fn read_keys(
    conn: &Connection,
    sql: &str,
    schema: &str,
    relation: &str,
    trace: &Option<TraceContext>,
) -> rusqlite::Result<Vec<ForeignKey>> {
    command_log::observe_blocking(
        trace,
        format_args!("{sql}\nParameters: ?1={relation:?}, ?2={schema:?}"),
        || {
            let mut statement = conn.prepare(sql)?;
            let pairs = statement.query_map([relation, schema], |row| {
                Ok(KeyColumn {
                    key: row.get::<_, i64>(0)?.to_string(),
                    target: RelationRef {
                        schema: schema.to_owned(),
                        relation: row.get(1)?,
                    },
                    column: row.get(2)?,
                    target_column: row.get(3)?,
                })
            })?;
            let pairs: rusqlite::Result<Vec<KeyColumn>> = pairs.collect();
            pairs.map(group_foreign_keys)
        },
    )
}

struct Interrupt(InterruptHandle);

#[async_trait]
impl Canceller for Interrupt {
    async fn cancel(&self) -> Result<(), String> {
        self.cancel_traced(None).await
    }
    async fn cancel_traced(&self, trace: Option<TraceContext>) -> Result<(), String> {
        command_log::observe(trace, "sqlite3_interrupt", async {
            self.0.interrupt();
            Ok(())
        })
        .await
    }
}

struct SqliteSession {
    trace: Option<TraceContext>,
    requests: Sender<Request>,
    interrupt: Arc<Interrupt>,
    /// True when the database of the session lives in memory, so the session stays.
    memory: bool,
}

impl SqliteSession {
    async fn ask<T>(
        &self,
        request: Request,
        answer: oneshot::Receiver<Result<T, ExecError>>,
    ) -> Result<T, ExecError> {
        self.requests
            .send(request)
            .map_err(|_| ExecError::new("The SQLite thread stopped."))?;
        answer
            .await
            .map_err(|_| ExecError::new("The SQLite thread stopped."))?
    }
}

#[async_trait]
impl Session for SqliteSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.trace = trace.clone();
        let _ = self.requests.send(Request::Trace(trace));
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        command_log::canceller(self.interrupt.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        let (reply, answer) = oneshot::channel();
        self.ask(Request::Context(context.clone(), reply), answer)
            .await
    }

    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
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
        match answer.await {
            Ok(true) => TransactionState::Open,
            Ok(false) => TransactionState::None,
            Err(_) => TransactionState::Unknown,
        }
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

    async fn read_foreign_keys(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        let (reply, answer) = oneshot::channel();
        let request = Request::ForeignKeys(schema.to_owned(), relation.to_owned(), reply);
        self.ask(request, answer).await
    }

    async fn read_referencing_keys(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        let (reply, answer) = oneshot::channel();
        let request = Request::ReferencingKeys(schema.to_owned(), relation.to_owned(), reply);
        self.ask(request, answer).await
    }
}
