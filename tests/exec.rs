mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::temp_db;
use sqln::catalog::{Columns, RelationKind, RelationRef};
use sqln::connector::sqlite::{SqliteConnector, SqliteTarget};
use sqln::connector::{TransactionCommand, TransactionState};
use sqln::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, Manager, ReadLimit, RunOptions, Settings,
    StatementStatus,
};
use sqln::grid::{Budget, Row, Value};
use tokio::sync::mpsc::Receiver;

const HUGE_SELECT: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT x FROM c";
const HUGE_COUNT: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT count(*) FROM c";
const HUGE_INSERT: &str = "INSERT INTO t WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT x FROM c";

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    budget: Arc<Budget>,
    connection: ConnectionId,
    console: ConsoleId,
}

#[derive(Debug, Default)]
struct Run {
    columns: Vec<Vec<String>>,
    rows: Vec<Row>,
    bytes: usize,
    statuses: Vec<StatementStatus>,
    limits: Vec<ReadLimit>,
    changed: Vec<Option<u64>>,
    end: Option<ExecutionEnd>,
    message: Option<String>,
    transaction: Option<TransactionState>,
    confirm_close: bool,
}

impl Harness {
    fn open(name: &str, settings: Settings) -> Self {
        Self::with_path(&temp_db(name), settings)
    }

    fn with_path(path: &Path, settings: Settings) -> Self {
        let mut target = SqliteTarget::new(path);
        target.allow_create = true;
        target.busy_timeout = Duration::from_millis(100);
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let connector = Arc::new(SqliteConnector::new(target));
        let (mut manager, events) = Manager::new(budget.clone()).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        Self { manager, events, budget, connection, console }
    }

    fn next(&mut self) -> ExecEvent {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(event) = self.events.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "no event arrived");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn collect(&mut self, execution: u64) -> Run {
        let mut run = Run::default();
        loop {
            match self.next() {
                ExecEvent::Columns { columns, .. } => run.columns.push(columns),
                ExecEvent::Rows { rows, bytes, .. } => {
                    run.rows.extend(rows);
                    run.bytes += bytes;
                }
                ExecEvent::StatementFinished { status, end, .. } => {
                    run.statuses.push(status);
                    run.limits.push(end.map_or(ReadLimit::None, |end| end.limit));
                    run.changed.push(end.and_then(|end| end.changed));
                }
                ExecEvent::TransactionChanged { state, .. } => run.transaction = Some(state),
                ExecEvent::ConfirmClose { .. } => run.confirm_close = true,
                ExecEvent::ExecutionFinished { execution: finished, end, message, .. }
                    if finished == execution =>
                {
                    run.end = Some(end);
                    run.message = message;
                    return run;
                }
                _ => {}
            }
        }
    }

    fn run_on(&mut self, console: ConsoleId, statements: &[&str], manual: bool) -> Run {
        let statements = statements.iter().map(|sql| (*sql).to_owned()).collect();
        let execution = self.manager.run(console, statements, RunOptions { manual, ..RunOptions::default() });
        self.collect(execution)
    }

    fn run(&mut self, statements: &[&str]) -> Run {
        self.run_on(self.console, statements, false)
    }

    fn cell(&mut self, sql: &str) -> Value {
        self.run(&[sql]).rows.remove(0)[0].clone()
    }
}

fn number(text: &str) -> Value {
    Value::Number(text.into())
}

fn success(count: usize) -> Vec<StatementStatus> {
    vec![StatementStatus::Success; count]
}

