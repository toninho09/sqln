//! The task of one console. It runs scripts one statement at a time, and it keeps
//! the session between executions while a transaction stays open.

mod data_edit;
mod import;

use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedReceiver};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use super::{
    Binding, Command, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, ResultSink,
    RunRequest, Settings, StatementKey, StatementStatus, StreamControl, context,
};
use crate::command_log::TraceContext;
use crate::connector::{ExecError, StatementEnd, TransactionCommand, TransactionState};
use crate::export::ExportWriter;
use crate::grid::Budget;
use crate::pool::Lease;
use crate::sql::{is_continuous, with_row_limit};

/// A delay that stays longer than any statement. It stands for "no time limit".
const NO_LIMIT: Duration = Duration::from_secs(60 * 60 * 24 * 365);

#[derive(Clone, Copy, PartialEq, Eq)]
enum StopReason {
    User,
    Timeout,
}

enum Waited {
    Finished {
        result: Result<StatementEnd, ExecError>,
        /// False when a cancel request can still reach this session.
        reusable: bool,
    },
    /// The connection closed before the statement answered.
    Closed,
}

impl Waited {
    /// Returns true when the session cannot run another statement.
    fn closes_session(&self) -> bool {
        matches!(
            self,
            Waited::Closed
                | Waited::Finished {
                    reusable: false,
                    ..
                }
        )
    }
}

/// How one statement runs: the row cap, the file of an export, and the time limit.
struct Delivery {
    cap: usize,
    export: Option<ExportWriter>,
    stream: bool,
    timed: bool,
    capture: Option<super::sink::Capture>,
    parameters: Vec<crate::sql::parameters::Parameter>,
    /// True for a continuous statement: no time limit, a live result, and a stop that
    /// only the connector handles.
    live: bool,
}

pub struct ConsoleTask {
    console: ConsoleId,
    binding: Binding,
    /// The settings of the current execution. A saved change acts from the next one.
    settings: Settings,
    budget: Arc<Budget>,
    events: Sender<ExecEvent>,
    held: Option<Lease>,
    transaction: TransactionState,
    trace: Option<TraceContext>,
    cancellation: tokio_util::sync::CancellationToken,
    stream_stopped: Option<Arc<StreamControl>>,
}

impl ConsoleTask {
    pub fn new(
        console: ConsoleId,
        binding: Binding,
        budget: Arc<Budget>,
        events: Sender<ExecEvent>,
        stream_stopped: Option<Arc<StreamControl>>,
    ) -> Self {
        let settings = binding.settings().clone();
        Self {
            console,
            settings,
            binding,
            budget,
            events,
            stream_stopped,
            held: None,
            transaction: TransactionState::None,
            trace: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        }
    }

    pub async fn serve(mut self, mut commands: UnboundedReceiver<Command>) {
        while let Some(command) = commands.recv().await {
            self.settings = self.binding.settings().clone();
            self.cancellation = tokio_util::sync::CancellationToken::new();
            match command {
                Command::Edit {
                    execution,
                    job,
                    trace,
                } => {
                    self.trace = trace;
                    self.run_edit(execution, job, &mut commands).await;
                }
                Command::Run(request) => self.run_script(request, &mut commands).await,
                Command::Import(request) => {
                    self.trace = None;
                    if let Some(lease) = &mut self.held {
                        lease.session().set_trace_context(None);
                    }
                    self.run_import(request, &mut commands).await;
                }
                Command::Transaction(command, trace) => {
                    self.trace = trace;
                    if let Some(lease) = &mut self.held {
                        lease.session().set_trace_context(self.trace.clone());
                    }
                    self.control(command).await;
                }
                Command::Rebind(binding) => self.binding = binding,
                Command::Stop | Command::ConfirmClose(_) => {}
                Command::Close => break,
            }
        }
    }

    // `&mut self` keeps these futures `Send`, because a session is not `Sync`.
    async fn emit(&mut self, event: ExecEvent) {
        let _ = self.events.send(event).await;
    }

    async fn set_transaction(&mut self, state: TransactionState) {
        if self.transaction != state {
            self.transaction = state;
            let console = self.console;
            self.emit(ExecEvent::TransactionChanged { console, state })
                .await;
        }
    }

    /// A lost connection without a transaction leaves no transaction to report.
    async fn observe_transaction(&mut self, state: TransactionState) {
        if state != TransactionState::Unknown || self.transaction != TransactionState::None {
            self.set_transaction(state).await;
        }
    }

