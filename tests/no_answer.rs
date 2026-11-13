// A database that does not answer a cancellation. SQLite always answers, so these
// tests use a fake connector for the last-resort path of the plan.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{ctrl, key};
use async_trait::async_trait;
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::Catalog;
use sqln::clipboard::MemoryClipboard;
use sqln::connector::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState,
};
use sqln::exec::{
    ExecEvent, ExecutionEnd, Flow, Manager, ResultSink, RunOptions, Settings, StatementStatus,
};
use sqln::grid::Budget;
use sqln::sql::Dialect;
use sqln::ui::theme::Theme;
use tokio::sync::Notify;
use tokio::sync::mpsc::Receiver;

const GRACE: Duration = Duration::from_millis(50);

/// The shared behavior of the fake sessions.
struct Fake {
    hang: AtomicBool,
    answer_cancel: AtomicBool,
    success_after_cancel: AtomicBool,
    refuse_cancel: AtomicBool,
    hang_cancel: AtomicBool,
    dead: AtomicBool,
    stream_rows: AtomicBool,
    release: Notify,
    cancels: AtomicUsize,
    opened: AtomicUsize,
    transaction: Mutex<TransactionState>,
    statements: Mutex<Vec<String>>,
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            hang: AtomicBool::new(true),
            answer_cancel: AtomicBool::new(false),
            success_after_cancel: AtomicBool::new(false),
            refuse_cancel: AtomicBool::new(false),
            hang_cancel: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            stream_rows: AtomicBool::new(false),
            release: Notify::new(),
            cancels: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
            transaction: Mutex::new(TransactionState::None),
            statements: Mutex::new(Vec::new()),
        })
    }

    fn set_transaction(&self, state: TransactionState) {
        *self.transaction.lock().unwrap() = state;
    }
}

#[async_trait]
impl Connector for Fake {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeSession(Arc::clone(
            &SHARED.lock().unwrap().clone().unwrap(),
        ))))
    }
}

/// Each test thread runs alone in this file, because the fake connector reads this value.
static SHARED: Mutex<Option<Arc<Fake>>> = Mutex::new(None);
static SERIAL: Mutex<()> = Mutex::new(());

struct FakeSession(Arc<Fake>);

struct FakeCanceller(Arc<Fake>);

#[async_trait]
impl Canceller for FakeCanceller {
    async fn cancel(&self) -> Result<(), String> {
        self.0.cancels.fetch_add(1, Ordering::SeqCst);
        if self.0.answer_cancel.load(Ordering::SeqCst) {
            self.0.release.notify_one();
        }
        if self.0.refuse_cancel.load(Ordering::SeqCst) {
            return Err("no connection".to_owned());
        }
        if self.0.hang_cancel.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

#[async_trait]
impl Session for FakeSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(FakeCanceller(self.0.clone()))
    }

    async fn apply_context(&mut self, _: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.0.statements.lock().unwrap().push(sql.to_owned());
        if self.0.stream_rows.load(Ordering::SeqCst) {
            sink.set_columns(vec!["a".to_owned()]).await;
            let row = || -> sqln::grid::Row { [sqln::grid::Value::Null].into_iter().collect() };
            while sink.push(row()).await != Flow::Discard {
                tokio::task::yield_now().await;
            }
            return Err(ExecError::new("canceled"));
        }
        if self.0.hang.load(Ordering::SeqCst) {
            self.0.release.notified().await;
            if !self.0.success_after_cancel.load(Ordering::SeqCst) {
                return Err(ExecError::new("canceled"));
            }
        }
        Ok(StatementEnd::default())
    }

    async fn transaction_state(&mut self) -> TransactionState {
        *self.0.transaction.lock().unwrap()
    }

    async fn control(&mut self, _: TransactionCommand) -> Result<(), ExecError> {
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        ResetOutcome::Clean
    }

    async fn is_alive(&mut self) -> bool {
        !self.0.dead.load(Ordering::SeqCst)
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        Ok(Catalog::default())
    }

