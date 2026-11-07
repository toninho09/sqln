use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::FutureExt;
use sqln::catalog::Catalog;
use sqln::connector::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState,
};
use sqln::exec::{ConnectionId, ConsoleId, ExecEvent, Manager, ResultSink, RunOptions, Settings};
use sqln::grid::Budget;
use sqln::pool::{Lease, Pool};
use sqln::sql::Dialect;
use tokio::sync::mpsc::Receiver;
use tokio::time::{sleep, timeout};

const SHORT: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq)]
enum Behavior {
    Works,
    Refuses,
    Hangs,
}

struct Fake {
    behavior: Mutex<Behavior>,
    opened: AtomicUsize,
    reset: ResetOutcome,
}

impl Fake {
    fn new(behavior: Behavior, reset: ResetOutcome) -> Arc<Self> {
        Arc::new(Self {
            behavior: Mutex::new(behavior),
            opened: AtomicUsize::new(0),
            reset,
        })
    }
}

#[async_trait]
impl Connector for Fake {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let behavior = *self.behavior.lock().unwrap();
        match behavior {
            Behavior::Works => {
                self.opened.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(FakeSession {
                    reset: self.reset,
                    transaction: TransactionState::None,
                }))
            }
            Behavior::Refuses => Err(ConnectError::new("refused".to_owned())),
            Behavior::Hangs => std::future::pending().await,
        }
    }
}

struct FakeSession {
    reset: ResetOutcome,
    transaction: TransactionState,
}

struct NoCancel;

#[async_trait]
impl Canceller for NoCancel {
    async fn cancel(&self) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl Session for FakeSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(NoCancel)
    }
    async fn apply_context(&mut self, _: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }
    async fn run(&mut self, _: &str, _: ResultSink) -> Result<StatementEnd, ExecError> {
        Ok(StatementEnd::default())
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
        self.reset
    }
    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        Ok(Catalog::default())
    }
    async fn read_columns(&mut self, _: &str, _: &str) -> Result<Vec<String>, ExecError> {
        Ok(Vec::new())
    }
}

fn pool(fake: &Arc<Fake>, limit: usize) -> Pool {
    Pool::new(fake.clone(), limit, Duration::from_millis(100))
}

async fn take(pool: &Pool) -> Lease {
    match timeout(Duration::from_secs(5), pool.acquire()).await {
        Ok(Ok(lease)) => lease,
        Ok(Err(error)) => panic!("{error:?}"),
        Err(_) => panic!("no place arrived"),
    }
}

#[tokio::test]
async fn a_canceled_wait_returns_the_session_that_reached_it() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let lease = take(&pool).await;
    let mut wait = Box::pin(pool.acquire());
    assert!((&mut wait).now_or_never().is_none(), "the place is in use");
    // The release gives the session to the waiter, and the waiter stops before it reads it.
    lease.release().await;
    drop(wait);
    assert_eq!(pool.usage(), (0, 1));
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_canceled_wait_closes_a_session_of_an_old_generation() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let lease = take(&pool).await;
    let mut wait = Box::pin(pool.acquire());
    assert!((&mut wait).now_or_never().is_none(), "the place is in use");
    lease.release().await;
    pool.close_idle();
    drop(wait);
    assert_eq!(pool.usage(), (0, 1));
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_free_session_returns_for_the_next_use() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    take(&pool).await.release().await;
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 1);
    assert_eq!(pool.usage(), (0, 2));
}

#[tokio::test]
async fn a_failed_cleanup_discards_the_session() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Discard);
    let pool = pool(&fake, 2);
    take(&pool).await.release().await;
    assert_eq!(pool.usage(), (0, 2));
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn the_limit_holds_and_the_next_request_waits() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    let first = take(&pool).await;
    let _second = take(&pool).await;
    assert_eq!(pool.usage(), (2, 2));
    assert!(timeout(SHORT, pool.acquire()).await.is_err());
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);

    let waiting = tokio::spawn({
        let pool = pool.clone();
        async move { take(&pool).await.release().await }
    });
    sleep(SHORT).await;
    first.release().await;
    timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fake.opened.load(Ordering::SeqCst),
        2,
        "the waiter uses the free session"
    );
}

