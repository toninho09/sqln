// Tests with real servers. Start them with the commands of `spikes/RESULTS.md`, and
// load `scripts/test-env.sh`. Without the variables, these tests print "skipped".
//
// Each test uses its own tables, so the tests can run at the same time.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{ctrl, key};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::{Columns, RelationKind, RelationRef};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::{Definition, Source};
use sqln::connector::target::parse_uri;
use sqln::connector::{
    Connector, Session, TransactionCommand, TransactionState, network_connector,
};
use sqln::editor::KeyboardMode;
use sqln::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, Manager, ReadLimit, ResultSink, RunOptions,
    Settings, StatementKey, StatementStatus,
};
use sqln::form::Field;
use sqln::grid::{Budget, Row, Value};
use sqln::ui::theme::Theme;
use tokio::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Postgres,
    MySql,
}

struct Server {
    kind: Kind,
    uri: String,
}

fn servers() -> Vec<Server> {
    let mut found = Vec::new();
    for (name, kind) in [
        ("SQLN_TEST_POSTGRES", Kind::Postgres),
        ("SQLN_TEST_MYSQL", Kind::MySql),
    ] {
        match std::env::var(name) {
            Ok(uri) => found.push(Server { kind, uri }),
            Err(_) => println!("skipped: {name} is not set"),
        }
    }
    found
}

fn only(kind: Kind) -> Option<Server> {
    servers().into_iter().find(|server| server.kind == kind)
}

fn connector(server: &Server) -> Arc<dyn Connector> {
    network_connector(parse_uri(&server.uri).unwrap())
}

impl Server {
    /// Returns a statement that waits for `seconds` without rows.
    fn sleep(&self, seconds: u32) -> String {
        match self.kind {
            Kind::Postgres => format!("SELECT pg_sleep({seconds})"),
            Kind::MySql => format!("SELECT SLEEP({seconds})"),
        }
    }

    /// Returns a read with more rows than any test can receive.
    fn huge_read(&self) -> &'static str {
        match self.kind {
            Kind::Postgres => "SELECT generate_series(1, 2000000000)",
            Kind::MySql => "SELECT a.x FROM t_huge a, t_huge b, t_huge c",
        }
    }

    /// Creates the rows of `huge_read`. MySQL reads this join row by row.
    fn prepare_huge_read(&self, h: &mut Harness) {
        if self.kind == Kind::MySql {
            h.run_ok(&["CREATE TABLE IF NOT EXISTS t_huge (x INT) ENGINE=InnoDB"]);
            if h.count("t_huge") == "0" {
                let values = (0..1000)
                    .map(|x| format!("({x})"))
                    .collect::<Vec<_>>()
                    .join(",");
                h.run_ok(&[&format!("INSERT INTO t_huge VALUES {values}")]);
            }
        }
    }

    fn backend_id(&self) -> &'static str {
        match self.kind {
            Kind::Postgres => "SELECT pg_backend_pid()",
            Kind::MySql => "SELECT CONNECTION_ID()",
        }
    }

    fn kill(&self, id: &str) -> String {
        match self.kind {
            Kind::Postgres => format!("SELECT pg_terminate_backend({id})"),
            Kind::MySql => format!("KILL {id}"),
        }
    }
}

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
    results: Vec<(usize, ReadLimit)>,
    end: Option<ExecutionEnd>,
    transaction: Option<TransactionState>,
}

impl Run {
    fn text(&self, row: usize, column: usize) -> String {
        match &self.rows[row][column] {
            Value::Text(text) | Value::Number(text) => text.to_string(),
            other => panic!("not text: {other:?}"),
        }
    }

    fn error(&self) -> &str {
        self.statuses
            .iter()
            .find_map(|status| match status {
                StatementStatus::Error(message) => Some(message.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no error: {self:?}"))
    }
}

impl Harness {
    fn open(server: &Server, settings: Settings) -> Self {
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let (mut manager, events) = Manager::new(budget.clone()).unwrap();
        let connection = manager.add_connection(connector(server), settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            budget,
            connection,
            console,
        }
    }

    fn next(&mut self) -> ExecEvent {
        let deadline = Instant::now() + Duration::from_secs(60);
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
                ExecEvent::ResultDone { rows, limit, .. } => run.results.push((rows, limit)),
                ExecEvent::StatementFinished { status, end, .. } => {
                    run.statuses.push(status);
                    run.limits
                        .push(end.map_or(ReadLimit::None, |end| end.limit));
                    run.changed.push(end.and_then(|end| end.changed));
                }
                ExecEvent::TransactionChanged { state, .. } => run.transaction = Some(state),
                ExecEvent::ExecutionFinished {
                    execution: finished,
                    end,
                    ..
                } if finished == execution => {
                    run.end = Some(end);
                    return run;
                }
                _ => {}
            }
        }
    }

    fn start(&mut self, statements: &[&str], manual: bool) -> u64 {
        let statements = statements.iter().map(|sql| (*sql).to_owned()).collect();
        self.manager.run(
            self.console,
            statements,
            RunOptions {
                manual,
                ..RunOptions::default()
            },
        )
    }

    /// Waits for the first statement. The connection can take any time before it.
    fn wait_started(&mut self) {
        while !matches!(self.next(), ExecEvent::StatementStarted { .. }) {}
    }

    fn run(&mut self, statements: &[&str]) -> Run {
        let execution = self.start(statements, false);
        self.collect(execution)
    }

    fn run_ok(&mut self, statements: &[&str]) -> Run {
        let run = self.run(statements);
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{statements:?}: {run:?}"
        );
        run
    }

    fn count(&mut self, table: &str) -> String {
        self.run_ok(&[&format!("SELECT count(*) FROM {table}")])
            .text(0, 0)
    }

    fn control(&mut self, command: TransactionCommand) -> TransactionState {
        self.manager.transaction(self.console, command);
        loop {
            match self.next() {
                ExecEvent::TransactionChanged { state, .. } => return state,
                ExecEvent::TransactionFailed { message, .. } => panic!("{message}"),
                _ => {}
            }
        }
    }
}

fn success(count: usize) -> Vec<StatementStatus> {
    vec![StatementStatus::Success; count]
}

fn sessions(limit: usize) -> Settings {
    Settings {
        max_connections: limit,
        ..Settings::default()
    }
}

#[test]
fn values_keep_the_text_of_the_database_in_postgres() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run_ok(&[
        "SELECT 12345678901234567890.123456789::numeric, NULL::text, ''::text, '\\x00ff'::bytea, \
         '2026-10-30 12:34:56.789+00'::timestamptz AT TIME ZONE 'UTC', 'ação 日本'::text, \
         '{1,2}'::int[], '{\"a\": 1}'::jsonb, 1.50::float8, 7::int8, '1 day'::interval, true",
    ]);
    let row = &run.rows[0];
    assert_eq!(
        row[0],
        Value::Number("12345678901234567890.123456789".into())
    );
    assert_eq!(row[1], Value::Null);
    assert_eq!(row[2], Value::Text("".into()));
    assert_eq!(row[3], Value::Binary(Box::new([0x00, 0xFF])));
    assert_eq!(row[4], Value::Text("2026-10-30 12:34:56.789".into()));
    assert_eq!(row[5], Value::Text("ação 日本".into()));
    assert_eq!(row[6], Value::Text("{1,2}".into()));
    assert_eq!(row[7], Value::Text("{\"a\": 1}".into()));
    assert_eq!(row[8], Value::Number("1.5".into()));
    assert_eq!(row[9], Value::Number("7".into()));
    assert_eq!(row[10], Value::Text("1 day".into()));
    assert_eq!(row[11], Value::Text("t".into()));
}

#[test]
fn a_text_that_looks_like_binary_stays_text_in_postgres() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run_ok(&["SELECT '\\x00ff'::text"]);
    assert_eq!(run.rows[0][0], Value::Text("\\x00ff".into()));
}

#[test]
fn values_keep_the_text_of_the_database_in_mysql() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run_ok(
        &["SELECT 12345678901234567890.123456789, NULL, '', x'00ff', \
         TIMESTAMP '2026-10-30 12:34:56', 'ação 日本', 1.50, 7, CAST('{\"a\": 1}' AS JSON), \
         DATE '2026-10-30', CAST(x'41' AS BINARY)"],
    );
    let row = &run.rows[0];
    assert_eq!(
        row[0],
        Value::Number("12345678901234567890.123456789".into())
    );
    assert_eq!(row[1], Value::Null);
    assert_eq!(row[2], Value::Text("".into()));
    assert_eq!(row[3], Value::Binary(Box::new([0x00, 0xFF])));
    assert_eq!(row[4], Value::Text("2026-10-30 12:34:56".into()));
    assert_eq!(row[5], Value::Text("ação 日本".into()));
    assert_eq!(row[6], Value::Number("1.50".into()));
    assert_eq!(row[7], Value::Number("7".into()));
    assert_eq!(row[8], Value::Text("{\"a\": 1}".into()));
    assert_eq!(row[9], Value::Text("2026-10-30".into()));
    assert_eq!(row[10], Value::Binary(Box::new([0x41])));
}

