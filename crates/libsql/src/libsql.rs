//! libSQL connector, for Turso and `sqld`. A session holds one Hrana stream, which is one
//! SQLite connection on the server. The cleanup closes the stream, so the next statement
//! gets a new connection without state. The dialect and the catalog are the ones of
//! SQLite.

mod hrana;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use self::hrana::{Api, Arg, Cell, Outcome, Request, Response, StmtResult, Stream};
use crate::catalog::{
    Catalog, ForeignKey, KeyColumn, RelationRef, Schema, add_objects, group_foreign_keys,
};
use crate::connector::sqlite::{
    COLUMNS_SQL, DATABASES_SQL, FOREIGN_KEYS_SQL, MAIN_SCHEMA, REFERENCING_KEYS_SQL, relation,
    relations_sql, triggers_sql,
};
use crate::connector::target::NetworkTarget;
use crate::connector::{
    Canceller, Capabilities, ConnectError, Connector, ErrorKind, ExecError, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState, relation_command,
};
use crate::grid::{RowWriter, Value};
use crate::sink::{Flow, ResultSink};
use crate::sql::parameters::{Kind, Parameter};
use crate::sql::{Dialect, changes_state};
use crate::text;
use crate::trace::{self, TraceContext};

pub struct LibsqlConnector {
    target: NetworkTarget,
}

impl LibsqlConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self { target }
    }
}

#[async_trait]
impl Connector for LibsqlConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            parameters: true,
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let api = Api::new(&self.target)?;
        let mut stream = Stream {
            base: api.base.clone(),
            baton: None,
        };
        let check = [Request::execute("SELECT 1", Vec::new()), Request::Close];
        let checked = api
            .pipeline(&mut stream, &check, usize::MAX)
            .await
            .and_then(|outcomes| {
                outcomes
                    .into_iter()
                    .try_for_each(|outcome| ok(outcome).map(drop))
            });
        if let Err(error) = checked {
            return Err(if error.kind == ErrorKind::Auth {
                ConnectError::refused_password(text::LIBSQL_REFUSED)
            } else {
                ConnectError::new(error.message).kind(error.kind)
            });
        }
        stream.baton = None;
        Ok(Box::new(LibsqlSession {
            api,
            stream,
            read_only: false,
            broken: false,
            canceller: Arc::default(),
            trace: None,
        }))
    }
}

/// Gives the response of an outcome, or its error.
fn ok(outcome: Outcome) -> Result<Response, ExecError> {
    match outcome {
        Outcome::Ok { response } => Ok(response),
        Outcome::Error { error } => Err(error.into_exec_error()),
        Outcome::None => Err(ExecError::new(text::LIBSQL_BAD_ANSWER)),
    }
}

/// Gives the result of an outcome of an `execute` request.
fn executed(outcome: Option<Outcome>) -> Result<StmtResult, ExecError> {
    match outcome.map(ok).transpose()? {
        Some(Response::Execute { result }) => Ok(result),
        _ => Err(ExecError::new(text::LIBSQL_BAD_ANSWER)),
    }
}

/// Stops the wait for the statement in progress. Hrana cannot stop a statement on the
/// server.
#[derive(Default)]
struct LibsqlCanceller {
    running: Mutex<Option<CancellationToken>>,
}

impl LibsqlCanceller {
    fn running(&self) -> std::sync::MutexGuard<'_, Option<CancellationToken>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl Canceller for LibsqlCanceller {
    async fn cancel(&self) -> Result<(), String> {
        if let Some(token) = self.running().as_ref() {
            token.cancel();
        }
        Ok(())
    }
}

struct LibsqlSession {
    api: Api,
    stream: Stream,
    read_only: bool,
    /// True after a cancel or a closed stream: the state of the stream is unknown.
    broken: bool,
    canceller: Arc<LibsqlCanceller>,
    trace: Option<TraceContext>,
}

impl LibsqlSession {
    /// Sends the requests on the stream of the session.
    async fn call(
        &mut self,
        requests: Vec<Request>,
        limit: usize,
    ) -> Result<Vec<Outcome>, ExecError> {
        if self.broken {
            return Err(ExecError::with_kind(
                ErrorKind::Connection,
                text::LIBSQL_STREAM_CLOSED,
            ));
        }
        self.send(requests, limit).await
    }