#[tokio::test]
async fn requests_receive_places_in_order_of_arrival() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let first = take(&pool).await;
    let order = Arc::new(Mutex::new(Vec::new()));

    let mut tasks = Vec::new();
    for index in 0..5 {
        let (pool, order) = (pool.clone(), order.clone());
        tasks.push(tokio::spawn(async move {
            let lease = take(&pool).await;
            order.lock().unwrap().push(index);
            sleep(Duration::from_millis(5)).await;
            lease.release().await;
        }));
        sleep(Duration::from_millis(10)).await;
    }
    first.release().await;
    for task in tasks {
        timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(*order.lock().unwrap(), [0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn a_canceled_wait_leaves_the_queue_and_keeps_the_order() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let first = take(&pool).await;

    let canceled = tokio::spawn({
        let pool = pool.clone();
        async move { pool.acquire().await.map(|_| ()) }
    });
    sleep(SHORT).await;
    let waiting = tokio::spawn({
        let pool = pool.clone();
        async move { take(&pool).await.release().await }
    });
    sleep(SHORT).await;
    canceled.abort();
    sleep(SHORT).await;

    first.release().await;
    timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pool.usage(), (0, 1));
}

#[tokio::test]
async fn a_new_request_does_not_pass_the_queue() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Discard);
    let pool = pool(&fake, 1);
    let first = take(&pool).await;
    let waiting = tokio::spawn({
        let pool = pool.clone();
        async move {
            let lease = take(&pool).await;
            sleep(SHORT).await;
            lease.release().await;
        }
    });
    sleep(SHORT).await;
    first.release().await;
    assert_eq!(pool.usage(), (1, 1), "the waiter holds the place at once");
    assert!(
        timeout(Duration::from_millis(10), pool.acquire())
            .await
            .is_err()
    );
    timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_refused_connection_is_a_failure_and_frees_the_place() {
    let fake = Fake::new(Behavior::Refuses, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let error = timeout(Duration::from_secs(5), pool.acquire())
        .await
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.message, "refused");
    assert_eq!(pool.usage(), (0, 1));
}

#[tokio::test]
async fn a_silent_server_ends_at_the_connection_time_limit() {
    let fake = Fake::new(Behavior::Hangs, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let error = timeout(Duration::from_secs(5), pool.acquire())
        .await
        .unwrap()
        .err()
        .unwrap();
    assert!(error.message.contains("time limit"), "{error:?}");
    assert_eq!(pool.usage(), (0, 1));
}

#[tokio::test]
async fn a_canceled_connection_frees_the_place() {
    let fake = Fake::new(Behavior::Hangs, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    assert!(
        timeout(Duration::from_millis(10), pool.acquire())
            .await
            .is_err()
    );
    assert_eq!(pool.usage(), (0, 1));
}

#[tokio::test]
async fn a_dropped_lease_frees_the_place_and_wakes_the_queue() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let first = take(&pool).await;
    let waiting = tokio::spawn({
        let pool = pool.clone();
        async move { take(&pool).await.release().await }
    });
    sleep(SHORT).await;
    drop(first);
    timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_waiter_opens_a_new_session_after_a_failed_opening() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let first = take(&pool).await;
    let waiting = tokio::spawn({
        let pool = pool.clone();
        async move { pool.acquire().await.map(|_| ()) }
    });
    sleep(SHORT).await;
    *fake.behavior.lock().unwrap() = Behavior::Refuses;
    drop(first);
    let result = timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(
        result.is_err(),
        "the failure goes to the request, without a retry"
    );
    assert_eq!(pool.usage(), (0, 1));
}

#[tokio::test]
async fn an_idle_session_closes_at_the_idle_limit_and_a_session_in_use_stays() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    let in_use = take(&pool).await;
    take(&pool).await.release().await;
    let (closed, next) = pool.close_expired(SHORT);
    assert_eq!(closed, 0, "the session is younger than the limit");
    assert!(next > Duration::ZERO && next <= SHORT, "{next:?}");
    sleep(SHORT * 2).await;
    assert_eq!(pool.close_expired(SHORT), (1, SHORT));
    assert_eq!(pool.usage(), (1, 2));
    in_use.release().await;
    assert_eq!(pool.close_expired(SHORT).0, 0);
    assert!(pool.has_sessions());
}

#[tokio::test]
async fn the_older_idle_session_closes_and_the_newer_one_stays() {
    let limit = Duration::from_millis(200);
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    let (first, second) = (take(&pool).await, take(&pool).await);
    first.release().await;
    sleep(Duration::from_millis(120)).await;
    second.release().await;
    sleep(Duration::from_millis(120)).await;
    let (closed, next) = pool.close_expired(limit);
    assert_eq!(closed, 1);
    assert!(next < Duration::from_millis(100), "{next:?}");
    // The newer session serves the next use.
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn close_idle_ends_the_idle_sessions_and_a_later_use_opens_one() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    take(&pool).await.release().await;
    assert!(pool.has_sessions());
    pool.close_idle();
    assert!(!pool.has_sessions());
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_session_in_use_during_close_idle_closes_when_it_returns() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    let lease = take(&pool).await;
    pool.close_idle();
    assert!(
        pool.has_sessions(),
        "the session in use stays until its end"
    );
    lease.release().await;
    assert!(!pool.has_sessions());
    assert_eq!(pool.usage().0, 0);
}