#[test]
fn an_error_stops_the_script_and_reports_the_position() {
    for server in servers() {
        let table = "t_error";
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&[
            &format!("DROP TABLE IF EXISTS {table}"),
            &format!("CREATE TABLE {table} (a int)"),
        ]);
        let insert = format!("INSERT INTO {table} VALUES (1)");
        let run = h.run(&[&insert, "SELECT * FROMM nowhere", &insert]);
        assert_eq!(run.end, Some(ExecutionEnd::Failed), "{:?}", server.kind);
        assert_eq!(run.statuses[0], StatementStatus::Success);
        assert!(!run.error().is_empty());
        assert_eq!(run.statuses[2], StatementStatus::NotExecuted);
        assert_eq!(h.count(table), "1", "{:?}", server.kind);
        h.run_ok(&[&format!("DROP TABLE {table}")]);
    }
}

#[test]
fn the_error_offset_points_at_the_statement_text() {
    for server in servers() {
        let connector = connector(&server);
        let runtime = runtime();
        runtime.block_on(async {
            let mut session = connector.connect().await.unwrap();
            let sql = "SELECT 'ação',\n  1 + FROM nowhere";
            let error = session.run(sql, sink()).await.unwrap_err();
            let offset = error.offset.unwrap_or_else(|| panic!("{error:?}"));
            assert_eq!(&sql[offset..offset + 4], "FROM", "{:?}", server.kind);

            let error = session.run("SELECT 1 FROM", sink()).await.unwrap_err();
            assert_eq!(error.offset, Some(13), "{:?}", server.kind);
        });
    }
}

#[test]
fn changes_report_their_rows_and_ddl_reports_none() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let run = h.run_ok(&[
            "DROP TABLE IF EXISTS t_changes",
            "CREATE TABLE t_changes (a int)",
            "INSERT INTO t_changes VALUES (1), (2), (3)",
            "UPDATE t_changes SET a = 9 WHERE a > 1",
            "DELETE FROM t_changes WHERE a = 100",
            "DROP TABLE t_changes",
        ]);
        assert_eq!(
            run.changed,
            [None, None, Some(3), Some(2), Some(0), None],
            "{:?}",
            server.kind
        );
    }
}

#[test]
fn an_empty_result_keeps_its_columns() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let run = h.run_ok(&[
            "SELECT 1 AS first_name, 2 AS second_name FROM (SELECT 1 AS x) s WHERE x = 2",
        ]);
        assert_eq!(
            run.columns,
            [["first_name", "second_name"]],
            "{:?}",
            server.kind
        );
        assert!(run.rows.is_empty());
        assert_eq!(run.results, [(0, ReadLimit::None)]);
    }
}

#[test]
fn the_row_cap_ends_a_large_read_soon_and_the_session_stays_usable() {
    for server in servers() {
        let settings = Settings {
            row_cap: 100,
            max_connections: 1,
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        server.prepare_huge_read(&mut h);
        let started = Instant::now();
        let run = h.run(&[server.huge_read(), "SELECT 7"]);
        let elapsed = started.elapsed();
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{:?}: {run:?}",
            server.kind
        );
        assert_eq!(run.statuses, success(2));
        assert_eq!(run.limits[0], ReadLimit::RowCap);
        assert_eq!(run.rows.len(), 101);
        assert_eq!(run.text(100, 0), "7");
        assert!(
            elapsed < Duration::from_secs(5),
            "{:?} {elapsed:?}",
            server.kind
        );
        println!("{:?}: row cap in {elapsed:?}", server.kind);
    }
}

#[test]
fn the_row_cap_counts_rows_at_each_edge() {
    for server in servers() {
        let settings = Settings {
            row_cap: 3,
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        let rows = |count: usize| {
            let values: Vec<String> = (0..count)
                .map(|index| format!("SELECT {index} AS x"))
                .collect();
            values.join(" UNION ALL ")
        };
        assert_eq!(h.run_ok(&[&rows(2)]).limits, [ReadLimit::None]);
        assert_eq!(h.run_ok(&[&rows(3)]).limits, [ReadLimit::None]);
        let above = h.run_ok(&[&rows(4)]);
        assert_eq!(above.limits, [ReadLimit::RowCap], "{:?}", server.kind);
        assert_eq!(above.rows.len(), 3);
    }
}

#[test]
fn an_open_transaction_consumes_the_rows_above_the_cap() {
    for server in servers() {
        let settings = Settings {
            row_cap: 10,
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        server.prepare_huge_read(&mut h);
        h.run_ok(&[
            "DROP TABLE IF EXISTS t_cap_tx",
            "CREATE TABLE t_cap_tx (a int)",
        ]);
        // The read must outlast a cancel request, so that a wrong request has an effect.
        let sql = match server.kind {
            Kind::Postgres => "SELECT generate_series(1, 2000000)",
            Kind::MySql => "SELECT a.x FROM t_huge a, t_huge b",
        };
        let run = h.run_ok(&[
            "BEGIN",
            "INSERT INTO t_cap_tx VALUES (1)",
            sql,
            "INSERT INTO t_cap_tx VALUES (2)",
        ]);
        assert_eq!(run.limits[2], ReadLimit::RowCap, "{:?}", server.kind);
        assert_eq!(run.transaction, Some(TransactionState::Open));
        assert_eq!(
            h.control(TransactionCommand::Commit),
            TransactionState::None
        );
        assert_eq!(h.count("t_cap_tx"), "2", "{:?}", server.kind);
        h.run_ok(&["DROP TABLE t_cap_tx"]);
    }
}

#[test]
fn the_row_cap_does_not_stop_a_change_that_returns_rows() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let settings = Settings {
        row_cap: 10,
        ..Settings::default()
    };
    let mut h = Harness::open(&server, settings);
    let run = h.run_ok(&[
        "DROP TABLE IF EXISTS t_returning",
        "CREATE TABLE t_returning (a int)",
        "INSERT INTO t_returning SELECT generate_series(1, 5000) RETURNING a",
    ]);
    assert_eq!(run.limits[2], ReadLimit::RowCap);
    assert_eq!(h.count("t_returning"), "5000");
    h.run_ok(&["DROP TABLE t_returning"]);
}

#[test]
fn the_row_cap_does_not_stop_a_function_that_writes() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let settings = Settings {
        row_cap: 10,
        ..Settings::default()
    };
    let mut h = Harness::open(&server, settings);
    let run = h.run_ok(&[
        "DROP TABLE IF EXISTS t_function CASCADE",
        "CREATE TABLE t_function (a int)",
        "CREATE OR REPLACE FUNCTION t_function_write(v int) RETURNS int LANGUAGE sql AS \
         $$ INSERT INTO t_function VALUES (v) RETURNING a $$",
        "SELECT t_function_write(g) FROM generate_series(1, 3000) g",
    ]);
    assert_eq!(run.limits[3], ReadLimit::RowCap);
    assert_eq!(h.count("t_function"), "3000");
    h.run_ok(&["DROP FUNCTION t_function_write", "DROP TABLE t_function"]);
}

#[test]
fn stop_reaches_a_statement_in_progress_and_the_session_stays_usable() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(1));
        let execution = h.start(&[&server.sleep(30), "SELECT 'not executed'"], false);
        h.wait_started();
        // The runner sends the event before the text reaches the server.
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        h.manager.stop(h.console);
        let run = h.collect(execution);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            server.kind
        );
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Stopped),
            "{:?}: {run:?}",
            server.kind
        );
        assert_eq!(run.statuses[1], StatementStatus::NotExecuted);
        assert_eq!(h.run_ok(&["SELECT 5"]).text(0, 0), "5", "{:?}", server.kind);
    }
}

