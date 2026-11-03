//! The execution of an import. A table tab has no manual transaction, so `transaction`
//! stays `None` here, and a `Stop` without an answer closes the connection. The database
//! then undoes the open work.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::mpsc::UnboundedReceiver;

use super::{
    Command, ConsoleTask, Delivery, ExecEvent, ExecutionEnd, ExecutionId, StatementKey,
    StatementStatus, context, statement_status,
};
use crate::connector::{ExecError, TransactionCommand, TransactionState};
use crate::exec::ImportRequest;
use crate::import::feed::{Feed, LotEnd, Pending};
use crate::import::{BadRow, ImportMode};
use crate::pool::Lease;
use crate::ui::text;

type SharedFeed = Arc<Mutex<Feed>>;

/// The answer of the database to one `INSERT`.
enum Sent {
    /// The rows are in the table. `stop` holds a stop request that arrived meanwhile.
    Saved {
        rows: usize,
        stop: bool,
    },
    Refused(String),
    Stopped,
    /// The session is gone.
    Lost,
}

/// Where an import ended.
struct Outcome {
    end: ExecutionEnd,
    message: Option<String>,
}

impl Outcome {
    fn failed(message: String) -> Self {
        Self {
            end: ExecutionEnd::Failed,
            message: Some(message),
        }
    }

    fn stopped() -> Self {
        Self {
            end: ExecutionEnd::Stopped,
            message: None,
        }
    }

    fn lost() -> Self {
        Self {
            end: ExecutionEnd::ConnectionClosed,
            message: Some(text::IMPORT_LOST.to_owned()),
        }
    }
}

fn locked(feed: &SharedFeed) -> MutexGuard<'_, Feed> {
    feed.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs a step of the file on a thread that may block.
async fn on_file<T: Send + 'static>(
    feed: &SharedFeed,
    step: impl FnOnce(&mut Feed) -> T + Send + 'static,
) -> Result<T, String> {
    let feed = feed.clone();
    tokio::task::spawn_blocking(move || step(&mut locked(&feed)))
        .await
        .map_err(|_| text::IMPORT_TASK_FAILED.to_owned())
}

/// The first and the last line of a batch.
fn lines_of(rows: &[Pending]) -> (u64, u64) {
    let line = |row: Option<&Pending>| row.map_or(0, |row| row.line);
    (line(rows.first()), line(rows.last()))
}