    async fn read_columns(&mut self, _: &str, _: &str) -> Result<Vec<String>, ExecError> {
        Ok(Vec::new())
    }
}

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    connection: u64,
    console: u64,
    fake: Arc<Fake>,
}

fn settings() -> Settings {
    Settings {
        stop_grace: GRACE,
        sql_timeout: None,
        ..Settings::default()
    }
}

fn harness(fake: &Arc<Fake>) -> Harness {
    *SHARED.lock().unwrap() = Some(fake.clone());
    let budget = Arc::new(Budget::new(1 << 20));
    let (mut manager, events) = Manager::new(budget).unwrap();
    let connection = manager.add_connection(fake.clone(), settings());
    let console = manager.open_console(connection).unwrap();
    Harness {
        manager,
        events,
        connection,
        console,
        fake: fake.clone(),
    }
}

impl Harness {
    fn next(&mut self) -> ExecEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(event) = self.events.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "no event arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn start(&self, statements: &[&str]) -> u64 {
        let statements = statements.iter().map(|sql| (*sql).to_owned()).collect();
        self.manager
            .run(self.console, statements, RunOptions::default())
    }

    /// Waits until the session receives the statement.
    fn wait_running(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.fake.statements.lock().unwrap().len() < count {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn finish(
        &mut self,
    ) -> (
        Vec<StatementStatus>,
        ExecutionEnd,
        Vec<TransactionState>,
        bool,
    ) {
        let mut statuses = Vec::new();
        let mut states = Vec::new();
        let mut asked = false;
        loop {
            match self.next() {
                ExecEvent::StatementFinished { status, .. } => statuses.push(status),
                ExecEvent::TransactionChanged { state, .. } => states.push(state),
                ExecEvent::ConfirmClose { .. } => asked = true,
                ExecEvent::ExecutionFinished { end, .. } => return (statuses, end, states, asked),
                _ => {}
            }
        }
    }
}

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn without_a_transaction_the_connection_closes_after_the_grace_time() {
    let _serial = serial();
    let fake = Fake::new();
    let mut h = harness(&fake);
    h.start(&["SELECT slow", "SELECT next"]);
    h.wait_running(1);

    let started = Instant::now();
    h.manager.stop(h.console);
    let (statuses, end, states, asked) = h.finish();

    assert!(started.elapsed() >= GRACE, "the session had time to answer");
    assert_eq!(
        statuses,
        [StatementStatus::Unknown, StatementStatus::NotExecuted]
    );
    assert_eq!(end, ExecutionEnd::ConnectionClosed);
    assert!(!asked, "no transaction, so no question");
    assert!(
        states.is_empty(),
        "no transaction, so the state stays the same"
    );
    assert_eq!(fake.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(
        fake.statements.lock().unwrap().len(),
        1,
        "the next statement was not sent"
    );
    assert_eq!(
        h.manager.usage(h.connection).0,
        0,
        "the closed session frees its place"
    );

    fake.hang.store(false, Ordering::SeqCst);
    h.start(&["SELECT 1"]);
    h.finish();
    assert_eq!(
        fake.opened.load(Ordering::SeqCst),
        2,
        "a new need opens a new session"
    );
}

#[test]
fn an_answer_during_the_grace_time_keeps_the_session() {
    let _serial = serial();
    let fake = Fake::new();
    fake.answer_cancel.store(true, Ordering::SeqCst);
    let mut h = harness(&fake);
    h.start(&["SELECT slow"]);
    h.wait_running(1);
    h.manager.stop(h.console);
    let (statuses, end, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Stopped]);
    assert_eq!(end, ExecutionEnd::Stopped);

    fake.hang.store(false, Ordering::SeqCst);
    h.start(&["SELECT 1"]);
    h.finish();
    assert_eq!(
        fake.opened.load(Ordering::SeqCst),
        1,
        "the session stays in use"
    );
}

#[test]
fn a_statement_can_succeed_after_a_stop_and_the_script_still_ends() {
    let _serial = serial();
    let fake = Fake::new();
    fake.answer_cancel.store(true, Ordering::SeqCst);
    fake.success_after_cancel.store(true, Ordering::SeqCst);
    let mut h = harness(&fake);
    h.start(&["SELECT sleep(10)", "SELECT next"]);
    h.wait_running(1);
    h.manager.stop(h.console);
    let (statuses, end, _, _) = h.finish();
    assert_eq!(
        statuses,
        [StatementStatus::Success, StatementStatus::NotExecuted]
    );
    assert_eq!(end, ExecutionEnd::Stopped);
}

#[test]
fn with_a_transaction_the_close_needs_a_confirmation() {
    let _serial = serial();
    let fake = Fake::new();
    let mut h = harness(&fake);
    fake.hang.store(false, Ordering::SeqCst);
    fake.set_transaction(TransactionState::Open);
    h.start(&["BEGIN"]);
    h.finish();

    fake.hang.store(true, Ordering::SeqCst);
    h.start(&["UPDATE slow"]);
    h.wait_running(2);
    h.manager.stop(h.console);
    loop {
        if matches!(h.next(), ExecEvent::ConfirmClose { .. }) {
            break;
        }
    }

    h.manager.confirm_close(h.console, false);
    std::thread::sleep(GRACE * 2);
    assert!(h.events.try_recv().is_err(), "a refusal keeps the wait");

    h.manager.confirm_close(h.console, true);
    let (statuses, end, states, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Unknown]);
    assert_eq!(end, ExecutionEnd::ConnectionClosed);
    assert_eq!(states, [TransactionState::Unknown]);
}

