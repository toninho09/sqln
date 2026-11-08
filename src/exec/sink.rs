//! Receives rows from a connector and sends batches to the interface.

use std::sync::{Arc, Mutex};

use crate::data_edit::SavedRow;

#[derive(Debug, Default)]
pub(super) struct Captured {
    pub columns: Vec<String>,
    pub rows: Vec<Arc<SavedRow>>,
    pub incomplete: bool,
    pub limit: ReadLimit,
    pub changed: Option<u64>,
}

pub(super) type Capture = Arc<Mutex<Captured>>;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc::Sender;

use super::{ExecEvent, StatementKey};
use crate::export::ExportWriter;
use crate::grid::{Budget, Reservation, Row};
use crate::ui::text;

const BATCH_ROWS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// The grid accepts no more rows of this statement.
    Stop,
    /// The runner asked the database to stop, so the connector drops the other rows
    /// and reads to the answer of the database.
    Discard,
}

/// The reason that stopped the storage of rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadLimit {
    #[default]
    None,
    RowCap,
    Budget,
    ValueTooLarge,
}

pub struct ResultSink {
    key: StatementKey,
    events: Sender<ExecEvent>,
    budget: Arc<Budget>,
    cap: usize,
    stored: usize,
    limit: ReadLimit,
    batch: Vec<Row>,
    batch_bytes: usize,
    /// True between the columns of a result and its end.
    open: bool,
    /// The rows of the results that ended.
    total: usize,
    /// The first limit of the results that ended.
    first_limit: ReadLimit,
    stopping: Arc<AtomicBool>,
    /// The rows go to this file, and the grid receives nothing.
    export: Option<ExportWriter>,
    stream: bool,
    capture: Option<Capture>,
}

/// The rows of a batch that nobody received leave the budget.
impl Drop for ResultSink {
    fn drop(&mut self) {
        self.budget.release(self.batch_bytes);
    }
}

impl ResultSink {
    pub fn new(
        key: StatementKey,
        events: Sender<ExecEvent>,
        budget: Arc<Budget>,
        cap: usize,
    ) -> Self {
        Self {
            key,
            events,
            budget,
            cap,
            stored: 0,
            limit: ReadLimit::None,
            batch: Vec::new(),
            batch_bytes: 0,
            open: false,
            total: 0,
            first_limit: ReadLimit::None,
            stopping: Arc::new(AtomicBool::new(false)),
            export: None,
            stream: false,
            capture: None,
        }
    }

    pub(super) fn stream(&mut self, stream: bool) {
        self.stream = stream;
    }

    pub(super) fn capture_to(&mut self, capture: Capture) {
        self.capture = Some(capture);
    }

    /// Sends the rows to a file. They skip the row cap and the budget of the grid.
    pub fn export_to(&mut self, writer: ExportWriter) {
        self.export = Some(writer);
    }