#[test]
fn stop_in_a_large_read_keeps_the_loaded_rows() {
    for server in servers() {
        // This test is about the rows, not the time. A test build with a busy processor
        // can read for 14 s the rows on their way, and an optimized build needs 0.4 s.
        let settings = Settings {
            row_cap: usize::MAX,
            stop_grace: Duration::from_secs(50),
            ..sessions(1)
        };
        let mut h = Harness::open(&server, settings);
        server.prepare_huge_read(&mut h);
        let used_before = h.budget.used();
        let execution = h.start(&[server.huge_read()], false);
        let mut run = Run::default();
        while run.rows.len() < 500 {
            if let ExecEvent::Rows { rows, bytes, .. } = h.next() {
                run.rows.extend(rows);
                run.bytes += bytes;
            }
        }
        h.manager.stop(h.console);
        let rest = h.collect(execution);
        assert_eq!(
            rest.end,
            Some(ExecutionEnd::Stopped),
            "{:?}: {:?}",
            server.kind,
            rest.statuses
        );
        assert_eq!(
            h.budget.used(),
            used_before + run.bytes + rest.bytes,
            "{:?}",
            server.kind
        );
        assert_eq!(h.run_ok(&["SELECT 5"]).text(0, 0), "5", "{:?}", server.kind);
    }
}

#[test]
fn the_time_limit_stops_a_statement() {
    for server in servers() {
        let settings = Settings {
            sql_timeout: Some(Duration::from_millis(200)),
            ..sessions(1)
        };
        let mut h = Harness::open(&server, settings);
        let started = Instant::now();
        let run = h.run(&[&server.sleep(30)]);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            server.kind
        );
        assert_eq!(
            run.end,
            Some(ExecutionEnd::TimedOut),
            "{:?}: {run:?}",
            server.kind
        );
    }
}

#[test]
fn a_transaction_by_sql_keeps_the_session_until_commit() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(2));
        h.run_ok(&["DROP TABLE IF EXISTS t_tx", "CREATE TABLE t_tx (a int)"]);
        let first = h.run_ok(&["BEGIN", server.backend_id(), "INSERT INTO t_tx VALUES (1)"]);
        assert_eq!(
            first.transaction,
            Some(TransactionState::Open),
            "{:?}",
            server.kind
        );
        assert_eq!(h.manager.usage(h.connection).0, 1);
        let second = h.run_ok(&[server.backend_id()]);
        assert_eq!(first.text(0, 0), second.text(0, 0), "{:?}", server.kind);

        assert_eq!(
            h.control(TransactionCommand::Commit),
            TransactionState::None
        );
        assert_eq!(h.count("t_tx"), "1");

        h.run_ok(&["BEGIN", "INSERT INTO t_tx VALUES (2)"]);
        assert_eq!(
            h.control(TransactionCommand::Rollback),
            TransactionState::None
        );
        assert_eq!(h.count("t_tx"), "1");
        h.run_ok(&["DROP TABLE t_tx"]);
    }
}

#[test]
fn manual_mode_opens_a_transaction_before_the_first_statement() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&[
            "DROP TABLE IF EXISTS t_manual",
            "CREATE TABLE t_manual (a int)",
        ]);
        let execution = h.start(&["INSERT INTO t_manual VALUES (1)"], true);
        let run = h.collect(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Completed));
        assert_eq!(
            run.transaction,
            Some(TransactionState::Open),
            "{:?}",
            server.kind
        );
        assert_eq!(
            h.control(TransactionCommand::Rollback),
            TransactionState::None
        );
        assert_eq!(h.count("t_manual"), "0");
        h.run_ok(&["DROP TABLE t_manual"]);
    }
}

#[test]
fn a_commit_by_sql_ends_the_transaction_in_the_script() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let run = h.run_ok(&["BEGIN", "COMMIT", "SELECT 1"]);
        assert_eq!(
            run.transaction,
            Some(TransactionState::None),
            "{:?}",
            server.kind
        );
        assert_eq!(h.manager.usage(h.connection).0, 0);
    }
}

#[test]
fn mysql_ddl_ends_the_transaction_with_an_implicit_commit() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "DROP TABLE IF EXISTS t_implicit",
        "CREATE TABLE t_implicit (a int)",
    ]);
    let open = h.run_ok(&["BEGIN", "INSERT INTO t_implicit VALUES (1)"]);
    assert_eq!(open.transaction, Some(TransactionState::Open));
    let ddl = h.run_ok(&["CREATE TABLE IF NOT EXISTS t_implicit_2 (a int)"]);
    assert_eq!(ddl.transaction, Some(TransactionState::None));
    h.run_ok(&["ROLLBACK"]);
    assert_eq!(h.count("t_implicit"), "1");
    h.run_ok(&["DROP TABLE t_implicit, t_implicit_2"]);
}

#[test]
fn a_failed_transaction_in_postgres_accepts_only_a_rollback() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "DROP TABLE IF EXISTS t_failed",
        "CREATE TABLE t_failed (a int)",
    ]);
    h.run_ok(&["BEGIN", "INSERT INTO t_failed VALUES (1)"]);
    let failed = h.run(&["SELECT * FROM nowhere"]);
    assert_eq!(failed.end, Some(ExecutionEnd::Failed));
    assert!(failed.error().contains("42P01"), "{failed:?}");
    assert_eq!(failed.transaction, Some(TransactionState::Failed));

    // The first error stays the error of the statement, in the prepare step too.
    let next = h.run(&["SELECT 1"]);
    assert!(next.error().contains("25P02"), "{next:?}");

    h.manager.transaction(h.console, TransactionCommand::Commit);
    loop {
        match h.next() {
            ExecEvent::TransactionFailed { message, .. } => {
                assert!(message.contains("rollback"), "{message}");
                break;
            }
            ExecEvent::TransactionChanged { state, .. } => {
                assert_eq!(state, TransactionState::Failed);
            }
            _ => {}
        }
    }

    let rollback = h.run_ok(&["ROLLBACK"]);
    assert_eq!(rollback.transaction, Some(TransactionState::None));
    assert_eq!(h.count("t_failed"), "0");
    h.run_ok(&["DROP TABLE t_failed"]);
}

#[test]
fn a_failed_transaction_in_a_read_only_session_accepts_a_rollback() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let settings = Settings {
        read_only: true,
        ..Settings::default()
    };
    let mut h = Harness::open(&server, settings);
    h.run_ok(&["BEGIN"]);
    assert_eq!(
        h.run(&["SELECT * FROM nowhere"]).transaction,
        Some(TransactionState::Failed)
    );
    let rollback = h.run_ok(&["ROLLBACK"]);
    assert_eq!(rollback.transaction, Some(TransactionState::None));
}

#[test]
fn an_error_in_a_transaction_keeps_its_own_message() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&["BEGIN"]);
    let run = h.run(&["SELEC 1"]);
    assert!(run.error().contains("42601"), "{run:?}");
    assert_eq!(
        h.control(TransactionCommand::Rollback),
        TransactionState::None
    );

    // A syntax error reads the same in the two protocols. This error does not: a
    // second try in the failed transaction would report only the failed transaction.
    h.run_ok(&["BEGIN"]);
    let run = h.run(&["SELECT * FROM nowhere"]);
    assert!(run.error().contains("42P01"), "{run:?}");
    assert_eq!(
        h.control(TransactionCommand::Rollback),
        TransactionState::None
    );
}

#[test]
fn the_cleanup_removes_session_state_between_executions() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(1));
        let (set, read) = match server.kind {
            Kind::Postgres => ("SET application_name = 'dirty'", "SHOW application_name"),
            Kind::MySql => ("SET @dirty = 'dirty'", "SELECT @dirty"),
        };
        let first = h.run_ok(&[
            server.backend_id(),
            set,
            "CREATE TEMPORARY TABLE t_temp (a int)",
        ]);
        let second = h.run(&[server.backend_id(), read, "SELECT * FROM t_temp"]);
        assert_eq!(
            first.text(0, 0),
            second.text(0, 0),
            "the same session: {:?}",
            server.kind
        );
        assert_ne!(
            second.rows[1][0],
            Value::Text("dirty".into()),
            "{:?}",
            server.kind
        );
        assert_eq!(
            second.end,
            Some(ExecutionEnd::Failed),
            "{:?}: {second:?}",
            server.kind
        );
    }
}

#[test]
fn a_script_keeps_one_session_for_its_statements() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(3));
        let run = h.run_ok(&[
            "CREATE TEMPORARY TABLE t_script (a int)",
            "INSERT INTO t_script VALUES (1)",
            "SELECT * FROM t_script",
        ]);
        assert_eq!(run.rows.len(), 1, "{:?}", server.kind);
    }
}

#[test]
fn read_only_is_applied_at_each_reservation() {
    for server in servers() {
        let mut writer = Harness::open(&server, Settings::default());
        writer.run_ok(&[
            "DROP TABLE IF EXISTS t_read_only",
            "CREATE TABLE t_read_only (a int)",
        ]);
        let settings = Settings {
            read_only: true,
            ..sessions(1)
        };
        let mut h = Harness::open(&server, settings);
        for _ in 0..2 {
            let run = h.run(&["INSERT INTO t_read_only VALUES (1)"]);
            assert_eq!(
                run.end,
                Some(ExecutionEnd::Failed),
                "{:?}: {run:?}",
                server.kind
            );
        }
        assert_eq!(h.count("t_read_only"), "0");
        writer.run_ok(&["DROP TABLE t_read_only"]);
    }
}

