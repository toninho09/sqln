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
const CONNECT_LIMIT_ENDED: &str = "The connection time limit ended.";
const DISCONNECT_CHECK: Duration = Duration::from_millis(5);

pub type ConnectionId = u64;
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
    /// The rows of one page in a table tab.
    pub page_size: usize,
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
            page_size: 100,
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
    CatalogLoaded { connection: ConnectionId, catalog: Result<Catalog, String> },
    ColumnsLoaded {
        connection: ConnectionId,
        relation: RelationRef,
        columns: Result<Vec<String>, String>,
    },
    /// The test of a connection form ended.
    ConnectionTested { test: u64, result: Result<(), String> },
    /// The idle sessions of a connection closed.
    Disconnected { connection: ConnectionId },
}

impl ExecEvent {
    /// Returns the console of the event.
    pub fn console(&self) -> Option<ConsoleId> {
        match self {
            ExecEvent::Queued { console, .. }
            | ExecEvent::Started { console, .. }
            | ExecEvent::ConfirmClose { console, .. }
            | ExecEvent::ExecutionFinished { console, .. }
            | ExecEvent::TransactionChanged { console, .. }
            | ExecEvent::TransactionFailed { console, .. } => Some(*console),
            ExecEvent::StatementStarted { key }
            | ExecEvent::Columns { key, .. }
            | ExecEvent::Rows { key, .. }
            | ExecEvent::ResultDone { key, .. }
            | ExecEvent::StatementFinished { key, .. } => Some(key.console),
            ExecEvent::CatalogLoaded { .. }
            | ExecEvent::ColumnsLoaded { .. }
            | ExecEvent::ConnectionTested { .. }
            | ExecEvent::Disconnected { .. } => None,
        }
    }
}

/// The parameters of one execution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// True opens a transaction before the first statement, when none is open.
    pub manual: bool,
    /// The schema or database that the console selected.
    pub schema: Option<String>,
    /// A row cap for this execution. Without a value, the setting of the connection applies.
    pub row_cap: Option<usize>,
}

pub struct RunRequest {
    pub execution: ExecutionId,
    pub statements: Vec<String>,
    pub options: RunOptions,
}

/// The pool and the limits that a console uses.
#[derive(Clone)]
struct Binding {
    pool: Pool,
    settings: Settings,
}

enum Command {
    Run(RunRequest),
    Stop,
    ConfirmClose(bool),
    Transaction(TransactionCommand),
    /// The connection changed. The application sends this only to an idle console
    /// without a reserved session.
    Rebind(Binding),
    Close,
}

struct ConsoleEntry {
    connection: ConnectionId,
    commands: UnboundedSender<Command>,
}

pub struct Manager {
    runtime: Runtime,
    blocking: Arc<tokio::sync::Semaphore>,
    budget: Arc<Budget>,
    events: Sender<ExecEvent>,
    connections: HashMap<ConnectionId, Binding>,
    consoles: HashMap<ConsoleId, ConsoleEntry>,
    next_connection: ConnectionId,
    next_console: ConsoleId,
    next_execution: AtomicU64,
}