#[test]
fn a_query_delivers_columns_and_rows_as_database_text() {
    let mut h = Harness::open("rows", Settings::default());
    let run = h.run(&["SELECT 1 AS a, 1.5 AS b, 'ação' AS c, NULL AS d, x'00FF' AS e, 1e20 AS f"]);
    assert_eq!(run.columns, [["a", "b", "c", "d", "e", "f"]]);
    assert_eq!(run.statuses, success(1));
    assert_eq!(run.end, Some(ExecutionEnd::Completed));
    assert_eq!(run.rows[0][0], number("1"));
    assert_eq!(run.rows[0][1], number("1.5"));
    assert_eq!(run.rows[0][2], Value::Text("ação".into()));
    assert_eq!(run.rows[0][3], Value::Null);
    assert_eq!(run.rows[0][4], Value::Binary(Box::from([0x00, 0xFF])));
    assert_eq!(run.rows[0][5], number("1e20"));
}

#[test]
fn a_result_without_rows_keeps_its_columns() {
    let mut h = Harness::open("empty", Settings::default());
    let run = h.run(&["SELECT 1 AS a, 2 AS b WHERE 0"]);
    assert_eq!(run.columns, [["a", "b"]]);
    assert!(run.rows.is_empty());
    assert_eq!(run.statuses, success(1));
}

#[test]
fn invalid_utf8_text_stays_as_bytes() {
    let mut h = Harness::open("bytes", Settings::default());
    let value = h.cell("SELECT CAST(x'FF00' AS TEXT)");
    assert!(matches!(value, Value::Binary(_) | Value::Text(_)), "{value:?}");
    assert_eq!(h.cell("SELECT CAST(x'FF41' AS TEXT)"), Value::Binary(Box::from([0xFF, 0x41])));
}

#[test]
fn changes_report_their_row_count() {
    let mut h = Harness::open("changes", Settings::default());
    let run = h.run(&["CREATE TABLE t(a)", "INSERT INTO t VALUES (1), (2), (3)", "DELETE FROM t WHERE a > 1"]);
    assert_eq!(run.statuses, success(3));
    assert_eq!(&run.changed[1..], [Some(3), Some(2)]);
}

#[test]
fn the_script_stops_at_the_first_error() {
    let mut h = Harness::open("first-error", Settings::default());
    let run = h.run(&[
        "CREATE TABLE t(a)",
        "INSERT INTO t VALUES (1)",
        "SELECT * FROM missing",
        "INSERT INTO t VALUES (2)",
    ]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert_eq!(run.statuses[..2], success(2));
    assert!(matches!(&run.statuses[2], StatementStatus::Error(message) if message.contains("missing")));
    assert_eq!(run.statuses[3], StatementStatus::NotExecuted);
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("1"), "the first insert stays");
}

#[test]
fn the_row_cap_ends_a_large_read_quickly() {
    let mut h = Harness::open("cap", Settings { row_cap: 1_000, ..Settings::default() });
    let started = Instant::now();
    let run = h.run(&[HUGE_SELECT, "SELECT 2"]);
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(run.rows.len(), 1_001, "1,000 rows and the second statement");
    assert_eq!(run.limits, [ReadLimit::RowCap, ReadLimit::None]);
    assert_eq!(run.statuses, success(2));
}

#[test]
fn a_result_with_exactly_the_cap_has_no_limit_mark() {
    let mut h = Harness::open("cap-exact", Settings { row_cap: 3, ..Settings::default() });
    let run = h.run(&["SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3"]);
    assert_eq!(run.rows.len(), 3);
    assert_eq!(run.limits, [ReadLimit::None]);
}

#[test]
fn the_cap_does_not_change_a_statement_with_returning() {
    let mut h = Harness::open("returning", Settings { row_cap: 2, ..Settings::default() });
    let run = h.run(&["CREATE TABLE t(a)", "INSERT INTO t VALUES (1), (2), (3), (4), (5) RETURNING a"]);
    assert_eq!(run.rows.len(), 2);
    assert_eq!(run.limits[1], ReadLimit::RowCap);
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("5"));
}

#[test]
fn the_budget_ends_the_read_and_a_release_frees_it() {
    let settings = Settings { budget_bytes: 4_096, row_cap: 1_000_000, ..Settings::default() };
    let mut h = Harness::open("budget", settings);
    let run = h.run(&[HUGE_SELECT]);
    assert_eq!(run.limits, [ReadLimit::Budget]);
    assert!(!run.rows.is_empty());
    assert_eq!(h.budget.used(), run.bytes);
    assert!(run.bytes <= 4_096);
    h.budget.release(run.bytes);
    assert_eq!(h.budget.used(), 0);
}