#[test]
fn after_a_refusal_the_answer_of_the_database_ends_the_statement() {
    let _serial = serial();
    let fake = Fake::new();
    let mut h = harness(&fake);
    fake.hang.store(false, Ordering::SeqCst);
    fake.set_transaction(TransactionState::Open);
    h.start(&["BEGIN"]);
    h.finish();

    fake.hang.store(true, Ordering::SeqCst);
    h.start(&["UPDATE slow"]);
    h.wait_running(2);
    h.manager.stop(h.console);
    loop {
        if matches!(h.next(), ExecEvent::ConfirmClose { .. }) {
            break;
        }
    }
    h.manager.confirm_close(h.console, false);
    fake.release.notify_one();
    let (statuses, end, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Stopped]);
    assert_eq!(end, ExecutionEnd::Stopped);
    assert_eq!(
        h.manager.usage(h.connection).0,
        1,
        "the transaction keeps its session"
    );
}

#[test]
fn the_time_limit_uses_the_same_last_resort() {
    let _serial = serial();
    let fake = Fake::new();
    *SHARED.lock().unwrap() = Some(fake.clone());
    let budget = Arc::new(Budget::new(1 << 20));
    let settings = Settings {
        sql_timeout: Some(Duration::from_millis(20)),
        ..settings()
    };
    let (mut manager, events) = Manager::new(budget).unwrap();
    let connection = manager.add_connection(fake.clone(), settings);
    let console = manager.open_console(connection).unwrap();
    let mut h = Harness {
        manager,
        events,
        connection,
        console,
        fake: fake.clone(),
    };
    h.start(&["SELECT slow"]);
    let (statuses, end, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Unknown]);
    assert_eq!(end, ExecutionEnd::ConnectionClosed);
    assert_eq!(fake.cancels.load(Ordering::SeqCst), 1);
}

fn app(fake: &Arc<Fake>) -> App {
    *SHARED.lock().unwrap() = Some(fake.clone());
    let options = Options {
        connector: Some(fake.clone()),
        settings: settings(),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::plain())
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..30)
        .map(|y| {
            (0..100)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                + "\n"
        })
        .collect()
}

