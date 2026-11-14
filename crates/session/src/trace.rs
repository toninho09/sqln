//! Traces of the database commands. The Messages panel shows them.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::Sender;

use crate::connector::Canceller;
use crate::limited::limited;
use crate::message::MessageLevel;
use crate::sink::{ConnectionId, StatementKey};
use crate::sql::Dialect;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// One trace event for the command history of `target`.
#[derive(Debug)]
pub struct TraceEvent {
    pub target: u64,
    pub event: LogEvent,
}

type Sending<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// The channel of the trace events. A full or closed channel loses the event.
trait Out: Send + Sync {
    fn send(&self, event: TraceEvent) -> Sending<'_>;
    fn send_blocking(&self, event: TraceEvent);
    fn try_send(&self, event: TraceEvent);
}

impl<M: From<TraceEvent> + Send + 'static> Out for Sender<M> {
    fn send(&self, event: TraceEvent) -> Sending<'_> {
        Box::pin(async move {
            let _ = Sender::send(self, M::from(event)).await;
        })
    }

    fn send_blocking(&self, event: TraceEvent) {
        let _ = self.blocking_send(M::from(event));
    }

    fn try_send(&self, event: TraceEvent) {
        let _ = Sender::try_send(self, M::from(event));
    }
}

#[derive(Debug)]
pub enum LogEvent {
    Started {
        id: u64,
        key: Option<StatementKey>,
        user: bool,
        system: bool,
        header_len: usize,
        dialect: Dialect,
        text: String,
    },
    Finished {
        id: u64,
        level: MessageLevel,
        text: String,
        elapsed: Duration,
    },
}

/// The destination is fixed before an operation starts. Clones keep that destination.
#[derive(Clone)]
pub struct TraceContext {
    pub target: u64,
    pub connection: ConnectionId,
    pub name: String,
    pub shared: bool,
    pub key: Option<StatementKey>,
    pub user: bool,
    pub dialect: Dialect,
    events: Arc<dyn Out>,
}

impl TraceContext {
    pub fn new<M: From<TraceEvent> + Send + 'static>(
        target: u64,
        connection: ConnectionId,
        name: String,
        shared: bool,
        events: Sender<M>,
    ) -> Self {
        Self {
            target,
            connection,
            name,
            shared,
            key: None,
            user: false,
            dialect: Dialect::Postgres,
            events: Arc::new(events),
        }
    }

    fn started(&self, command: impl fmt::Display) -> (u64, TraceEvent) {
        let id = next_id();
        let origin = if self.shared {
            "Shared"
        } else if self.user {
            "User"
        } else {
            "Internal"
        };
        let header = limited(format_args!("Command {id} · {} · {origin}\n", self.name));
        let text = limited(format_args!("{header}{command}"));
        let header_len = header.len().min(text.len());
        (
            id,
            TraceEvent {
                target: self.target,
                event: LogEvent::Started {
                    id,
                    key: self.key,
                    user: self.user,
                    system: self.shared || !self.user,
                    header_len,
                    dialect: self.dialect,
                    text,
                },
            },
        )
    }

    pub fn start_blocking(&self, command: impl fmt::Display) -> CommandSpan {
        let (id, event) = self.started(command);
        self.events.send_blocking(event);
        CommandSpan {
            id,
            trace: self.clone(),
            started: Instant::now(),
            finished: false,
        }
    }

    pub fn internal(&self) -> Self {
        let mut trace = self.clone();
        trace.user = false;
        trace
    }

    pub fn start<C: fmt::Display>(
        &self,
        command: C,
    ) -> impl Future<Output = CommandSpan> + Send + use<C> {
        let (id, event) = self.started(command);
        let trace = self.clone();
        async move {
            trace.events.send(event).await;
            CommandSpan {
                id,
                trace,
                started: Instant::now(),
                finished: false,
            }
        }
    }
}

/// A dropped database future leaves an explicit unknown result, not a false success.
pub struct CommandSpan {
    id: u64,
    trace: TraceContext,
    started: Instant,
    finished: bool,
}

impl CommandSpan {
    fn result_event<T, E: fmt::Display>(&mut self, result: &Result<T, E>) -> TraceEvent {
        let elapsed = self.started.elapsed();
        self.finished = true;
        let (level, text) = match result {
            Ok(_) => (MessageLevel::Info, "Completed.".to_owned()),
            Err(error) => (MessageLevel::Error, limited(format_args!("Error: {error}"))),
        };
        TraceEvent {
            target: self.trace.target,
            event: LogEvent::Finished {
                id: self.id,
                level,
                text,
                elapsed,
            },
        }
    }

    fn finish_blocking<T, E: fmt::Display>(mut self, result: &Result<T, E>) {
        let event = self.result_event(result);
        self.trace.events.send_blocking(event);
    }

    pub fn finish<T, E: fmt::Display>(
        mut self,
        result: &Result<T, E>,
    ) -> impl Future<Output = ()> + Send + use<T, E> {
        let event = self.result_event(result);
        async move {
            self.trace.events.send(event).await;
        }
    }
}

impl Drop for CommandSpan {
    fn drop(&mut self) {
        if !self.finished {
            // Never block the runtime while it drops a canceled future. An unfinished
            // entry already says that the result is unknown.
            self.trace.events.try_send(TraceEvent {
                target: self.trace.target,
                event: LogEvent::Finished {
                    id: self.id,
                    level: MessageLevel::Info,
                    text: "Interrupted. Result unknown.".to_owned(),
                    elapsed: self.started.elapsed(),
                },
            });
        }
    }
}

pub fn start<C: fmt::Display>(
    trace: &Option<TraceContext>,
    command: C,
) -> impl Future<Output = Option<CommandSpan>> + Send + use<C> {
    let pending = trace.as_ref().map(|trace| trace.start(command));
    async move {
        match pending {
            Some(pending) => Some(pending.await),
            None => None,
        }
    }
}

pub fn finish<T, E: fmt::Display>(
    span: Option<CommandSpan>,
    result: &Result<T, E>,
) -> impl Future<Output = ()> + Send + use<T, E> {
    let pending = span.map(|span| span.finish(result));
    async move {
        if let Some(pending) = pending {
            pending.await;
        }
    }
}

pub fn observe<T, E: fmt::Display, F: Future<Output = Result<T, E>>>(
    trace: Option<TraceContext>,
    command: impl fmt::Display,
    future: F,
) -> impl Future<Output = Result<T, E>> {
    let pending = start(&trace, command);
    async move {
        let span = pending.await;
        let result = future.await;
        finish(span, &result).await;
        result
    }
}

pub fn observe_blocking<T, E: fmt::Display>(
    trace: &Option<TraceContext>,
    command: impl fmt::Display,
    run: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let span = trace.as_ref().map(|trace| trace.start_blocking(command));
    let result = run();
    if let Some(span) = span {
        span.finish_blocking(&result);
    }
    result
}

struct TracedCanceller {
    inner: Arc<dyn Canceller>,
    trace: Option<TraceContext>,
}

#[async_trait::async_trait]
impl Canceller for TracedCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.inner.cancel_traced(self.trace.clone()).await
    }
}

pub fn canceller(inner: Arc<dyn Canceller>, trace: &Option<TraceContext>) -> Arc<dyn Canceller> {
    if trace.is_none() {
        return inner;
    }
    Arc::new(TracedCanceller {
        inner,
        trace: trace.as_ref().map(TraceContext::internal),
    })
}
