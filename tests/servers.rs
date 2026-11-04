// Tests with real servers. Start them with the commands of `docs/RESULTS.md`, and
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
use sqln::catalog::{Catalog, Columns, Object, RelationKind, RelationRef};
use sqln::clipboard::MemoryClipboard;
use sqln::compare::data::{DataCompare, Plan, RowChange, plan as data_plan};
use sqln::compare::ddl::script;
use sqln::compare::schema::{Change, SchemaModel, Table, build, diff};
use sqln::compare::sql::{compare_sql, rows_sql, table_columns_sql};
use sqln::connection::{Definition, Source};
use sqln::connector::target::parse_uri;
use sqln::connector::{
    Connector, Session, TransactionCommand, TransactionState, network_connector,
};
use sqln::definition::{Target as DdlTarget, definition_sql, definition_text};
use sqln::diagram::mermaid::to_mermaid;
use sqln::diagram::sql::{diagram_sql, parse_column, parse_key};
use sqln::diagram::{Focus, Model};
use sqln::editor::KeyboardMode;
use sqln::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, Manager, ReadLimit, ResultSink, RunOptions,
    Settings, StatementKey, StatementStatus,
};
use sqln::export::{ExportOptions, ExportWriter};
use sqln::form::Field;
use sqln::grid::{Budget, ResultTable, Row, Value};
use sqln::import::statement::{Target, TargetColumn};
use sqln::import::{ImportFormat, ImportJob, ImportMode, ImportProgress, ReadOptions, log_path};
use sqln::plan::{Explain, PlanView, plan_view};
use sqln::sql::{Dialect, split};
use sqln::structure::{Scope, Section, SessionSignal, signal_sql, structure_sql};
use sqln::ui::text;
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
    notices: Vec<String>,
    commands: Vec<String>,
}

impl Run {
    fn text(&self, row: usize, column: usize) -> String {
        match self.rows[row].get(column).unwrap() {
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
                ExecEvent::CommandLog {
                    event: sqln::command_log::LogEvent::Started { text, .. },
                    ..
                } => run.commands.push(text),
                ExecEvent::Columns { columns, .. } => run.columns.push(columns),
                ExecEvent::Rows { rows, bytes, .. } => {
                    run.rows.extend(rows);
                    run.bytes += bytes;
                }
                ExecEvent::ResultDone { rows, limit, .. } => run.results.push((rows, limit)),
                ExecEvent::Notice { text, .. } => run.notices.push(text),
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
fn command_history_captures_effective_sql_cleanup_and_session_reuse() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.manager
            .set_console_history(h.console, 1, "test database".to_owned());
        let run = h.run_ok(&["SELECT 123 AS history_value"]);
        let commands = run.commands.join("\n");
        assert!(
            commands.contains("SELECT 123 AS history_value\nLIMIT 1001"),
            "{commands}"
        );
        assert!(commands.contains("Internal"));
        match server.kind {
            Kind::Postgres => {
                assert!(commands.contains("Parse/Describe"));
                assert!(commands.contains("transaction_timestamp()"));
                assert!(commands.contains("DISCARD TEMP"));
            }
            Kind::MySql => assert!(commands.contains("COM_RESET_CONNECTION")),
        }
        let run = h.run_ok(&["SELECT 456 AS history_value"]);
        let commands = run.commands.join("\n");
        assert!(commands.contains("SELECT 456 AS history_value\nLIMIT 1001"));
        if server.kind == Kind::MySql {
            assert!(commands.contains("COM_PING"));
        }
    }
}

#[test]
fn command_history_captures_internal_cancellation_requests() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.manager
            .set_console_history(h.console, 1, "test database".to_owned());
        let execution = h.start(&[&server.sleep(10)], false);
        h.wait_started();
        // Let the driver enter the command before asking for cancellation.
        std::thread::sleep(Duration::from_millis(100));
        h.manager.stop(h.console);
        let run = h.collect(execution);
        let commands = run.commands.join("\n");
        let expected = match server.kind {
            Kind::Postgres => "PostgreSQL CancelRequest",
            Kind::MySql => "KILL QUERY",
        };
        assert!(commands.contains(expected), "{commands}");
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
        row.get(0).unwrap(),
        Value::Number("12345678901234567890.123456789")
    );
    assert_eq!(row.get(1).unwrap(), Value::Null);
    assert_eq!(row.get(2).unwrap(), Value::Text(""));
    assert_eq!(row.get(3).unwrap(), Value::binary(&[0x00, 0xFF]));
    assert_eq!(row.get(4).unwrap(), Value::Text("2026-10-30 12:34:56.789"));
    assert_eq!(row.get(5).unwrap(), Value::Text("ação 日本"));
    assert_eq!(row.get(6).unwrap(), Value::Text("{1,2}"));
    assert_eq!(row.get(7).unwrap(), Value::Text("{\"a\": 1}"));
    assert_eq!(row.get(8).unwrap(), Value::Number("1.5"));
    assert_eq!(row.get(9).unwrap(), Value::Number("7"));
    assert_eq!(row.get(10).unwrap(), Value::Text("1 day"));
    assert_eq!(row.get(11).unwrap(), Value::Text("t"));
}

#[test]
fn a_text_that_looks_like_binary_stays_text_in_postgres() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run_ok(&["SELECT '\\x00ff'::text"]);
    assert_eq!(run.rows[0].get(0).unwrap(), Value::Text("\\x00ff"));
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
        row.get(0).unwrap(),
        Value::Number("12345678901234567890.123456789")
    );
    assert_eq!(row.get(1).unwrap(), Value::Null);
    assert_eq!(row.get(2).unwrap(), Value::Text(""));
    assert_eq!(row.get(3).unwrap(), Value::binary(&[0x00, 0xFF]));
    assert_eq!(row.get(4).unwrap(), Value::Text("2026-10-30 12:34:56"));
    assert_eq!(row.get(5).unwrap(), Value::Text("ação 日本"));
    assert_eq!(row.get(6).unwrap(), Value::Number("1.50"));
    assert_eq!(row.get(7).unwrap(), Value::Number("7"));
    assert_eq!(row.get(8).unwrap(), Value::Text("{\"a\": 1}"));
    assert_eq!(row.get(9).unwrap(), Value::Text("2026-10-30"));
    assert_eq!(row.get(10).unwrap(), Value::binary(&[0x41]));
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
fn an_idle_session_closes_at_the_idle_limit_and_the_next_run_opens_one() {
    for server in servers() {
        let settings = Settings {
            idle_timeout: Some(Duration::from_millis(200)),
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        h.run_ok(&["SELECT 1"]);
        assert!(h.manager.is_connected(h.connection), "{:?}", server.kind);
        while !matches!(h.next(), ExecEvent::IdleClosed { .. }) {}
        assert!(!h.manager.is_connected(h.connection), "{:?}", server.kind);
        assert_eq!(h.run_ok(&["SELECT 7"]).text(0, 0), "7");
    }
}

/// Without the added `LIMIT`, a transaction reads every row of this result, because a
/// cancellation would end the transaction.
#[test]
fn the_added_limit_ends_a_large_read_in_a_transaction_soon_and_keeps_the_transaction() {
    for server in servers() {
        let settings = Settings {
            row_cap: 100,
            max_connections: 1,
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        server.prepare_huge_read(&mut h);
        let started = Instant::now();
        let execution = h.start(&[server.huge_read()], true);
        let run = h.collect(execution);
        let elapsed = started.elapsed();
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{:?}: {run:?}",
            server.kind
        );
        assert_eq!(run.limits, [ReadLimit::RowCap]);
        assert_eq!(run.rows.len(), 100);
        assert_eq!(run.transaction, Some(TransactionState::Open));
        assert!(
            elapsed < Duration::from_secs(5),
            "{:?} {elapsed:?}",
            server.kind
        );
        assert_eq!(
            h.control(TransactionCommand::Rollback),
            TransactionState::None
        );
        println!("{:?}: added limit in {elapsed:?}", server.kind);
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
            second.rows[1].get(0).unwrap(),
            Value::Text("dirty"),
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
fn the_postgres_cleanup_keeps_the_statements_of_the_driver() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, sessions(1));
    h.run_ok(&[
        "DROP TYPE IF EXISTS t_mood",
        "CREATE TYPE t_mood AS ENUM ('calm')",
        "PREPARE p_dirty AS SELECT 1",
    ]);
    let run = h.run(&["EXECUTE p_dirty"]);
    assert!(run.error().contains("26000"), "{run:?}");
    // The first read of an enum makes the driver keep a statement for the type lookup.
    // The next session uses that statement again.
    for _ in 0..2 {
        h.run_ok(&["SELECT 'calm'::t_mood"]);
    }
    h.run_ok(&["DROP TYPE t_mood"]);
}

#[test]
fn a_user_removal_of_the_prepared_statements_keeps_the_types_readable() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let names = ["t_lost_a", "t_lost_b", "t_lost_c", "t_lost_d", "t_lost_e"];
    let mut h = Harness::open(&server, sessions(1));
    for name in names {
        h.run_ok(&[
            format!("DROP TYPE IF EXISTS {name}").as_str(),
            format!("CREATE TYPE {name} AS ENUM ('calm')").as_str(),
        ]);
    }
    // Each new type needs the type lookup of the driver after the removal.
    h.run_ok(&["SELECT 'calm'::t_lost_a"]);
    h.run_ok(&["DISCARD ALL", "SELECT 'calm'::t_lost_b"]);
    h.run_ok(&["SELECT 'calm'::t_lost_c"]);
    // In a transaction, a failed lookup would also stop the transaction.
    h.run_ok(&[
        "BEGIN",
        "DEALLOCATE ALL",
        "SELECT 'calm'::t_lost_d",
        "COMMIT",
    ]);
    h.run_ok(&["SELECT 'calm'::t_lost_e"]);
    for name in names {
        h.run_ok(&[format!("DROP TYPE {name}").as_str()]);
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
        h.manager.load_catalog(h.connection, 0, Vec::new());
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
fn a_list_of_schemas_keeps_its_schemas_and_the_current_one() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let create = match server.kind {
            Kind::Postgres => "CREATE SCHEMA IF NOT EXISTS",
            Kind::MySql => "CREATE DATABASE IF NOT EXISTS",
        };
        h.run_ok(&[
            &format!("{create} vis_shown"),
            &format!("{create} vis_hidden"),
        ]);
        h.manager
            .load_catalog(h.connection, 0, vec!["vis_shown".to_owned()]);
        let catalog = loop {
            if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
                break catalog.unwrap();
            }
        };
        let current = catalog.current_schema.clone().unwrap();
        let mut found: Vec<&str> = catalog
            .schemas
            .iter()
            .map(|schema| schema.name.as_str())
            .collect();
        found.sort_unstable();
        let mut wanted = vec![current.as_str(), "vis_shown"];
        wanted.sort_unstable();
        assert_eq!(found, wanted, "{:?}", server.kind);

        h.manager
            .test_connection(9, connector(&server), Duration::from_secs(20), true);
        let names = loop {
            if let ExecEvent::ConnectionTested { test: 9, result } = h.next() {
                break result.unwrap().unwrap();
            }
        };
        assert!(names.contains(&"vis_hidden".to_owned()), "{names:?}");
        assert!(names.contains(&current), "{names:?}");
        assert!(
            !names
                .iter()
                .any(|name| name == "pg_catalog" || name.eq_ignore_ascii_case("information_schema")),
            "{names:?}"
        );
    }
}