    async fn send(
        &mut self,
        requests: Vec<Request>,
        limit: usize,
    ) -> Result<Vec<Outcome>, ExecError> {
        let token = CancellationToken::new();
        *self.canceller.running() = Some(token.clone());
        let answer = tokio::select! {
            answer = self.api.pipeline(&mut self.stream, &requests, limit) => Some(answer),
            () = token.cancelled() => None,
        };
        *self.canceller.running() = None;
        match answer {
            None => {
                self.broken = true;
                Err(ExecError::with_kind(
                    ErrorKind::Canceled,
                    text::LIBSQL_CANCELED,
                ))
            }
            Some(Err(error)) => {
                // A failed request leaves the stream in an unknown state.
                self.broken = true;
                Err(error)
            }
            Some(Ok(outcomes)) => Ok(outcomes),
        }
    }

    /// Runs one statement and returns its result.
    async fn execute(
        &mut self,
        sql: &str,
        args: Vec<Arg>,
        limit: usize,
    ) -> Result<StmtResult, ExecError> {
        let outcomes = self.call(vec![Request::execute(sql, args)], limit).await?;
        executed(outcomes.into_iter().next())
    }

    /// Runs a catalog read and returns its cells as text.
    async fn query(
        &mut self,
        sql: &str,
        args: Vec<Arg>,
    ) -> Result<Vec<Vec<Option<String>>>, ExecError> {
        let trace = self.trace.clone();
        let result = trace::observe(trace, sql, self.execute(sql, args, usize::MAX)).await?;
        Ok(text_rows(result))
    }

    async fn read(
        &mut self,
        sql: &str,
        parameters: &[Parameter],
        sink: &mut ResultSink,
    ) -> Result<(bool, Option<u64>), ExecError> {
        let args = parameters.iter().map(arg).collect();
        let result = self.execute(sql, args, sink.memory_limit()).await?;
        if result.cols.is_empty() {
            return Ok((false, Some(result.affected_row_count)));
        }
        let (names, types) = result
            .cols
            .into_iter()
            .map(|col| (col.name.unwrap_or_default(), col.decltype))
            .unzip();
        sink.set_typed_columns(names, types).await;
        let mut row = RowWriter::default();
        for cells in result.rows {
            for cell in cells {
                write_cell(&mut row, cell)?;
            }
            if sink.push(row.finish()).await != Flow::Continue {
                break;
            }
        }
        Ok((true, None))
    }

    async fn read_keys(
        &mut self,
        sql: &str,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        let command = relation_command(sql, schema, relation);
        let trace = self.trace.clone();
        let result = trace::observe(
            trace,
            command,
            self.execute(sql, names(schema, relation), usize::MAX),
        )
        .await?;
        let mut pairs = Vec::new();
        for row in text_rows(result) {
            let [Some(key), Some(target), Some(column), Some(target_column)] =
                <[Option<String>; 4]>::try_from(row)
                    .map_err(|_| ExecError::new(text::LIBSQL_BAD_ANSWER))?
            else {
                continue;
            };
            pairs.push(KeyColumn {
                key,
                target: RelationRef {
                    schema: schema.to_owned(),
                    relation: target,
                },
                column,
                target_column,
            });
        }
        Ok(group_foreign_keys(pairs))
    }
}

/// The arguments `?1` (the relation) and `?2` (the schema) of a catalog query.
fn names(schema: &str, relation: &str) -> Vec<Arg> {
    vec![
        Arg::Text {
            value: relation.to_owned(),
        },
        Arg::Text {
            value: schema.to_owned(),
        },
    ]
}

/// The value of a parameter, with the same rules as the SQLite connector.
fn arg(parameter: &Parameter) -> Arg {
    match parameter.kind {
        Kind::Null => Arg::Null,
        Kind::Boolean => Arg::Integer {
            value: if parameter.value == "true" { "1" } else { "0" }.to_owned(),
        },
        Kind::Text | Kind::Number => Arg::Text {
            value: parameter.value.clone(),
        },
    }
}

fn write_cell(row: &mut RowWriter, cell: Cell) -> Result<(), ExecError> {
    match cell {
        Cell::Null => row.push(Value::Null),
        Cell::Integer { value } => row.push(Value::Number(&value)),
        Cell::Float { value } => row.push(Value::Number(&format!("{value:?}"))),
        Cell::Text { value } => row.push(Value::Text(&value)),
        Cell::Blob { base64 } => row.push(Value::binary(&hrana::decode(&base64)?)),
    }
    Ok(())
}