impl ConsoleTask {
    /// Sends one statement. A lost session leaves `lease` empty.
    async fn send(
        &mut self,
        lease: &mut Option<Lease>,
        key: StatementKey,
        sql: &str,
        rows: usize,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Sent {
        let Some(active) = lease.as_mut() else {
            return Sent::Lost;
        };
        let delivery = Delivery {
            cap: self.settings.row_cap,
            export: None,
            timed: false,
        };
        let (waited, reason) = self
            .run_statement(key, active, sql, delivery, commands)
            .await;
        if waited.closes_session() {
            *lease = None;
        }
        let (status, end) = statement_status(waited, reason);
        match status {
            StatementStatus::Success => {
                let saved = end.and_then(|end| end.changed).map(usize::try_from);
                Sent::Saved {
                    rows: saved.and_then(Result::ok).unwrap_or(rows),
                    stop: reason.is_some(),
                }
            }
            StatementStatus::Error(message) => Sent::Refused(message),
            StatementStatus::Unknown => Sent::Lost,
            _ => Sent::Stopped,
        }
    }

    /// Sends the rows of a failed batch one by one. It returns the end of the import.
    async fn retry_rows(
        &mut self,
        lease: &mut Option<Lease>,
        feed: &SharedFeed,
        key: &mut StatementKey,
        rows: Vec<Pending>,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Option<Outcome> {
        let mut failed: Vec<BadRow> = Vec::new();
        let mut outcome = None;
        for row in rows {
            let sql = locked(feed).statement(std::slice::from_ref(&row));
            key.index += 1;
            match self.send(lease, *key, &sql, 1, commands).await {
                Sent::Saved { rows, stop } => {
                    locked(feed).add_rows(rows);
                    if stop {
                        outcome = Some(Outcome::stopped());
                        break;
                    }
                }
                Sent::Refused(reason) => {
                    failed.push(BadRow {
                        line: row.line,
                        source: row.source,
                        reason,
                    });
                    let guard = locked(feed);
                    if guard.at_limit(failed.len()) {
                        outcome = Some(Outcome::failed(guard.limit_reason()));
                        break;
                    }
                }
                Sent::Stopped => {
                    outcome = Some(Outcome::stopped());
                    break;
                }
                Sent::Lost => {
                    outcome = Some(Outcome::lost());
                    break;
                }
            }
        }
        match on_file(feed, move |feed| feed.log_rows(&failed)).await {
            Ok(Ok(())) => outcome,
            Ok(Err(reason)) | Err(reason) => Some(Outcome::failed(reason)),
        }
    }

    /// Sends the batches of the file. It returns the end of the import.
    async fn send_lots(
        &mut self,
        lease: &mut Option<Lease>,
        feed: &SharedFeed,
        mode: ImportMode,
        execution: ExecutionId,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Outcome {
        let console = self.console;
        let mut key = StatementKey {
            console,
            execution,
            index: 0,
        };
        loop {
            if self.stop_is_pending(commands).await {
                return Outcome::stopped();
            }
            let lot = match on_file(feed, Feed::next_lot).await {
                Ok(lot) => lot,
                Err(reason) => return Outcome::failed(reason),
            };
            // A rollback removes all the rows, so they need no trip to the database.
            let useless = mode == ImportMode::AllOrNothing && matches!(lot.end, LotEnd::Stop(_));
            if !lot.rows.is_empty() && !useless {
                let sql = locked(feed).statement(&lot.rows);
                key.index += 1;
                let count = lot.rows.len();
                match self.send(lease, key, &sql, count, commands).await {
                    Sent::Saved { rows, stop } => {
                        locked(feed).add_rows(rows);
                        if stop {
                            return Outcome::stopped();
                        }
                    }
                    Sent::Refused(_) if mode == ImportMode::LogErrors => {
                        let end = self
                            .retry_rows(lease, feed, &mut key, lot.rows, commands)
                            .await;
                        if let Some(outcome) = end {
                            return outcome;
                        }
                    }
                    Sent::Refused(reason) => {
                        let (first, last) = lines_of(&lot.rows);
                        return Outcome::failed(text::import_bad_lot(first, last, &reason));
                    }
                    Sent::Stopped => return Outcome::stopped(),
                    Sent::Lost => return Outcome::lost(),
                }
            }
            match lot.end {
                LotEnd::More => {}
                LotEnd::Done => {
                    return Outcome {
                        end: ExecutionEnd::Completed,
                        message: None,
                    };
                }
                LotEnd::Stop(reason) => return Outcome::failed(reason),
            }
        }
    }

    /// Ends the transaction of an import. It returns the reason of a failed commit.
    /// A failed rollback adds nothing to the first reason, and the session then closes.
    /// A database can end the transaction first: SQLite undoes it after an interrupt.
    async fn end_transaction(lease: &mut Option<Lease>, commit: bool) -> Result<(), ExecError> {
        let Some(active) = lease.as_mut() else {
            return Ok(());
        };
        let session = active.session();
        if commit {
            return session.control(TransactionCommand::Commit).await;
        }
        if session.transaction_state().await != TransactionState::None {
            let _ = session.control(TransactionCommand::Rollback).await;
        }
        Ok(())
    }

    /// Prepares the session and opens the file.
    async fn open_import(
        &mut self,
        lease: &mut Lease,
        request: ImportRequest,
    ) -> Result<(SharedFeed, ImportMode), String> {
        let job = request.job;
        let mode = job.mode;
        let context = context(&self.settings, None);
        let session = lease.session();
        session
            .apply_context(&context)
            .await
            .map_err(|error| error.message)?;
        let opened = tokio::task::spawn_blocking(move || Feed::open(job)).await;
        let feed = opened.map_err(|_| text::IMPORT_TASK_FAILED.to_owned())??;
        if mode == ImportMode::AllOrNothing {
            session
                .control(TransactionCommand::Begin)
                .await
                .map_err(|error| error.message)?;
        }
        Ok((Arc::new(Mutex::new(feed)), mode))
    }

    /// Runs one import from the first batch to the end of its transaction.
    pub(super) async fn run_import(
        &mut self,
        request: ImportRequest,
        commands: &mut UnboundedReceiver<Command>,
    ) {
        let execution = request.execution;
        let console = self.console;
        self.emit(ExecEvent::Queued { console, execution }).await;
        let Some(mut lease) = self.acquire(execution, commands).await else {
            return;
        };
        self.emit(ExecEvent::Started { console, execution }).await;
        let (feed, mode) = match self.open_import(&mut lease, request).await {
            Ok(opened) => opened,
            Err(message) => {
                self.finish(execution, ExecutionEnd::Failed, Some(message))
                    .await;
                drop_or_release(lease).await;
                return;
            }
        };
        let mut lease = Some(lease);
        let mut outcome = self
            .send_lots(&mut lease, &feed, mode, execution, commands)
            .await;

        let atomic = mode == ImportMode::AllOrNothing;
        let complete = outcome.end == ExecutionEnd::Completed;
        if atomic {
            if let Err(error) = Self::end_transaction(&mut lease, complete).await {
                outcome = Outcome::failed(error.message);
            }
            if outcome.end != ExecutionEnd::Completed {
                locked(&feed).clear_rows();
            }
        }
        if let Some(lease) = lease {
            drop_or_release(lease).await;
        }
        let closed = on_file(&feed, Feed::close).await;
        if let (Ok(Err(reason)) | Err(reason), None) = (closed, &outcome.message) {
            outcome.message = Some(reason);
        }
        self.finish(execution, outcome.end, outcome.message).await;
    }
}

/// A table tab has no commit, so a session with an open transaction cannot stay. The
/// close of the connection undoes that transaction.
async fn drop_or_release(mut lease: Lease) {
    match lease.session().transaction_state().await {
        TransactionState::None => lease.release().await,
        _ => drop(lease),
    }
}