/// PostGIS puts `geography` in `public`. A console with another schema still finds it.
#[test]
fn a_console_schema_keeps_the_types_of_the_default_path() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "CREATE SCHEMA IF NOT EXISTS path_other",
        "DROP TABLE IF EXISTS path_other.path_t",
        "CREATE TABLE path_other.path_t (v int)",
        "DROP TABLE IF EXISTS public.path_t",
        "CREATE TABLE public.path_t (v int)",
        "INSERT INTO public.path_t VALUES (1)",
        "DROP TYPE IF EXISTS public.path_point",
        "CREATE TYPE public.path_point AS (x int)",
    ]);
    let in_schema = |h: &mut Harness, sql: &str, only_schema: bool| {
        let options = RunOptions {
            schema: Some("path_other".to_owned()),
            only_schema,
            ..RunOptions::default()
        };
        let execution = h.manager.run(h.console, vec![sql.to_owned()], options);
        h.collect(execution)
    };
    let run = in_schema(&mut h, "SELECT NULL::path_point AS v", false);
    assert_eq!(run.end, Some(ExecutionEnd::Completed), "{run:?}");
    // The schema of the console comes first.
    let run = in_schema(&mut h, "SELECT current_schema()", false);
    assert_eq!(run.text(0, 0), "path_other");
    let run = in_schema(&mut h, "SELECT count(*) FROM path_t", false);
    assert_eq!(run.text(0, 0), "0");
    // The compare keeps only its schema in the path.
    let run = in_schema(&mut h, "SELECT NULL::path_point AS v", true);
    assert!(run.error().contains("path_point"), "{run:?}");
    // The cleanup of the session restores the default path.
    assert_eq!(h.count("path_t"), "1");
    h.run_ok(&[
        "DROP TABLE path_other.path_t",
        "DROP TABLE public.path_t",
        "DROP TYPE public.path_point",
    ]);
}

fn load_catalog(h: &mut Harness) -> Catalog {
    h.manager.load_catalog(h.connection, 0, Vec::new());
    loop {
        if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
            break catalog.unwrap();
        }
    }
}

/// Returns the labels of the objects of a schema, in the order of the tree.
fn object_labels(catalog: &Catalog, schema: &str) -> Vec<String> {
    let schema = catalog.schema(schema).expect("the schema");
    schema
        .objects
        .iter()
        .map(|object| object.label.clone())
        .collect()
}

fn find_object<'a>(catalog: &'a Catalog, schema: &str, label: &str) -> &'a Object {
    let schema = catalog.schema(schema).expect("the schema");
    let found = schema.objects.iter().find(|object| object.label == label);
    found.unwrap_or_else(|| panic!("{label} is not in {:?}", schema.objects))
}

/// Reads a DDL as the application does: one execution, with only the schema in the path.
fn read_ddl(h: &mut Harness, dialect: Dialect, target: &DdlTarget) -> String {
    let statements = definition_sql(dialect, target).expect("a DDL query");
    let options = RunOptions {
        row_cap: Some(usize::MAX),
        schema: Some(target.schema.clone()),
        only_schema: true,
        ..RunOptions::default()
    };
    let execution = h.manager.run(h.console, statements, options);
    let mut results: Vec<Vec<Row>> = Vec::new();
    loop {
        match h.next() {
            ExecEvent::Rows { key, rows, bytes } if key.execution == execution => {
                h.budget.release(bytes);
                if results.len() <= key.index {
                    results.resize_with(key.index + 1, Vec::new);
                }
                results[key.index].extend(rows);
            }
            ExecEvent::StatementFinished {
                key,
                status: StatementStatus::Error(message),
                ..
            } if key.execution == execution => panic!("{message}"),
            ExecEvent::ExecutionFinished {
                execution: finished,
                ..
            } if finished == execution => break,
            _ => {}
        }
    }
    definition_text(dialect, target, "test", &results).unwrap_or_else(|reason| panic!("{reason}"))
}

fn relation_target(schema: &str, table: &str, kind: RelationKind) -> DdlTarget {
    let relation = RelationRef {
        schema: schema.into(),
        relation: table.into(),
    };
    DdlTarget::relation(&relation, kind)
}

/// Returns the keys that reach a table, as `columns > table (columns)`.
fn referencing_keys(h: &mut Harness, schema: &str, table: &str) -> Vec<String> {
    let relation = RelationRef {
        schema: schema.into(),
        relation: table.into(),
    };
    h.manager.load_columns(h.connection, relation);
    let keys = loop {
        if let ExecEvent::ColumnsLoaded { referencing, .. } = h.next() {
            break referencing;
        }
    };
    keys.iter()
        .map(|key| {
            format!(
                "{} > {} ({})",
                key.columns.join(", "),
                key.target.relation,
                key.target_columns.join(", ")
            )
        })
        .collect()
}

#[test]
fn postgres_lists_the_objects_of_a_schema_and_reads_their_ddl() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "DROP SCHEMA IF EXISTS obj_pg CASCADE",
        "DROP SCHEMA IF EXISTS obj_ext CASCADE",
        "CREATE SCHEMA obj_pg",
        "CREATE SCHEMA obj_ext",
        "CREATE EXTENSION IF NOT EXISTS citext SCHEMA obj_ext",
        "CREATE TYPE obj_pg.mood AS ENUM ('sad', 'it''s ok')",
        "CREATE DOMAIN obj_pg.amount AS integer DEFAULT 0 NOT NULL \
         CONSTRAINT positive CHECK (VALUE >= 0)",
        "CREATE SEQUENCE obj_pg.counter START 5 INCREMENT 2",
        "CREATE TABLE obj_pg.customers (id int PRIMARY KEY, name text UNIQUE)",
        "CREATE TABLE obj_pg.orders (id serial PRIMARY KEY, total obj_pg.amount, \
         mood obj_pg.mood, customer_id int, \
         CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES obj_pg.customers (id))",
        "CREATE INDEX orders_total ON obj_pg.orders (total)",
        "CREATE VIEW obj_pg.v_orders AS SELECT id FROM obj_pg.orders",
        "CREATE FUNCTION obj_pg.twice(x integer) RETURNS integer LANGUAGE sql AS 'SELECT x * 2'",
        "CREATE FUNCTION obj_pg.twice(x text) RETURNS text LANGUAGE sql AS 'SELECT x || x'",
        "CREATE PROCEDURE obj_pg.touch() LANGUAGE sql AS 'SELECT 1'",
        "CREATE FUNCTION obj_pg.stamp() RETURNS trigger LANGUAGE plpgsql \
         AS 'BEGIN RETURN NEW; END'",
        "CREATE TRIGGER orders_stamp BEFORE INSERT ON obj_pg.orders \
         FOR EACH ROW EXECUTE FUNCTION obj_pg.stamp()",
        "CREATE AGGREGATE obj_pg.total_of(integer) (SFUNC = int4pl, STYPE = integer)",
    ]);
    let catalog = load_catalog(&mut h);
    assert_eq!(
        object_labels(&catalog, "obj_pg"),
        [
            "stamp",
            "touch",
            "twice(x integer)",
            "twice(x text)",
            "counter",
            "orders_id_seq",
            "orders_stamp (orders)",
            "amount",
            "mood"
        ]
    );
    // The functions of an extension stay out.
    assert!(object_labels(&catalog, "obj_ext").is_empty());

    let ddl = |h: &mut Harness, label: &str| {
        let target = DdlTarget::object("obj_pg", find_object(&catalog, "obj_pg", label));
        read_ddl(h, Dialect::Postgres, &target)
    };
    let text = ddl(&mut h, "twice(x integer)");
    assert!(
        text.contains("CREATE OR REPLACE FUNCTION obj_pg.twice(x integer)"),
        "{text}"
    );
    assert!(text.contains("x * 2"), "{text}");
    let text = ddl(&mut h, "touch");
    assert!(text.contains("PROCEDURE obj_pg.touch()"), "{text}");
    let text = ddl(&mut h, "orders_stamp (orders)");
    assert!(
        text.contains("CREATE TRIGGER orders_stamp BEFORE INSERT ON "),
        "{text}"
    );
    let text = ddl(&mut h, "counter");
    assert!(text.contains("INCREMENT BY 2"), "{text}");
    assert!(text.contains("START WITH 5"), "{text}");
    assert!(!text.contains("OWNED BY"), "{text}");
    let text = ddl(&mut h, "orders_id_seq");
    assert!(text.contains("OWNED BY obj_pg.orders.id;"), "{text}");
    let text = ddl(&mut h, "mood");
    assert!(
        text.contains("AS ENUM (\n    E'sad',\n    E'it''s ok'\n);"),
        "{text}"
    );
    let text = ddl(&mut h, "amount");
    assert!(
        text.contains("AS integer\n    DEFAULT 0\n    NOT NULL\n"),
        "{text}"
    );
    assert!(text.contains("CONSTRAINT \"positive\" CHECK"), "{text}");
    let view = relation_target("obj_pg", "v_orders", RelationKind::View);
    let text = read_ddl(&mut h, Dialect::Postgres, &view);
    assert!(
        text.contains("CREATE OR REPLACE VIEW obj_pg.v_orders AS\n"),
        "{text}"
    );

    // The text of each table makes the same tables again.
    let tables =
        ["customers", "orders"].map(|table| relation_target("obj_pg", table, RelationKind::Table));
    let first: Vec<String> = tables
        .iter()
        .map(|table| read_ddl(&mut h, Dialect::Postgres, table))
        .collect();
    assert!(first[1].contains(text::DEFINITION_WRITTEN), "{}", first[1]);
    assert!(
        first[1].contains("CREATE INDEX \"orders_total\""),
        "{}",
        first[1]
    );
    h.run_ok(&["DROP TABLE obj_pg.orders, obj_pg.customers CASCADE"]);
    for text in &first {
        let statements: Vec<String> = split(text, Dialect::Postgres)
            .iter()
            .map(|statement| text[statement.start..statement.end].to_owned())
            .collect();
        let options = RunOptions {
            schema: Some("obj_pg".to_owned()),
            ..RunOptions::default()
        };
        let execution = h.manager.run(h.console, statements, options);
        let run = h.collect(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Completed), "{text}: {run:?}");
    }
    let again: Vec<String> = tables
        .iter()
        .map(|table| read_ddl(&mut h, Dialect::Postgres, table))
        .collect();
    assert_eq!(again, first);
    assert_eq!(
        referencing_keys(&mut h, "obj_pg", "customers"),
        ["id > orders (customer_id)"]
    );
    h.run_ok(&["DROP SCHEMA obj_pg CASCADE", "DROP SCHEMA obj_ext CASCADE"]);
}