#[test]
fn a_value_larger_than_the_budget_is_reported() {
    let mut h = Harness::open("oversize", Settings { budget_bytes: 16, ..Settings::default() });
    let run = h.run(&["SELECT hex(zeroblob(1000))"]);
    assert_eq!(run.limits, [ReadLimit::ValueTooLarge]);
    assert!(run.rows.is_empty());
    assert_eq!(h.budget.used(), 0);
}

#[test]
fn the_time_limit_applies_per_statement_and_stops_the_script() {
    let settings = Settings { sql_timeout: Some(Duration::from_millis(100)), ..Settings::default() };
    let mut h = Harness::open("timeout", settings);
    let started = Instant::now();
    let run = h.run(&[HUGE_COUNT, "SELECT 2"]);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert_eq!(run.statuses, [StatementStatus::Timeout, StatementStatus::NotExecuted]);
    assert_eq!(run.end, Some(ExecutionEnd::TimedOut));
    assert_eq!(h.cell("SELECT 1"), number("1"), "the next execution works");
}

#[test]
fn a_stop_interrupts_the_statement_and_the_following_ones() {
    let mut h = Harness::open("stop", Settings::default());
    let statements = vec![HUGE_COUNT.to_owned(), "SELECT 2".to_owned()];
    let execution = h.manager.run(h.console, statements, RunOptions::default());
    std::thread::sleep(Duration::from_millis(100));
    h.manager.stop(h.console);
    let run = h.collect(execution);
    assert_eq!(run.statuses, [StatementStatus::Stopped, StatementStatus::NotExecuted]);
    assert_eq!(run.end, Some(ExecutionEnd::Stopped));
    assert_eq!(h.cell("SELECT 1"), number("1"));
}

#[test]
fn a_stop_while_waiting_sends_no_sql() {
    let settings = Settings { max_connections: 1, ..Settings::default() };
    let mut h = Harness::open("queue", settings);
    h.run(&["CREATE TABLE t(a)", "BEGIN", "INSERT INTO t VALUES (1)"]);
    let other = h.manager.open_console(h.connection).unwrap();

    let execution = h.manager.run(other, vec!["INSERT INTO t VALUES (2)".to_owned()], RunOptions::default());
    std::thread::sleep(Duration::from_millis(100));
    h.manager.stop(other);
    let run = h.collect(execution);
    assert_eq!(run.end, Some(ExecutionEnd::CanceledBeforeStart));
    assert!(run.statuses.is_empty());

    assert_eq!(h.cell("SELECT count(*) FROM t"), number("1"), "the first console keeps its session");
}

#[test]
fn a_console_waits_and_runs_when_the_transaction_of_another_ends() {
    let settings = Settings { max_connections: 1, ..Settings::default() };
    let mut h = Harness::open("wait", settings);
    h.run(&["CREATE TABLE t(a)", "BEGIN"]);
    let other = h.manager.open_console(h.connection).unwrap();

    let execution = h.manager.run(other, vec!["SELECT 7".to_owned()], RunOptions::default());
    std::thread::sleep(Duration::from_millis(100));
    h.run(&["COMMIT"]);
    let run = h.collect(execution);
    assert_eq!(run.rows[0][0], number("7"));
}

#[test]
fn a_transaction_keeps_its_session_between_executions() {
    let mut h = Harness::open("transaction", Settings::default());
    h.run(&["CREATE TABLE t(a)"]);
    let run = h.run(&["BEGIN", "INSERT INTO t VALUES (1)"]);
    assert_eq!(run.transaction, Some(TransactionState::Open));

    assert_eq!(h.cell("SELECT count(*) FROM t"), number("1"), "the same session sees the row");
    let other = h.manager.open_console(h.connection).unwrap();
    let run = h.run_on(other, &["SELECT count(*) FROM t"], false);
    assert_eq!(run.rows[0][0], number("0"), "another session does not");

    let run = h.run(&["ROLLBACK"]);
    assert_eq!(run.transaction, Some(TransactionState::None));
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("0"));
}