#[test]
fn the_mysql_state_read_keeps_a_read_only_session() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "DROP TABLE IF EXISTS t_probe",
        "CREATE TABLE t_probe (a int)",
    ]);
    let run = h.run(&[
        "SET SESSION TRANSACTION READ ONLY",
        "SELECT 1",
        "INSERT INTO t_probe VALUES (1)",
    ]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed), "{run:?}");
    assert!(run.error().contains("1792"), "{run:?}");
    assert_eq!(h.count("t_probe"), "0");
    h.run_ok(&["DROP TABLE t_probe"]);
}

#[test]
fn the_mysql_state_read_keeps_the_settings_of_the_next_transaction() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&["DROP TABLE IF EXISTS t_next", "CREATE TABLE t_next (a int)"]);
    let run = h.run(&[
        "SET TRANSACTION READ ONLY",
        "BEGIN",
        "INSERT INTO t_next VALUES (1)",
    ]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed), "{run:?}");
    assert!(run.error().contains("1792"), "{run:?}");
    h.run(&["ROLLBACK"]);
    h.run_ok(&["DROP TABLE t_next"]);
}

#[test]
fn the_mysql_state_read_keeps_the_answer_of_the_last_statement() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run_ok(&[
        "DROP TABLE IF EXISTS t_answer",
        "CREATE TABLE t_answer (a int)",
        "INSERT INTO t_answer VALUES (1), (2), (3)",
        "SELECT ROW_COUNT()",
        "SELECT CAST('x' AS SIGNED)",
        "SHOW WARNINGS",
        "DROP TABLE t_answer",
    ]);
    assert_eq!(run.text(0, 0), "3");
    assert_eq!(run.text(2, 1), "1292", "{run:?}");
}

#[test]
fn the_mysql_state_follows_each_kind_of_statement() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, sessions(1));
    h.run_ok(&[
        "DROP TABLE IF EXISTS t_state",
        "CREATE TABLE t_state (a int) ENGINE=InnoDB",
    ]);
    assert_eq!(
        h.run_ok(&["BEGIN"]).transaction,
        Some(TransactionState::Open)
    );
    let read = h.run_ok(&["SELECT * FROM t_state"]);
    assert_eq!(read.transaction, None, "a read keeps the open state");
    let commit = h.run_ok(&["COMMIT", "SELECT 1"]);
    assert_eq!(commit.transaction, Some(TransactionState::None));

    let set = h.run_ok(&["SET autocommit = 0", "INSERT INTO t_state VALUES (1)"]);
    assert_eq!(set.transaction, Some(TransactionState::Open), "{set:?}");
    assert_eq!(
        h.control(TransactionCommand::Rollback),
        TransactionState::None
    );

    // An error packet has no state, so the connector asks the server.
    h.run_ok(&["BEGIN"]);
    let failed = h.run(&["SELECT * FROM nowhere"]);
    assert_eq!(failed.end, Some(ExecutionEnd::Failed));
    assert_eq!(
        failed.transaction, None,
        "the transaction stays open: {failed:?}"
    );
    assert_eq!(h.manager.usage(h.connection).0, 1);
    assert_eq!(
        h.control(TransactionCommand::Rollback),
        TransactionState::None
    );
    h.run_ok(&["DROP TABLE t_state"]);
}

#[test]
fn a_deadlock_that_ends_the_transaction_reaches_the_state() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut first = Harness::open(&server, Settings::default());
    let mut second = Harness::open(&server, Settings::default());
    first.run_ok(&[
        "DROP TABLE IF EXISTS t_deadlock",
        "CREATE TABLE t_deadlock (id int PRIMARY KEY, a int) ENGINE=InnoDB",
        "INSERT INTO t_deadlock VALUES (1, 0), (2, 0)",
    ]);
    first.run_ok(&["BEGIN", "UPDATE t_deadlock SET a = 1 WHERE id = 1"]);
    second.run_ok(&["BEGIN", "UPDATE t_deadlock SET a = 2 WHERE id = 2"]);
    let waiting = first.start(&["UPDATE t_deadlock SET a = 1 WHERE id = 2"], false);
    std::thread::sleep(Duration::from_millis(200));
    let victim = second.run(&["UPDATE t_deadlock SET a = 2 WHERE id = 1"]);
    assert!(victim.error().contains("1213"), "{victim:?}");
    assert_eq!(
        victim.transaction,
        Some(TransactionState::None),
        "{victim:?}"
    );
    first.collect(waiting);
    assert_eq!(
        first.control(TransactionCommand::Rollback),
        TransactionState::None
    );
    first.run_ok(&["DROP TABLE t_deadlock"]);
}

#[test]
fn the_catalog_lists_relations_and_the_current_schema() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&[
            "DROP VIEW IF EXISTS v_catalog",
            "DROP TABLE IF EXISTS t_catalog",
            "CREATE TABLE t_catalog (id int, full_name text)",
            "CREATE VIEW v_catalog AS SELECT id FROM t_catalog",
        ]);
        h.manager.load_catalog(h.connection, 0);
        let catalog = loop {
            if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
                break catalog.unwrap();
            }
        };
        let current = catalog.current_schema.clone().unwrap();
        let (schema, table) = catalog.relation(None, "t_catalog").unwrap();
        assert_eq!(schema.name, current, "{:?}", server.kind);
        assert_eq!(table.kind, RelationKind::Table);
        assert_eq!(table.columns, Columns::NotLoaded);
        let (_, view) = catalog.relation(None, "v_catalog").unwrap();
        assert_eq!(view.kind, RelationKind::View);
        assert!(!catalog.schemas.iter().any(|schema| {
            schema.name.eq_ignore_ascii_case("information_schema")
                || schema.name == "pg_catalog"
                || schema.name == "mysql"
        }));

        let relation = RelationRef {
            schema: current,
            relation: "t_catalog".into(),
        };
        h.manager.load_columns(h.connection, relation);
        let columns = loop {
            if let ExecEvent::ColumnsLoaded { columns, .. } = h.next() {
                break columns.unwrap();
            }
        };
        assert_eq!(columns, ["id", "full_name"], "{:?}", server.kind);
        h.run_ok(&["DROP VIEW v_catalog", "DROP TABLE t_catalog"]);
    }
}

#[test]
fn call_shows_a_result_for_each_set_in_mysql() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let settings = Settings {
        row_cap: 2,
        ..Settings::default()
    };
    let mut h = Harness::open(&server, settings);
    let run = h.run_ok(&[
        "DROP PROCEDURE IF EXISTS p_sets",
        "CREATE PROCEDURE p_sets() BEGIN \
           SELECT 1 AS a UNION ALL SELECT 2 UNION ALL SELECT 3; \
           SELECT 'x' AS b; \
         END",
        "CALL p_sets()",
        "SELECT 9 AS after_call",
    ]);
    assert_eq!(run.columns, [["a"], ["b"], ["after_call"]]);
    assert_eq!(
        run.results,
        [
            (2, ReadLimit::RowCap),
            (1, ReadLimit::None),
            (1, ReadLimit::None)
        ]
    );
    assert_eq!(run.limits[2], ReadLimit::RowCap);
    h.run_ok(&["DROP PROCEDURE p_sets"]);
}

#[test]
fn copy_with_a_client_stream_is_refused_and_the_session_stays_usable() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, sessions(1));
    h.run_ok(&["DROP TABLE IF EXISTS t_copy", "CREATE TABLE t_copy (a int)"]);
    for sql in ["COPY t_copy FROM STDIN", "COPY t_copy TO STDOUT"] {
        let run = h.run(&[sql]);
        assert_eq!(run.end, Some(ExecutionEnd::Failed));
        assert!(run.error().contains("COPY"), "{run:?}");
    }
    assert_eq!(h.run_ok(&["SELECT 5"]).text(0, 0), "5");
    h.run_ok(&["DROP TABLE t_copy"]);
}

#[test]
fn explain_uses_the_common_grid() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let run = h.run_ok(&["EXPLAIN SELECT 1"]);
        assert!(!run.columns[0].is_empty(), "{:?}", server.kind);
        assert!(!run.rows.is_empty());
    }
}