#[test]
fn mysql_lists_the_objects_of_a_schema_and_reads_their_ddl() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "DROP TRIGGER IF EXISTS obj_my_stamp",
        "DROP PROCEDURE IF EXISTS obj_my_touch",
        "DROP FUNCTION IF EXISTS obj_my_twice",
        "DROP TABLE IF EXISTS obj_my_orders",
        "DROP TABLE IF EXISTS obj_my_customers",
        "CREATE TABLE obj_my_customers (id INT PRIMARY KEY)",
        "CREATE TABLE obj_my_orders (id INT PRIMARY KEY, customer_id INT, \
         CONSTRAINT obj_my_customer FOREIGN KEY (customer_id) REFERENCES obj_my_customers (id))",
        "CREATE FUNCTION obj_my_twice(x INT) RETURNS INT DETERMINISTIC RETURN x * 2",
        "CREATE PROCEDURE obj_my_touch() BEGIN SELECT 1; SELECT 2; END",
        "CREATE TRIGGER obj_my_stamp BEFORE INSERT ON obj_my_orders \
         FOR EACH ROW SET NEW.id = NEW.id",
    ]);
    let catalog = load_catalog(&mut h);
    let schema = catalog.current_schema.clone().unwrap();
    let labels = object_labels(&catalog, &schema);
    let mine: Vec<&str> = labels
        .iter()
        .map(String::as_str)
        .filter(|label| label.starts_with("obj_my_"))
        .collect();
    assert_eq!(
        mine,
        [
            "obj_my_touch",
            "obj_my_twice",
            "obj_my_stamp (obj_my_orders)"
        ]
    );
    let ddl = |h: &mut Harness, label: &str| {
        let target = DdlTarget::object(&schema, find_object(&catalog, &schema, label));
        read_ddl(h, Dialect::MySql, &target)
    };
    let text = ddl(&mut h, "obj_my_twice");
    assert!(text.contains("FUNCTION `obj_my_twice`"), "{text}");
    let text = ddl(&mut h, "obj_my_touch");
    assert!(text.contains("PROCEDURE `obj_my_touch`"), "{text}");
    assert!(text.ends_with("END;\n"), "{text}");
    let statements = split(&text, Dialect::MySql);
    assert_eq!(statements.len(), 1, "{text}");
    let text = ddl(&mut h, "obj_my_stamp (obj_my_orders)");
    assert!(text.contains("TRIGGER `obj_my_stamp`"), "{text}");
    let table = relation_target(&schema, "obj_my_orders", RelationKind::Table);
    let text = read_ddl(&mut h, Dialect::MySql, &table);
    assert!(text.contains("CREATE TABLE `obj_my_orders`"), "{text}");
    assert!(text.contains("FOREIGN KEY (`customer_id`)"), "{text}");
    assert!(!text.contains(text::DEFINITION_WRITTEN), "{text}");
    assert_eq!(
        referencing_keys(&mut h, &schema, "obj_my_customers"),
        ["id > obj_my_orders (customer_id)"]
    );
    h.run_ok(&[
        "DROP TRIGGER obj_my_stamp",
        "DROP PROCEDURE obj_my_touch",
        "DROP FUNCTION obj_my_twice",
        "DROP TABLE obj_my_orders",
        "DROP TABLE obj_my_customers",
    ]);
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

/// The text of a connection of `certuser`. A full path in `key` replaces the folder.
fn client_uri(
    server: &TlsServer,
    folder: &std::path::Path,
    certificate: &str,
    key: &str,
) -> String {
    format!(
        "{}://certuser@localhost:{}/spike?sslmode=verify-full&sslrootcert={}&sslcert={}&sslkey={}",
        server.scheme,
        server.port,
        folder.join("ca.crt").display(),
        folder.join(certificate).display(),
        folder.join(key).display(),
    )
}

fn mysql_tls_servers() -> Vec<TlsServer> {
    let mut found = tls_servers();
    found.retain(|server| server.scheme == "mysql");
    found
}

#[test]
fn tls_accepts_a_client_certificate() {
    let Some(folder) = certs() else { return };
    for server in tls_servers() {
        let uri = client_uri(&server, &folder, "client.crt", server.user_key);
        let TlsServer { scheme, port, .. } = server;
        assert!(try_connect(&uri).is_ok(), "{scheme}");
        let without = format!(
            "{scheme}://certuser@localhost:{port}/spike?sslmode=verify-full&sslrootcert={}",
            folder.join("ca.crt").display(),
        );
        assert!(try_connect(&without).is_err(), "{scheme}");
    }
}

#[test]
fn tls_accepts_each_form_of_a_client_key_on_mysql() {
    let Some(folder) = certs() else { return };
    let forms = [
        ("client.crt", "client.rsa.key", "RSA PRIVATE KEY"),
        ("client.crt", "client.pk8.key", "PRIVATE KEY"),
        ("client-ec.crt", "client-ec.key", "EC PRIVATE KEY"),
    ];
    for server in mysql_tls_servers() {
        for (certificate, key, form) in forms {
            let text = std::fs::read_to_string(folder.join(key)).unwrap();
            assert!(text.contains(&format!("-----BEGIN {form}-----")), "{key}");
            let uri = client_uri(&server, &folder, certificate, key);
            assert_eq!(try_connect(&uri).err(), None, "{key}");
        }
    }
}

#[test]
fn a_bad_client_key_on_mysql_is_a_connection_error() {
    let Some(folder) = certs() else { return };
    let bad = std::env::temp_dir().join(format!("sqln-key-{}.pem", std::process::id()));
    let files = [
        ("no key", "no key in this file\n"),
        (
            "bad key",
            "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n",
        ),
    ];
    for server in mysql_tls_servers() {
        for (name, text) in files {
            std::fs::write(&bad, text).unwrap();
            let uri = client_uri(&server, &folder, "client.crt", &bad.display().to_string());
            let error = try_connect(&uri).err().unwrap();
            println!("{name}: {error}");
        }
        let uri = client_uri(&server, &folder, "client.crt", "missing.key");
        let error = try_connect(&uri).err().unwrap();
        println!("missing key: {error}");
    }
    let _ = std::fs::remove_file(bad);
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
        common::focus_form_field(&mut self.app, field);
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

        // A DELETE without WHERE asks first, and the focus starts on Cancel.
        s.run_script("BEGIN;\nDELETE FROM app_orders;");
        s.wait_for("Confirm the run");
        s.press(key(KeyCode::Tab));
        s.press(key(KeyCode::Enter));
        s.wait_until(|s| !s.app.console().is_busy());
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
        table
            .rows
            .iter()
            .map(|row| row.get(0).unwrap().display())
            .collect()
    }
}

#[test]
fn the_structure_queries_read_the_catalog_of_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&[
            "DROP VIEW IF EXISTS s_view",
            "DROP TABLE IF EXISTS s_child",
            "DROP TABLE IF EXISTS s_parent",
            "CREATE TABLE s_parent (a int, b int, PRIMARY KEY (a, b))",
            "CREATE TABLE s_child (\
               id int PRIMARY KEY, \
               label varchar(30) NOT NULL DEFAULT 'x', \
               pa int, \
               pb int, \
               CONSTRAINT s_fk FOREIGN KEY (pa, pb) REFERENCES s_parent (a, b) \
                 ON DELETE CASCADE)",
            "CREATE INDEX s_idx ON s_child (label, pa)",
            "CREATE VIEW s_view AS SELECT id FROM s_child",
        ]);
        h.manager.load_catalog(h.connection, 0, Vec::new());
        let catalog = loop {
            if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
                break catalog.unwrap();
            }
        };
        let schema = catalog.current_schema.clone().unwrap();
        let dialect = match server.kind {
            Kind::Postgres => Dialect::Postgres,
            Kind::MySql => Dialect::MySql,
        };
        let child = RelationRef {
            schema: schema.clone(),
            relation: "s_child".into(),
        };
        let mut read = |scope, target: &RelationRef, section| {
            let sql = structure_sql(dialect, scope, target, section).unwrap();
            h.run_ok(&[&sql])
        };

        let columns = read(Scope::Relation, &child, Section::Columns);
        assert_eq!(
            columns.columns[0],
            ["name", "type", "nullable", "default", "key"],
            "{:?}",
            server.kind
        );
        assert_eq!(columns.rows.len(), 4, "{:?}", server.kind);
        assert_eq!(columns.text(0, 0), "id");
        assert_eq!(columns.text(0, 2), "no");
        assert_eq!(columns.text(0, 4), "PK");
        assert_eq!(
            columns.rows[1].get(4).unwrap(),
            Value::Null,
            "{:?}",
            server.kind
        );
        assert!(columns.text(1, 1).contains("30"), "{:?}", server.kind);
        assert!(columns.text(1, 3).contains('x'), "{:?}", server.kind);
        assert_eq!(columns.text(2, 2), "yes");

        let indexes = read(Scope::Relation, &child, Section::Indexes);
        assert_eq!(indexes.columns[0], ["name", "columns", "unique"]);
        let index = (0..indexes.rows.len())
            .find(|row| indexes.text(*row, 0) == "s_idx")
            .unwrap();
        assert_eq!(indexes.text(index, 1), "label, pa", "{:?}", server.kind);
        assert_eq!(indexes.text(index, 2), "no");
        let primary = (0..indexes.rows.len())
            .find(|row| indexes.text(*row, 1) == "id")
            .unwrap();
        assert_eq!(indexes.text(primary, 2), "yes");

        let keys = read(Scope::Relation, &child, Section::ForeignKeys);
        assert_eq!(
            keys.columns[0],
            ["name", "columns", "references", "on_update", "on_delete"]
        );
        assert_eq!(keys.rows.len(), 1, "{:?}", server.kind);
        assert_eq!(keys.text(0, 0), "s_fk");
        assert_eq!(keys.text(0, 1), "pa, pb");
        assert_eq!(keys.text(0, 2), format!("{schema}.s_parent (a, b)"));
        assert_eq!(keys.text(0, 3), "NO ACTION", "{:?}", server.kind);
        assert_eq!(keys.text(0, 4), "CASCADE");

        let view = RelationRef {
            schema: schema.clone(),
            relation: "s_view".into(),
        };
        let columns = read(Scope::Relation, &view, Section::Columns);
        assert_eq!(columns.rows.len(), 1, "{:?}", server.kind);

        let in_schema = RelationRef {
            schema: schema.clone(),
            relation: String::new(),
        };
        let wide = read(Scope::Schema, &in_schema, Section::Columns);
        assert_eq!(
            wide.columns[0],
            ["schema", "relation", "name", "type", "nullable", "default"]
        );
        let found = |run: &Run| {
            (0..run.rows.len())
                .filter(|row| run.text(*row, 1).starts_with("s_"))
                .count()
        };
        assert_eq!(found(&wide), 7, "{:?}", server.kind);

        let everything = read(Scope::Connection, &RelationRef::default(), Section::Columns);
        assert_eq!(found(&everything), 7, "{:?}", server.kind);
        assert!(
            !(0..everything.rows.len()).any(|row| {
                let name = everything.text(row, 0);
                name == "information_schema" || name == "mysql" || name.starts_with("pg_")
            }),
            "{:?}",
            server.kind
        );
        h.run_ok(&[
            "DROP VIEW s_view",
            "DROP TABLE s_child",
            "DROP TABLE s_parent",
        ]);
    }
}

/// Reads the diagram of a schema, with only the tables whose names start with `prefix`.
/// The tests share each server, so the schema holds the tables of other tests.
fn er_model(h: &mut Harness, dialect: Dialect, schema: &str, prefix: &str) -> Model {
    let [columns, keys] = diagram_sql(dialect, schema).unwrap();
    let ours = |name: &str| name.starts_with(prefix);
    let (column_rows, key_rows) = (h.run_ok(&[&columns]).rows, h.run_ok(&[&keys]).rows);
    let columns = column_rows.iter().filter_map(parse_column);
    let columns = columns.filter(|row| ours(&row.relation.relation)).collect();
    let keys = key_rows.iter().filter_map(parse_key);
    let keys = keys
        .filter(|row| ours(&row.relation.relation) || ours(&row.target.relation))
        .collect();
    Model::build(columns, keys, &Focus::Schema(schema.to_owned()))
}