    async fn finish(&mut self, execution: ExecutionId, end: ExecutionEnd, message: Option<String>) {
        let console = self.console;
        self.emit(ExecEvent::ExecutionFinished {
            console,
            execution,
            end,
            message,
        })
        .await;
    }

    async fn acquire(
        &mut self,
        execution: ExecutionId,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Option<Lease> {
        if let Some(mut lease) = self.held.take() {
            lease.session().set_trace_context(self.trace.clone());
            return Some(lease);
        }
        let pool = self.binding.pool.clone();
        let mut acquire = pin!(pool.acquire_traced(self.trace.clone()));
        loop {
            tokio::select! {
                () = self.cancellation.cancelled() => {
                    self.finish(execution, ExecutionEnd::CanceledBeforeStart, None).await;
                    return None;
                }
                result = &mut acquire => match result {
                    Ok(lease) => return Some(lease),
                    Err(error) => {
                        if let Some(refusal) = self.binding.refusal(&error) {
                            self.emit(refusal).await;
                        }
                        self.finish(execution, ExecutionEnd::Failed, Some(error.message)).await;
                        return None;
                    }
                },
                command = commands.recv() => match command {
                    None | Some(Command::Stop | Command::Close) => {
                        self.finish(execution, ExecutionEnd::CanceledBeforeStart, None).await;
                        return None;
                    }
                    Some(other) => self.refuse(other).await,
                },
            }
        }
    }

    async fn run_script(&mut self, request: RunRequest, commands: &mut UnboundedReceiver<Command>) {
        let RunRequest {
            agent,
            trace,
            execution,
            statements,
            options,
        } = request;
        if let Some(message) =
            continuous_refusal(&statements, self.binding.pool.dialect(), options.live)
        {
            self.finish(execution, ExecutionEnd::Refused, Some(message.to_owned()))
                .await;
            return;
        }
        let _agent_hold;
        let agent_run = agent.is_some();
        if let Some(agent) = agent {
            _agent_hold = match agent.reservation {
                Ok(hold) => Some(hold),
                Err(error) => {
                    self.finish(execution, ExecutionEnd::Refused, Some(error.message))
                        .await;
                    return;
                }
            };
            self.cancellation = agent.cancel;
            if self.held.is_some() || self.transaction != TransactionState::None {
                self.finish(
                    execution,
                    ExecutionEnd::Refused,
                    Some(
                        "An agent cannot use an open or unknown transaction. Use its own console."
                            .into(),
                    ),
                )
                .await;
                return;
            }
            // The application validates agent SQL on a blocking worker before it queues the run.
            // Full access removes only this agent restriction, never the connection's own.
            self.settings.read_only |= agent.read_only;
        } else {
            _agent_hold = None;
        }
        self.trace = trace;
        let console = self.console;
        self.emit(ExecEvent::Queued { console, execution }).await;
        // The state of a closed session does not apply to the next session.
        let reserved = self.held.is_some();
        if !reserved {
            self.transaction = TransactionState::None;
        }
        let Some(mut lease) = self.acquire(execution, commands).await else {
            return;
        };
        self.emit(ExecEvent::Started { console, execution }).await;

        // A reserved session has this context, and a failed transaction refuses `SET`.
        let context = context(&self.settings, options.schema, options.only_schema);
        let cap = options.row_cap.unwrap_or(self.settings.row_cap);
        let export = options.export;
        let untimed = options.untimed;
        if !reserved && let Err(error) = lease.session().apply_context(&context).await {
            self.finish(execution, ExecutionEnd::Failed, Some(error.message))
                .await;
            self.keep_or_release(lease).await;
            return;
        }
        if options.manual && self.transaction == TransactionState::None {
            if let Err(error) = lease.session().control(TransactionCommand::Begin).await {
                self.finish(execution, ExecutionEnd::Failed, Some(error.message))
                    .await;
                self.keep_or_release(lease).await;
                return;
            }
            self.set_transaction(TransactionState::Open).await;
        }

        let mut end = ExecutionEnd::Completed;
        let mut lease = Some(lease);
        for (index, sql) in statements.iter().enumerate() {
            let key = StatementKey {
                console,
                execution,
                index,
            };
            let stop_requested = self.stop_is_pending(commands).await;
            let Some(active) = lease.as_mut().filter(|_| {
                !stop_requested
                    && (end == ExecutionEnd::Completed
                        || (options.continue_on_error && end == ExecutionEnd::Failed))
            }) else {
                if stop_requested && end == ExecutionEnd::Completed {
                    end = ExecutionEnd::Stopped;
                }
                self.emit_not_executed(key).await;
                continue;
            };

            self.emit(ExecEvent::StatementStarted { key }).await;
            let delivery = Delivery {
                cap,
                export: export.clone(),
                stream: options.stream,
                // An export ends with the user. It has no time limit.
                timed: export.is_none() && !untimed,
                capture: None,
                parameters: options.parameters.clone(),
                live: options.live && is_continuous(sql, self.binding.pool.dialect()),
            };
            let (waited, reason) = self
                .run_statement(key, active, sql, delivery, commands)
                .await;
            let closed = waited.closes_session();
            let error_offset = match &waited {
                Waited::Finished {
                    result: Err(error), ..
                } => error.offset,
                _ => None,
            };
            let (status, statement_end) = statement_status(waited, reason);
            if closed {
                lease = None;
                if self.transaction != TransactionState::None {
                    self.set_transaction(TransactionState::Unknown).await;
                }
            } else if let Some(active) = lease.as_mut() {
                active.session().set_trace_context(self.trace.clone());
                let state = active.session().transaction_state().await;
                self.observe_transaction(state).await;
                if state == TransactionState::Unknown {
                    lease = None;
                }
            }
            end = match (&status, reason) {
                (StatementStatus::Unknown, _) => ExecutionEnd::ConnectionClosed,
                (StatementStatus::Error(_), _) => ExecutionEnd::Failed,
                (StatementStatus::Timeout, _) => ExecutionEnd::TimedOut,
                (StatementStatus::Stopped, _) | (_, Some(StopReason::User)) => {
                    ExecutionEnd::Stopped
                }
                (_, Some(StopReason::Timeout)) => ExecutionEnd::TimedOut,
                _ => end,
            };
            let end_event = ExecEvent::StatementFinished {
                key,
                status,
                end: statement_end,
                error_offset,
            };
            self.emit(end_event).await;
        }

        let mut message = None;
        // A noninteractive run cannot leave a transaction open. A batch that holds
        // the transaction ends it with its own COMMIT or ROLLBACK statement.
        if (agent_run || (options.stream && !options.hold_transaction))
            && let Some(active) = lease.as_mut()
            && active.session().transaction_state().await != TransactionState::None
        {
            message = Some(if agent_run {
                crate::ui::text::AGENT_ROLLED_BACK.to_owned()
            } else {
                "The statement leaves an open transaction. SQLN rolls it back.".to_owned()
            });
            if end == ExecutionEnd::Completed {
                end = ExecutionEnd::Failed;
            }
            if active
                .session()
                .control(TransactionCommand::Rollback)
                .await
                .is_err()
            {
                // Closing the session ends its transaction.
                lease = None;
                self.observe_transaction(TransactionState::None).await;
            }
        }
        if let Some(lease) = lease {
            self.keep_or_release(lease).await;
        }
        if let Some(export) = export {
            // The sync can take time, and this task shares its thread with other consoles.
            let _ = tokio::task::spawn_blocking(move || export.seal()).await;
        }
        self.finish(execution, end, message).await;
    }

    /// Reads the waiting commands. Each console accepts only one execution.
    async fn stop_is_pending(&mut self, commands: &mut UnboundedReceiver<Command>) -> bool {
        let mut stop = self.cancellation.is_cancelled();
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Stop | Command::Close => stop = true,
                other => self.refuse(other).await,
            }
        }
        stop
    }

    async fn refuse(&mut self, command: Command) {
        let console = self.console;
        match command {
            Command::Run(request) => {
                let message = "The console already has an execution.".to_owned();
                let execution = request.execution;
                self.finish(execution, ExecutionEnd::Refused, Some(message))
                    .await;
            }
            Command::Edit { execution, .. } => {
                self.finish(
                    execution,
                    ExecutionEnd::Refused,
                    Some(crate::ui::text::TABLE_BUSY.to_owned()),
                )
                .await;
            }
            Command::Import(request) => {
                let message = "The console already has an execution.".to_owned();
                self.finish(request.execution, ExecutionEnd::Refused, Some(message))
                    .await;
            }
            Command::Transaction(..) => {
                let message = "The console has an execution in progress.".to_owned();
                self.emit(ExecEvent::TransactionFailed { console, message })
                    .await;
            }
            Command::Stop | Command::ConfirmClose(_) | Command::Rebind(_) | Command::Close => {}
        }
    }

    async fn emit_not_executed(&mut self, key: StatementKey) {
        let status = StatementStatus::NotExecuted;
        self.emit(ExecEvent::StatementFinished {
            key,
            status,
            end: None,
            error_offset: None,
        })
        .await;
    }

    /// Waits for one statement, with its time limit and the stop request.
    /// The `run` future stays active after a cancellation, so the session reads the answer.
    async fn run_statement(
        &mut self,
        key: StatementKey,
        lease: &mut Lease,
        sql: &str,
        delivery: Delivery,
        commands: &mut UnboundedReceiver<Command>,
    ) -> (Waited, Option<StopReason>) {
        let Delivery {
            cap,
            export,
            stream,
            timed,
            capture,
            parameters,
            live,
        } = delivery;
        let mut sink = ResultSink::new(key, self.events.clone(), self.budget.clone(), cap);
        sink.stream(stream);
        if live {
            sink.live();
        }
        let time_limit = self.settings.sql_timeout.filter(|_| timed && !live);
        // The row after the cap proves that the result has more rows. An export has no cap.
        let rows = cap.saturating_add(1);
        let limited = (!stream
            && capture.is_none()
            && self.settings.cap_in_sql
            && export.is_none()
            && cap < usize::MAX)
            .then(|| with_row_limit(sql, self.binding.pool.dialect(), rows))
            .flatten();
        if let Some(capture) = capture {
            sink.capture_to(capture);
        }
        if let Some(export) = export {
            sink.export_to(export);
        }
        if limited.is_some() {
            self.emit(ExecEvent::LimitAdded { key, rows }).await;
        }
        // An error in the added clause points to the end of the text of the user.
        let user_end = sql.len();
        let sql = limited.as_deref().unwrap_or(sql);
        let bound = if parameters.is_empty() {
            None
        } else {
            match crate::sql::parameters::bind(sql, self.binding.pool.dialect(), &parameters) {
                Ok(bound) => Some(bound),
                Err(message) => {
                    return (
                        Waited::Finished {
                            result: Err(ExecError::new(message)),
                            reusable: true,
                        },
                        None,
                    );
                }
            }
        };
        let stopping = sink.stop_token();
        let session = lease.session();
        let trace = self.trace.as_ref().map(|trace| {
            let mut trace = trace.clone();
            trace.key = Some(key);
            trace.user = true;
            trace
        });
        session.set_trace_context(trace);
        let canceller = session.canceller();
        let stream_stopped = self.stream_stopped.clone().filter(|_| stream);
        if let Some(control) = &stream_stopped {
            control.start(time_limit);
        }
        let cancel = || {
            stopping.cancel();
            if let Some(stopped) = &stream_stopped {
                stopped.stop();
            }
            let canceller = canceller.clone();
            // A continuous statement waits for the stop token and ends by itself.
            tokio::spawn(async move {
                if live {
                    Ok(())
                } else {
                    canceller.cancel().await
                }
            })
        };

        let mut run = if let Some(bound) = &bound {
            session.run_parameters(&bound.sql, &bound.parameters, sink)
        } else {
            session.run(sql, sink)
        };
        let settings = self.settings.clone();
        let mut limit = pin!(sleep(time_limit.unwrap_or(NO_LIMIT)));
        let mut grace = pin!(sleep(NO_LIMIT));
        let mut reason = None;
        let mut asked = false;
        let mut inbox_open = true;
        let mut request: Option<JoinHandle<Result<(), String>>> = None;

        loop {
            tokio::select! {
                result = &mut run => {
                    let reusable = settle(request, settings.stop_grace).await;
                    let result = result.map_err(|mut error| {
                        error.offset = error.offset.map(|offset| {
                            bound.as_ref().map_or(offset, |bound| bound.source_offset(offset)).min(user_end)
                        });
                        error
                    });
                    return (Waited::Finished { result, reusable }, reason);
                }
                sent = async { request.as_mut().expect("the guard checks the request").await },
                    if request.is_some() =>
                {
                    request = None;
                    // Without the request, the statement has no reason to stop soon.
                    if !matches!(sent, Ok(Ok(()))) {
                        grace.as_mut().reset(tokio::time::Instant::now());
                    }
                }
                () = self.cancellation.cancelled(), if reason.is_none() => {
                    reason = Some(StopReason::User);
                    request = Some(cancel());
                    grace.as_mut().reset(tokio::time::Instant::now() + settings.stop_grace);
                }
                () = &mut limit, if reason.is_none() && time_limit.is_some() => {
                    reason = Some(StopReason::Timeout);
                    request = Some(cancel());
                    grace.as_mut().reset(tokio::time::Instant::now() + settings.stop_grace);
                }
                () = &mut grace, if reason.is_some() && !asked => {
                    if self.transaction == TransactionState::None {
                        return (Waited::Closed, reason);
                    }
                    asked = true;
                    let (console, execution) = (key.console, key.execution);
                    self.emit(ExecEvent::ConfirmClose { console, execution }).await;
                }
                command = commands.recv(), if inbox_open => match command {
                    Some(Command::ConfirmClose(true)) => return (Waited::Closed, reason),
                    Some(Command::ConfirmClose(false)) => {}
                    Some(Command::Stop | Command::Close) | None => {
                        inbox_open = command.is_some();
                        if reason.is_none() {
                            reason = Some(StopReason::User);
                            request = Some(cancel());
                            grace.as_mut().reset(tokio::time::Instant::now() + settings.stop_grace);
                        }
                    }
                    Some(other) => self.refuse(other).await,
                },
            }
        }
    }

    async fn keep_or_release(&mut self, mut lease: Lease) {
        let state = lease.session().transaction_state().await;
        self.observe_transaction(state).await;
        match state {
            TransactionState::None => lease.release().await,
            TransactionState::Open | TransactionState::Failed => self.held = Some(lease),
            TransactionState::Unknown => drop(lease),
        }
    }

    /// Applies a commit or a rollback on the session of the open transaction.
    async fn control(&mut self, command: TransactionCommand) {
        let console = self.console;
        let Some(mut lease) = self.held.take() else {
            let message = "No transaction is open.".to_owned();
            self.emit(ExecEvent::TransactionFailed { console, message })
                .await;
            return;
        };
        let result = lease.session().control(command).await;
        self.keep_or_release(lease).await;
        if let Err(error) = result {
            self.emit(ExecEvent::TransactionFailed {
                console,
                message: error.message,
            })
            .await;
        }
    }
}