#[test]
fn an_error_during_the_read_keeps_the_rows_and_their_budget_entry() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(
        &server,
        Settings {
            row_cap: usize::MAX,
            ..Settings::default()
        },
    );
    let run = h.run(&["SELECT 1 / (450 - g) FROM generate_series(1, 1000) g"]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert!(run.error().contains("22012"), "{run:?}");
    assert_eq!(run.rows.len(), 449);
    assert_eq!(run.results, [(449, ReadLimit::None)]);
    assert_eq!(h.budget.used(), run.bytes);
}

#[test]
fn an_idle_session_that_stopped_gets_a_replacement_without_an_error() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(1));
        let id = h.run_ok(&[server.backend_id()]).text(0, 0);
        let mut admin = Harness::open(&server, sessions(1));
        admin.run_ok(&[&server.kill(&id)]);
        std::thread::sleep(Duration::from_millis(200));
        let next = h.run_ok(&[server.backend_id()]);
        assert_ne!(next.text(0, 0), id, "{:?}", server.kind);
    }
}

#[test]
fn a_lost_transaction_becomes_unknown_and_frees_the_session() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(1));
        let id = h.run_ok(&["BEGIN", server.backend_id()]).text(0, 0);
        let mut admin = Harness::open(&server, sessions(1));
        admin.run_ok(&[&server.kill(&id)]);
        std::thread::sleep(Duration::from_millis(200));

        let lost = h.run(&["SELECT 1"]);
        assert_eq!(
            lost.end,
            Some(ExecutionEnd::Failed),
            "{:?}: {lost:?}",
            server.kind
        );
        assert_eq!(
            lost.transaction,
            Some(TransactionState::Unknown),
            "{:?}",
            server.kind
        );
        assert_eq!(h.manager.usage(h.connection).0, 0, "{:?}", server.kind);

        // The application asks for a confirmation, and then the next run gets a session.
        let next = h.run_ok(&[server.backend_id()]);
        assert_ne!(next.text(0, 0), id, "{:?}", server.kind);
    }
}

#[test]
fn a_statement_on_a_lost_connection_reports_an_error() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(1));
        let id = h.run_ok(&[server.backend_id()]).text(0, 0);
        let mut admin = Harness::open(&server, sessions(1));
        let execution = h.start(&[&server.sleep(30), "SELECT 1"], false);
        std::thread::sleep(Duration::from_millis(300));
        // The cleanup of the first run returned the same connection to this run.
        admin.run_ok(&[&server.kill(&id)]);
        let run = h.collect(execution);
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Failed),
            "{:?}: {run:?}",
            server.kind
        );
        assert_eq!(run.statuses[1], StatementStatus::NotExecuted);
        assert_eq!(
            run.transaction, None,
            "no transaction was open: {:?}",
            server.kind
        );
        assert_eq!(h.manager.usage(h.connection).0, 0, "{:?}", server.kind);
        assert_eq!(h.run_ok(&["SELECT 5"]).text(0, 0), "5", "{:?}", server.kind);
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

/// Returns a sink for a test that reads only the result of `run`.
fn sink() -> ResultSink {
    sink_with_cap(1000)
}

fn sink_with_cap(cap: usize) -> ResultSink {
    let (events, inbox) = tokio::sync::mpsc::channel(1024);
    std::mem::forget(inbox);
    let key = StatementKey {
        console: 1,
        execution: 1,
        index: 0,
    };
    ResultSink::new(key, events, Arc::new(Budget::new(1 << 30)), cap)
}

async fn one_value(session: &mut Box<dyn Session>, sql: &str) -> Result<(), String> {
    session
        .run(sql, sink())
        .await
        .map(|_| ())
        .map_err(|error| error.message)
}

#[test]
fn a_late_cancel_does_not_reach_the_next_statement() {
    for server in servers() {
        let connector = connector(&server);
        runtime().block_on(async {
            let mut session = connector.connect().await.unwrap();
            // The next statement leaves at once: `cancel` returns after the server
            // handled the request, so the request cannot reach that statement.
            for _ in 0..200 {
                one_value(&mut session, "SELECT 1").await.unwrap();
                session.canceller().cancel().await.unwrap();
                let next = one_value(&mut session, &server.sleep(0)).await;
                assert_eq!(next, Ok(()), "{:?}", server.kind);
                assert_ne!(session.transaction_state().await, TransactionState::Unknown);
            }
        });
    }
}

#[test]
fn the_row_cap_of_a_short_read_does_not_reach_the_next_statement() {
    for server in servers() {
        let settings = Settings {
            row_cap: 50,
            ..sessions(1)
        };
        let mut h = Harness::open(&server, settings);
        h.run_ok(&[
            "DROP TABLE IF EXISTS t_cap_next",
            "CREATE TABLE t_cap_next (a int)",
        ]);
        let values = (0..80)
            .map(|x| format!("({x})"))
            .collect::<Vec<_>>()
            .join(",");
        h.run_ok(&[&format!("INSERT INTO t_cap_next VALUES {values}")]);
        // A result a little above the cap ends near the moment of the cancel request.
        for _ in 0..100 {
            let run = h.run(&[
                "SELECT a FROM t_cap_next",
                "UPDATE t_cap_next SET a = a + 1",
            ]);
            assert_eq!(
                run.end,
                Some(ExecutionEnd::Completed),
                "{:?}: {:?}",
                server.kind,
                run.statuses
            );
        }
        h.run_ok(&["DROP TABLE t_cap_next"]);
    }
}

