//! The contract of the database connectors. Only this module knows driver types.

pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod target;
mod tls;

use std::sync::Arc;

use async_trait::async_trait;

use self::mysql::MySqlConnector;
use self::postgres::PostgresConnector;
use self::target::{Engine, NetworkTarget};
use crate::catalog::Catalog;
use crate::exec::{ReadLimit, ResultSink};
use crate::sql::Dialect;

/// Session values that the runner applies at each reservation,
/// because the session cleanup removes them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionContext {
    pub read_only: bool,
    pub schema: Option<String>,
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
}

impl Default for Capabilities {
    fn default() -> Self {
        Self { namespace: Namespace::None, transactions: true }
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
pub struct ConnectError(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecError {
    pub message: String,
    /// Byte offset of the error in the statement, when the database reports it.
    pub offset: Option<usize>,
}

impl ExecError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), offset: None }
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
    }
}

#[async_trait]
pub trait Connector: Send + Sync {
    fn dialect(&self) -> Dialect;
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError>;
}

#[async_trait]
pub trait Session: Send {
    fn canceller(&self) -> Arc<dyn Canceller>;
    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError>;
    /// Runs one statement. The future must stay active after a cancellation,
    /// so that the session reads the answer of the database.
    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError>;
    async fn transaction_state(&mut self) -> TransactionState;
    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError>;
    async fn reset(&mut self) -> ResetOutcome;
    /// Returns false when the connection of an idle session stopped.
    async fn is_alive(&mut self) -> bool {
        true
    }
    /// Reads schemas and relations, without their columns.
    async fn read_catalog(&mut self) -> Result<Catalog, ExecError>;
    async fn read_columns(&mut self, schema: &str, relation: &str) -> Result<Vec<String>, ExecError>;
}