/// Waits for a cancel request that is on its way. A late request must not reach the
/// next statement, so a session with an open request cannot run again.
async fn settle(request: Option<JoinHandle<Result<(), String>>>, limit: Duration) -> bool {
    let Some(mut request) = request else {
        return true;
    };
    let settled = timeout(limit, &mut request).await.is_ok();
    if !settled {
        request.abort();
    }
    settled
}

/// Returns the reason to refuse a run with a continuous statement: it must be the last
/// statement, and only a console accepts it.
fn continuous_refusal(
    statements: &[String],
    dialect: crate::sql::Dialect,
    live: bool,
) -> Option<&'static str> {
    let position = statements
        .iter()
        .position(|sql| is_continuous(sql, dialect))?;
    if !live {
        Some(crate::ui::text::CONTINUOUS_CONSOLE_ONLY)
    } else if position + 1 < statements.len() {
        Some(crate::ui::text::CONTINUOUS_NOT_LAST)
    } else {
        None
    }
}

fn statement_status(
    waited: Waited,
    reason: Option<StopReason>,
) -> (StatementStatus, Option<StatementEnd>) {
    match (waited, reason) {
        (Waited::Closed, _) => (StatementStatus::Unknown, None),
        (
            Waited::Finished {
                result: Ok(end), ..
            },
            _,
        ) => (StatementStatus::Success, Some(end)),
        (Waited::Finished { result: Err(_), .. }, Some(StopReason::User)) => {
            (StatementStatus::Stopped, None)
        }
        (Waited::Finished { result: Err(_), .. }, Some(StopReason::Timeout)) => {
            (StatementStatus::Timeout, None)
        }
        (
            Waited::Finished {
                result: Err(error), ..
            },
            None,
        ) => (StatementStatus::Error(error.message), None),
    }
}

impl StatementEnd {
    pub fn reached_limit(&self) -> bool {
        self.limit != ReadLimit::None
    }
}
