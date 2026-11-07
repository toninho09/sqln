//! A fake connector for the tests that need no database. One handler chooses the
//! answer of each statement, and the session keeps the usual transaction states.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use sqln::catalog::Catalog;
use sqln::connector::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState,
};
use sqln::exec::ResultSink;
use sqln::sql::Dialect;

/// A canceller that never answers: the default of the fake sessions.
pub struct NoCancel;

#[async_trait]
impl Canceller for NoCancel {
    async fn cancel(&self) -> Result<(), String> {
        Ok(())
    }
}

/// The answer of one statement. The handler receives the text and the sink.
pub type RunHandler = Arc<
    dyn Fn(String, ResultSink) -> BoxFuture<'static, Result<StatementEnd, ExecError>> + Send + Sync,
>;

/// What a connection attempt does.
#[derive(Clone)]
pub enum Connect {
    Works,
    /// The attempt fails with this reason.
    Refuses(String),
    /// The attempt never ends.
    Hangs,
}

struct Inner {
    dialect: Dialect,
    connect: Mutex<Connect>,
    run: RunHandler,
    reset: Mutex<ResetOutcome>,
    opened: AtomicUsize,
    /// The statements that reached the sessions, in order.
    statements: Mutex<Vec<String>>,
}

/// A connector whose sessions answer what a handler chooses.
#[derive(Clone)]
pub struct MockConnector(Arc<Inner>);

impl MockConnector {
    pub fn new(dialect: Dialect, run: RunHandler) -> Self {
        Self(Arc::new(Inner {
            dialect,
            connect: Mutex::new(Connect::Works),
            run,
            reset: Mutex::new(ResetOutcome::Clean),
            opened: AtomicUsize::new(0),
            statements: Mutex::new(Vec::new()),
        }))
    }

    /// Sets what a connection attempt does. A test may change it while it runs.
    pub fn when_connecting(&self, connect: Connect) {
        *self.0.connect.lock().unwrap() = connect;
    }

    /// Sets the answer of a reset.
    pub fn on_reset(&self, reset: ResetOutcome) {
        *self.0.reset.lock().unwrap() = reset;
    }

    /// The count of the connections that opened.
    pub fn opened(&self) -> usize {
        self.0.opened.load(Ordering::SeqCst)
    }

    /// The statements that reached the sessions, in order.
    pub fn statements(&self) -> Vec<String> {
        self.0.statements.lock().unwrap().clone()
    }
}

#[async_trait]
impl Connector for MockConnector {
    fn dialect(&self) -> Dialect {
        self.0.dialect
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let connect = self.0.connect.lock().unwrap().clone();
        match connect {
            Connect::Hangs => return std::future::pending().await,
            Connect::Refuses(reason) => return Err(ConnectError::new(reason)),
            Connect::Works => {}
        }
        self.0.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(MockSession {
            inner: Arc::clone(&self.0),
            transaction: TransactionState::None,
        }))
    }
}

struct MockSession {
    inner: Arc<Inner>,
    transaction: TransactionState,
}

#[async_trait]
impl Session for MockSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(NoCancel)
    }

    async fn apply_context(&mut self, _: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }

    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.inner.statements.lock().unwrap().push(sql.to_owned());
        (self.inner.run)(sql.to_owned(), sink).await
    }

    async fn transaction_state(&mut self) -> TransactionState {
        self.transaction
    }

    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError> {
        self.transaction = match command {
            TransactionCommand::Begin => TransactionState::Open,
            TransactionCommand::Commit | TransactionCommand::Rollback => TransactionState::None,
        };
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        *self.inner.reset.lock().unwrap()
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        Ok(Catalog::default())
    }

    async fn read_columns(&mut self, _: &str, _: &str) -> Result<Vec<String>, ExecError> {
        Ok(Vec::new())
    }
}
