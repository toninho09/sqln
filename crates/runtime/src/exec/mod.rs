//! Executions. Each console has one task, and that task owns the reserved session.
//! The types of the executions come from `sqln-exec`.

mod ai;
mod runner;

pub use sqln_exec::exec::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedSender};

use crate::catalog::RelationRef;
use crate::command_log::TraceContext;
use crate::connector::{ConnectError, Connector, Failure, Fix, SessionContext, TransactionCommand};
use crate::grid::Budget;
use crate::import::ImportJob;
use crate::pool::Pool;
use crate::wake::Wake;
const EVENT_CAPACITY: usize = 256;
/// The events that the relay of the interface holds after the bounded channel.
const RELAY_CAPACITY: usize = 64;
const DISCONNECT_CHECK: Duration = Duration::from_millis(5);

/// The pool and the limits that a console uses.
#[derive(Clone)]
struct Binding {
    connection: ConnectionId,
    /// A refusal from an older pool says nothing about the password of this pool.
    serial: u64,
    pool: Pool,
    /// The consoles of the connection share these values, so a saved change reaches them.
    settings: Arc<Mutex<Settings>>,
}

impl Binding {
    /// Do not keep the guard across an `await`.
    fn settings(&self) -> MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn refusal(&self, error: &ConnectError) -> Option<ExecEvent> {
        error
            .password_refused
            .then_some(ExecEvent::PasswordRefused {
                connection: self.connection,
                pool: self.serial,
            })
    }
}

enum Command {
    Edit {
        execution: ExecutionId,
        job: crate::data_edit::Job,
        trace: Option<TraceContext>,
    },
    Run(RunRequest),
    Import(ImportRequest),
    Stop,
    ConfirmClose(bool),
    Transaction(TransactionCommand, Option<TraceContext>),
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
    stream_stopped: Option<Arc<StreamControl>>,
    connections: HashMap<ConnectionId, Binding>,
    consoles: HashMap<ConsoleId, ConsoleEntry>,
    next_connection: ConnectionId,
    next_console: ConsoleId,
    next_execution: AtomicU64,
    next_pool: u64,
    histories: HashMap<ConsoleId, TraceContext>,
    active_history: Option<TraceContext>,
    /// Wakes the main loop after an event or a background operation. It does nothing
    /// outside the interface.
    wake: Wake,
}

impl Manager {
    pub fn new(budget: Arc<Budget>) -> std::io::Result<(Self, Receiver<ExecEvent>)> {
        Self::with_event_capacity(budget, EVENT_CAPACITY)
    }

    /// The manager of the interface. Each event and each finished background operation
    /// wakes the main loop, so the loop can sleep until it has work.
    pub fn with_wake(
        budget: Arc<Budget>,
        wake: Wake,
    ) -> std::io::Result<(Self, Receiver<ExecEvent>)> {
        let (mut manager, events) = Self::with_event_capacity(budget, EVENT_CAPACITY)?;
        // The senders of the events live in many crates. One relay wakes the loop for
        // all of them, and its bounded channel keeps the pressure on the database.
        let (relay, receiver) = mpsc::channel(RELAY_CAPACITY);
        manager
            .runtime
            .spawn(relay_events(events, relay, wake.clone()));
        manager.wake = wake;
        Ok((manager, receiver))
    }

    /// Keeps one event queued and signals cancellation to the streaming consumer.
    pub fn new_streaming(
        budget: Arc<Budget>,
        stopped: Arc<StreamControl>,
    ) -> std::io::Result<(Self, Receiver<ExecEvent>)> {
        let (mut manager, receiver) = Self::with_event_capacity(budget, 1)?;
        manager.stream_stopped = Some(stopped);
        Ok((manager, receiver))
    }