fn pump_until(app: &mut App, wanted: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.drain_events();
        let text = screen(app);
        if text.contains(wanted) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{wanted:?} did not appear:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn press(app: &mut App, event: crossterm::event::KeyEvent) {
    app.on_event(Event::Key(event));
}

fn prefix(app: &mut App, c: char) {
    press(app, ctrl('w'));
    press(app, key(KeyCode::Char(c)));
}

#[test]
fn the_application_asks_before_it_closes_a_connection_with_a_transaction() {
    let _serial = serial();
    let fake = Fake::new();
    let mut app = app(&fake);
    fake.hang.store(false, Ordering::SeqCst);
    fake.set_transaction(TransactionState::Open);
    app.on_event(Event::Paste("BEGIN;".to_owned()));
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Transaction open");
    pump_until(&mut app, "Idle");

    fake.hang.store(true, Ordering::SeqCst);
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Running");
    prefix(&mut app, 'x');
    pump_until(&mut app, "[Close the connection]");
    assert!(screen(&mut app).contains("[Keep waiting]"));

    // The first option keeps the wait, so an accidental Enter loses nothing.
    press(&mut app, key(KeyCode::Enter));
    assert!(!screen(&mut app).contains("[Close the connection]"));
    assert!(screen(&mut app).contains("Stopping"));
    fake.release.notify_one();
    pump_until(&mut app, "Idle");
    assert!(screen(&mut app).contains("Transaction open"));
}

#[test]
fn two_consoles_ask_in_turn_and_each_answer_reaches_its_own_console() {
    let _serial = serial();
    let fake = Fake::new();
    let mut app = app(&fake);
    fake.hang.store(false, Ordering::SeqCst);
    fake.set_transaction(TransactionState::Open);
    app.on_event(Event::Paste("BEGIN;".to_owned()));
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Transaction open");
    pump_until(&mut app, "Idle");
    prefix(&mut app, 'h');
    press(&mut app, key(KeyCode::Char('n')));
    app.on_event(Event::Paste("BEGIN;".to_owned()));
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Transaction open");
    pump_until(&mut app, "Idle");

    fake.hang.store(true, Ordering::SeqCst);
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Running");
    prefix(&mut app, 'p');
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Running");
    prefix(&mut app, 'x');
    prefix(&mut app, 'n');
    prefix(&mut app, 'x');

    // Both consoles ask. The second question waits for the answer to the first one.
    pump_until(&mut app, ": The statement did not answer");
    std::thread::sleep(GRACE * 3);
    app.drain_events();
    let first = if screen(&mut app).contains("console-1: The") {
        0
    } else {
        1
    };
    let second = 1 - first;
    let other = format!("console-{}: The statement", second + 1);
    assert!(!screen(&mut app).contains(&other), "{}", screen(&mut app));
    press(&mut app, key(KeyCode::Enter));
    pump_until(&mut app, &other);
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Enter));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.consoles()[second].transaction != TransactionState::Unknown {
        app.drain_events();
        assert!(Instant::now() < deadline, "{}", screen(&mut app));
    }
    assert_eq!(
        app.consoles()[first].transaction,
        TransactionState::Open,
        "the first answer keeps the wait"
    );
    assert!(app.consoles()[first].is_busy());
    fake.release.notify_one();
    while app.consoles()[first].is_busy() {
        app.drain_events();
        assert!(Instant::now() < deadline, "{}", screen(&mut app));
    }
}