impl Manager {
    pub fn new(budget: Arc<Budget>) -> std::io::Result<(Self, Receiver<ExecEvent>)> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let (events, receiver) = mpsc::channel(EVENT_CAPACITY);
        let manager = Self {
            runtime,
            blocking: Arc::new(tokio::sync::Semaphore::new(1)),
            budget,
            events,
            connections: HashMap::new(),
            consoles: HashMap::new(),
            next_connection: 1,
            next_console: 1,
            next_execution: AtomicU64::new(1),
        };
        Ok((manager, receiver))
    }

    /// Runs one blocking system operation at a time, without blocking the interface.
    pub fn spawn_blocking(&self, operation: impl FnOnce() + Send + 'static) {
        let blocking = self.blocking.clone();
        self.runtime.spawn(async move {
            let Ok(permit) = blocking.acquire_owned().await else { return };
            let _ = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                operation();
            }).await;
        });
    }

    /// Adds a pool. This opens no session, because a connection starts at its first use.
    pub fn add_connection(
        &mut self,
        connector: Arc<dyn Connector>,
        settings: Settings,
    ) -> ConnectionId {
        let connection = self.next_connection;
        self.next_connection += 1;
        self.connections.insert(connection, binding(connector, settings));
        connection
    }

    /// Gives a connection a new pool, and its consoles use that pool from now on.
    /// The application calls this only when the connection has no session.
    pub fn replace_connection(
        &mut self,
        connection: ConnectionId,
        connector: Arc<dyn Connector>,
        settings: Settings,
    ) {
        let binding = binding(connector, settings);
        for entry in self.consoles.values().filter(|entry| entry.connection == connection) {
            let _ = entry.commands.send(Command::Rebind(binding.clone()));
        }
        self.connections.insert(connection, binding);
    }

    /// Removes a pool. Its idle sessions close in the runtime.
    pub fn remove_connection(&mut self, connection: ConnectionId) {
        if let Some(binding) = self.connections.remove(&connection) {
            self.runtime.spawn(async move { binding.pool.close_idle() });
        }
    }

    /// Returns the places in use and the limit of a pool.
    pub fn usage(&self, connection: ConnectionId) -> (usize, usize) {
        self.connections.get(&connection).map_or((0, 0), |binding| binding.pool.usage())
    }

    /// Returns true while a pool holds a session.
    pub fn is_connected(&self, connection: ConnectionId) -> bool {
        self.connections.get(&connection).is_some_and(|binding| binding.pool.has_sessions())
    }

    pub fn open_console(&mut self, connection: ConnectionId) -> Option<ConsoleId> {
        let binding = self.connections.get(&connection)?;
        let console = self.next_console;
        self.next_console += 1;
        let (commands, inbox) = mpsc::unbounded_channel();
        let task = runner::ConsoleTask::new(
            console,
            binding.clone(),
            self.budget.clone(),
            self.events.clone(),
        );
        self.runtime.spawn(task.serve(inbox));
        self.consoles.insert(console, ConsoleEntry { connection, commands });
        Some(console)
    }

    fn send(&self, console: ConsoleId, command: Command) {
        if let Some(entry) = self.consoles.get(&console) {
            let _ = entry.commands.send(command);
        }
    }

    /// Accepts an execution and returns its identity.
    pub fn run(
        &self,
        console: ConsoleId,
        statements: Vec<String>,
        options: RunOptions,
    ) -> ExecutionId {
        let execution = self.next_execution.fetch_add(1, Ordering::Relaxed);
        self.send(console, Command::Run(RunRequest { execution, statements, options }));
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
    pub fn load_catalog(&self, connection: ConnectionId) {
        let Some(binding) = self.connections.get(&connection) else { return };
        let pool = binding.pool.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let catalog = match pool.acquire().await {
                Ok(mut lease) => {
                    let result = lease.session().read_catalog().await;
                    lease.release().await;
                    result.map_err(|error| error.message)
                }
                Err(error) => Err(error.0),
            };
            let _ = events.send(ExecEvent::CatalogLoaded { connection, catalog }).await;
        });
    }

    pub fn load_columns(&self, connection: ConnectionId, relation: RelationRef) {
        let Some(binding) = self.connections.get(&connection) else { return };
        let pool = binding.pool.clone();
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
            let _ = events.send(ExecEvent::ColumnsLoaded { connection, relation, columns }).await;
        });
    }

    /// Closes the idle sessions of a pool, and it waits for a session in use, such as
    /// the session of a catalog read. That session closes when it returns.
    pub fn disconnect(&self, connection: ConnectionId) {
        let Some(binding) = self.connections.get(&connection) else { return };
        let pool = binding.pool.clone();
        let limit = binding.settings.stop_grace;
        let events = self.events.clone();
        self.runtime.spawn(async move {
            pool.close_idle();
            let deadline = tokio::time::Instant::now() + limit;
            while pool.has_sessions() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(DISCONNECT_CHECK).await;
            }
            let _ = events.send(ExecEvent::Disconnected { connection }).await;
        });
    }

    /// Opens one session with the rules of a saved connection, and closes it.
    pub fn test_connection(&self, test: u64, connector: Arc<dyn Connector>, limit: Duration) {
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let result = match tokio::time::timeout(limit, connector.connect()).await {
                Ok(Ok(_session)) => Ok(()),
                Ok(Err(error)) => Err(error.0),
                Err(_) => Err(CONNECT_LIMIT_ENDED.to_owned()),
            };
            let _ = events.send(ExecEvent::ConnectionTested { test, result }).await;
        });
    }

    /// Ends the tasks without a wait for a statement that does not answer.
    pub fn shutdown(self) {
        self.runtime.shutdown_background();
    }
}

fn binding(connector: Arc<dyn Connector>, settings: Settings) -> Binding {
    let pool = Pool::new(connector, settings.max_connections, settings.connect_timeout);
    Binding { pool, settings }
}

fn context(settings: &Settings, schema: Option<String>) -> SessionContext {
    SessionContext { read_only: settings.read_only, schema }
}