fn text_rows(result: StmtResult) -> Vec<Vec<Option<String>>> {
    result
        .rows
        .into_iter()
        .map(|row| row.into_iter().map(Cell::into_text).collect())
        .collect()
}

/// Returns the first cell of each row that has a value.
fn first_cells(rows: Vec<Vec<Option<String>>>) -> Vec<String> {
    rows.into_iter()
        .filter_map(|row| row.into_iter().next().flatten())
        .collect()
}

#[async_trait]
impl Session for LibsqlSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        trace::canceller(self.canceller.clone(), &self.trace)
    }

    /// `sqld` refuses `PRAGMA query_only`, so the connector refuses a write itself.
    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.read_only = context.read_only;
        Ok(())
    }

    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.run_parameters(sql, &[], sink).await
    }

    async fn run_parameters(
        &mut self,
        sql: &str,
        parameters: &[Parameter],
        mut sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        if self.read_only && changes_state(sql, Dialect::Sqlite) {
            return Err(ExecError::with_kind(
                ErrorKind::Permission,
                text::LIBSQL_READ_ONLY,
            ));
        }
        let command = crate::sql::parameters::command(sql, parameters);
        let trace = self.trace.clone();
        let read = trace::observe(trace, command, self.read(sql, parameters, &mut sink)).await;
        let (rows, limit) = sink.finish().await;
        read.map(|(has_columns, changed)| StatementEnd {
            rows,
            has_columns,
            changed,
            limit,
        })
    }

    async fn transaction_state(&mut self) -> TransactionState {
        if self.broken {
            return TransactionState::Unknown;
        }
        // A session without a stream has no transaction.
        if self.stream.baton.is_none() {
            return TransactionState::None;
        }
        let answer = self.call(vec![Request::GetAutocommit], usize::MAX).await;
        match answer.map(|outcomes| outcomes.into_iter().next().map(ok)) {
            Ok(Some(Ok(Response::GetAutocommit {
                is_autocommit: true,
            }))) => TransactionState::None,
            Ok(Some(Ok(Response::GetAutocommit {
                is_autocommit: false,
            }))) => TransactionState::Open,
            _ => TransactionState::Unknown,
        }
    }

    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError> {
        let sql = match command {
            TransactionCommand::Begin => "BEGIN",
            TransactionCommand::Commit => "COMMIT",
            TransactionCommand::Rollback => "ROLLBACK",
        };
        let trace = self.trace.clone();
        trace::observe(trace, sql, self.execute(sql, Vec::new(), usize::MAX))
            .await
            .map(drop)
    }

    /// Closes the stream, which ends its transaction and removes its temporary tables.
    async fn reset(&mut self) -> ResetOutcome {
        self.read_only = false;
        if self.broken {
            return ResetOutcome::Discard;
        }
        if self.stream.baton.is_some() && self.send(vec![Request::Close], usize::MAX).await.is_err()
        {
            return ResetOutcome::Discard;
        }
        self.stream.baton = None;
        self.stream.base.clone_from(&self.api.base);
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let names = first_cells(self.query(DATABASES_SQL, Vec::new()).await?);
        let mut schemas = Vec::with_capacity(names.len());
        let mut objects = Vec::new();
        for name in names {
            let relations = self
                .query(&relations_sql(&name), Vec::new())
                .await?
                .into_iter()
                .filter_map(|row| match <[Option<String>; 2]>::try_from(row) {
                    Ok([Some(relation_name), Some(kind)]) => Some(relation(relation_name, &kind)),
                    _ => None,
                })
                .collect();
            for row in self.query(&triggers_sql(&name), Vec::new()).await? {
                if let Ok([Some(trigger), Some(table)]) = <[Option<String>; 2]>::try_from(row) {
                    objects.push([name.clone(), trigger, "trigger".to_owned(), table]);
                }
            }
            schemas.push(Schema {
                name,
                relations,
                ..Schema::default()
            });
        }
        add_objects(&mut schemas, objects);
        Ok(Catalog {
            schemas,
            current_schema: Some(MAIN_SCHEMA.to_owned()),
        })
    }

    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        let command = relation_command(COLUMNS_SQL, schema, relation);
        let trace = self.trace.clone();
        let result = trace::observe(
            trace,
            command,
            self.execute(COLUMNS_SQL, names(schema, relation), usize::MAX),
        )
        .await?;
        Ok(first_cells(text_rows(result)))
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