#[test]
fn closing_a_mysql_session_closes_its_socket_during_a_read() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let connector = connector(&server);
    let mut h = Harness::open(&server, Settings::default());
    server.prepare_huge_read(&mut h);
    // The comment names this statement in the process list of the server.
    let sql = format!("{} /* closed read */", server.huge_read());
    let processes =
        format!("SELECT count(*) FROM information_schema.PROCESSLIST WHERE INFO = '{sql}'");
    // The runtime stays alive during the check: the end of a runtime also closes the
    // sockets of its tasks, and that would hide a task that keeps this socket open.
    let runtime = runtime();
    runtime.block_on(async {
        let mut session = connector.connect().await.unwrap();
        let run = session.run(&sql, sink_with_cap(usize::MAX));
        assert!(
            tokio::time::timeout(Duration::from_millis(500), run)
                .await
                .is_err()
        );
        assert_eq!(
            h.run_ok(&[&processes]).text(0, 0),
            "1",
            "the statement must have started"
        );
        drop(session);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.run_ok(&[&processes]).text(0, 0) != "0" {
        assert!(
            Instant::now() < deadline,
            "the statement of the closed session continues"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_transaction_commit_failure_on_a_lost_connection_is_reported() {
    for server in servers() {
        let mut h = Harness::open(&server, sessions(1));
        let id = h.run_ok(&["BEGIN", server.backend_id()]).text(0, 0);
        let mut admin = Harness::open(&server, sessions(1));
        admin.run_ok(&[&server.kill(&id)]);
        std::thread::sleep(Duration::from_millis(200));
        h.manager.transaction(h.console, TransactionCommand::Commit);
        let mut failed = false;
        let mut state = None;
        while !failed || state.is_none() {
            match h.next() {
                ExecEvent::TransactionFailed { .. } => failed = true,
                ExecEvent::TransactionChanged { state: changed, .. } => state = Some(changed),
                _ => {}
            }
        }
        assert_eq!(state, Some(TransactionState::Unknown), "{:?}", server.kind);
        assert_eq!(h.manager.usage(h.connection).0, 0, "{:?}", server.kind);
    }
}

#[test]
fn a_wrong_password_fails_at_once_and_shows_no_password() {
    for server in servers() {
        let uri = server.uri.replacen(":spike@", ":wrong-secret-value@", 1);
        let bad = Server {
            kind: server.kind,
            uri,
        };
        let mut h = Harness::open(&bad, Settings::default());
        let execution = h.start(&["SELECT 1"], false);
        loop {
            if let ExecEvent::ExecutionFinished {
                execution: finished,
                end,
                message,
                ..
            } = h.next()
                && finished == execution
            {
                assert_eq!(end, ExecutionEnd::Failed);
                let message = message.unwrap();
                println!("{:?}: {message}", server.kind);
                assert!(!message.contains("wrong-secret-value"), "{message}");
                break;
            }
        }
    }
}

#[test]
fn a_silent_server_ends_at_the_connection_time_limit() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    for scheme in ["postgres", "mysql"] {
        let uri = format!("{scheme}://u:p@127.0.0.1:{port}/d");
        let settings = Settings {
            connect_timeout: Duration::from_millis(300),
            ..Settings::default()
        };
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let connector = network_connector(parse_uri(&uri).unwrap());
        let (mut manager, mut events) = Manager::new(budget).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        let started = Instant::now();
        manager.run(console, vec!["SELECT 1".to_owned()], RunOptions::default());
        loop {
            if let Ok(ExecEvent::ExecutionFinished { end, message, .. }) = events.try_recv() {
                assert_eq!(end, ExecutionEnd::Failed);
                assert!(message.unwrap().contains("time limit"));
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(3), "{scheme}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

struct TlsServer {
    scheme: &'static str,
    port: u16,
    user_key: &'static str,
}

fn tls_servers() -> Vec<TlsServer> {
    let port = |name: &str| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
    };
    let mut found = Vec::new();
    if let Some(port) = port("SQLN_PG_TLS_PORT") {
        found.push(TlsServer {
            scheme: "postgres",
            port,
            user_key: "client.pk8.key",
        });
    }
    if let Some(port) = port("SQLN_MYSQL_TLS_PORT") {
        found.push(TlsServer {
            scheme: "mysql",
            port,
            user_key: "client.rsa.key",
        });
    }
    found
}

fn certs() -> Option<std::path::PathBuf> {
    let folder = std::env::var("SQLN_CERTS")
        .ok()
        .map(std::path::PathBuf::from);
    folder.filter(|folder| folder.join("ca.crt").exists() && std::env::var("SQLN_TEST_TLS").is_ok())
}

fn try_connect(uri: &str) -> Result<Box<dyn Session>, String> {
    let connector = network_connector(parse_uri(uri).unwrap());
    runtime().block_on(async {
        let session = connector.connect().await;
        session.map_err(|error| error.message)
    })
}

fn user_of(server: &TlsServer) -> &'static str {
    if server.scheme == "postgres" {
        "postgres"
    } else {
        "root"
    }
}

#[test]
fn tls_verifies_the_certificate_and_the_name() {
    let Some(folder) = certs() else {
        println!("skipped: SQLN_TEST_TLS is not set");
        return;
    };
    let ca = folder.join("ca.crt");
    let other = folder.join("other-ca.crt");
    for server in tls_servers() {
        let TlsServer { scheme, port, .. } = server;
        let user = user_of(&server);
        let good = format!(
            "{scheme}://{user}:spike@localhost:{port}/spike?sslmode=verify-full&sslrootcert={}",
            ca.display()
        );
        let runtime = runtime();
        let connector = network_connector(parse_uri(&good).unwrap());
        runtime.block_on(async {
            let mut session = connector.connect().await.unwrap();
            let sql = if scheme == "postgres" {
                "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()".to_owned()
            } else {
                "SHOW SESSION STATUS LIKE 'Ssl_cipher'".to_owned()
            };
            one_value(&mut session, &sql).await.unwrap();
            // The cancel request uses a new connection with the same TLS rules.
            session.canceller().cancel().await.unwrap();
        });

        let wrong_ca = format!(
            "{scheme}://{user}:spike@localhost:{port}/spike?sslmode=verify-full&sslrootcert={}",
            other.display()
        );
        let error = try_connect(&wrong_ca).err().unwrap();
        println!("{scheme}, wrong CA: {error}");

        let wrong_name = format!(
            "{scheme}://{user}:spike@127.0.0.1:{port}/spike?sslmode=verify-full&sslrootcert={}",
            ca.display()
        );
        let error = try_connect(&wrong_name).err().unwrap();
        println!("{scheme}, wrong name: {error}");
    }
}

#[test]
fn tls_refuses_a_server_without_tls() {
    let Some(folder) = certs() else { return };
    let ca = folder.join("ca.crt");
    for server in servers() {
        let (scheme, port, user) = match server.kind {
            Kind::Postgres => ("postgres", 54329, "postgres"),
            Kind::MySql => ("mysql", 33069, "root"),
        };
        let uri = format!(
            "{scheme}://{user}:spike@localhost:{port}/spike?sslmode=verify-full&sslrootcert={}",
            ca.display()
        );
        let error = try_connect(&uri).err().unwrap();
        println!("{scheme}, server without TLS: {error}");
    }
}

#[test]
fn tls_accepts_a_client_certificate() {
    let Some(folder) = certs() else { return };
    for server in tls_servers() {
        let TlsServer {
            scheme,
            port,
            user_key,
        } = server;
        let uri = format!(
            "{scheme}://certuser@localhost:{port}/spike?sslmode=verify-full&sslrootcert={}&sslcert={}&sslkey={}",
            folder.join("ca.crt").display(),
            folder.join("client.crt").display(),
            folder.join(user_key).display(),
        );
        assert!(try_connect(&uri).is_ok(), "{scheme}");
        let without = format!(
            "{scheme}://certuser@localhost:{port}/spike?sslmode=verify-full&sslrootcert={}",
            folder.join("ca.crt").display(),
        );
        assert!(try_connect(&without).is_err(), "{scheme}");
    }
}

#[test]
fn a_missing_certificate_file_is_a_clear_error() {
    for scheme in ["postgres", "mysql"] {
        let uri = format!(
            "{scheme}://u@db.example.test/d?sslmode=verify-full&sslrootcert=/missing/ca.crt"
        );
        let error = try_connect(&uri).err().unwrap();
        assert!(error.contains("/missing/ca.crt"), "{error}");
    }
}

struct Screen {
    app: App,
}

impl Screen {
    fn open(server: &Server) -> Self {
        let options = Options {
            keyboard: KeyboardMode::Conventional,
            connector: Some(connector(server)),
            label: Some("test server".to_owned()),
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
        }
    }

    /// Opens the server as a connection with a definition, as the form saves it.
    fn with_definition(server: &Server) -> Self {
        let target = parse_uri(&server.uri).unwrap();
        let definition = Definition::new("real", Source::Network(target));
        let options = Options {
            connections: vec![definition],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
        }
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.code(KeyCode::Char(c));
        }
    }

    fn focus_form(&mut self, field: Field) {
        for _ in 0..32 {
            if self.app.form().unwrap().focus == field {
                return;
            }
            self.code(KeyCode::Tab);
        }
        panic!("{field:?} is not in the form");
    }

    fn choose_schema(&mut self, name: Option<&str>) {
        self.prefix('d');
        self.wait_for("default of the connection");
        self.wait_until(|s| {
            let catalog = s.app.connections()[0].catalog.as_ref();
            catalog.is_some() && !s.screen().contains("Reading the catalog")
        });
        let catalog = self.app.connections()[0].catalog.as_ref().unwrap();
        let index = catalog
            .schemas
            .iter()
            .position(|schema| Some(schema.name.as_str()) == name);
        for _ in 0..catalog.schemas.len() + 1 {
            self.code(KeyCode::Up);
        }
        for _ in 0..index.map_or(0, |index| index + 1) {
            self.code(KeyCode::Down);
        }
        self.code(KeyCode::Enter);
    }

    fn wait_until(&mut self, condition: impl Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if condition(self) {
                return;
            }
            let screen = self.screen();
            assert!(
                Instant::now() < deadline,
                "the condition did not happen:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn text(&self) -> String {
        self.app.console().editor.text()
    }

    fn replace_text(&mut self, sql: &str) {
        self.press(ctrl('a'));
        self.press(key(KeyCode::Delete));
        self.app.on_event(Event::Paste(sql.to_owned()));
        assert_eq!(self.text(), sql);
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row.trim_end().to_owned() + "\n"
            })
            .collect()
    }

    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            let screen = self.screen();
            if screen.contains(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{text:?} is not on the screen:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn run_script(&mut self, sql: &str) {
        self.app.console_mut().focus = sqln::console::Focus::Editor;
        self.replace_text(sql);
        self.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
        std::thread::sleep(Duration::from_millis(1));
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.app.console_mut().is_busy() {
            self.app.drain_events();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
    }

    fn prefix(&mut self, c: char) {
        self.press(ctrl('w'));
        self.press(key(KeyCode::Char(c)));
    }
}

#[test]
fn the_application_runs_a_script_and_completes_real_names() {
    for server in servers() {
        let mut s = Screen::open(&server);
        s.wait_for("test server");
        s.run_script(
            "DROP TABLE IF EXISTS app_orders;\n\
             CREATE TABLE app_orders (id int, customer_name varchar(30));\n\
             INSERT INTO app_orders VALUES (1, 'Ana'), (2, 'Bruno');\n\
             SELECT * FROM app_orders ORDER BY id;",
        );
        s.wait_for("customer_name");
        s.wait_for("Bruno");
        s.wait_for("2 rows");

        s.replace_text("SELECT * FROM app_ord");
        s.app.console_mut().focus = sqln::console::Focus::Editor;
        s.press(key(KeyCode::Char('e')));
        s.wait_for("tbl app_orders");
        s.press(key(KeyCode::Esc));

        s.run_script("BEGIN;\nDELETE FROM app_orders;");
        s.wait_for("Transaction open");
        s.prefix('R');
        s.wait_for("No transaction");
        s.run_script("SELECT count(*) AS total FROM app_orders;");
        s.wait_for("total");

        s.run_script("DROP TABLE app_orders;");
        s.app.shutdown();
    }
}

impl Screen {
    /// Opens a table of the first connection and waits for its first page.
    fn open_table(&mut self, name: &str) {
        common::open_table(&mut self.app, 0, name);
        self.wait_table();
    }

    fn wait_table(&mut self) {
        self.wait_until(|s| s.app.table().is_some_and(|tab| !tab.is_busy()));
    }

    /// Replaces the text of the WHERE field.
    fn set_filter(&mut self, text: &str) {
        self.prefix('k');
        let old = self.app.table().unwrap().filter.text().chars().count();
        self.code(KeyCode::End);
        for _ in 0..old {
            self.code(KeyCode::Backspace);
        }
        self.type_text(text);
    }

    fn table_ids(&self) -> Vec<String> {
        let table = &self.app.table().unwrap().result.table;
        table.rows.iter().map(|row| row[0].display()).collect()
    }
}

#[test]
fn a_table_view_pages_filters_and_orders_rows_of_each_server() {
    for server in servers() {
        let mut s = Screen::open(&server);
        s.wait_for("test server");
        let fill = match server.kind {
            Kind::Postgres => {
                "INSERT INTO app_pages SELECT g, 'row' || g FROM generate_series(1, 250) g;"
            }
            Kind::MySql => {
                "INSERT INTO app_pages WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 250) \
                 SELECT x, CONCAT('row', x) FROM n;"
            }
        };
        s.run_script(&format!(
            "DROP TABLE IF EXISTS app_pages;\nCREATE TABLE app_pages (id int, label varchar(30));\n{fill}"
        ));
        s.wait_for("changed");

        s.open_table("app_pages");
        assert_eq!(s.table_ids().len(), 100);
        assert!(s.app.table().unwrap().has_more);
        s.prefix('k');
        s.code(KeyCode::Tab);
        s.type_text("id ASC");
        s.code(KeyCode::Enter);
        s.wait_table();
        s.prefix('j');
        s.code(KeyCode::Char('m'));
        s.wait_table();
        assert_eq!(s.table_ids().len(), 200);
        assert_eq!(s.app.table().unwrap().result.view.row, 0);
        s.wait_for("Page 1 of 2+");

        // The next loaded page needs no load. The last page loads before the move.
        s.code(KeyCode::Char(']'));
        assert_eq!(s.app.table().unwrap().result.view.row, 100);
        s.wait_for("Page 2 of 2+");
        s.code(KeyCode::Char(']'));
        s.wait_table();
        assert_eq!(s.table_ids().len(), 250);
        assert!(!s.app.table().unwrap().has_more);
        assert_eq!(s.app.table().unwrap().result.view.row, 200);
        assert_eq!(s.table_ids()[200], "201");
        s.wait_for("Page 3 of 3");
        s.code(KeyCode::Char('['));
        assert_eq!(s.app.table().unwrap().result.view.row, 100);
        s.code(KeyCode::Char(']'));
        s.code(KeyCode::Char(']'));
        assert_eq!(s.app.table().unwrap().result.view.row, 200);
        assert!(!s.app.table().unwrap().is_busy());

        s.prefix('k');
        s.code(KeyCode::Tab);
        s.code(KeyCode::End);
        for _ in 0.."id ASC".len() {
            s.code(KeyCode::Backspace);
        }

        // The filter and the order run in the database.
        s.prefix('k');
        s.type_text("id % 50 = 0 -- a note");
        s.code(KeyCode::Tab);
        s.type_text("id DESC");
        s.code(KeyCode::Enter);
        s.wait_table();
        assert_eq!(s.table_ids(), ["250", "200", "150", "100", "50"]);

        // A text with a second statement never reaches the server.
        s.set_filter("id = 1; DROP TABLE app_pages");
        s.code(KeyCode::Enter);
        s.wait_for("must stay in one statement");
        assert!(s.table_ids().is_empty());

        // A server error clears the grid and shows its text.
        s.set_filter("no_such_column = 1");
        s.code(KeyCode::Enter);
        s.wait_table();
        assert!(s.table_ids().is_empty());
        s.wait_for("no_such_column");

        // The sort key keeps the applied filter, and the order follows the column.
        s.set_filter("");
        s.code(KeyCode::Enter);
        s.wait_table();
        s.prefix('j');
        s.code(KeyCode::Right);
        s.code(KeyCode::Char('s'));
        s.wait_table();
        assert_eq!(s.app.table().unwrap().order.text(), "label ASC");
        assert_eq!(&s.table_ids()[..3], ["1", "10", "100"]);
        s.app.shutdown();

        let mut s = Screen::open(&server);
        s.run_script("DROP TABLE app_pages;");
        s.app.shutdown();
    }
}

#[test]
fn a_table_view_quotes_the_names_of_each_server() {
    for server in servers() {
        let mut s = Screen::open(&server);
        s.wait_for("test server");
        let (create, insert, drop) = match server.kind {
            Kind::Postgres => (
                "CREATE TABLE \"App Items\" (\"select\" int, \"Mixed Case\" text)",
                "INSERT INTO \"App Items\" VALUES (1, 'a'), (2, 'b')",
                "DROP TABLE \"App Items\"",
            ),
            Kind::MySql => (
                "CREATE TABLE `App Items` (`select` int, `Mixed Case` text)",
                "INSERT INTO `App Items` VALUES (1, 'a'), (2, 'b')",
                "DROP TABLE `App Items`",
            ),
        };
        let drop_first = drop.replace("DROP TABLE", "DROP TABLE IF EXISTS");
        s.run_script(&format!("{drop_first};\n{create};\n{insert};"));
        s.wait_for("changed");
        s.open_table("App Items");
        assert_eq!(s.table_ids(), ["1", "2"]);
        s.wait_for("Mixed Case");
        s.prefix('k');
        s.type_text("sel");
        // The list shows the keywords at once, and the columns after their read.
        s.wait_for("col select");
        s.code(KeyCode::Tab);
        // The list quotes a word that the database reserves.
        let text = s.app.table().unwrap().filter.text().to_owned();
        assert!(text.contains('"') || text.contains('`'), "{text}");
        s.type_text(" = 2");
        s.code(KeyCode::Enter);
        s.wait_table();
        assert_eq!(s.table_ids(), ["2"]);
        s.app.shutdown();

        let mut s = Screen::open(&server);
        s.run_script(&format!("{drop};"));
        s.app.shutdown();
    }
}

#[test]
fn the_mysql_delimiter_creates_a_routine_from_the_editor() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut s = Screen::open(&server);
    s.run_script(
        "DROP PROCEDURE IF EXISTS app_delimited;\n\
         DELIMITER $$\n\
         CREATE PROCEDURE app_delimited()\n\
         BEGIN\n\
           SELECT 'one' AS first_set;\n\
           SELECT 'two' AS second_set;\n\
         END$$\n\
         DELIMITER ;\n\
         CALL app_delimited();",
    );
    s.wait_for("Result 2");
    s.wait_for("second_set");
    s.press(key(KeyCode::Char('[')));
    s.prefix('j');
    s.press(key(KeyCode::Char('[')));
    s.wait_for("first_set");
    s.run_script("DROP PROCEDURE app_delimited;");
    s.app.shutdown();
}

fn create_namespace(server: &Server) -> &'static str {
    match server.kind {
        Kind::Postgres => "CREATE SCHEMA IF NOT EXISTS app_choice",
        Kind::MySql => "CREATE DATABASE IF NOT EXISTS app_choice",
    }
}

#[test]
fn the_schema_choice_changes_where_a_name_resolves() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script(&format!(
            "{};\n\
             DROP TABLE IF EXISTS app_choice.only_there;\n\
             CREATE TABLE app_choice.only_there (a int);\n\
             INSERT INTO app_choice.only_there VALUES (41);",
            create_namespace(&server),
        ));
        s.wait_until(|s| s.app.connections()[0].catalog.is_some());
        s.choose_schema(Some("app_choice"));
        s.wait_for("real › app_choice › console-1");

        let query = "SELECT a + 1 AS shown FROM only_there;";
        s.run_script(query);
        s.wait_for("42");

        // A new physical connection receives the same choice.
        s.prefix('h');
        s.code(KeyCode::Char('D'));
        s.wait_for("Disconnected.");
        s.prefix('l');
        s.run_script(query);
        s.wait_for("42");

        s.choose_schema(None);
        s.run_script(query);
        s.wait_for("only_there");
        assert!(s.screen().contains('✗'), "{}", s.screen());
        s.app.shutdown();
    }
}

#[test]
fn the_control_of_the_schema_uses_the_word_of_the_database() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        let word = match server.kind {
            Kind::Postgres => "Schema: [-]",
            Kind::MySql => "Database: [-]",
        };
        s.wait_for(word);
    }
}

