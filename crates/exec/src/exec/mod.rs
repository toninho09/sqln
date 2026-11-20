//! The types of the executions: events, ends, options, and requests. `sqln-runtime`
//! runs them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::catalog::{Catalog, ColumnNote, ForeignKey, RelationRef};
use crate::command_log::{LogEvent, TraceContext, TraceEvent};
use crate::connector::{ConnectError, StatementEnd, TransactionState};
use crate::export::ExportWriter;
use crate::grid::Row;
use crate::import::ImportJob;
pub use crate::progress::{ProgressSnapshot, ReadEnd, ReadMeter, ReadProgress, percent};
pub use crate::sink::{
    ConnectionId, ConsoleId, ExecutionId, Flow, ReadLimit, ResultSink, SinkEvent, SinkMessage,
    StatementKey,
};
pub use crate::store::connection::Settings;

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
    AgentMetadata {
        job: String,
        result: Result<AgentMetadata, String>,
    },
    EditFinished {
        console: ConsoleId,
        execution: ExecutionId,
        result: Result<crate::data_edit::Outcome, crate::data_edit::Failure>,
    },
    CommandLog {
        target: u64,
        event: LogEvent,
    },
    Queued {
        console: ConsoleId,
        execution: ExecutionId,
    },
    Started {
        console: ConsoleId,
        execution: ExecutionId,
    },
    StatementStarted {
        key: StatementKey,
    },
    Columns {
        key: StatementKey,
        columns: Vec<String>,
        /// The row cap of a live result: at the cap, the grid drops its oldest rows.
        live: Option<usize>,
        /// The counts of a read that the connector updates while it runs.
        progress: Option<Arc<ReadProgress>>,
    },
    Rows {
        key: StatementKey,
        rows: Vec<Row>,
        bytes: usize,
    },
    /// One result ended. A statement can send more than one result.
    ResultDone {
        key: StatementKey,
        rows: usize,
        limit: ReadLimit,
    },
    /// A notice or a warning of the database about a statement.
    Notice {
        key: StatementKey,
        text: String,
    },
    /// The database received the statement with `LIMIT rows` at its end.
    LimitAdded {
        key: StatementKey,
        rows: usize,
    },
    StatementFinished {
        key: StatementKey,
        status: StatementStatus,
        end: Option<StatementEnd>,
        /// Byte offset of an error in the statement, when the database reports it.
        error_offset: Option<usize>,
    },
    /// A statement did not answer, and closing the connection loses an open transaction.
    ConfirmClose {
        console: ConsoleId,
        execution: ExecutionId,
    },
    ExecutionFinished {
        console: ConsoleId,
        execution: ExecutionId,
        end: ExecutionEnd,
        message: Option<String>,
    },
    TransactionChanged {
        console: ConsoleId,
        state: TransactionState,
    },
    /// `state` is the transaction state after the failure. A failed `COMMIT` can end
    /// the transaction, and then nothing is left to try again.
    TransactionFailed {
        console: ConsoleId,
        message: String,
        state: TransactionState,
    },
    /// `read` is the number that the caller gave to `load_catalog`.
    CatalogLoaded {
        connection: ConnectionId,
        read: u64,
        catalog: Result<Catalog, String>,
    },
    ColumnsLoaded {
        connection: ConnectionId,
        relation: RelationRef,
        columns: Result<Vec<String>, String>,
        /// Empty, or one note for each column.
        notes: Vec<ColumnNote>,
        foreign_keys: Vec<ForeignKey>,
        /// The keys of other tables that reach the relation.
        referencing: Vec<ForeignKey>,
    },
    /// A test of a connection ended. A test that reads the schema names holds them.
    ConnectionTested {
        test: u64,
        result: Result<Option<Vec<String>>, ConnectError>,
    },
    /// The database refused the password of a new session of this pool.
    PasswordRefused {
        connection: ConnectionId,
        pool: u64,
    },
    /// The idle sessions of a connection closed.
    Disconnected {
        connection: ConnectionId,
    },
    /// Idle sessions of a connection reached the idle limit and closed.
    IdleClosed {
        connection: ConnectionId,
    },
}

impl From<SinkEvent> for ExecEvent {
    fn from(event: SinkEvent) -> Self {
        match event {
            SinkEvent::Columns {
                key,
                columns,
                live,
                progress,
            } => ExecEvent::Columns {
                key,
                columns,
                live,
                progress,
            },
            SinkEvent::Rows { key, rows, bytes } => ExecEvent::Rows { key, rows, bytes },
            SinkEvent::ResultDone { key, rows, limit } => {
                ExecEvent::ResultDone { key, rows, limit }
            }
            SinkEvent::Notice { key, text } => ExecEvent::Notice { key, text },
        }
    }
}

impl From<TraceEvent> for ExecEvent {
    fn from(TraceEvent { target, event }: TraceEvent) -> Self {
        ExecEvent::CommandLog { target, event }
    }
}

impl SinkMessage for ExecEvent {
    fn unsent_bytes(&self) -> usize {
        match self {
            ExecEvent::Rows { bytes, .. } => *bytes,
            _ => 0,
        }
    }
}