    fn with_event_capacity(
        budget: Arc<Budget>,
        capacity: usize,
    ) -> std::io::Result<(Self, Receiver<ExecEvent>)> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let (events, receiver) = mpsc::channel(capacity);
        let manager = Self {
            runtime,
            blocking: Arc::new(tokio::sync::Semaphore::new(1)),
            budget,
            events,
            stream_stopped: None,
            connections: HashMap::new(),
            consoles: HashMap::new(),
            next_connection: 1,
            next_console: 1,
            next_execution: AtomicU64::new(1),
            next_pool: 1,
            histories: HashMap::new(),
            active_history: None,
            wake: Wake::new(),
        };
        Ok((manager, receiver))
    }

    /// Runs one blocking system operation at a time, without blocking the interface.
    pub fn spawn_blocking(&self, operation: impl FnOnce() + Send + 'static) {
        let blocking = self.blocking.clone();
        let wake = self.wake.clone();
        self.runtime.spawn(async move {
            let Ok(permit) = blocking.acquire_owned().await else {
                return;
            };
            let _ = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                operation();
            })
            .await;
            // The operation sent its result through its own channel.
            wake.wake();
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
        let binding = self.binding(connection, connector, settings);
        self.connections.insert(connection, binding);
        connection
    }

    fn binding(
        &mut self,
        connection: ConnectionId,
        connector: Arc<dyn Connector>,
        settings: Settings,
    ) -> Binding {
        let serial = self.next_pool;
        self.next_pool += 1;
        let pool = Pool::new(
            connector,
            settings.max_connections,
            settings.connect_timeout,
        );
        if let Some(limit) = settings.idle_timeout {
            self.close_idle_sessions(connection, &pool, limit);
        }
        Binding {
            connection,
            serial,
            pool,
            settings: Arc::new(Mutex::new(settings)),
        }
    }

    /// Changes the limits of a connection. Each execution reads them at its start, so a
    /// running script keeps its values. The pool keeps its size, its connect time limit,
    /// and its idle limit: a change of these values needs `replace_connection`.
    pub fn update_settings(&self, connection: ConnectionId, settings: Settings) {
        if let Some(binding) = self.connections.get(&connection) {
            *binding.settings() = settings;
        }
    }

    /// Closes each idle session of the pool after `limit`. A session in use stays, also
    /// the session of an open transaction. The task ends after the end of the pool.
    fn close_idle_sessions(&self, connection: ConnectionId, pool: &Pool, limit: Duration) {
        let pool = pool.downgrade();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let mut wait = limit;
            loop {
                tokio::time::sleep(wait).await;
                let Some(pool) = pool.upgrade() else {
                    return;
                };
                let (closed, next) = pool.close_expired(limit);
                drop(pool);
                wait = next;
                if closed > 0 {
                    let _ = events.send(ExecEvent::IdleClosed { connection }).await;
                }
            }
        });
    }

    /// Returns the serial of the pool that a connection uses now.
    pub fn pool_serial(&self, connection: ConnectionId) -> Option<u64> {
        self.connections
            .get(&connection)
            .map(|binding| binding.serial)
    }

    /// Gives a connection a new pool, and its consoles use that pool from now on.
    /// The application calls this only when the connection has no session.
    pub fn replace_connection(
        &mut self,
        connection: ConnectionId,
        connector: Arc<dyn Connector>,
        settings: Settings,
    ) {
        let binding = self.binding(connection, connector, settings);
        for entry in self
            .consoles
            .values()
            .filter(|entry| entry.connection == connection)
        {
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
        self.connections
            .get(&connection)
            .map_or((0, 0), |binding| binding.pool.usage())
    }

    /// Returns true while a pool holds a session.
    pub fn is_connected(&self, connection: ConnectionId) -> bool {
        self.connections
            .get(&connection)
            .is_some_and(|binding| binding.pool.has_sessions())
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
            self.stream_stopped.clone(),
        );
        self.runtime.spawn(task.serve(inbox));
        self.consoles.insert(
            console,
            ConsoleEntry {
                connection,
                commands,
            },
        );
        Some(console)
    }

    pub fn set_console_history(&mut self, console: ConsoleId, target: u64, name: String) {
        if let Some(entry) = self.consoles.get(&console) {
            let mut trace =
                TraceContext::new(target, entry.connection, name, false, self.events.clone());
            if let Some(binding) = self.connections.get(&entry.connection) {
                trace.dialect = binding.pool.dialect();
            }
            self.histories.insert(console, trace);
        }
    }

    pub fn set_active_history(&mut self, console: Option<ConsoleId>) {
        self.active_history = console.and_then(|id| self.histories.get(&id).cloned());
    }

    fn shared_trace(&self, connection: ConnectionId) -> Option<TraceContext> {
        self.active_history
            .as_ref()
            .filter(|trace| trace.connection == connection)
            .map(|trace| {
                let mut trace = trace.clone();
                trace.shared = true;
                trace
            })
    }

    fn console_trace(&self, console: ConsoleId) -> Option<TraceContext> {
        self.histories.get(&console).cloned().or_else(|| {
            self.consoles
                .get(&console)
                .and_then(|entry| self.shared_trace(entry.connection))
        })
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
        self.send(
            console,
            Command::Run(RunRequest {
                agent: None,
                trace: self.console_trace(console),
                execution,
                statements,
                options,
            }),
        );
        execution
    }

    pub fn run_agent(
        &self,
        console: ConsoleId,
        statements: Vec<String>,
        mut options: RunOptions,
        cancel: tokio_util::sync::CancellationToken,
        internal: bool,
        read_only: bool,
    ) -> ExecutionId {
        options.manual = false;
        options.export = None;
        options.stream = false;
        if !internal {
            options.untimed = false;
            options.row_cap = None;
        }
        let execution = self.next_execution.fetch_add(1, Ordering::Relaxed);
        self.send(
            console,
            Command::Run(RunRequest {
                agent: Some(AgentRun {
                    reservation: crate::agent::Hold::new(
                        statements.iter().map(String::capacity).sum(),
                        &self.budget,
                    ),
                    cancel,
                    read_only,
                }),
                trace: self.console_trace(console),
                execution,
                statements,
                options,
            }),
        );
        execution
    }

    pub fn edit(&self, console: ConsoleId, job: crate::data_edit::Job) -> ExecutionId {
        let execution = self.next_execution.fetch_add(1, Ordering::Relaxed);
        self.send(
            console,
            Command::Edit {
                execution,
                job,
                trace: self.console_trace(console),
            },
        );
        execution
    }

    /// Accepts an import and returns its identity.
    pub fn import(&self, console: ConsoleId, job: ImportJob) -> ExecutionId {
        let execution = self.next_execution.fetch_add(1, Ordering::Relaxed);
        self.send(console, Command::Import(ImportRequest { execution, job }));
        execution
    }

    pub fn agent_metadata(
        &self,
        console: ConsoleId,
        connection: ConnectionId,
        job: String,
        request: MetadataRead,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        let binding = self.connections.get(&connection).cloned();
        let events = self.events.clone();
        let trace = self.console_trace(console);
        self.runtime.spawn(async move {
            // The job waits for this event, so a missing connection must still answer.
            let Some(binding) = binding else {
                let result = Err("The connection is not available. Connect it and submit the operation again.".into());
                let _ = events.send(ExecEvent::AgentMetadata { job, result }).await;
                return;
            };
            let read = async {
                let mut lease = match binding.pool.acquire_traced(trace).await {
                    Ok(lease) => lease,
                    Err(error) => {
                        if let Some(event) = binding.refusal(&error) { let _ = events.send(event).await; }
                        return Err(Failure::from(error));
                    }
                };
                let metadata = async {
                    lease.session().apply_context(&SessionContext { read_only: true, ..Default::default() }).await.map_err(Failure::from)?;
                    match &request {
                        MetadataRead::Catalog(schemas) => lease.session().read_catalog_of(schemas).await.map(AgentMetadata::Catalog),
                        MetadataRead::Fields(topic) => lease.session().read_noted_columns(crate::connector::kafka::TOPICS, topic).await.map(|(paths, notes)| AgentMetadata::Fields(paths, notes)),
                    }.map_err(Failure::from)
                };
                let limit = binding.settings().sql_timeout;
                let result = match limit {
                    Some(limit) => tokio::time::timeout(limit, metadata).await.map_err(|_| Failure::new("The metadata time limit ended. Try a smaller operation.", Some(Fix::SqlTimeout)))?,
                    None => metadata.await,
                };
                lease.release().await;
                result
            };
            let result = tokio::select! { result = read => result, () = cancel.cancelled() => Err("The assistant operation was cancelled.".into()) };
            let _ = events.send(ExecEvent::AgentMetadata { job, result }).await;
        });
    }

    /// Bridges a process signal flag to the existing cancellation command.
    pub fn stop_when(&self, console: ConsoleId, stopped: Arc<AtomicBool>) {
        let Some(entry) = self.consoles.get(&console) else {
            return;
        };
        let commands = entry.commands.clone();
        self.runtime.spawn(async move {
            while !commands.is_closed() {
                if stopped.load(Ordering::Relaxed) {
                    let _ = commands.send(Command::Stop);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
    }

    pub fn stop(&self, console: ConsoleId) {
        self.send(console, Command::Stop);
    }

    pub fn confirm_close(&self, console: ConsoleId, close: bool) {
        self.send(console, Command::ConfirmClose(close));
    }

    pub fn transaction(&self, console: ConsoleId, command: TransactionCommand) {
        self.send(
            console,
            Command::Transaction(command, self.console_trace(console)),
        );
    }

    pub fn close_console(&mut self, console: ConsoleId) {
        self.send(console, Command::Close);
        self.consoles.remove(&console);
        self.histories.remove(&console);
        if self.active_history.as_ref().is_some_and(|trace| {
            !self
                .histories
                .values()
                .any(|item| item.target == trace.target)
        }) {
            self.active_history = None;
        }
    }

    /// Reads the catalog of `schemas` through the pool, so it respects the connection
    /// limit. An empty list reads all schemas. Two reads can overlap and end in any
    /// order, so the answer carries `read`.
    pub fn load_catalog(&self, connection: ConnectionId, read: u64, schemas: Vec<String>) {
        let Some(binding) = self.connections.get(&connection).cloned() else {
            return;
        };
        let events = self.events.clone();
        let trace = self.shared_trace(connection);
        self.runtime.spawn(async move {
            let catalog = match binding.pool.acquire_traced(trace).await {
                Ok(mut lease) => {
                    let result = lease.session().read_catalog_of(&schemas).await;
                    lease.release().await;
                    result.map_err(|error| Failure::from(error).one_line())
                }
                Err(error) => {
                    if let Some(refusal) = binding.refusal(&error) {
                        let _ = events.send(refusal).await;
                    }
                    Err(Failure::from(error).one_line())
                }
            };
            let _ = events
                .send(ExecEvent::CatalogLoaded {
                    connection,
                    read,
                    catalog,
                })
                .await;
        });
    }

    pub fn load_columns(&self, connection: ConnectionId, relation: RelationRef) {
        let Some(binding) = self.connections.get(&connection).cloned() else {
            return;
        };
        let events = self.events.clone();
        let trace = self.shared_trace(connection);
        self.runtime.spawn(async move {
            let ((columns, notes), foreign_keys, referencing) =
                match binding.pool.acquire_traced(trace).await {
                    Ok(mut lease) => {
                        let session = lease.session();
                        let (schema, name) = (&relation.schema, &relation.relation);
                        let columns = session.read_noted_columns(schema, name).await;
                        // A failure here only costs the suggestions of `JOIN` and the key
                        // navigation.
                        let (keys, referencing) = match &columns {
                            Ok(_) => (
                                session.read_foreign_keys(schema, name).await,
                                session.read_referencing_keys(schema, name).await,
                            ),
                            Err(_) => (Ok(Vec::new()), Ok(Vec::new())),
                        };
                        lease.release().await;
                        let columns = match columns {
                            Ok((columns, notes)) => (Ok(columns), notes),
                            Err(error) => (Err(Failure::from(error).one_line()), Vec::new()),
                        };
                        (
                            columns,
                            keys.unwrap_or_default(),
                            referencing.unwrap_or_default(),
                        )
                    }
                    Err(error) => {
                        if let Some(refusal) = binding.refusal(&error) {
                            let _ = events.send(refusal).await;
                        }
                        (
                            (Err(Failure::from(error).one_line()), Vec::new()),
                            Vec::new(),
                            Vec::new(),
                        )
                    }
                };
            let _ = events
                .send(ExecEvent::ColumnsLoaded {
                    connection,
                    relation,
                    columns,
                    notes,
                    foreign_keys,
                    referencing,
                })
                .await;
        });
    }

    /// Closes the idle sessions of a pool, and it waits for a session in use, such as
    /// the session of a catalog read. That session closes when it returns.
    pub fn disconnect(&self, connection: ConnectionId) {
        let Some(binding) = self.connections.get(&connection) else {
            return;
        };
        let pool = binding.pool.clone();
        let limit = binding.settings().stop_grace;
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

    /// Opens one session with the rules of a saved connection, and closes it. With
    /// `names`, the session also reads the names of the schemas inside the same limit.
    pub fn test_connection(
        &self,
        test: u64,
        connector: Arc<dyn Connector>,
        limit: Duration,
        names: bool,
    ) {
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let probe = async {
                let mut session = connector.connect().await?;
                if !names {
                    return Ok(None);
                }
                let names = session.read_schema_names().await;
                names
                    .map(Some)
                    .map_err(|error| ConnectError::new(error.message))
            };
            let result = match tokio::time::timeout(limit, probe).await {
                Ok(result) => result,
                Err(_) => Err(ConnectError::new(crate::text::CONNECT_LIMIT_ENDED)
                    .fix(Some(crate::connector::Fix::ConnectLimit))),
            };
            let _ = events
                .send(ExecEvent::ConnectionTested { test, result })
                .await;
        });
    }

    /// Ends the tasks without a wait for a statement that does not answer.
    pub fn shutdown(self) {
        self.runtime.shutdown_background();
    }
}

fn context(settings: &Settings, schema: Option<String>, only_schema: bool) -> SessionContext {
    SessionContext {
        read_only: settings.read_only,
        schema,
        only_schema,
    }
}

/// Moves the events to the channel of the interface and wakes the main loop after each one.
async fn relay_events(mut events: Receiver<ExecEvent>, relay: Sender<ExecEvent>, wake: Wake) {
    while let Some(event) = events.recv().await {
        if relay.send(event).await.is_err() {
            return;
        }
        wake.wake();
    }
}
