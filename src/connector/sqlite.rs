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
    group_foreign_keys,
};
use crate::exec::{Flow, ResultSink};
use crate::grid::{RowWriter, Value};
use crate::sql::Dialect;

const MAIN_SCHEMA: &str = "main";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqliteTarget {
    pub path: PathBuf,
    pub allow_create: bool,
    /// `None` keeps the default of SQLite.
    pub foreign_keys: Option<bool>,
    pub busy_timeout: Duration,
}

impl SqliteTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            allow_create: false,
            foreign_keys: None,
            busy_timeout: Duration::from_secs(5),
        }
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

fn open(target: &SqliteTarget) -> Result<Connection, ConnectError> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if target.allow_create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let failed = |error: rusqlite::Error| ConnectError::new(error.to_string());
    let conn = Connection::open_with_flags(&target.path, flags).map_err(failed)?;
    conn.busy_timeout(target.busy_timeout).map_err(failed)?;
    if let Some(enabled) = target.foreign_keys {
        conn.pragma_update(None, "foreign_keys", enabled)
            .map_err(failed)?;
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

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        validate_path(&self.target.path)?;
        let target = self.target.clone();
        let (opened, wait_opened) = oneshot::channel();
        let (requests, inbox) = channel();
        thread::Builder::new()
            .name("sqlite-session".to_owned())
            .spawn(move || match open(&target) {
                Ok(conn) => {
                    let _ = opened.send(Ok(conn.get_interrupt_handle()));
                    serve(&conn, &inbox);
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
        }))
    }
}

type Reply<T> = oneshot::Sender<Result<T, ExecError>>;

enum Request {
    Context(SessionContext, Reply<()>),
    Run(String, ResultSink, Reply<StatementEnd>),
    TransactionOpen(oneshot::Sender<bool>),
    Control(TransactionCommand, Reply<()>),
    Catalog(Reply<Catalog>),
    Columns(String, String, Reply<Vec<String>>),
    ForeignKeys(String, String, Reply<Vec<ForeignKey>>),
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
fn serve(conn: &Connection, inbox: &Receiver<Request>) {
    while let Ok(request) = inbox.recv() {
        match request {
            Request::Context(context, reply) => {
                let _ = reply.send(apply_context(conn, &context));
            }
            Request::Run(sql, sink, reply) => {
                let _ = reply.send(run(conn, &sql, sink));
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
                let _ = reply.send(conn.execute_batch(sql).map_err(exec_error));
            }
            Request::Catalog(reply) => {
                let _ = reply.send(read_catalog(conn).map_err(exec_error));
            }
            Request::Columns(schema, relation, reply) => {
                let _ = reply.send(read_columns(conn, &schema, &relation).map_err(exec_error));
            }
            Request::ForeignKeys(schema, relation, reply) => {
                let keys = read_foreign_keys(conn, &schema, &relation);
                let _ = reply.send(keys.map_err(exec_error));
            }
        }
    }
}

fn apply_context(conn: &Connection, context: &SessionContext) -> Result<(), ExecError> {
    conn.pragma_update(None, "query_only", context.read_only)
        .map_err(exec_error)
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

fn run(conn: &Connection, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
    let mut statement = conn.prepare(sql).map_err(exec_error)?;
    let read_only = statement.readonly();
    let has_columns = statement.column_count() > 0;
    if has_columns {
        sink.set_columns_blocking(
            statement
                .column_names()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        );
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

fn read_catalog(conn: &Connection) -> rusqlite::Result<Catalog> {
    let mut names = conn.prepare("SELECT name FROM pragma_database_list WHERE name <> 'temp'")?;
    let names: Vec<String> = names
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let mut schemas = Vec::with_capacity(names.len());
    for name in names {
        let sql = format!(
            "SELECT name, type FROM {}.sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name",
            quoted(&name),
        );
        let mut statement = conn.prepare(&sql)?;
        let relations = statement
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
                })
            })?
            .collect::<Result<_, _>>()?;
        schemas.push(Schema {
            name,
            relations,
            partial: false,
        });
    }
    Ok(Catalog {
        schemas,
        current_schema: Some(MAIN_SCHEMA.to_owned()),
    })
}

fn read_columns(conn: &Connection, schema: &str, relation: &str) -> rusqlite::Result<Vec<String>> {
    let sql = "SELECT name FROM pragma_table_info(?1, ?2)";
    let mut statement = conn.prepare(sql)?;
    let names = statement.query_map([relation, schema], |row| row.get(0))?;
    names.collect()
}

/// Reads the keys of a relation. A key without target columns points to the primary key
/// of its table, and it stays out of the list.
fn read_foreign_keys(
    conn: &Connection,
    schema: &str,
    relation: &str,
) -> rusqlite::Result<Vec<ForeignKey>> {
    let sql = "SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1, ?2) \
               WHERE \"to\" IS NOT NULL ORDER BY id, seq";
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
}

struct Interrupt(InterruptHandle);

#[async_trait]
impl Canceller for Interrupt {
    async fn cancel(&self) -> Result<(), String> {
        self.0.interrupt();
        Ok(())
    }
}

struct SqliteSession {
    requests: Sender<Request>,
    interrupt: Arc<Interrupt>,
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
    fn canceller(&self) -> Arc<dyn Canceller> {
        self.interrupt.clone()
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        let (reply, answer) = oneshot::channel();
        self.ask(Request::Context(context.clone(), reply), answer)
            .await
    }

    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
        let (reply, answer) = oneshot::channel();
        self.ask(Request::Run(sql.to_owned(), sink, reply), answer)
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

    /// A new connection is the only cleanup that removes temporary tables.
    async fn reset(&mut self) -> ResetOutcome {
        ResetOutcome::Discard
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
}