/// Describes each link as `child(columns) -> schema.parent(columns)`.
fn er_links(model: &Model) -> Vec<String> {
    model
        .links
        .iter()
        .map(|link| {
            let (child, parent) = (&model.entities[link.child], &model.entities[link.parent]);
            let from: Vec<&str> = link
                .child_columns
                .iter()
                .map(|&column| child.columns[column].name.as_str())
                .collect();
            let to: Vec<&str> = link
                .parent_columns
                .iter()
                .map(|column| column.map_or("?", |column| parent.columns[column].name.as_str()))
                .collect();
            format!(
                "{}({}) -> {}.{}({})",
                child.relation.relation,
                from.join(","),
                parent.relation.schema,
                parent.relation.relation,
                to.join(",")
            )
        })
        .collect()
}

#[test]
fn the_diagram_queries_read_tables_keys_and_other_schemas_of_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let (dialect, other) = match server.kind {
            Kind::Postgres => (Dialect::Postgres, "CREATE SCHEMA IF NOT EXISTS erk_other"),
            Kind::MySql => (Dialect::MySql, "CREATE DATABASE IF NOT EXISTS erk_other"),
        };
        h.manager.load_catalog(h.connection, 0, Vec::new());
        let schema = loop {
            if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
                break catalog.unwrap().current_schema.unwrap();
            }
        };
        let audit = format!(
            "CREATE TABLE erk_other.erk_audit (id int PRIMARY KEY, child int, \
             CONSTRAINT erk_audit_child FOREIGN KEY (child) REFERENCES {schema}.erk_child (id))"
        );
        h.run_ok(&[
            other,
            "DROP TABLE IF EXISTS erk_other.erk_audit",
            "DROP VIEW IF EXISTS erk_view",
            "DROP TABLE IF EXISTS erk_child",
            "DROP TABLE IF EXISTS erk_parent",
            "DROP TABLE IF EXISTS erk_other.erk_users",
            "CREATE TABLE erk_other.erk_users (id int PRIMARY KEY)",
            "CREATE TABLE erk_parent (a int, b int, PRIMARY KEY (a, b))",
            "CREATE TABLE erk_child (id int PRIMARY KEY, pa int NOT NULL, pb int NOT NULL, \
               owner int, \
               CONSTRAINT erk_fk FOREIGN KEY (pa, pb) REFERENCES erk_parent (a, b), \
               CONSTRAINT erk_owner FOREIGN KEY (owner) REFERENCES erk_other.erk_users (id))",
            &audit,
            "CREATE VIEW erk_view AS SELECT id FROM erk_child",
        ]);
        let model = er_model(&mut h, dialect, &schema, "erk_");
        let entities: Vec<(String, bool)> = model
            .entities
            .iter()
            .map(|entity| (entity.relation.relation.clone(), entity.stub))
            .collect();
        // The view stays out, and the tables of the other schema are stubs.
        assert_eq!(
            entities,
            [
                ("erk_child".to_owned(), false),
                ("erk_parent".to_owned(), false),
                ("erk_audit".to_owned(), true),
                ("erk_users".to_owned(), true),
            ],
            "{:?}",
            server.kind
        );
        assert_eq!(
            er_links(&model),
            [
                format!("erk_child(pa,pb) -> {schema}.erk_parent(a,b)"),
                "erk_child(owner) -> erk_other.erk_users(?)".to_owned(),
                format!("erk_audit() -> {schema}.erk_child(id)"),
            ],
            "{:?}",
            server.kind
        );
        let parent = &model.entities[1];
        assert_eq!(parent.columns[1].primary, Some(2), "{:?}", server.kind);
        assert!(parent.columns[0].ty.contains("int"), "{:?}", server.kind);
        let text = to_mermaid(&model, false);
        assert!(text.contains("e2 ||..o{ e1 : \"pa, pb\""), "{text}");
        assert!(text.contains("e4 |o..o{ e1 : \"owner\""), "{text}");
        h.run_ok(&[
            "DROP TABLE erk_other.erk_audit",
            "DROP VIEW erk_view",
            "DROP TABLE erk_child",
            "DROP TABLE erk_parent",
            "DROP TABLE erk_other.erk_users",
        ]);
    }
}

#[test]
fn a_partitioned_table_appears_once_with_its_keys() {
    for server in servers()
        .into_iter()
        .filter(|server| server.kind == Kind::Postgres)
    {
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&[
            "DROP TABLE IF EXISTS erp_part_ref",
            "DROP TABLE IF EXISTS erp_part",
            "DROP TABLE IF EXISTS erp_part_parent",
            "CREATE TABLE erp_part_parent (id int PRIMARY KEY)",
            "CREATE TABLE erp_part (id int, k int, owner int REFERENCES erp_part_parent, \
               PRIMARY KEY (id, k)) PARTITION BY RANGE (k)",
            "CREATE TABLE erp_part_1 PARTITION OF erp_part FOR VALUES FROM (0) TO (10)",
            "CREATE TABLE erp_part_2 PARTITION OF erp_part FOR VALUES FROM (10) TO (20)",
            "CREATE TABLE erp_part_ref (id int PRIMARY KEY, pid int, pk int, \
               FOREIGN KEY (pid, pk) REFERENCES erp_part (id, k))",
        ]);
        let model = er_model(&mut h, Dialect::Postgres, "public", "erp_");
        let names: Vec<&str> = model
            .entities
            .iter()
            .map(|entity| entity.relation.relation.as_str())
            .collect();
        assert_eq!(names, ["erp_part", "erp_part_parent", "erp_part_ref"]);
        assert_eq!(
            er_links(&model),
            [
                "erp_part(owner) -> public.erp_part_parent(id)",
                "erp_part_ref(pid,pk) -> public.erp_part(id,k)",
            ]
        );
        h.run_ok(&[
            "DROP TABLE erp_part_ref",
            "DROP TABLE erp_part",
            "DROP TABLE erp_part_parent",
        ]);
    }
}

#[test]
fn a_hostile_name_stays_a_name_in_the_structure_queries() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let dialect = match server.kind {
            Kind::Postgres => Dialect::Postgres,
            Kind::MySql => Dialect::MySql,
        };
        let name = "x'; DROP TABLE s_safe; -- \\";
        let quoted = sqln::sql::quote_identifier(name, dialect);
        let (drop_table, create) = (
            format!("DROP TABLE IF EXISTS {quoted}"),
            format!("CREATE TABLE {quoted} (id int)"),
        );
        h.run_ok(&[
            "DROP TABLE IF EXISTS s_safe",
            "CREATE TABLE s_safe (id int)",
            &drop_table,
            &create,
        ]);
        h.manager.load_catalog(h.connection, 0, Vec::new());
        let schema = loop {
            if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
                break catalog.unwrap().current_schema.unwrap();
            }
        };
        let target = RelationRef {
            schema,
            relation: name.into(),
        };
        for section in Section::ALL {
            let sql = structure_sql(dialect, Scope::Relation, &target, section).unwrap();
            let run = h.run_ok(&[&sql]);
            if section == Section::Columns {
                assert_eq!(run.rows.len(), 1, "{:?}", server.kind);
            }
        }
        h.run_ok(&["SELECT * FROM s_safe", &drop_table, "DROP TABLE s_safe"]);
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
        assert!(
            s.app
                .console()
                .messages
                .iter()
                .any(|message| message.text.contains("changed"))
        );

        s.open_table("app_pages");
        assert_eq!(s.table_ids().len(), 100);
        assert!(s.app.table().unwrap().has_more);
        s.prefix('j');
        s.code(KeyCode::Char('c'));
        s.wait_table();
        assert_eq!(s.app.table().unwrap().total, Some(250));
        assert_eq!(s.table_ids().len(), 100);
        s.wait_for("100 of 250 rows loaded");
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
        assert!(
            s.app
                .console()
                .messages
                .iter()
                .any(|message| message.text.contains("changed"))
        );
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
    assert!(
        matches!(run.rows[0].get(0).unwrap(), Value::Null),
        "{run:?}"
    );
    assert_ne!(
        run.text(1, 0),
        first,
        "MySQL cannot leave a database, so the session closes"
    );
}

#[test]
fn an_export_reads_past_the_row_cap_and_stops_on_request() {
    for server in servers() {
        let settings = Settings {
            row_cap: 10,
            max_connections: 1,
            // A debug build needs time to drain the MySQL rows already in transit.
            stop_grace: Duration::from_secs(50),
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        server.prepare_huge_read(&mut h);
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("servers-export")
            .join(format!("{:?}.csv", server.kind));
        let options = |writer: &ExportWriter| RunOptions {
            export: Some(writer.clone()),
            ..RunOptions::default()
        };

        let read = match server.kind {
            Kind::Postgres => "SELECT generate_series(1, 5000) AS x",
            Kind::MySql => "SELECT a.x FROM t_huge a, t_huge b LIMIT 5000",
        };
        let writer = ExportWriter::create(&path, ExportOptions::default()).unwrap();
        let execution = h
            .manager
            .run(h.console, vec![read.to_owned()], options(&writer));
        let run = h.collect(execution);
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Completed),
            "{:?}: {run:?}",
            server.kind
        );
        assert!(run.rows.is_empty());
        assert_eq!(writer.rows(), 5000);
        writer.promote(&path).unwrap();
        let lines = std::fs::read_to_string(&path).unwrap().lines().count();
        assert_eq!(lines, 5001, "{:?}", server.kind);

        let writer = ExportWriter::create(&path, ExportOptions::default()).unwrap();
        let sql = vec![server.huge_read().to_owned()];
        let execution = h.manager.run(h.console, sql, options(&writer));
        let deadline = Instant::now() + Duration::from_secs(30);
        while writer.rows() < 1000 {
            assert!(Instant::now() < deadline, "{:?}", server.kind);
            std::thread::sleep(Duration::from_millis(1));
        }
        h.manager.stop(h.console);
        let run = h.collect(execution);
        assert_eq!(
            run.end,
            Some(ExecutionEnd::Stopped),
            "{:?}: {run:?}",
            server.kind
        );
        writer.discard();
        assert_eq!(h.run_ok(&["SELECT 7"]).text(0, 0), "7");
    }
}

#[test]
fn a_notice_of_postgres_arrives_with_its_detail_and_its_hint() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h
        .run_ok(&["DO $$ BEGIN RAISE NOTICE 'hello' USING DETAIL = 'more', HINT = 'look'; END $$"]);
    assert_eq!(run.notices, ["NOTICE: hello\nmore\nlook"]);
}

#[test]
fn a_notice_before_an_error_of_postgres_stays() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run(&["DO $$ BEGIN RAISE NOTICE 'before'; RAISE EXCEPTION 'boom'; END $$"]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert_eq!(run.notices, ["NOTICE: before"]);
}

#[test]
fn a_loop_of_notices_stops_at_the_limit_and_counts_the_rest() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run =
        h.run_ok(&["DO $$ BEGIN FOR i IN 1..250 LOOP RAISE NOTICE 'n%', i; END LOOP; END $$"]);
    assert_eq!(run.notices.len(), 101);
    assert_eq!(run.notices[99], "NOTICE: n100");
    assert_eq!(run.notices[100], "150 more notices.");
}