impl ExecEvent {
    /// Returns the console of the event.
    pub fn console(&self) -> Option<ConsoleId> {
        match self {
            ExecEvent::EditFinished { console, .. }
            | ExecEvent::Queued { console, .. }
            | ExecEvent::Started { console, .. }
            | ExecEvent::ConfirmClose { console, .. }
            | ExecEvent::ExecutionFinished { console, .. }
            | ExecEvent::TransactionChanged { console, .. }
            | ExecEvent::TransactionFailed { console, .. } => Some(*console),
            ExecEvent::StatementStarted { key }
            | ExecEvent::Columns { key, .. }
            | ExecEvent::Rows { key, .. }
            | ExecEvent::ResultDone { key, .. }
            | ExecEvent::Notice { key, .. }
            | ExecEvent::LimitAdded { key, .. }
            | ExecEvent::StatementFinished { key, .. } => Some(key.console),
            ExecEvent::AgentMetadata { .. }
            | ExecEvent::CommandLog { .. }
            | ExecEvent::CatalogLoaded { .. }
            | ExecEvent::PasswordRefused { .. }
            | ExecEvent::ColumnsLoaded { .. }
            | ExecEvent::ConnectionTested { .. }
            | ExecEvent::Disconnected { .. }
            | ExecEvent::IdleClosed { .. } => None,
        }
    }
}

impl ExecEvent {
    /// Returns the execution of the event.
    pub fn execution(&self) -> Option<ExecutionId> {
        match self {
            ExecEvent::EditFinished { execution, .. }
            | ExecEvent::Queued { execution, .. }
            | ExecEvent::Started { execution, .. }
            | ExecEvent::ConfirmClose { execution, .. }
            | ExecEvent::ExecutionFinished { execution, .. } => Some(*execution),
            ExecEvent::StatementStarted { key }
            | ExecEvent::Columns { key, .. }
            | ExecEvent::Rows { key, .. }
            | ExecEvent::ResultDone { key, .. }
            | ExecEvent::Notice { key, .. }
            | ExecEvent::LimitAdded { key, .. }
            | ExecEvent::StatementFinished { key, .. } => Some(key.execution),
            _ => None,
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
    /// True keeps only `schema` in the `search_path` of PostgreSQL. The compare needs it,
    /// so the database writes the names of other schemas with their schema.
    pub only_schema: bool,
    /// A row cap for this execution. Without a value, the setting of the connection applies.
    pub row_cap: Option<usize>,
    /// The file of an export. The rows go there, and the time limit does not apply.
    pub export: Option<ExportWriter>,
    /// True sends rows with backpressure, without the grid cap or storage budget.
    pub stream: bool,
    /// True keeps running after a statement error, so a batch script reports every
    /// failure. A timeout, a stop, or a closed connection still ends the run.
    pub continue_on_error: bool,
    /// True leaves a transaction open at the end of the run. A batch that commits
    /// or rolls back with its own statements uses this, because the streaming end
    /// check would otherwise roll the transaction back.
    pub hold_transaction: bool,
    /// True for a read that ends with the user, as an export does: the time limit does
    /// not apply.
    pub untimed: bool,
    /// Values captured when the user requests this execution.
    pub parameters: Vec<crate::sql::parameters::Parameter>,
    /// True accepts a continuous statement at the end of the run. It has no time limit
    /// and grows its result until the user stops it. Only a console sets this.
    pub live: bool,
}

/// Shares cancellation and the SQL deadline with the streaming output.
#[derive(Default)]
pub struct StreamControl {
    stopped: AtomicBool,
    deadline: Mutex<Option<Instant>>,
}

impl StreamControl {
    pub fn start(&self, timeout: Option<Duration>) {
        *self.deadline.lock().unwrap_or_else(PoisonError::into_inner) =
            timeout.and_then(|timeout| Instant::now().checked_add(timeout));
        self.stopped.store(false, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    /// The deadline stays active until the consumer finishes its output.
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
            || self
                .deadline
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some_and(|deadline| Instant::now() >= deadline)
    }
}

pub struct AgentRun {
    pub reservation: Result<crate::agent::Hold, crate::agent::Error>,
    pub cancel: tokio_util::sync::CancellationToken,
    /// False only for a session with full access. A read-only connection stays read-only.
    pub read_only: bool,
}

#[derive(Debug)]
pub enum AgentMetadata {
    Catalog(Catalog),
    /// The field paths of a Kafka topic, with a note for each path.
    Fields(Vec<String>, Vec<ColumnNote>),
}

/// What an agent metadata read reads.
pub enum MetadataRead {
    /// The catalog of these schemas. An empty list reads all schemas.
    Catalog(Vec<String>),
    /// The field paths of one Kafka topic.
    Fields(String),
}

pub struct RunRequest {
    pub agent: Option<AgentRun>,
    pub trace: Option<TraceContext>,
    pub execution: ExecutionId,
    pub statements: Vec<String>,
    pub options: RunOptions,
}

pub struct ImportRequest {
    pub execution: ExecutionId,
    pub job: ImportJob,
}