#[test]
fn the_navigator_reads_a_real_catalog() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script(&format!(
            "{};\n\
             DROP TABLE IF EXISTS app_choice.tree_table;\n\
             CREATE TABLE app_choice.tree_table (first_column int);",
            create_namespace(&server),
        ));
        s.prefix('h');
        s.code(KeyCode::Right);
        s.wait_until(|s| s.app.connections()[0].catalog.is_some());
        let catalog = s.app.connections()[0].catalog.as_ref().unwrap();
        let index = catalog
            .schemas
            .iter()
            .position(|item| item.name == "app_choice")
            .unwrap();
        for _ in 0..=index {
            s.code(KeyCode::Down);
        }
        s.code(KeyCode::Right);
        s.wait_for("tree_table");
        s.code(KeyCode::Down);
        s.code(KeyCode::Right);
        s.wait_for("first_column");
        s.app.shutdown();
    }
}

#[test]
fn a_connected_definition_changes_only_its_name() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script("SELECT 1;");
        s.wait_for("● real");
        s.prefix('h');
        s.code(KeyCode::Char('e'));
        assert!(s.app.form().unwrap().display_only);
        s.wait_for("This connection is connected.");
        s.type_text("-renamed");
        s.focus_form(Field::Save);
        s.code(KeyCode::Enter);
        assert!(s.app.form().is_none());
        s.wait_for("● real-renamed");
        assert_eq!(
            s.app.connections()[0].definition.as_ref().unwrap().name,
            "real-renamed"
        );
        s.app.shutdown();
    }
}