#[test]
fn a_statement_on_a_missing_table_reports_a_notice_on_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let run = h.run_ok(&["DROP TABLE IF EXISTS t_never_created"]);
        assert_eq!(run.notices.len(), 1, "{:?}: {run:?}", server.kind);
        assert!(
            run.notices[0].contains("t_never_created"),
            "{:?}: {run:?}",
            server.kind
        );
    }
}

#[test]
fn a_new_run_has_no_old_notice() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&["DROP TABLE IF EXISTS t_never_created"]);
        let run = h.run_ok(&["SELECT 1"]);
        assert_eq!(run.notices, Vec::<String>::new(), "{:?}", server.kind);
    }
}

#[test]
fn a_value_cut_by_mysql_reports_the_warning_of_the_server() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, sessions(1));
    h.run_ok(&[
        "DROP TABLE IF EXISTS t_notice",
        "CREATE TABLE t_notice (a VARCHAR(2))",
    ]);
    let run = h.run_ok(&[
        "SET SESSION sql_mode = ''",
        "INSERT INTO t_notice VALUES ('abc')",
    ]);
    assert_eq!(
        run.notices,
        ["Warning: Data truncated for column 'a' at row 1 (1265)"]
    );
}

#[test]
fn a_read_with_a_warning_keeps_its_rows_in_mysql() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    let run = h.run_ok(&["SELECT CAST('abc' AS SIGNED)"]);
    assert_eq!(run.text(0, 0), "0");
    assert_eq!(run.notices.len(), 1, "{run:?}");
    assert!(run.notices[0].contains("1292"), "{run:?}");
}

#[test]
fn show_warnings_of_the_user_still_works_and_adds_no_copy_in_mysql() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, sessions(1));
    let run = h.run_ok(&["SELECT CAST('abc' AS SIGNED)", "SHOW WARNINGS"]);
    assert_eq!(run.rows.len(), 2, "{run:?}");
    assert_eq!(run.notices.len(), 1, "{run:?}");
}

#[test]
fn the_columns_bring_the_foreign_keys_of_the_relation() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        h.run_ok(&[
            "DROP TABLE IF EXISTS t_fk_child",
            "DROP TABLE IF EXISTS t_fk_parent",
            "CREATE TABLE t_fk_parent (a int, b int, c int UNIQUE, PRIMARY KEY (a, b))",
            "CREATE TABLE t_fk_child (x int, y int, z int, \
             CONSTRAINT fk_one FOREIGN KEY (y, x) REFERENCES t_fk_parent (a, b), \
             CONSTRAINT fk_two FOREIGN KEY (z) REFERENCES t_fk_parent (c))",
        ]);
        h.manager.load_catalog(h.connection, 0, Vec::new());
        let catalog = loop {
            if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
                break catalog.unwrap();
            }
        };
        let schema = catalog.current_schema.clone().unwrap();
        h.manager.load_columns(
            h.connection,
            RelationRef {
                schema: schema.clone(),
                relation: "t_fk_child".into(),
            },
        );
        let keys = loop {
            if let ExecEvent::ColumnsLoaded { foreign_keys, .. } = h.next() {
                break foreign_keys;
            }
        };
        assert_eq!(keys.len(), 2, "{:?}: {keys:?}", server.kind);
        assert_eq!(keys[0].columns, ["y", "x"], "{:?}", server.kind);
        assert_eq!(keys[0].target_columns, ["a", "b"], "{:?}", server.kind);
        assert_eq!(keys[0].target.schema, schema, "{:?}", server.kind);
        assert_eq!(keys[0].target.relation, "t_fk_parent");
        assert_eq!(keys[1].columns, ["z"], "{:?}", server.kind);
        assert_eq!(keys[1].target_columns, ["c"], "{:?}", server.kind);
        h.run_ok(&["DROP TABLE t_fk_child", "DROP TABLE t_fk_parent"]);
    }
}

#[test]
fn a_foreign_key_to_another_schema_names_that_schema_in_postgres() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    h.run_ok(&[
        "DROP SCHEMA IF EXISTS s_fk CASCADE",
        "CREATE SCHEMA s_fk",
        "CREATE TABLE s_fk.target (id int PRIMARY KEY)",
        "CREATE TABLE s_fk.source (id int REFERENCES s_fk.target (id))",
    ]);
    h.manager.load_columns(
        h.connection,
        RelationRef {
            schema: "s_fk".into(),
            relation: "source".into(),
        },
    );
    let keys = loop {
        if let ExecEvent::ColumnsLoaded { foreign_keys, .. } = h.next() {
            break foreign_keys;
        }
    };
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert_eq!(keys[0].target.schema, "s_fk");
    h.run_ok(&["DROP SCHEMA s_fk CASCADE"]);
}

// ---------------------------------------------------------------------------
// The import.

impl Server {
    fn dialect(&self) -> Dialect {
        match self.kind {
            Kind::Postgres => Dialect::Postgres,
            Kind::MySql => Dialect::MySql,
        }
    }

    /// Returns a table with one column of each kind.
    fn import_table(&self, name: &str) -> String {
        match self.kind {
            Kind::Postgres => format!(
                "CREATE TABLE {name} (id integer PRIMARY KEY, name text, data bytea, \
                 flag boolean, amount numeric(30, 5), doc jsonb)"
            ),
            Kind::MySql => format!(
                "CREATE TABLE {name} (id int PRIMARY KEY, name text, data blob, \
                 flag tinyint, amount decimal(30, 5), doc json)"
            ),
        }
    }

    /// Returns a read of the columns of `import_table`, as text.
    fn import_read(&self, name: &str) -> String {
        match self.kind {
            Kind::Postgres => format!(
                "SELECT id::text, name, upper(encode(data, 'hex')), flag::text, \
                 amount::text, doc::text FROM {name} ORDER BY id"
            ),
            Kind::MySql => format!(
                "SELECT CAST(id AS CHAR), name, HEX(data), CAST(flag AS CHAR), \
                 CAST(amount AS CHAR), CAST(doc AS CHAR) FROM {name} ORDER BY id"
            ),
        }
    }

    /// Returns the table in the text of a statement.
    fn table_name(&self, name: &str) -> String {
        match self.kind {
            Kind::Postgres => format!("\"public\".\"{name}\""),
            Kind::MySql => format!("`spike`.`{name}`"),
        }
    }

    fn import_target(&self, name: &str, columns: &[&str], base64: bool) -> Target {
        Target {
            table: self.table_name(name),
            columns: columns
                .iter()
                .enumerate()
                .map(|(source, column)| TargetColumn {
                    name: (*column).to_owned(),
                    binary: *column == "data",
                    source,
                })
                .collect(),
            dialect: self.dialect(),
            base64,
        }
    }
}

fn import_file(name: &str, text: &str) -> std::path::PathBuf {
    let path = common::temp_dir(name).join("data.txt");
    std::fs::write(&path, text).unwrap();
    path
}

fn import_job(
    server: &Server,
    path: &std::path::Path,
    table: &str,
    format: ImportFormat,
    columns: &[&str],
    mode: ImportMode,
) -> ImportJob {
    ImportJob {
        path: path.to_owned(),
        read: ReadOptions {
            format,
            dialect: server.dialect(),
            ..ReadOptions::default()
        },
        columns: columns.iter().map(|name| (*name).to_owned()).collect(),
        target: server.import_target(table, columns, !format.is_delimited()),
        mode,
        batch: 2,
        error_limit: 1000,
        log: log_path(path, "x"),
        progress: Arc::new(ImportProgress::default()),
    }
}

impl Harness {
    fn import(&mut self, job: ImportJob) -> (ExecutionEnd, Option<String>) {
        let execution = self.manager.import(self.console, job);
        loop {
            if let ExecEvent::ExecutionFinished {
                execution: finished,
                end,
                message,
                ..
            } = self.next()
                && finished == execution
            {
                return (end, message);
            }
        }
    }
}

const ALL_COLUMNS: [&str; 6] = ["id", "name", "data", "flag", "amount", "doc"];

#[test]
fn an_import_keeps_the_value_of_each_type() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let table = "t_import_types";
        h.run(&[&format!("DROP TABLE IF EXISTS {table}")]);
        h.run_ok(&[&server.import_table(table)]);
        let file = concat!(
            r#"{"id": 1, "name": "a'b\\c\"d", "data": "AAEC/w==", "flag": true, "#,
            r#""amount": 1234567890123456789012.12345, "doc": {"k": [1, 2]}}"#,
            "\n",
            r#"{"id": 2, "name": null, "data": null, "flag": false, "amount": 0.5, "doc": null}"#,
            "\n",
            r#"{"id": 3, "name": "ação 日本語"}"#,
            "\n",
        );
        let path = import_file(&format!("server-types-{:?}", server.kind), file);
        let job = import_job(
            &server,
            &path,
            table,
            ImportFormat::JsonLines,
            &ALL_COLUMNS,
            ImportMode::AllOrNothing,
        );
        let (end, message) = h.import(job);
        assert_eq!(
            end,
            ExecutionEnd::Completed,
            "{:?}: {message:?}",
            server.kind
        );
        let run = h.run_ok(&[&server.import_read(table)]);
        assert_eq!(run.text(0, 1), r#"a'b\c"d"#, "{:?}", server.kind);
        assert_eq!(run.text(0, 2), "000102FF", "{:?}", server.kind);
        assert_eq!(
            run.text(0, 4),
            "1234567890123456789012.12345",
            "{:?}",
            server.kind
        );
        assert!(run.text(0, 5).contains("\"k\""), "{:?}", server.kind);
        assert_eq!(run.rows[1].get(1), Some(Value::Null), "{:?}", server.kind);
        assert_eq!(run.text(2, 1), "ação 日本語", "{:?}", server.kind);
        let flag = run.text(0, 3);
        assert!(flag == "true" || flag == "1", "{flag}");
        h.run_ok(&[&format!("DROP TABLE {table}")]);
    }
}

#[test]
fn a_file_of_an_export_returns_the_same_rows_on_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let (from, to) = ("t_import_from", "t_import_to");
        h.run(&[&format!("DROP TABLE IF EXISTS {from}, {to}")]);
        h.run_ok(&[&server.import_table(from), &server.import_table(to)]);
        let binary = match server.kind {
            Kind::Postgres => "'\\x00ff41'",
            Kind::MySql => "X'00ff41'",
        };
        let text = "A plain back slash \\\\ and a quote ''";
        let flag = if server.kind == Kind::Postgres {
            "true"
        } else {
            "1"
        };
        h.run_ok(&[&format!(
            "INSERT INTO {from} (id, name, data, flag, amount, doc) VALUES \
             (1, '{text}', {binary}, {flag}, 1.5, '{{\"a\": 1}}'), \
             (2, '', NULL, NULL, NULL, NULL)"
        )]);
        for (export, format, base64) in [
            (ExportFormatOf::Csv, ImportFormat::Csv, false),
            (ExportFormatOf::Json, ImportFormat::Json, true),
        ] {
            h.run_ok(&[&format!("DELETE FROM {to}")]);
            let path = common::temp_dir(&format!("server-trip-{:?}-{format:?}", server.kind))
                .join("data.txt");
            let options = ExportOptions {
                format: export.format(),
                ..ExportOptions::default()
            };
            let writer = ExportWriter::create(&path, options).unwrap();
            let select =
                format!("SELECT id, name, data, flag, amount, doc FROM {from} ORDER BY id");
            let run = RunOptions {
                export: Some(writer.clone()),
                ..RunOptions::default()
            };
            let execution = h.manager.run(h.console, vec![select], run);
            assert_eq!(h.collect(execution).end, Some(ExecutionEnd::Completed));
            writer.promote(&path).unwrap();
            let mut job = import_job(
                &server,
                &path,
                to,
                format,
                &ALL_COLUMNS,
                ImportMode::AllOrNothing,
            );
            job.target.base64 = base64;
            let (end, message) = h.import(job);
            assert_eq!(
                end,
                ExecutionEnd::Completed,
                "{:?}: {message:?}",
                server.kind
            );
            let read = |h: &mut Harness, name: &str| {
                let run = h.run_ok(&[&server.import_read(name)]);
                run.rows
                    .iter()
                    .map(|row| row.cells().map(Value::display).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                read(&mut h, from),
                read(&mut h, to),
                "{:?} {format:?}",
                server.kind
            );
        }
        h.run_ok(&[&format!("DROP TABLE {from}"), &format!("DROP TABLE {to}")]);
    }
}