#[test]
fn an_unknown_transaction_needs_a_confirmation_before_the_next_run() {
    let _serial = serial();
    let fake = Fake::new();
    let mut app = app(&fake);
    fake.hang.store(false, Ordering::SeqCst);
    fake.set_transaction(TransactionState::Open);
    app.on_event(Event::Paste("BEGIN;".to_owned()));
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Transaction open");
    pump_until(&mut app, "Idle");

    fake.hang.store(true, Ordering::SeqCst);
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "Running");
    prefix(&mut app, 'x');
    pump_until(&mut app, "[Close the connection]");
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Enter));
    pump_until(&mut app, "Transaction unknown");
    pump_until(&mut app, "The result is unknown");
    pump_until(&mut app, "Idle");

    let sent = fake.statements.lock().unwrap().len();
    fake.hang.store(false, Ordering::SeqCst);
    fake.set_transaction(TransactionState::None);
    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "The connection closed with an open transaction");
    press(&mut app, key(KeyCode::Enter));
    std::thread::sleep(GRACE);
    assert_eq!(
        fake.statements.lock().unwrap().len(),
        sent,
        "the default option sends no SQL"
    );

    press(&mut app, key(KeyCode::F(5)));
    pump_until(&mut app, "[Run statement]");
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Enter));
    pump_until(&mut app, "No transaction");
    pump_until(&mut app, "Idle");
    assert_eq!(fake.statements.lock().unwrap().len(), sent + 1);
}

#[test]
fn a_cancel_request_that_cannot_leave_closes_the_connection_at_once() {
    let _serial = serial();
    let fake = Fake::new();
    fake.refuse_cancel.store(true, Ordering::SeqCst);
    *SHARED.lock().unwrap() = Some(fake.clone());
    let budget = Arc::new(Budget::new(1 << 20));
    let settings = Settings {
        stop_grace: Duration::from_secs(30),
        ..settings()
    };
    let (mut manager, events) = Manager::new(budget).unwrap();
    let connection = manager.add_connection(fake.clone(), settings);
    let console = manager.open_console(connection).unwrap();
    let mut h = Harness {
        manager,
        events,
        connection,
        console,
        fake: fake.clone(),
    };
    h.start(&["SELECT slow"]);
    h.wait_running(1);

    let started = Instant::now();
    h.manager.stop(h.console);
    let (statuses, end, _, asked) = h.finish();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(statuses, [StatementStatus::Unknown]);
    assert_eq!(end, ExecutionEnd::ConnectionClosed);
    assert!(!asked);
}

#[test]
fn a_cancel_request_on_its_way_keeps_the_session_out_of_the_next_execution() {
    let _serial = serial();
    let fake = Fake::new();
    fake.hang_cancel.store(true, Ordering::SeqCst);
    fake.success_after_cancel.store(true, Ordering::SeqCst);
    let mut h = harness(&fake);
    h.start(&["SELECT slow"]);
    h.wait_running(1);
    h.manager.stop(h.console);
    while fake.cancels.load(Ordering::SeqCst) == 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
    fake.release.notify_one();
    let (statuses, end, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Success]);
    assert_eq!(end, ExecutionEnd::Stopped);
    assert_eq!(fake.opened.load(Ordering::SeqCst), 1);

    fake.hang.store(false, Ordering::SeqCst);
    h.start(&["SELECT next"]);
    let (statuses, _, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Success]);
    assert_eq!(
        fake.opened.load(Ordering::SeqCst),
        2,
        "the old session must not run again"
    );
}

#[test]
fn a_closed_session_gets_a_new_connection_in_the_pool() {
    let _serial = serial();
    let fake = Fake::new();
    fake.hang.store(false, Ordering::SeqCst);
    fake.dead.store(true, Ordering::SeqCst);
    let mut h = harness(&fake);
    h.start(&["SELECT 1"]);
    h.finish();
    h.start(&["SELECT 2"]);
    let (statuses, end, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Success]);
    assert_eq!(end, ExecutionEnd::Completed);
    assert_eq!(fake.opened.load(Ordering::SeqCst), 2);
}

#[test]
fn the_stop_request_tells_the_connector_to_drop_the_rows() {
    let _serial = serial();
    let fake = Fake::new();
    fake.stream_rows.store(true, Ordering::SeqCst);
    let mut h = harness(&fake);
    h.start(&["SELECT many"]);
    h.wait_running(1);
    h.manager.stop(h.console);
    let (statuses, end, _, _) = h.finish();
    assert_eq!(statuses, [StatementStatus::Stopped]);
    assert_eq!(end, ExecutionEnd::Stopped);
}