#[test]
fn the_form_of_a_connected_definition_disconnects_and_changes_the_pool() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script("SELECT 1;");
        s.wait_for("● real");
        s.prefix('h');
        s.code(KeyCode::Char('e'));
        s.focus_form(Field::Disconnect);
        s.code(KeyCode::Enter);
        s.wait_until(|s| !s.app.form().unwrap().display_only);
        s.focus_form(Field::Pool);
        s.code(KeyCode::Backspace);
        s.type_text("2");
        s.focus_form(Field::Save);
        s.code(KeyCode::Enter);
        assert!(s.app.form().is_none(), "{}", s.screen());
        s.prefix('l');
        s.run_script("SELECT 7 AS seven;");
        s.wait_for("seven");
        s.wait_for("Pool 0/2");
        s.app.shutdown();
    }
}

#[test]
fn the_disconnect_waits_for_a_transaction() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script("BEGIN;");
        s.wait_for("Transaction open");
        s.prefix('h');
        s.code(KeyCode::Char('D'));
        s.wait_for("console-1 has an execution or a transaction");
        s.wait_for("Transaction open");
        s.prefix('l');
        s.prefix('R');
        s.wait_for("No transaction");
        s.app.shutdown();
    }
}

#[test]
fn two_consoles_keep_separate_transactions() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script(
            "DROP TABLE IF EXISTS app_two;\n\
             CREATE TABLE app_two (a int);",
        );
        s.run_script("BEGIN;\nINSERT INTO app_two VALUES (1);");
        s.wait_for("Transaction open");
        s.prefix('h');
        s.code(KeyCode::Char('n'));
        assert_eq!(s.app.consoles().len(), 2);
        s.run_script("SELECT count(*) AS other_view FROM app_two;");
        s.wait_for("other_view");
        s.wait_for("No transaction");
        s.prefix('p');
        s.wait_for("Transaction open");
        s.prefix('C');
        s.wait_for("No transaction");
        s.run_script("DROP TABLE app_two;");
        s.app.shutdown();
    }
}

#[test]
fn the_exit_commits_the_answer_of_each_console_on_its_own_session() {
    for server in servers() {
        let mut s = Screen::with_definition(&server);
        s.run_script(
            "DROP TABLE IF EXISTS app_exit;\n\
             CREATE TABLE app_exit (a int);",
        );
        s.run_script("BEGIN;\nINSERT INTO app_exit VALUES (1);");
        s.prefix('h');
        s.code(KeyCode::Char('n'));
        s.run_script("BEGIN;\nINSERT INTO app_exit VALUES (2);");
        s.wait_for("Transaction open");
        s.prefix('q');
        s.wait_for("console-1: This console has an open transaction");
        s.code(KeyCode::Tab);
        s.code(KeyCode::Tab);
        s.code(KeyCode::Enter);
        s.wait_for("console-2: This console has an open transaction");
        s.code(KeyCode::Tab);
        s.code(KeyCode::Enter);
        s.wait_until(|s| s.app.should_quit());
        s.app.shutdown();

        let mut h = Harness::open(&server, Settings::default());
        assert_eq!(
            h.count("app_exit"),
            "1",
            "commit in the first console, rollback in the second"
        );
        h.run_ok(&["DROP TABLE app_exit"]);
    }
}

#[test]
fn the_form_tests_a_real_server_and_a_wrong_password_stays_hidden() {
    for server in servers() {
        let mut s = Screen::open(&server);
        s.prefix('h');
        s.code(KeyCode::Char('a'));
        if server.kind == Kind::MySql {
            s.code(KeyCode::Right);
        }
        s.code(KeyCode::Tab);
        s.type_text("tested");
        s.focus_form(Field::Entry);
        s.code(KeyCode::Right);
        s.focus_form(Field::Uri);
        s.app.on_event(Event::Paste(server.uri.clone()));
        s.focus_form(Field::Test);
        s.code(KeyCode::Enter);
        s.wait_for("The connection works.");

        s.focus_form(Field::Uri);
        for _ in 0..server.uri.chars().count() {
            s.code(KeyCode::Backspace);
        }
        let wrong = server.uri.replace(":spike@", ":hunter2-secret@");
        s.app.on_event(Event::Paste(wrong));
        s.focus_form(Field::Test);
        s.code(KeyCode::Enter);
        s.wait_until(|s| s.app.form().unwrap().test.is_none());
        let screen = s.screen();
        assert!(!screen.contains("hunter2-secret"), "{screen}");
        assert!(!screen.contains("The connection works."), "{screen}");
        s.app.shutdown();
    }
}

#[test]
fn a_client_key_in_another_form_gets_a_clear_message() {
    let path = std::env::temp_dir().join(format!("sqln-key-{}.pem", std::process::id()));
    std::fs::write(
        &path,
        "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n",
    )
    .unwrap();
    let uri = format!(
        "mysql://reader@127.0.0.1:1/app?sslmode=verify-full&sslcert={0}&sslkey={0}",
        path.display(),
    );
    let connector = network_connector(parse_uri(&uri).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let error = runtime.block_on(connector.connect()).err().unwrap();
    assert!(error.message.contains("PKCS#1"), "{error:?}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_change_of_schema_by_sql_ends_with_its_execution() {
    for server in servers() {
        let settings = Settings {
            max_connections: 1,
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        h.run_ok(&[create_namespace(&server)]);
        let (change, read, expected) = match server.kind {
            Kind::Postgres => (
                "SET search_path TO app_choice",
                "SHOW search_path",
                "\"$user\", public",
            ),
            Kind::MySql => ("USE app_choice", "SELECT DATABASE()", "spike"),
        };
        let run = h.run(&[change, read]);
        assert_eq!(run.text(0, 0), "app_choice");
        let backend = h.run(&[server.backend_id()]).text(0, 0);
        assert_eq!(h.run(&[read]).text(0, 0), expected, "{:?}", server.kind);
        assert_eq!(
            h.run(&[server.backend_id()]).text(0, 0),
            backend,
            "the same session stays in use"
        );
    }
}

#[test]
fn a_mysql_session_without_a_database_closes_after_a_use() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let uri = server.uri.replace("/spike", "");
    let definition = Server {
        kind: Kind::MySql,
        uri,
    };
    let settings = Settings {
        max_connections: 1,
        ..Settings::default()
    };
    let mut h = Harness::open(&definition, settings);
    let first = h.run(&["USE spike", server.backend_id()]).text(0, 0);
    let run = h.run(&["SELECT DATABASE()", server.backend_id()]);
    assert!(matches!(run.rows[0][0], Value::Null), "{run:?}");
    assert_ne!(
        run.text(1, 0),
        first,
        "MySQL cannot leave a database, so the session closes"
    );
}