    /// Returns the flag that ends the storage of rows.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stopping.clone()
    }

    fn store(&mut self, row: Row) -> Flow {
        if self.stopping.load(Ordering::SeqCst) {
            return Flow::Discard;
        }
        if self.stream {
            self.batch.push(row);
            self.stored += 1;
            return Flow::Continue;
        }
        if let Some(capture) = &self.capture {
            let mut captured = capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if captured.rows.len() >= self.cap {
                captured.incomplete = true;
                captured.limit = ReadLimit::RowCap;
                self.limit = ReadLimit::RowCap;
                return Flow::Stop;
            }
            match SavedRow::reserve(row, &self.budget) {
                Ok(row) => {
                    captured.rows.push(row);
                    self.stored += 1;
                    return Flow::Continue;
                }
                Err(_) => {
                    captured.incomplete = true;
                    captured.limit = ReadLimit::Budget;
                    self.limit = ReadLimit::Budget;
                    return Flow::Stop;
                }
            }
        }
        if let Some(export) = &self.export {
            if !export.write_row(row.cells()) {
                return Flow::Stop;
            }
            self.stored += 1;
            return Flow::Continue;
        }
        if self.limit != ReadLimit::None {
            return Flow::Stop;
        }
        // The row after the cap proves that the result has more rows.
        if self.stored == self.cap {
            self.limit = ReadLimit::RowCap;
            return Flow::Stop;
        }
        match self.budget.reserve(&row) {
            Reservation::Accepted(bytes) => {
                self.batch.push(row);
                self.batch_bytes += bytes;
                self.stored += 1;
                Flow::Continue
            }
            Reservation::BudgetReached => {
                self.limit = ReadLimit::Budget;
                Flow::Stop
            }
            Reservation::ValueTooLarge => {
                self.limit = ReadLimit::ValueTooLarge;
                Flow::Stop
            }
        }
    }

    fn take_batch(&mut self) -> Option<ExecEvent> {
        if self.batch.is_empty() {
            return None;
        }
        Some(ExecEvent::Rows {
            key: self.key,
            rows: std::mem::take(&mut self.batch),
            bytes: std::mem::take(&mut self.batch_bytes),
        })
    }

    fn start_result(&mut self, columns: Vec<String>) -> Option<ExecEvent> {
        self.open = true;
        if let Some(capture) = &self.capture {
            capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .columns = columns;
            return None;
        }
        if let Some(export) = &self.export {
            export.start(&columns);
            return None;
        }
        Some(ExecEvent::Columns {
            key: self.key,
            columns,
        })
    }

    /// Ends the open result. A statement can send more than one result, and the
    /// row cap applies to each one.
    fn end_result(&mut self) -> Option<ExecEvent> {
        if !std::mem::take(&mut self.open) {
            return None;
        }
        let rows = std::mem::take(&mut self.stored);
        let limit = std::mem::take(&mut self.limit);
        self.total += rows;
        if self.first_limit == ReadLimit::None {
            self.first_limit = limit;
        }
        (self.export.is_none() && self.capture.is_none()).then_some(ExecEvent::ResultDone {
            key: self.key,
            rows,
            limit,
        })
    }

    pub async fn set_columns(&mut self, columns: Vec<String>) {
        if let Some(event) = self.start_result(columns) {
            let _ = self.events.send(event).await;
        }
    }

    pub async fn push(&mut self, row: Row) -> Flow {
        let flow = self.store(row);
        if self.stream || self.batch.len() >= BATCH_ROWS || flow != Flow::Continue {
            self.flush().await;
        }
        flow
    }

    async fn flush(&mut self) {
        if let Some(event) = self.take_batch()
            && let Err(error) = self.events.send(event).await
            && let ExecEvent::Rows { bytes, .. } = error.0
        {
            self.budget.release(bytes);
        }
    }

    /// Sends the notices of the database. `more` counts the notices that stayed out.
    pub async fn notices(&mut self, texts: Vec<String>, more: usize) {
        let more = (more > 0).then(|| text::more_notices(more));
        for text in texts.into_iter().chain(more) {
            let key = self.key;
            let _ = self.events.send(ExecEvent::Notice { key, text }).await;
        }
    }

    /// Sends the waiting rows and closes the open result.
    pub async fn close_result(&mut self) {
        self.flush().await;
        if let Some(event) = self.end_result() {
            let _ = self.events.send(event).await;
        }
    }

    pub async fn finish(mut self) -> (usize, ReadLimit) {
        self.close_result().await;
        (self.total, self.first_limit)
    }

    /// For a connector that owns a thread. The interface limits the speed of this call.
    pub fn set_columns_blocking(&mut self, columns: Vec<String>) {
        if let Some(event) = self.start_result(columns) {
            let _ = self.events.blocking_send(event);
        }
    }

    pub fn push_blocking(&mut self, row: Row) -> Flow {
        let flow = self.store(row);
        if self.stream || self.batch.len() >= BATCH_ROWS || flow != Flow::Continue {
            self.flush_blocking();
        }
        flow
    }

    fn flush_blocking(&mut self) {
        if let Some(event) = self.take_batch() {
            self.send_blocking(event);
        }
    }

    fn send_blocking(&self, event: ExecEvent) {
        if let Err(error) = self.events.blocking_send(event)
            && let ExecEvent::Rows { bytes, .. } = error.0
        {
            self.budget.release(bytes);
        }
    }

    pub fn finish_blocking(mut self) -> (usize, ReadLimit) {
        self.flush_blocking();
        if let Some(event) = self.end_result() {
            let _ = self.events.blocking_send(event);
        }
        (self.total, self.first_limit)
    }
}