#[test]
fn an_error_inside_a_transaction_keeps_it_open() {
    let mut h = Harness::open("tx-error", Settings::default());
    h.run(&["CREATE TABLE t(a)"]);
    let run = h.run(&["BEGIN", "INSERT INTO t VALUES (1)", "SELECT * FROM missing"]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert_eq!(run.transaction, Some(TransactionState::Open));
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("1"));
}

#[test]
fn the_session_stays_the_same_during_a_script_and_changes_after_it() {
    let mut h = Harness::open("session", Settings::default());
    let run = h.run(&["CREATE TEMP TABLE t(a)", "INSERT INTO t VALUES (1)", "SELECT count(*) FROM t"]);
    assert_eq!(run.statuses, success(3));
    let run = h.run(&["SELECT count(*) FROM t"]);
    assert!(matches!(&run.statuses[0], StatementStatus::Error(message) if message.contains("no such table")));
}

#[test]
fn manual_mode_opens_a_transaction_and_commit_confirms_it() {
    let mut h = Harness::open("manual", Settings::default());
    h.run(&["CREATE TABLE t(a)"]);
    let console = h.console;

    let run = h.run_on(console, &["INSERT INTO t VALUES (1)"], true);
    assert_eq!(run.transaction, Some(TransactionState::Open));

    h.manager.transaction(console, TransactionCommand::Rollback);
    assert!(matches!(h.next(), ExecEvent::TransactionChanged { state: TransactionState::None, .. }));
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("0"));

    h.run_on(console, &["INSERT INTO t VALUES (2)"], true);
    h.manager.transaction(console, TransactionCommand::Commit);
    assert!(matches!(h.next(), ExecEvent::TransactionChanged { state: TransactionState::None, .. }));
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("1"));
}

#[test]
fn commit_without_a_transaction_reports_a_failure() {
    let mut h = Harness::open("no-tx", Settings::default());
    h.manager.transaction(h.console, TransactionCommand::Commit);
    assert!(matches!(h.next(), ExecEvent::TransactionFailed { .. }));
}

#[test]
fn a_second_execution_in_the_same_console_is_refused() {
    let mut h = Harness::open("refuse", Settings::default());
    let first = h.manager.run(h.console, vec![HUGE_COUNT.to_owned()], RunOptions::default());
    std::thread::sleep(Duration::from_millis(100));
    let second = h.manager.run(h.console, vec!["SELECT 1".to_owned()], RunOptions::default());
    let run = h.collect(second);
    assert_eq!(run.end, Some(ExecutionEnd::Refused));
    h.manager.stop(h.console);
    h.collect(first);
}

#[test]
fn a_commit_during_an_execution_is_refused() {
    let mut h = Harness::open("refuse-commit", Settings::default());
    let execution = h.manager.run(h.console, vec![HUGE_COUNT.to_owned()], RunOptions::default());
    std::thread::sleep(Duration::from_millis(100));
    h.manager.transaction(h.console, TransactionCommand::Commit);
    loop {
        if matches!(h.next(), ExecEvent::TransactionFailed { .. }) {
            break;
        }
    }
    h.manager.stop(h.console);
    h.collect(execution);
}

#[test]
fn an_interrupted_change_in_a_transaction_reports_the_rollback_of_the_database() {
    let mut h = Harness::open("tx-interrupt", Settings::default());
    h.run(&["CREATE TABLE t(a)", "INSERT INTO t VALUES (1)"]);
    h.run(&["BEGIN", "DELETE FROM t"]);
    let execution = h.manager.run(h.console, vec![HUGE_INSERT.to_owned()], RunOptions::default());
    std::thread::sleep(Duration::from_millis(100));
    h.manager.stop(h.console);
    let run = h.collect(execution);
    assert_eq!(run.statuses, [StatementStatus::Stopped]);
    assert_eq!(run.transaction, Some(TransactionState::None), "SQLite ends the transaction");
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("1"), "the delete did not stay");
}

