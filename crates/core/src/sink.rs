//! Receives rows from a connector and sends batches to the interface.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::grid::SavedRow;

pub type ConnectionId = u64;
pub type ConsoleId = u64;
pub type ExecutionId = u64;

/// Identifies a statement. The interface ignores events of an old execution.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct StatementKey {
    pub console: ConsoleId,
    pub execution: ExecutionId,
    pub index: usize,
}

#[derive(Debug, Default)]
pub struct Captured {
    pub columns: Vec<String>,
    pub rows: Vec<Arc<SavedRow>>,
    pub incomplete: bool,
    pub limit: ReadLimit,
    pub changed: Option<u64>,
}

pub type Capture = Arc<Mutex<Captured>>;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::grid::{Budget, Reservation, Row};
use crate::progress::ReadProgress;
use crate::text;

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

/// What a sink sends to the reader of the results, in the order of the read.
#[derive(Debug)]
pub enum SinkEvent {
    Columns {
        key: StatementKey,
        columns: Vec<String>,
        /// The database type of each column, such as `int4`. Empty when the connector
        /// gives no types; `None` for a column without a known type.
        types: Vec<Option<String>>,
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
    Notice { key: StatementKey, text: String },
}

/// The message of a channel that receives the events of a sink.
pub trait SinkMessage: From<SinkEvent> + Send + 'static {
    /// Returns the reserved bytes of rows that the channel did not deliver.
    fn unsent_bytes(&self) -> usize;
}

impl SinkMessage for SinkEvent {
    fn unsent_bytes(&self) -> usize {
        match self {
            SinkEvent::Rows { bytes, .. } => *bytes,
            _ => 0,
        }
    }
}

/// A file that receives the rows of a result in place of the grid.
pub trait RowOutput: Send {
    fn start_rows(&self, columns: &[String]);
    /// Writes one row. A false value tells the reader to stop.
    fn write(&self, row: &Row) -> bool;
}

type Sending<'a> = Pin<Box<dyn Future<Output = usize> + Send + 'a>>;

/// The channel of a sink. Each call returns the reserved bytes that it did not deliver.
trait Out: Send + Sync {
    fn send(&self, event: SinkEvent) -> Sending<'_>;
    fn send_blocking(&self, event: SinkEvent) -> usize;
}

impl<M: SinkMessage> Out for Sender<M> {
    fn send(&self, event: SinkEvent) -> Sending<'_> {
        Box::pin(async move {
            match Sender::send(self, M::from(event)).await {
                Ok(()) => 0,
                Err(error) => error.0.unsent_bytes(),
            }
        })
    }

    fn send_blocking(&self, event: SinkEvent) -> usize {
        match self.blocking_send(M::from(event)) {
            Ok(()) => 0,
            Err(error) => error.0.unsent_bytes(),
        }
    }
}

pub struct ResultSink {
    key: StatementKey,
    events: Box<dyn Out>,
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
    stopping: CancellationToken,
    /// The rows go to this file, and the grid receives nothing.
    export: Option<Box<dyn RowOutput>>,
    stream: bool,
    /// The row cap of a live result. The grid drops its oldest rows at the cap, so the
    /// sink sends every row and reserves no budget.
    live: Option<usize>,
    capture: Option<Capture>,
    /// The counts that the connector updates while it reads.
    progress: Option<Arc<ReadProgress>>,
}

/// The rows of a batch that nobody received leave the budget.
impl Drop for ResultSink {
    fn drop(&mut self) {
        self.budget.release(self.batch_bytes);
    }
}

impl ResultSink {
    pub fn new<M: SinkMessage>(
        key: StatementKey,
        events: Sender<M>,
        budget: Arc<Budget>,
        cap: usize,
    ) -> Self {
        Self {
            key,
            events: Box::new(events),
            budget,
            cap,
            stored: 0,
            limit: ReadLimit::None,
            batch: Vec::new(),
            batch_bytes: 0,
            open: false,
            total: 0,
            first_limit: ReadLimit::None,
            stopping: CancellationToken::new(),
            export: None,
            stream: false,
            live: None,
            capture: None,
            progress: None,
        }
    }

    pub fn stream(&mut self, stream: bool) {
        self.stream = stream;
    }

    /// Starts a result that grows until the user stops it.
    pub fn live(&mut self) {
        self.live = Some(self.cap);
    }

    pub fn capture_to(&mut self, capture: Capture) {
        self.capture = Some(capture);
    }

    /// Sends the counts of the read to the screen with the columns of the next result.
    /// Call it before `set_columns`.
    pub fn track(&mut self, progress: Arc<ReadProgress>) {
        self.progress = Some(progress);
    }

    /// Sends the rows to a file. They skip the row cap and the budget of the grid.
    pub fn export_to(&mut self, writer: impl RowOutput + 'static) {
        self.export = Some(Box::new(writer));
    }

