//! The contract of the database connectors. Only this module knows driver types.

pub mod duckdb;
pub mod mysql;
pub mod postgres;
pub mod redis;
pub mod sqlite;
pub mod target;
mod tls;

use std::sync::Arc;

use async_trait::async_trait;

use self::mysql::MySqlConnector;
use self::postgres::PostgresConnector;
use self::redis::RedisConnector;
use self::target::{Engine, NetworkTarget};
use crate::catalog::{Catalog, ForeignKey};
use crate::command_log::TraceContext;
use crate::exec::{ReadLimit, ResultSink};
use crate::sql::Dialect;

/// The database notices that one statement keeps. A loop of notices must not fill the
/// memory, so a later notice only adds to a count.
pub const MAX_NOTICES: usize = 100;

/// Session values that the runner applies at each reservation,
/// because the session cleanup removes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionContext {
    pub read_only: bool,
    pub schema: Option<String>,
    /// True keeps only `schema` in the `search_path` of PostgreSQL. Without it, the
    /// default path of the session stays after the schema.
    pub only_schema: bool,
}

/// What the console can choose at each execution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Namespace {
    #[default]
    None,
    Schema,
    Database,
}

/// The features of a source. Future sources do not have all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub namespace: Namespace,
    pub transactions: bool,
    /// True when a table tab can filter and sort with SQL text.
    pub table_filters: bool,
    /// True when a table tab can submit pending updates.
    pub data_edit: bool,
    /// True when consoles accept named scalar parameters.
    pub parameters: bool,
    /// True when the source holds its state in one session, so the pool keeps exactly
    /// one place and never closes an idle session. An in-memory SQLite database needs
    /// this, because a new session starts empty.
    pub single_session: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            namespace: Namespace::None,
            transactions: true,
            table_filters: true,
            data_edit: true,
            parameters: false,
            single_session: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransactionState {
    #[default]
    None,
    Open,
    /// The transaction stays open, and the database refuses statements until `ROLLBACK`.
    Failed,
    /// The connection stopped before the database confirmed the state.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionCommand {
    Begin,
    Commit,
    Rollback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetOutcome {
    /// Another execution can use the session.
    Clean,
    /// The runner must close the session.
    Discard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectError {
    pub message: String,
    /// The database refused the password. A failure of the network or of TLS stays false.
    pub password_refused: bool,
}

impl ConnectError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            password_refused: false,
        }
    }

    pub fn refused_password(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            password_refused: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecError {
    pub message: String,
    /// Byte offset of the error in the statement, when the database reports it.
    pub offset: Option<usize>,
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl ExecError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            offset: None,
        }
    }
}

/// The result of one statement that the database completed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatementEnd {
    pub rows: usize,
    /// True when the statement has a result with columns.
    pub has_columns: bool,
    /// Rows that the statement changed, for a statement without a result.
    pub changed: Option<u64>,
    pub limit: ReadLimit,
}

/// Asks the database to stop the statement in progress. It uses no session state,
/// because the session stays busy with that statement. `cancel` returns after the
/// database handled the request, so the request cannot reach a later statement.
#[async_trait]
pub trait Canceller: Send + Sync {
    async fn cancel(&self) -> Result<(), String>;
    async fn cancel_traced(&self, _trace: Option<TraceContext>) -> Result<(), String> {
        self.cancel().await
    }
}

/// Writes an error with its causes. The display text of a driver error is often only
/// its class, as in "error connecting to server".
fn describe(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        // Some drivers repeat the text of the cause in their own text.
        if !text.contains(&cause_text) {
            text = format!("{text}: {cause_text}");
        }
        source = cause.source();
    }
    text
}

/// Opens the connector of a network address.
pub fn network_connector(target: NetworkTarget) -> Arc<dyn Connector> {
    match target.engine {
        Engine::Postgres => Arc::new(PostgresConnector::new(target)),
        Engine::MySql => Arc::new(MySqlConnector::new(target)),
        Engine::Redis => Arc::new(RedisConnector::new(target)),
    }
}

/// Names a query of the catalog with its parameters, for the log of commands.
pub(crate) fn relation_command(sql: &str, schema: &str, relation: &str) -> String {
    format!("{sql}\nParameters: schema={schema:?}, relation={relation:?}")
}

#[async_trait]
pub trait Connector: Send + Sync {
    fn dialect(&self) -> Dialect;
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError>;
    async fn connect_traced(
        &self,
        trace: Option<TraceContext>,
    ) -> Result<Box<dyn Session>, ConnectError> {
        let mut session = self.connect().await?;
        session.set_trace_context(trace);
        Ok(session)
    }
}

#[async_trait]
pub trait Session: Send {
    fn set_trace_context(&mut self, _trace: Option<TraceContext>) {}
    fn canceller(&self) -> Arc<dyn Canceller>;
    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError>;
    /// Runs one statement. The future must stay active after a cancellation,
    /// so that the session reads the answer of the database.
    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError>;
    /// Runs native scalar bindings. A source must explicitly implement this path.
    async fn run_parameters(
        &mut self,
        sql: &str,
        parameters: &[crate::sql::parameters::Parameter],
        sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        if parameters.is_empty() {
            self.run(sql, sink).await
        } else {
            Err(ExecError::new(
                "This connector does not support query parameters.",
            ))
        }
    }
    async fn transaction_state(&mut self) -> TransactionState;
    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError>;
    async fn reset(&mut self) -> ResetOutcome;
    /// Returns false when the connection of an idle session stopped.
    async fn is_alive(&mut self) -> bool {
        true
    }
    /// Reads schemas and relations, without their columns.
    async fn read_catalog(&mut self) -> Result<Catalog, ExecError>;
    /// Reads the catalog of the schemas of the list and of the current schema. An empty
    /// list reads all schemas.
    async fn read_catalog_of(&mut self, schemas: &[String]) -> Result<Catalog, ExecError> {
        let mut catalog = self.read_catalog().await?;
        catalog.keep_schemas(schemas);
        Ok(catalog)
    }
    /// Reads the names of all schemas, for the choice of the visible schemas.
    async fn read_schema_names(&mut self) -> Result<Vec<String>, ExecError> {
        let catalog = self.read_catalog().await?;
        Ok(catalog
            .schemas
            .into_iter()
            .map(|schema| schema.name)
            .collect())
    }
    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError>;
    /// Reads the keys that leave a relation. A source without keys keeps this default.
    async fn read_foreign_keys(
        &mut self,
        _schema: &str,
        _relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        Ok(Vec::new())
    }
    /// Reads the keys of other tables that reach a relation. A source without keys keeps
    /// this default.
    async fn read_referencing_keys(
        &mut self,
        _schema: &str,
        _relation: &str,
    ) -> Result<Vec<ForeignKey>, ExecError> {
        Ok(Vec::new())
    }
}