#[test]
fn read_only_applies_at_each_reservation() {
    let path = temp_db("read-only");
    Harness::with_path(&path, Settings::default()).run(&["CREATE TABLE t(a)"]);

    let mut h = Harness::with_path(&path, Settings { read_only: true, ..Settings::default() });
    for _ in 0..2 {
        let run = h.run(&["INSERT INTO t VALUES (1)"]);
        assert!(matches!(&run.statuses[0], StatementStatus::Error(message) if message.contains("readonly")), "{run:?}");
    }
    assert_eq!(h.cell("SELECT count(*) FROM t"), number("0"));
}

#[test]
fn a_locked_database_shows_an_error_after_the_lock_time_limit() {
    let mut h = Harness::open("locked", Settings::default());
    h.run(&["CREATE TABLE t(a)", "BEGIN IMMEDIATE", "INSERT INTO t VALUES (1)"]);
    let other = h.manager.open_console(h.connection).unwrap();
    let started = Instant::now();
    let run = h.run_on(other, &["INSERT INTO t VALUES (2)"], false);
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(matches!(&run.statuses[0], StatementStatus::Error(message) if message.contains("locked")), "{run:?}");
}

#[test]
fn a_missing_file_is_a_failure_without_permission_to_create_it() {
    let path = temp_db("missing");
    let budget = Arc::new(Budget::new(1_000));
    let connector = Arc::new(SqliteConnector::new(SqliteTarget::new(&path)));
    let (mut manager, mut events) = Manager::new(budget).unwrap();
    let connection = manager.add_connection(connector, Settings::default());
    let console = manager.open_console(connection).unwrap();
    let execution = manager.run(console, vec!["SELECT 1".to_owned()], RunOptions::default());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        if let Ok(ExecEvent::ExecutionFinished { execution: finished, end, message, .. }) = events.try_recv() {
            assert_eq!(finished, execution);
            assert_eq!(end, ExecutionEnd::Failed);
            assert!(message.is_some());
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!path.exists(), "a failure must not create the file");
}

#[test]
fn memory_databases_are_refused_in_the_text() {
    use sqln::connector::sqlite::validate_path;
    for text in [":memory:", "", "  ", "file::memory:", "FILE:x.db?mode=memory", "x.db?mode=memory&cache=shared"] {
        assert!(validate_path(Path::new(text)).is_err(), "{text:?}");
    }
    assert!(validate_path(Path::new("data/app.db")).is_ok());
}

#[test]
fn the_catalog_reads_relations_first_and_columns_on_demand() {
    let mut h = Harness::open("catalog", Settings::default());
    h.run(&["CREATE TABLE users(id, \"Full Name\")", "CREATE VIEW names AS SELECT id FROM users"]);

    h.manager.load_catalog(h.connection);
    let catalog = loop {
        if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
            break catalog.unwrap();
        }
    };
    assert_eq!(catalog.current_schema.as_deref(), Some("main"));
    let main = catalog.schema("main").unwrap();
    assert_eq!(main.relations.len(), 2);
    assert_eq!(main.relations[0].name, "names");
    assert_eq!(main.relations[0].kind, RelationKind::View);
    assert_eq!(main.relations[1].columns, Columns::NotLoaded);

    let relation = RelationRef { schema: "main".to_owned(), relation: "users".to_owned() };
    h.manager.load_columns(h.connection, relation.clone());
    loop {
        if let ExecEvent::ColumnsLoaded { relation: loaded, columns, .. } = h.next() {
            assert_eq!(loaded, relation);
            assert_eq!(columns.unwrap(), ["id", "Full Name"]);
            break;
        }
    }
}
