//! The task of one console. It runs scripts one statement at a time, and it keeps
//! the session between executions while a transaction stays open.

use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedReceiver};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use super::{
    Binding, Command, ConsoleId, ExecEvent, ExecutionEnd, ExecutionId, ReadLimit, ResultSink,
    RunRequest, StatementKey, StatementStatus, context,
};
use crate::connector::{ExecError, StatementEnd, TransactionCommand, TransactionState};
use crate::grid::Budget;
use crate::pool::Lease;

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

pub struct ConsoleTask {
    console: ConsoleId,
    binding: Binding,
    budget: Arc<Budget>,
    events: Sender<ExecEvent>,
    held: Option<Lease>,
    transaction: TransactionState,
}

impl ConsoleTask {
    pub fn new(
        console: ConsoleId,
        binding: Binding,
        budget: Arc<Budget>,
        events: Sender<ExecEvent>,
    ) -> Self {
        Self {
            console,
            binding,
            budget,
            events,
            held: None,
            transaction: TransactionState::None,
        }
    }

    pub async fn serve(mut self, mut commands: UnboundedReceiver<Command>) {
        while let Some(command) = commands.recv().await {
            match command {
                Command::Run(request) => self.run_script(request, &mut commands).await,
                Command::Transaction(command) => self.control(command).await,
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
        if let Some(lease) = self.held.take() {
            return Some(lease);
        }
        let pool = self.binding.pool.clone();
        let mut acquire = pin!(pool.acquire());
        loop {
            tokio::select! {
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
            execution,
            statements,
            options,
        } = request;
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
        let context = context(&self.binding.settings, options.schema);
        let cap = options.row_cap.unwrap_or(self.binding.settings.row_cap);
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
            let Some(active) = lease
                .as_mut()
                .filter(|_| end == ExecutionEnd::Completed && !stop_requested)
            else {
                if stop_requested && end == ExecutionEnd::Completed {
                    end = ExecutionEnd::Stopped;
                }
                self.emit_not_executed(key).await;
                continue;
            };

            self.emit(ExecEvent::StatementStarted { key }).await;
            let (waited, reason) = self.run_statement(key, active, sql, cap, commands).await;
            let closed = matches!(
                waited,
                Waited::Closed
                    | Waited::Finished {
                        reusable: false,
                        ..
                    }
            );
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

        if let Some(lease) = lease {
            self.keep_or_release(lease).await;
        }
        self.finish(execution, end, None).await;
    }

    /// Reads the waiting commands. Each console accepts only one execution.
    async fn stop_is_pending(&mut self, commands: &mut UnboundedReceiver<Command>) -> bool {
        let mut stop = false;
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
            Command::Transaction(_) => {
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
        cap: usize,
        commands: &mut UnboundedReceiver<Command>,
    ) -> (Waited, Option<StopReason>) {
        let sink = ResultSink::new(key, self.events.clone(), self.budget.clone(), cap);
        let stopping = sink.stop_flag();
        let session = lease.session();
        let canceller = session.canceller();
        let cancel = || {
            stopping.store(true, Ordering::SeqCst);
            let canceller = canceller.clone();
            tokio::spawn(async move { canceller.cancel().await })
        };

        let mut run = session.run(sql, sink);
        let settings = self.binding.settings.clone();
        let mut limit = pin!(sleep(settings.sql_timeout.unwrap_or(NO_LIMIT)));
        let mut grace = pin!(sleep(NO_LIMIT));
        let mut reason = None;
        let mut asked = false;
        let mut inbox_open = true;
        let mut request: Option<JoinHandle<Result<(), String>>> = None;

        loop {
            tokio::select! {
                result = &mut run => {
                    let reusable = settle(request, settings.stop_grace).await;
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
                () = &mut limit, if reason.is_none() && settings.sql_timeout.is_some() => {
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