/// The formats of the export that return to a table on a server.
#[derive(Clone, Copy)]
enum ExportFormatOf {
    Csv,
    Json,
}

impl ExportFormatOf {
    fn format(self) -> sqln::export::ExportFormat {
        match self {
            Self::Csv => sqln::export::ExportFormat::Csv,
            Self::Json => sqln::export::ExportFormat::Json,
        }
    }
}

#[test]
fn each_import_mode_does_its_work_on_each_server() {
    const FILE: &str = "id,name\n1,a\n2,b\n3,c\n1,again\n5,e\n";
    for server in servers() {
        for mode in ImportMode::ALL {
            let mut h = Harness::open(&server, Settings::default());
            let table = "t_import_modes";
            h.run(&[&format!("DROP TABLE IF EXISTS {table}")]);
            h.run_ok(&[&server.import_table(table)]);
            let path = import_file(&format!("server-modes-{:?}-{mode:?}", server.kind), FILE);
            let job = import_job(
                &server,
                &path,
                table,
                ImportFormat::Csv,
                &["id", "name"],
                mode,
            );
            let (log, progress) = (job.log.clone(), job.progress.clone());
            let (end, message) = h.import(job);
            let (expected_end, rows) = match mode {
                ImportMode::AllOrNothing => (ExecutionEnd::Failed, "0"),
                ImportMode::ByBatch => (ExecutionEnd::Failed, "2"),
                ImportMode::LogErrors => (ExecutionEnd::Completed, "4"),
            };
            let tag = format!("{:?} {mode:?}: {message:?}", server.kind);
            assert_eq!(end, expected_end, "{tag}");
            assert_eq!(h.count(table), rows, "{tag}");
            assert_eq!(progress.rows().to_string(), rows, "{tag}");
            if mode == ImportMode::LogErrors {
                let text = std::fs::read_to_string(log).unwrap();
                assert!(text.contains("Line 5:"), "{tag}: {text}");
                assert!(text.contains("1,again"), "{tag}: {text}");
            } else {
                assert!(message.unwrap().contains("lines 4 to 5"), "{tag}");
            }
            h.run_ok(&[&format!("DROP TABLE {table}")]);
        }
    }
}

#[test]
fn stop_undoes_all_or_nothing_on_each_server() {
    let mut file = String::from("id,name\n");
    for row in 0..200_000 {
        file.push_str(&format!("{row},x\n"));
    }
    for server in servers() {
        let settings = Settings {
            // A debug build needs time to end a statement in transit.
            stop_grace: Duration::from_secs(50),
            ..Settings::default()
        };
        let mut h = Harness::open(&server, settings);
        let table = "t_import_stop";
        h.run(&[&format!("DROP TABLE IF EXISTS {table}")]);
        h.run_ok(&[&server.import_table(table)]);
        let path = import_file(&format!("server-stop-{:?}", server.kind), &file);
        let job = import_job(
            &server,
            &path,
            table,
            ImportFormat::Csv,
            &["id", "name"],
            ImportMode::AllOrNothing,
        );
        let progress = job.progress.clone();
        let execution = h.manager.import(h.console, job);
        let deadline = Instant::now() + Duration::from_secs(30);
        while progress.rows() < 100 {
            assert!(Instant::now() < deadline, "{:?}", server.kind);
            std::thread::sleep(Duration::from_millis(1));
        }
        h.manager.stop(h.console);
        let run = h.collect(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{:?}", server.kind);
        assert_eq!(progress.rows(), 0, "{:?}", server.kind);
        assert_eq!(h.count(table), "0", "{:?}", server.kind);
        h.run_ok(&[&format!("DROP TABLE {table}")]);
    }
}

#[test]
fn a_literal_of_an_import_ignores_the_settings_of_the_session() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let table = "t_import_literal";
        h.run(&[&format!("DROP TABLE IF EXISTS {table}")]);
        h.run_ok(&[&server.import_table(table)]);
        let setting = match server.kind {
            Kind::Postgres => "SET standard_conforming_strings = off",
            Kind::MySql => "SET SESSION sql_mode = 'NO_BACKSLASH_ESCAPES'",
        };
        let target = server.import_target(table, &["id", "name", "data"], false);
        let hostile = "a\\'); DROP TABLE x; --\\";
        let cells = [
            Some("1".to_owned()),
            Some(hostile.to_owned()),
            Some("b\\c".to_owned()),
        ];
        let insert = format!("{}{}", target.head(), target.values(&cells).unwrap());
        // One execution keeps one session, so the setting reaches the `INSERT`.
        h.run_ok(&[setting, &insert]);
        let run = h.run_ok(&[&server.import_read(table)]);
        assert_eq!(run.text(0, 1), hostile, "{:?}", server.kind);
        assert_eq!(run.text(0, 2), "625C63", "{:?}", server.kind);
        h.run_ok(&[&format!("DROP TABLE {table}")]);
    }
}

#[test]
fn the_dialog_reads_the_binary_type_of_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let table = "t_import_dialog";
        h.run(&[&format!("DROP TABLE IF EXISTS {table}")]);
        h.run_ok(&[&server.import_table(table)]);

        let path = import_file(
            &format!("server-dialog-{:?}", server.kind),
            "id,name,data\n7,Ana,0x00FF\n",
        );
        let mut s = Screen::with_definition(&server);
        s.open_table(table);
        s.prefix('j');
        s.press(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT));
        s.wait_until(|s| s.screen().contains("Import to"));
        s.type_text(&path.display().to_string());
        s.code(KeyCode::Tab);
        s.wait_until(|s| s.screen().contains("3 file columns"));
        s.code(KeyCode::Tab);
        s.code(KeyCode::Enter);
        s.wait_until(|s| s.screen().contains("Import complete"));
        s.app.shutdown();

        let run = h.run_ok(&[&server.import_read(table)]);
        assert_eq!(run.text(0, 2), "00FF", "{:?}", server.kind);
        h.run_ok(&[&format!("DROP TABLE {table}")]);
    }
}

// ---------------------------------------------------------------------------
// The plan of a statement.

/// Creates the tables of one test. The tests run together on one server, so each test
/// uses its own names.
fn create_plan_tables(h: &mut Harness, name: &str) {
    h.run_ok(&[
        &format!("DROP TABLE IF EXISTS p_child_{name}"),
        &format!("DROP TABLE IF EXISTS p_parent_{name}"),
        &format!("CREATE TABLE p_parent_{name} (id int PRIMARY KEY, label varchar(20))"),
        &format!("CREATE TABLE p_child_{name} (id int PRIMARY KEY, parent_id int)"),
        &format!("INSERT INTO p_parent_{name} VALUES (1, 'a'), (2, 'b')"),
        &format!("INSERT INTO p_child_{name} VALUES (1, 1), (2, 1), (3, 2)"),
    ]);
}

fn plan_statement(name: &str) -> String {
    format!(
        "SELECT p.label, count(*) FROM p_parent_{name} p \
         JOIN p_child_{name} c ON c.parent_id = p.id GROUP BY p.label ORDER BY 2 LIMIT 1"
    )
}

fn plan_of(dialect: Dialect, run: Run) -> PlanView {
    let mut table = ResultTable::new(run.columns[0].clone());
    table.append(run.rows, run.bytes);
    plan_view(dialect, &table).unwrap_or_else(|| panic!("{table:?}"))
}

fn plan_cells(view: &PlanView, title: &str) -> Vec<String> {
    let column = view.columns.iter().position(|item| item == title).unwrap();
    view.rows
        .iter()
        .map(|row| row.get(column).unwrap().full_text())
        .collect()
}

#[test]
fn each_server_gives_a_plan_that_reads_as_a_tree() {
    for server in servers() {
        let dialect = server.dialect();
        let mut h = Harness::open(&server, Settings::default());
        create_plan_tables(&mut h, "tree");
        let statement = plan_statement("tree");
        let estimate = Explain::new(dialect, false).unwrap();
        let view = plan_of(dialect, h.run_ok(&[&estimate.wrap(&statement)]));
        let nodes = plan_cells(&view, text::PLAN_NODE);
        assert!(nodes.len() > 2, "{nodes:?}");
        assert!(nodes[0].starts_with("Limit"), "{nodes:?}");
        assert!(nodes[1].starts_with("└─ "), "{nodes:?}");
        assert!(view.columns.iter().any(|item| item == text::PLAN_COST));
        assert!(!view.columns.iter().any(|item| item == text::PLAN_TIME));
        assert!(view.summary.contains("most:"), "{}", view.summary);

        let real = Explain::new(dialect, true).unwrap();
        let view = plan_of(dialect, h.run_ok(&[&real.wrap(&statement)]));
        assert!(view.columns.iter().any(|item| item == text::PLAN_TIME));
        assert!(view.columns.iter().any(|item| item == text::PLAN_LOOPS));
        assert!(view.summary.contains(" ms "), "{}", view.summary);
        let actual = plan_cells(&view, text::PLAN_ACTUAL_ROWS);
        assert_eq!(actual[0], "1", "the limit reads one row");
    }
}

#[test]
fn an_error_in_a_plan_points_after_the_prefix() {
    for server in servers() {
        let connector = connector(&server);
        let runtime = runtime();
        runtime.block_on(async {
            let mut session = connector.connect().await.unwrap();
            let explain = Explain::new(server.dialect(), false).unwrap();
            let sql = explain.wrap("SELECT 1 FROM");
            let error = session.run(&sql, sink()).await.unwrap_err();
            assert_eq!(
                error.offset,
                Some(explain.prefix_len() + 13),
                "{:?}",
                server.kind
            );
        });
    }
}

#[test]
fn a_plan_without_a_real_run_changes_nothing() {
    for server in servers() {
        let dialect = server.dialect();
        let mut h = Harness::open(&server, Settings::default());
        create_plan_tables(&mut h, "estimate");
        let explain = Explain::new(dialect, false).unwrap();
        h.run_ok(&[&explain.wrap("UPDATE p_child_estimate SET parent_id = 9")]);
        assert_eq!(h.count("p_child_estimate WHERE parent_id = 9"), "0");
    }
}

#[test]
fn a_real_plan_of_postgres_keeps_its_change() {
    let Some(server) = only(Kind::Postgres) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    create_plan_tables(&mut h, "real");
    let explain = Explain::new(Dialect::Postgres, true).unwrap();
    h.run_ok(&[&explain.wrap("UPDATE p_child_real SET parent_id = 9")]);
    assert_eq!(h.count("p_child_real WHERE parent_id = 9"), "3");
}

