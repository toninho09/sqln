//! Executions. Each console has one task, and that task owns the reserved session.

mod runner;
mod sink;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedSender};

pub use self::sink::{Flow, ReadLimit, ResultSink};
use crate::catalog::{Catalog, RelationRef};
use crate::connector::{Connector, SessionContext, StatementEnd, TransactionCommand, TransactionState};
use crate::grid::{Budget, DEFAULT_LIMIT, Row};
use crate::pool::Pool;

const EVENT_CAPACITY: usize = 256;

pub type ConsoleId = u64;
pub type ExecutionId = u64;

/// Identifies a statement. The interface ignores events of an old execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatementKey {
    pub console: ConsoleId,
    pub execution: ExecutionId,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub row_cap: usize,
    /// `None` runs without a time limit.
    pub sql_timeout: Option<Duration>,
    pub connect_timeout: Duration,
    /// The time that a statement has to answer after a cancellation.
    pub stop_grace: Duration,
    pub max_connections: usize,
    pub budget_bytes: usize,
    pub read_only: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            row_cap: 1_000,
            sql_timeout: Some(Duration::from_secs(30)),
            connect_timeout: Duration::from_secs(30),
            stop_grace: Duration::from_secs(5),
            max_connections: 5,
            budget_bytes: DEFAULT_LIMIT,
            read_only: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementStatus {
    Running,
    Success,
    Error(String),
    NotExecuted,
    Stopped,
    Timeout,
    /// The connection closed before the answer, so the effect is unknown.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionEnd {
    Completed,
    Failed,
    Stopped,
    TimedOut,
    /// The execution stopped while it waited for a connection.
    CanceledBeforeStart,
    ConnectionClosed,
    /// The console already has an execution, so this one did not start.
    Refused,
}

#[derive(Debug)]
pub enum ExecEvent {
    Queued { console: ConsoleId, execution: ExecutionId },
    Started { console: ConsoleId, execution: ExecutionId },
    StatementStarted { key: StatementKey },
    Columns { key: StatementKey, columns: Vec<String> },
    Rows { key: StatementKey, rows: Vec<Row>, bytes: usize },
    /// One result ended. A statement can send more than one result.
    ResultDone { key: StatementKey, rows: usize, limit: ReadLimit },
    StatementFinished {
        key: StatementKey,
        status: StatementStatus,
        end: Option<StatementEnd>,
        /// Byte offset of an error in the statement, when the database reports it.
        error_offset: Option<usize>,
    },
    /// A statement did not answer, and closing the connection loses an open transaction.
    ConfirmClose { console: ConsoleId, execution: ExecutionId },
    ExecutionFinished {
        console: ConsoleId,
        execution: ExecutionId,
        end: ExecutionEnd,
        message: Option<String>,
    },
    TransactionChanged { console: ConsoleId, state: TransactionState },
    TransactionFailed { console: ConsoleId, message: String },
    CatalogLoaded(Result<Catalog, String>),
    ColumnsLoaded { relation: RelationRef, columns: Result<Vec<String>, String> },
}

pub struct RunRequest {
    pub execution: ExecutionId,
    pub statements: Vec<String>,
    /// True opens a transaction before the first statement, when none is open.
    pub manual: bool,
}

enum Command {
    Run(RunRequest),
    Stop,
    ConfirmClose(bool),
    Transaction(TransactionCommand),
    Close,
}

pub struct Manager {
    runtime: Runtime,
    pool: Pool,
    settings: Settings,
    budget: Arc<Budget>,
    events: Sender<ExecEvent>,
    consoles: HashMap<ConsoleId, UnboundedSender<Command>>,
    next_console: ConsoleId,
    next_execution: AtomicU64,
}

impl Manager {
    pub fn new(
        connector: Arc<dyn Connector>,
        settings: Settings,
        budget: Arc<Budget>,
    ) -> std::io::Result<(Self, Receiver<ExecEvent>)> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let pool = Pool::new(connector, settings.max_connections, settings.connect_timeout);
        let (events, receiver) = mpsc::channel(EVENT_CAPACITY);
        let manager = Self {
            runtime,
            pool,
            settings,
            budget,
            events,
            consoles: HashMap::new(),
            next_console: 1,
            next_execution: AtomicU64::new(1),
        };
        Ok((manager, receiver))
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn open_console(&mut self) -> ConsoleId {
        let console = self.next_console;
        self.next_console += 1;
        let (commands, inbox) = mpsc::unbounded_channel();
        let task = runner::ConsoleTask::new(
            console,
            self.pool.clone(),
            self.settings.clone(),
            self.budget.clone(),
            self.events.clone(),
        );
        self.runtime.spawn(task.serve(inbox));
        self.consoles.insert(console, commands);
        console
    }

    fn send(&self, console: ConsoleId, command: Command) {
        if let Some(commands) = self.consoles.get(&console) {
            let _ = commands.send(command);
        }
    }

    /// Accepts an execution and returns its identity.
    pub fn run(&self, console: ConsoleId, statements: Vec<String>, manual: bool) -> ExecutionId {
        let execution = self.next_execution.fetch_add(1, Ordering::Relaxed);
        self.send(console, Command::Run(RunRequest { execution, statements, manual }));
        execution
    }

    pub fn stop(&self, console: ConsoleId) {
        self.send(console, Command::Stop);
    }

    pub fn confirm_close(&self, console: ConsoleId, close: bool) {
        self.send(console, Command::ConfirmClose(close));
    }

    pub fn transaction(&self, console: ConsoleId, command: TransactionCommand) {
        self.send(console, Command::Transaction(command));
    }

    pub fn close_console(&mut self, console: ConsoleId) {
        self.send(console, Command::Close);
        self.consoles.remove(&console);
    }

    /// Reads the catalog through the pool, so it respects the connection limit.
    pub fn load_catalog(&self) {
        let pool = self.pool.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = match pool.acquire().await {
                Ok(mut lease) => {
                    let result = lease.session().read_catalog().await;
                    lease.release().await;
                    result.map_err(|error| error.message)
                }
                Err(error) => Err(error.0),
            };
            let _ = events.send(ExecEvent::CatalogLoaded(result)).await;
        });
    }

    pub fn load_columns(&self, relation: RelationRef) {
        let pool = self.pool.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let columns = match pool.acquire().await {
                Ok(mut lease) => {
                    let session = lease.session();
                    let result = session.read_columns(&relation.schema, &relation.relation).await;
                    lease.release().await;
                    result.map_err(|error| error.message)
                }
                Err(error) => Err(error.0),
            };
            let _ = events.send(ExecEvent::ColumnsLoaded { relation, columns }).await;
        });
    }

    /// Ends the tasks without a wait for a statement that does not answer.
    pub fn shutdown(self) {
        self.runtime.shutdown_background();
    }
}

fn context(settings: &Settings) -> SessionContext {
    SessionContext { read_only: settings.read_only, schema: None }
}