#[tokio::test]
async fn a_session_that_starts_after_close_idle_returns_to_the_pool() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 2);
    pool.close_idle();
    take(&pool).await.release().await;
    assert!(pool.has_sessions());
}

#[tokio::test]
async fn close_idle_gives_the_free_place_to_a_waiter() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    take(&pool).await.release().await;
    pool.close_idle();
    assert_eq!(pool.usage().0, 0);
    take(&pool).await.release().await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn close_idle_between_the_grant_and_its_read_closes_the_session() {
    let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
    let pool = pool(&fake, 1);
    let lease = take(&pool).await;
    let waiting = {
        let pool = pool.clone();
        tokio::spawn(async move { take(&pool).await })
    };
    // The waiter enters the queue before the place arrives.
    tokio::task::yield_now().await;
    lease.release().await;
    // The grant is on its way to the waiter, and this close changes the generation.
    pool.close_idle();
    let lease = waiting.await.unwrap();
    lease.release().await;
    assert!(
        !pool.has_sessions(),
        "the session of the old generation closes"
    );
    assert_eq!(pool.usage().0, 0);
}

/// A manager with the fake source. The manager owns a runtime, so these tests are not async.
struct Watched {
    manager: Manager,
    events: Receiver<ExecEvent>,
    connection: ConnectionId,
    console: ConsoleId,
}

impl Watched {
    fn open(idle_timeout: Duration) -> Self {
        let fake = Fake::new(Behavior::Works, ResetOutcome::Clean);
        let settings = Settings {
            idle_timeout: Some(idle_timeout),
            ..Settings::default()
        };
        let (mut manager, events) = Manager::new(Arc::new(Budget::new(1 << 20))).unwrap();
        let connection = manager.add_connection(fake, settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            connection,
            console,
        }
    }

    /// Waits for an event of the kind, and returns false at the deadline.
    fn wait(&mut self, wanted: fn(&ExecEvent) -> bool, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Ok(event) = self.events.try_recv()
                && wanted(&event)
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    fn run(&mut self, manual: bool) {
        let options = RunOptions {
            manual,
            ..RunOptions::default()
        };
        self.manager
            .run(self.console, vec!["SELECT 1".to_owned()], options);
        let finished = |event: &ExecEvent| matches!(event, ExecEvent::ExecutionFinished { .. });
        assert!(
            self.wait(finished, Duration::from_secs(5)),
            "the run did not end"
        );
    }
}

fn idle_closed(event: &ExecEvent) -> bool {
    matches!(event, ExecEvent::IdleClosed { .. })
}

#[test]
fn the_manager_closes_an_idle_session_at_the_idle_limit_and_the_next_run_opens_one() {
    let mut w = Watched::open(Duration::from_millis(100));
    w.run(false);
    assert!(w.manager.is_connected(w.connection));
    assert!(w.wait(idle_closed, Duration::from_secs(5)));
    assert!(!w.manager.is_connected(w.connection));
    w.run(false);
    assert!(w.manager.is_connected(w.connection));
    w.manager.shutdown();
}

#[test]
fn an_open_transaction_keeps_its_session_after_the_idle_limit() {
    let limit = Duration::from_millis(100);
    let mut w = Watched::open(limit);
    w.run(true);
    assert!(
        !w.wait(idle_closed, limit * 4),
        "the transaction keeps its session"
    );
    assert!(w.manager.is_connected(w.connection));
    // The end of the transaction returns the session, and the idle limit applies.
    w.manager
        .transaction(w.console, TransactionCommand::Rollback);
    assert!(w.wait(idle_closed, Duration::from_secs(5)));
    assert!(!w.manager.is_connected(w.connection));
    w.manager.shutdown();
}

#[tokio::test]
async fn a_single_session_source_holds_one_place_and_no_idle_close() {
    let connector = sqln::connector::sqlite::SqliteConnector::new(
        sqln::connector::sqlite::SqliteTarget::in_memory(),
    );
    let pool = Pool::new(
        std::sync::Arc::new(connector),
        5,
        Duration::from_millis(100),
    );
    assert_eq!(pool.usage(), (0, 1));
    let lease = take(&pool).await;
    lease.release().await;
    // The idle session of an in-memory database never closes, or the data is lost.
    let (closed, _) = pool.close_expired(Duration::ZERO);
    assert_eq!(closed, 0);
    assert!(pool.has_sessions());
}