#[test]
fn the_answer_of_mysql_for_a_write_is_a_plan_with_one_node() {
    let Some(server) = only(Kind::MySql) else {
        return;
    };
    let mut h = Harness::open(&server, Settings::default());
    create_plan_tables(&mut h, "write");
    let explain = Explain::new(Dialect::MySql, false).unwrap();
    let run = h.run_ok(&[&explain.wrap("UPDATE p_child_write SET parent_id = 9")]);
    let view = plan_of(Dialect::MySql, run);
    assert_eq!(view.rows.len(), 1);
}

#[test]
fn the_keys_of_a_console_show_the_plan_of_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        create_plan_tables(&mut h, "keys");
        let mut s = Screen::with_definition(&server);
        s.prefix('h');
        s.code(KeyCode::Enter);
        s.wait_until(|s| !s.app.console().is_busy() && s.app.console().connection.is_some());
        s.app.on_event(Event::Paste(plan_statement("keys")));
        s.press(ctrl('w'));
        s.press(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT));
        s.wait_until(|s| {
            let console = s.app.console();
            !console.is_busy() && console.results.first().is_some_and(|result| result.plan)
        });
        let screen = s.screen();
        assert!(screen.contains("Plan"), "{screen}");
        assert!(screen.contains("Share"), "{screen}");

        s.press(ctrl('w'));
        s.press(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        s.wait_until(|s| {
            let console = s.app.console();
            let columns = console.results.first().map(|item| &item.table.columns);
            columns.is_some_and(|columns| columns.iter().any(|name| name == text::PLAN_TIME))
        });
        s.app.shutdown();
    }
}

// ---------------------------------------------------------------------------
// The sessions of a server.

fn session_list(h: &mut Harness, dialect: Dialect) -> Run {
    let nothing = RelationRef::default();
    let sql = structure_sql(dialect, Scope::Sessions, &nothing, Section::Columns).unwrap();
    h.run_ok(&[&sql])
}

fn session_ids(run: &Run) -> Vec<String> {
    run.rows
        .iter()
        .map(|row| row.get(0).unwrap().full_text())
        .collect()
}

/// Starts a long statement in a session of its own. It returns the number of the
/// session and the execution.
fn start_busy_session(server: &Server, busy: &mut Harness) -> (String, u64) {
    let id = busy.run_ok(&[server.backend_id()]).text(0, 0);
    let execution = busy.start(&[&server.sleep(30)], false);
    busy.wait_started();
    (id, execution)
}

/// Waits until the list shows the long statement of a session.
fn wait_for_session(server: &Server, reader: &mut Harness, id: &str) -> Run {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let run = session_list(reader, server.dialect());
        let row = session_ids(&run).iter().position(|item| item == id);
        let shows_query = row.is_some_and(|row| {
            let last = run.rows[row].columns() - 1;
            let query = run.rows[row].get(last).unwrap().full_text();
            query.to_lowercase().contains("sleep")
        });
        if shows_query {
            return run;
        }
        assert!(Instant::now() < deadline, "{run:?}");
        // This list takes a global lock on MySQL, so a slow pace leaves the other tests free.
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn the_list_shows_another_session_and_hides_its_reader() {
    for server in servers() {
        let mut busy = Harness::open(&server, sessions(1));
        let (id, execution) = start_busy_session(&server, &mut busy);
        let mut reader = Harness::open(&server, sessions(1));
        let own = reader.run_ok(&[server.backend_id()]).text(0, 0);
        let run = wait_for_session(&server, &mut reader, &id);
        assert!(!session_ids(&run).contains(&own), "{:?}", server.kind);

        let signal = signal_sql(
            server.dialect(),
            SessionSignal::EndSession,
            id.parse().unwrap(),
        );
        reader.run_ok(&[&signal.unwrap()]);
        busy.collect(execution);
    }
}

#[test]
fn a_cancel_stops_the_query_and_keeps_the_session() {
    for server in servers() {
        let mut busy = Harness::open(&server, sessions(1));
        let (id, execution) = start_busy_session(&server, &mut busy);
        let mut reader = Harness::open(&server, sessions(1));
        wait_for_session(&server, &mut reader, &id);
        let start = Instant::now();
        let signal = signal_sql(
            server.dialect(),
            SessionSignal::CancelQuery,
            id.parse().unwrap(),
        );
        reader.run_ok(&[&signal.unwrap()]);
        busy.collect(execution);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            server.kind
        );
        let next = busy.run_ok(&[server.backend_id()]);
        assert_eq!(next.text(0, 0), id, "{:?}", server.kind);
    }
}

#[test]
fn an_end_closes_the_other_session() {
    for server in servers() {
        let mut busy = Harness::open(&server, sessions(1));
        let (id, execution) = start_busy_session(&server, &mut busy);
        let mut reader = Harness::open(&server, sessions(1));
        wait_for_session(&server, &mut reader, &id);
        let signal = signal_sql(
            server.dialect(),
            SessionSignal::EndSession,
            id.parse().unwrap(),
        );
        reader.run_ok(&[&signal.unwrap()]);
        let ended = busy.collect(execution);
        assert_ne!(
            ended.end,
            Some(ExecutionEnd::Completed),
            "{:?}",
            server.kind
        );
        std::thread::sleep(Duration::from_millis(200));
        let next = busy.run_ok(&[server.backend_id()]);
        assert_ne!(next.text(0, 0), id, "{:?}", server.kind);
    }
}

#[test]
fn a_signal_to_a_missing_session_reaches_the_tab_of_each_server() {
    let missing = 2_147_483_647;
    if let Some(server) = only(Kind::Postgres) {
        let mut h = Harness::open(&server, Settings::default());
        let sql = signal_sql(Dialect::Postgres, SessionSignal::CancelQuery, missing);
        let run = h.run_ok(&[&sql.unwrap()]);
        assert_eq!(run.rows[0].get(0).unwrap().full_text(), "f");
    }
    if let Some(server) = only(Kind::MySql) {
        let mut h = Harness::open(&server, Settings::default());
        let sql = signal_sql(Dialect::MySql, SessionSignal::EndSession, missing);
        let run = h.run(&[&sql.unwrap()]);
        assert_eq!(run.end, Some(ExecutionEnd::Failed));
        assert!(run.error().contains("Unknown thread id"), "{run:?}");
    }
}

#[test]
fn the_keys_of_a_tab_end_a_session_of_each_server() {
    for server in servers() {
        let mut busy = Harness::open(&server, sessions(1));
        let (id, execution) = start_busy_session(&server, &mut busy);
        let mut reader = Harness::open(&server, sessions(1));
        wait_for_session(&server, &mut reader, &id);

        let mut s = Screen::with_definition(&server);
        s.prefix('h');
        s.press(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        s.wait_until(|s| {
            let done = s
                .app
                .table()
                .is_some_and(|tab| tab.loaded() > 0 && !tab.is_busy());
            done && s.screen().contains("(sessions)")
        });
        // The active sessions come first, and the long statement can have company.
        let row = {
            let tab = s.app.table().unwrap();
            let rows = &tab.result.table.rows;
            rows.iter()
                .position(|row| row.get(0).unwrap().full_text() == id)
                .unwrap()
        };
        for _ in 0..row {
            s.code(KeyCode::Down);
        }
        s.press(KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT));
        s.wait_until(|s| s.screen().contains(&format!("Session {id} of")));
        s.code(KeyCode::Right);
        s.code(KeyCode::Enter);
        let ended = busy.collect(execution);
        assert_ne!(
            ended.end,
            Some(ExecutionEnd::Completed),
            "{:?}",
            server.kind
        );
        s.wait_until(|s| {
            s.app
                .table()
                .is_some_and(|tab| !tab.is_busy() && tab.signal_note.is_none())
                && s.screen().contains(text::SESSION_SIGNAL_SENT)
        });
        s.app.shutdown();
    }
}

/// Reads the compare model of a schema, one statement per execution, with the schema in
/// the context of the session, as the compare tab does.
fn compare_model(h: &mut Harness, dialect: Dialect, schema: &str) -> SchemaModel {
    let results: Vec<Vec<Row>> = compare_sql(dialect, schema)
        .unwrap()
        .into_iter()
        .map(|sql| {
            let options = RunOptions {
                schema: Some(schema.to_owned()),
                only_schema: true,
                row_cap: Some(usize::MAX),
                ..RunOptions::default()
            };
            let execution = h.manager.run(h.console, vec![sql.clone()], options);
            let run = h.collect(execution);
            assert_eq!(run.end, Some(ExecutionEnd::Completed), "{sql}: {run:?}");
            run.rows
        })
        .collect();
    build(dialect, schema, &results)
}

/// Runs a script statement by statement in one execution.
fn run_script(h: &mut Harness, dialect: Dialect, script: &str) {
    let statements: Vec<&str> = split(script, dialect)
        .iter()
        .map(|statement| &script[statement.start..statement.end])
        .collect();
    h.run_ok(&statements);
}

#[test]
fn the_structure_script_makes_the_target_schema_equal_on_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let (dialect, schemas, source, target) = match server.kind {
            Kind::Postgres => (
                Dialect::Postgres,
                [
                    "DROP SCHEMA IF EXISTS cmp_src CASCADE",
                    "DROP SCHEMA IF EXISTS cmp_dst CASCADE",
                    "CREATE SCHEMA cmp_src",
                    "CREATE SCHEMA cmp_dst",
                ],
                vec![
                    "CREATE TABLE cmp_src.owners (id serial PRIMARY KEY, \
                       name text NOT NULL DEFAULT 'x', code varchar(10) UNIQUE, \
                       CONSTRAINT owners_name_check CHECK (length(name) > 0))",
                    "CREATE TABLE cmp_src.items (\
                       id bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, \
                       owner int REFERENCES cmp_src.owners (id) ON DELETE CASCADE, \
                       price numeric(10, 2) DEFAULT 0, label text)",
                    "CREATE INDEX items_label ON cmp_src.items (lower(label))",
                ],
                vec![
                    "CREATE TABLE cmp_dst.owners (id serial PRIMARY KEY, name varchar(5), \
                       code varchar(10))",
                    "CREATE TABLE cmp_dst.old (id int)",
                    "INSERT INTO cmp_dst.owners (name, code) VALUES ('one', 'a')",
                ],
            ),
            Kind::MySql => (
                Dialect::MySql,
                [
                    "DROP DATABASE IF EXISTS cmp_src",
                    "DROP DATABASE IF EXISTS cmp_dst",
                    "CREATE DATABASE cmp_src",
                    "CREATE DATABASE cmp_dst",
                ],
                vec![
                    "CREATE TABLE cmp_src.owners (id int AUTO_INCREMENT PRIMARY KEY, \
                       name varchar(20) NOT NULL DEFAULT 'x', code varchar(10), \
                       UNIQUE KEY owners_code (code), \
                       CONSTRAINT owners_name_check CHECK (char_length(name) > 0))",
                    "CREATE TABLE cmp_src.items (id bigint AUTO_INCREMENT PRIMARY KEY, \
                       owner int, price decimal(10, 2) DEFAULT 0, label varchar(40), \
                       created timestamp NULL DEFAULT CURRENT_TIMESTAMP \
                         ON UPDATE CURRENT_TIMESTAMP, \
                       KEY items_label (label), \
                       CONSTRAINT items_owner FOREIGN KEY (owner) \
                         REFERENCES cmp_src.owners (id) ON DELETE CASCADE)",
                ],
                vec![
                    "CREATE TABLE cmp_dst.owners (id int AUTO_INCREMENT PRIMARY KEY, \
                       name varchar(5), code varchar(10))",
                    "CREATE TABLE cmp_dst.old (id int)",
                    "INSERT INTO cmp_dst.owners (name, code) VALUES ('one', 'a')",
                ],
            ),
        };
        h.run_ok(&schemas);
        h.run_ok(&source);
        h.run_ok(&target);
        let from = compare_model(&mut h, dialect, "cmp_src");
        let to = compare_model(&mut h, dialect, "cmp_dst");
        let differences = diff(&from, &to);
        let changes: Vec<(Change, String, String)> = differences
            .iter()
            .map(|difference| {
                let name = difference.object().name().to_owned();
                (difference.change, difference.table.clone(), name)
            })
            .collect();
        assert!(
            changes.contains(&(Change::Create, "items".to_owned(), "items".to_owned())),
            "{:?}: {changes:?}",
            server.kind
        );
        assert!(
            changes.contains(&(Change::Alter, "owners".to_owned(), "name".to_owned())),
            "{:?}: {changes:?}",
            server.kind
        );
        assert!(
            changes.contains(&(Change::Drop, "old".to_owned(), "old".to_owned())),
            "{:?}: {changes:?}",
            server.kind
        );
        let script = script(&from, &to, &differences, true);
        run_script(&mut h, dialect, &script);
        let after = diff(&from, &compare_model(&mut h, dialect, "cmp_dst"));
        assert!(after.is_empty(), "{:?}: {after:?}\n{script}", server.kind);
        let name = h.run_ok(&["SELECT name FROM cmp_dst.owners"]);
        assert_eq!(name.text(0, 0), "one", "{:?}", server.kind);
        h.run_ok(&schemas[..2]);
    }
}