    /// The configured ceiling for temporary connector buffers, including streaming output.
    pub fn memory_limit(&self) -> usize {
        self.budget.limit()
    }

    /// Returns the token that ends the storage of rows.
    pub fn stop_token(&self) -> CancellationToken {
        self.stopping.clone()
    }

    fn store(&mut self, row: Row) -> Flow {
        if self.stopping.is_cancelled() {
            return Flow::Discard;
        }
        if self.live.is_some() {
            self.batch.push(row);
            self.stored += 1;
            return Flow::Continue;
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
            if !export.write(&row) {
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

    fn take_batch(&mut self) -> Option<SinkEvent> {
        if self.batch.is_empty() {
            return None;
        }
        Some(SinkEvent::Rows {
            key: self.key,
            rows: std::mem::take(&mut self.batch),
            bytes: std::mem::take(&mut self.batch_bytes),
        })
    }

    fn start_result(
        &mut self,
        columns: Vec<String>,
        types: Vec<Option<String>>,
    ) -> Option<SinkEvent> {
        self.open = true;
        if let Some(capture) = &self.capture {
            capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .columns = columns;
            return None;
        }
        if let Some(export) = &self.export {
            export.start_rows(&columns);
            return None;
        }
        Some(SinkEvent::Columns {
            key: self.key,
            columns,
            types,
            live: self.live,
            progress: self.progress.clone(),
        })
    }

    /// Ends the open result. A statement can send more than one result, and the
    /// row cap applies to each one.
    fn end_result(&mut self) -> Option<SinkEvent> {
        if !std::mem::take(&mut self.open) {
            return None;
        }
        let rows = std::mem::take(&mut self.stored);
        let limit = std::mem::take(&mut self.limit);
        self.total += rows;
        if self.first_limit == ReadLimit::None {
            self.first_limit = limit;
        }
        (self.export.is_none() && self.capture.is_none()).then_some(SinkEvent::ResultDone {
            key: self.key,
            rows,
            limit,
        })
    }

    pub async fn set_columns(&mut self, columns: Vec<String>) {
        self.set_typed_columns(columns, Vec::new()).await;
    }

    /// Starts a result with the database type of each column. `types` has one item for
    /// each column.
    pub async fn set_typed_columns(&mut self, columns: Vec<String>, types: Vec<Option<String>>) {
        if let Some(event) = self.start_result(columns, types) {
            self.events.send(event).await;
        }
    }

    pub async fn push(&mut self, row: Row) -> Flow {
        let flow = self.store(row);
        if self.stream || self.batch.len() >= BATCH_ROWS || flow != Flow::Continue {
            self.flush().await;
        }
        flow
    }

    /// Sends the waiting rows. A continuous statement calls it before it waits.
    pub async fn flush(&mut self) {
        if let Some(event) = self.take_batch() {
            let unsent = self.events.send(event).await;
            self.release(unsent);
        }
    }

    /// Sends the notices of the database. `more` counts the notices that stayed out.
    pub async fn notices(&mut self, texts: Vec<String>, more: usize) {
        let more = (more > 0).then(|| text::more_notices(more));
        for text in texts.into_iter().chain(more) {
            let key = self.key;
            self.events.send(SinkEvent::Notice { key, text }).await;
        }
    }

    /// Sends the waiting rows and closes the open result.
    pub async fn close_result(&mut self) {
        self.flush().await;
        if let Some(event) = self.end_result() {
            self.events.send(event).await;
        }
    }

    pub async fn finish(mut self) -> (usize, ReadLimit) {
        self.close_result().await;
        (self.total, self.first_limit)
    }

    /// For a connector that owns a thread. The interface limits the speed of this call.
    pub fn set_columns_blocking(&mut self, columns: Vec<String>) {
        self.set_typed_columns_blocking(columns, Vec::new());
    }

    /// Starts a result with the database type of each column, for a connector that owns
    /// a thread. `types` has one item for each column.
    pub fn set_typed_columns_blocking(&mut self, columns: Vec<String>, types: Vec<Option<String>>) {
        if let Some(event) = self.start_result(columns, types) {
            self.events.send_blocking(event);
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

    fn send_blocking(&self, event: SinkEvent) {
        let unsent = self.events.send_blocking(event);
        self.release(unsent);
    }

    /// Returns to the budget the rows that nobody received.
    fn release(&self, unsent: usize) {
        if unsent > 0 {
            self.budget.release(unsent);
        }
    }

    pub fn finish_blocking(mut self) -> (usize, ReadLimit) {
        self.flush_blocking();
        if let Some(event) = self.end_result() {
            self.events.send_blocking(event);
        }
        (self.total, self.first_limit)
    }
}