#[test]
fn a_structure_compare_between_two_dialects_counts_only_names_nulls_and_keys() {
    let (Some(postgres), Some(mysql)) = (only(Kind::Postgres), only(Kind::MySql)) else {
        return;
    };
    let mut pg = Harness::open(&postgres, Settings::default());
    let mut my = Harness::open(&mysql, Settings::default());
    pg.run_ok(&[
        "DROP SCHEMA IF EXISTS cmp_cross CASCADE",
        "CREATE SCHEMA cmp_cross",
        "CREATE TABLE cmp_cross.t (id int PRIMARY KEY, name text NOT NULL, code text UNIQUE)",
    ]);
    my.run_ok(&[
        "DROP DATABASE IF EXISTS cmp_cross",
        "CREATE DATABASE cmp_cross",
        "CREATE TABLE cmp_cross.t (id bigint PRIMARY KEY, name varchar(9), \
           code varchar(9), UNIQUE KEY t_code (code))",
    ]);
    let from = compare_model(&mut pg, Dialect::Postgres, "cmp_cross");
    let to = compare_model(&mut my, Dialect::MySql, "cmp_cross");
    let differences = diff(&from, &to);
    let names: Vec<&str> = differences
        .iter()
        .map(|difference| difference.object().name())
        .collect();
    // Only the `NULL` rule of `name` differs. The types and the key names do not count.
    assert_eq!(names, ["name"], "{differences:?}");
    pg.run_ok(&["DROP SCHEMA cmp_cross CASCADE"]);
    my.run_ok(&["DROP DATABASE cmp_cross"]);
}

/// One table of a data compare: its model, its columns, and its rows.
struct DataSide {
    table: Table,
    columns: Vec<String>,
    rows: Vec<Row>,
}

/// Reads a table as the compare tab does: the column query, and then each row.
fn data_side(h: &mut Harness, dialect: Dialect, schema: &str, table: &str) -> DataSide {
    let options = RunOptions {
        schema: Some(schema.to_owned()),
        only_schema: true,
        row_cap: Some(usize::MAX),
        ..RunOptions::default()
    };
    let sql = table_columns_sql(dialect, schema, table).unwrap();
    let execution = h.manager.run(h.console, vec![sql], options);
    let run = h.collect(execution);
    assert_eq!(run.end, Some(ExecutionEnd::Completed), "{run:?}");
    let model = build(dialect, schema, &[run.rows]);
    let read = h.run_ok(&[&rows_sql(dialect, schema, table)]);
    DataSide {
        table: model.tables[table].clone(),
        columns: read.columns[0].clone(),
        rows: read.rows,
    }
}

fn data_compare(source: DataSide, target: DataSide, dialect: Dialect) -> (DataCompare, Plan) {
    let plan = data_plan(&source.table, &target.table, dialect).unwrap();
    let mut data = DataCompare::new(source.columns, &plan.key).unwrap();
    data.add_source(source.rows);
    data.set_target_columns(&target.columns, &plan.skipped)
        .unwrap();
    data.add_target(target.rows, &Budget::new(usize::MAX));
    data.finish();
    (data, plan)
}

#[test]
fn the_data_script_makes_the_target_rows_equal_on_each_server() {
    for server in servers() {
        let mut h = Harness::open(&server, Settings::default());
        let (dialect, setup) = match server.kind {
            Kind::Postgres => (
                Dialect::Postgres,
                [
                    "DROP SCHEMA IF EXISTS cmp_dsrc CASCADE",
                    "DROP SCHEMA IF EXISTS cmp_ddst CASCADE",
                    "CREATE SCHEMA cmp_dsrc",
                    "CREATE SCHEMA cmp_ddst",
                    "CREATE TABLE cmp_dsrc.t (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
                       name text, price numeric(10, 2), flag boolean, data bytea, \
                       at timestamp, total numeric GENERATED ALWAYS AS (price * 2) STORED)",
                    "CREATE TABLE cmp_ddst.t (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
                       name text, price numeric(10, 2), flag boolean, data bytea, \
                       at timestamp, total numeric GENERATED ALWAYS AS (price * 2) STORED)",
                    "INSERT INTO cmp_dsrc.t (name, price, flag, data, at) VALUES \
                       ('same', 10.5, true, '\\x00ff', '2024-01-02 03:04:05'), \
                       ('it''s', 2, false, NULL, NULL), ('new', 3, NULL, '\\x5c', NULL)",
                    "INSERT INTO cmp_ddst.t (name, price, flag, data, at) VALUES \
                       ('same', 10.50, true, '\\x00ff', '2024-01-02 03:04:05'), \
                       ('old', 2, true, NULL, NULL)",
                ],
            ),
            Kind::MySql => (
                Dialect::MySql,
                [
                    "DROP DATABASE IF EXISTS cmp_dsrc",
                    "DROP DATABASE IF EXISTS cmp_ddst",
                    "CREATE DATABASE cmp_dsrc",
                    "CREATE DATABASE cmp_ddst",
                    "CREATE TABLE cmp_dsrc.t (id int AUTO_INCREMENT PRIMARY KEY, \
                       name varchar(20), price decimal(10, 2), flag tinyint(1), \
                       data varbinary(4), at datetime, \
                       total decimal(12, 2) AS (price * 2) STORED)",
                    "CREATE TABLE cmp_ddst.t (id int AUTO_INCREMENT PRIMARY KEY, \
                       name varchar(20), price decimal(10, 2), flag tinyint(1), \
                       data varbinary(4), at datetime, \
                       total decimal(12, 2) AS (price * 2) STORED)",
                    "INSERT INTO cmp_dsrc.t (name, price, flag, data, at) VALUES \
                       ('same', 10.5, 1, X'00FF', '2024-01-02 03:04:05'), \
                       ('it''s', 2, 0, NULL, NULL), ('new\\\\', 3, NULL, X'5C', NULL)",
                    "INSERT INTO cmp_ddst.t (id, name, price, flag, data, at) VALUES \
                       (1, 'same', 10.50, 1, X'00FF', '2024-01-02 03:04:05'), \
                       (2, 'old', 2, 1, NULL, NULL), (4, 'gone', 4, 0, NULL, NULL)",
                ],
            ),
        };
        h.run_ok(&setup);
        let source = data_side(&mut h, dialect, "cmp_dsrc", "t");
        let target = data_side(&mut h, dialect, "cmp_ddst", "t");
        let (data, plan) = data_compare(source, target, dialect);
        assert_eq!(plan.skipped, ["total"], "{:?}", server.kind);
        assert_eq!(plan.overriding, dialect == Dialect::Postgres);
        let kinds: Vec<&str> = data
            .changes
            .iter()
            .map(|change| match change {
                RowChange::Insert(_) => "insert",
                RowChange::Update { .. } => "update",
                RowChange::Delete(_) => "delete",
            })
            .collect();
        let expected: &[&str] = match server.kind {
            Kind::Postgres => &["update", "insert"],
            Kind::MySql => &["update", "delete", "insert"],
        };
        assert_eq!(kinds, expected, "{:?}", server.kind);
        let table = "cmp_ddst.t";
        let script = data.script(table, dialect, true, plan.overriding);
        run_script(&mut h, dialect, &script);
        let source = data_side(&mut h, dialect, "cmp_dsrc", "t");
        let target = data_side(&mut h, dialect, "cmp_ddst", "t");
        let (after, _) = data_compare(source, target, dialect);
        assert!(
            after.changes.is_empty(),
            "{:?}: {:?}\n{script}",
            server.kind,
            after.changes
        );
        h.run_ok(&setup[..2]);
    }
}

#[test]
fn a_data_compare_between_two_dialects_writes_the_script_of_the_target() {
    let (Some(postgres), Some(mysql)) = (only(Kind::Postgres), only(Kind::MySql)) else {
        return;
    };
    let mut pg = Harness::open(&postgres, Settings::default());
    let mut my = Harness::open(&mysql, Settings::default());
    pg.run_ok(&[
        "DROP SCHEMA IF EXISTS cmp_dcross CASCADE",
        "CREATE SCHEMA cmp_dcross",
        "CREATE TABLE cmp_dcross.x (id bigint PRIMARY KEY, name text, price numeric(10, 2))",
        "INSERT INTO cmp_dcross.x VALUES (1, 'a', 1.5), (2, 'b\\c', 2)",
    ]);
    my.run_ok(&[
        "DROP DATABASE IF EXISTS cmp_dcross",
        "CREATE DATABASE cmp_dcross",
        "CREATE TABLE cmp_dcross.x (id int PRIMARY KEY, name varchar(9), price decimal(10, 2))",
        "INSERT INTO cmp_dcross.x VALUES (1, 'a', 1.50), (3, 'c', 3)",
    ]);
    let source = data_side(&mut pg, Dialect::Postgres, "cmp_dcross", "x");
    let target = data_side(&mut my, Dialect::MySql, "cmp_dcross", "x");
    let (data, plan) = data_compare(source, target, Dialect::MySql);
    assert_eq!(data.changes.len(), 2, "{:?}", data.changes);
    let script = data.script("cmp_dcross.x", Dialect::MySql, true, plan.overriding);
    run_script(&mut my, Dialect::MySql, &script);
    let source = data_side(&mut pg, Dialect::Postgres, "cmp_dcross", "x");
    let target = data_side(&mut my, Dialect::MySql, "cmp_dcross", "x");
    let (after, _) = data_compare(source, target, Dialect::MySql);
    assert!(after.changes.is_empty(), "{:?}\n{script}", after.changes);
    pg.run_ok(&["DROP SCHEMA cmp_dcross CASCADE"]);
    my.run_ok(&["DROP DATABASE cmp_dcross"]);
}
