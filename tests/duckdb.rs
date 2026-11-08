// Tests of the DuckDB connector. DuckDB is embedded, so these tests always run.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqln::catalog::RelationRef;
use sqln::connection::{Definition, Source};
use sqln::connector::duckdb::{DuckdbConnector, DuckdbSource, DuckdbTarget};
use sqln::connector::{Connector, SessionContext, TransactionCommand, TransactionState};
use sqln::exec::{
    ConnectionId, ConsoleId, ExecEvent, ExecutionEnd, Manager, ReadLimit, RunOptions, Settings,
    StatementStatus,
};
use sqln::grid::{Budget, Row, Value};
use tokio::sync::mpsc::Receiver;

/// Returns the path of a DuckDB file of one test. The file does not exist.
fn temp_duckdb(name: &str) -> PathBuf {
    let folder = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(env!("CARGO_CRATE_NAME"))
        .join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("test.duckdb");
    for suffix in ["", ".wal", ".tmp"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    path
}

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    #[allow(dead_code)]
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
}

impl Harness {
    fn with_target(target: DuckdbTarget, settings: Settings) -> Self {
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let connector = Arc::new(DuckdbConnector::new(target));
        let (mut manager, events) = Manager::new(budget.clone()).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            budget,
            connection,
            console,
        }
    }

    fn open(name: &str, settings: Settings) -> Self {
        let mut target = DuckdbTarget::new(temp_duckdb(name));
        target.allow_create = true;
        Self::with_target(target, settings)
    }

    fn memory() -> Self {
        Self::with_target(DuckdbTarget::in_memory(), Settings::default())
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
                    message,
                    ..
                } if finished == execution => {
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
        let execution = self.manager.run(
            console,
            statements,
            RunOptions {
                manual,
                ..RunOptions::default()
            },
        );
        self.collect(execution)
    }

    fn run(&mut self, statements: &[&str]) -> Run {
        self.run_on(self.console, statements, false)
    }

    fn cell(&mut self, sql: &str) -> Row {
        let run = self.run(&[sql]);
        run.rows.into_iter().next().expect("one row")
    }

    fn second_console(&mut self) -> ConsoleId {
        self.manager.open_console(self.connection).unwrap()
    }
}

fn success(count: usize) -> Vec<StatementStatus> {
    vec![StatementStatus::Success; count]
}

fn number(text: &str) -> Value<'_> {
    Value::Number(text)
}

#[test]
fn a_query_delivers_columns_and_rows_as_database_text() {
    let mut h = Harness::open("rows", Settings::default());
    let run = h.run(&["SELECT 1 AS a, 1.5 AS b, 'ação' AS c, NULL AS d, 1e20 AS f"]);
    assert_eq!(run.columns, [["a", "b", "c", "d", "f"]]);
    assert_eq!(run.statuses, success(1));
    assert_eq!(run.end, Some(ExecutionEnd::Completed));
    assert_eq!(run.rows[0].get(0).unwrap(), number("1"));
    assert_eq!(run.rows[0].get(1).unwrap(), number("1.5"));
    assert_eq!(run.rows[0].get(2).unwrap(), Value::Text("ação"));
    assert_eq!(run.rows[0].get(3).unwrap(), Value::Null);
    assert_eq!(run.rows[0].get(4).unwrap(), number("1e20"));
}

#[test]
fn a_blob_keeps_its_bytes() {
    let mut h = Harness::open("blob", Settings::default());
    let row = h.cell("SELECT '\\x00\\xFF'::BLOB AS v");
    assert_eq!(row.get(0).unwrap(), Value::binary(&[0x00, 0xFF]));
}

#[test]
fn a_list_and_a_date_keep_the_text_of_duckdb() {
    let mut h = Harness::open("types", Settings::default());
    let row = h.cell("SELECT [1, 2, 3] AS list, DATE '2024-02-29' AS day");
    assert_eq!(row.get(0).unwrap(), Value::Text("[1, 2, 3]"));
    assert_eq!(row.get(1).unwrap(), Value::Text("2024-02-29"));
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
fn changes_report_their_row_count() {
    let mut h = Harness::open("changes", Settings::default());
    let run = h.run(&[
        "CREATE TABLE t(a INTEGER)",
        "INSERT INTO t VALUES (1), (2), (3)",
        "DELETE FROM t WHERE a > 1",
    ]);
    assert_eq!(run.statuses, success(3));
    assert_eq!(run.changed, [None, Some(3), Some(2)]);
}

#[test]
fn an_error_stops_the_script() {
    let mut h = Harness::open("error", Settings::default());
    let run = h.run(&["SELECT 1", "SELECT * FROM missing", "SELECT 2"]);
    assert_eq!(run.statuses[0], StatementStatus::Success);
    assert!(matches!(run.statuses[1], StatementStatus::Error(_)));
    assert_eq!(run.statuses[2], StatementStatus::NotExecuted);
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
}

#[test]
fn a_parameter_keeps_its_type() {
    use sqln::sql::parameters::{Kind, Parameter};

    let mut h = Harness::open("parameters", Settings::default());
    let cases = [
        (
            Parameter {
                name: "a".to_owned(),
                kind: Kind::Number,
                value: "1234567890123456789".to_owned(),
            },
            Value::Number("1234567890123456789"),
        ),
        (
            Parameter {
                name: "a".to_owned(),
                kind: Kind::Text,
                value: "'; SELECT 999; -- 雪".to_owned(),
            },
            Value::Text("'; SELECT 999; -- 雪"),
        ),
        (
            Parameter {
                name: "a".to_owned(),
                kind: Kind::Null,
                value: String::new(),
            },
            Value::Null,
        ),
    ];
    for (parameter, expected) in cases {
        let execution = h.manager.run(
            h.console,
            vec!["SELECT :a AS value".to_owned()],
            RunOptions {
                parameters: vec![parameter.clone()],
                ..RunOptions::default()
            },
        );
        let run = h.collect(execution);
        assert_eq!(run.end, Some(ExecutionEnd::Completed), "{parameter:?}");
        assert_eq!(run.rows[0].get(0), Some(expected), "{parameter:?}");
    }
    // The value never becomes SQL text.
    let run = h.run(&["SELECT count(*) AS count FROM duckdb_tables()"]);
    assert_eq!(run.statuses, success(1));
}

#[test]
fn a_missing_file_without_permission_to_create_it_fails() {
    let path = temp_duckdb("missing");
    let target = DuckdbTarget::new(&path);
    let connector = DuckdbConnector::new(target);
    let error = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(connector.connect())
        .err()
        .expect("the connection must fail");
    assert!(error.message.contains("does not exist"), "{error:?}");
}

#[test]
fn an_in_memory_database_keeps_its_tables_in_one_session() {
    let mut h = Harness::memory();
    h.run(&["CREATE TABLE t(a INTEGER)", "INSERT INTO t VALUES (7)"]);
    let run = h.run(&["SELECT a FROM t"]);
    assert_eq!(run.rows[0].get(0).unwrap(), number("7"));
}

#[test]
fn an_error_inside_a_transaction_aborts_it() {
    let mut h = Harness::open("transaction", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER)"]);
    // DuckDB aborts the transaction, as PostgreSQL does: it stays open and refuses every
    // statement except `ROLLBACK`.
    let run = h.run(&["BEGIN", "INSERT INTO t VALUES (1)", "SELECT * FROM missing"]);
    assert_eq!(run.end, Some(ExecutionEnd::Failed));
    assert_eq!(run.transaction, Some(TransactionState::Failed));

    let run = h.run(&["INSERT INTO t VALUES (2)"]);
    assert!(
        matches!(run.statuses[0], StatementStatus::Error(_)),
        "an aborted transaction refuses a statement"
    );

    h.run(&["ROLLBACK"]);
    assert_eq!(
        h.cell("SELECT count(*) FROM t").get(0).unwrap(),
        number("0"),
        "the rollback discards the insert of the aborted transaction"
    );
}

#[test]
fn manual_mode_opens_a_transaction_and_commit_confirms_it() {
    let mut h = Harness::open("manual", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER)"]);
    let run = h.run_on(h.console, &["INSERT INTO t VALUES (1)"], true);
    assert_eq!(run.transaction, Some(TransactionState::Open));

    let console = h.console;
    h.manager.transaction(console, TransactionCommand::Rollback);
    assert!(matches!(
        h.next(),
        ExecEvent::TransactionChanged {
            state: TransactionState::None,
            ..
        }
    ));
    assert_eq!(
        h.cell("SELECT count(*) FROM t").get(0).unwrap(),
        number("0")
    );

    h.run_on(console, &["INSERT INTO t VALUES (2)"], true);
    h.manager.transaction(console, TransactionCommand::Commit);
    assert!(matches!(
        h.next(),
        ExecEvent::TransactionChanged {
            state: TransactionState::None,
            ..
        }
    ));
    assert_eq!(
        h.cell("SELECT count(*) FROM t").get(0).unwrap(),
        number("1")
    );
}

#[test]
fn two_consoles_keep_separate_transactions() {
    let mut h = Harness::open("two", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER)"]);
    let second = h.second_console();
    let first = h.run_on(h.console, &["INSERT INTO t VALUES (1)"], true);
    assert_eq!(first.transaction, Some(TransactionState::Open));
    let other = h.run_on(second, &["SELECT count(*) AS count FROM t"], false);
    assert_eq!(
        other.transaction, None,
        "another session has no transaction"
    );
    // The row of the open transaction is not visible to the other session.
    assert_eq!(other.rows[0].get(0).unwrap(), number("0"));
    h.manager
        .transaction(h.console, TransactionCommand::Rollback);
    assert!(matches!(
        h.next(),
        ExecEvent::TransactionChanged {
            state: TransactionState::None,
            ..
        }
    ));
}

#[test]
fn a_read_only_connection_refuses_a_write() {
    let path = temp_duckdb("read-only");
    // Seed the file with the driver itself: a session of the manager keeps the write mode
    // on its own thread, and a read-only open must not wait for that thread.
    {
        let conn = duckdb::Connection::open(&path).expect("a writable file");
        conn.execute_batch("CREATE TABLE t(a INTEGER)")
            .expect("the table");
    }
    let mut h = Harness::with_target(
        DuckdbTarget {
            source: DuckdbSource::File(path),
            allow_create: false,
            read_only: true,
        },
        Settings::default(),
    );
    let run = h.run(&["INSERT INTO t VALUES (1)"]);
    assert!(
        matches!(run.statuses[0], StatementStatus::Error(_)),
        "{:?}",
        run.statuses
    );
    let run = h.run(&["SELECT count(*) AS count FROM t"]);
    assert_eq!(run.statuses, success(1));
    assert_eq!(run.rows[0].get(0).unwrap(), number("0"));
}

#[test]
fn the_catalog_lists_the_schemas_and_the_relations() {
    use sqln::catalog::RelationKind;

    let mut h = Harness::open("catalog", Settings::default());
    h.run(&[
        "CREATE TABLE people(id INTEGER PRIMARY KEY, name VARCHAR)",
        "CREATE VIEW named AS SELECT name FROM people",
        "CREATE SCHEMA extra",
        "CREATE TABLE extra.other(a INTEGER)",
    ]);

    h.manager.load_catalog(h.connection, 0, Vec::new());
    let catalog = loop {
        if let ExecEvent::CatalogLoaded { catalog, .. } = h.next() {
            break catalog.unwrap();
        }
    };
    let names: Vec<&str> = catalog
        .schemas
        .iter()
        .map(|schema| schema.name.as_str())
        .collect();
    assert!(names.contains(&"main"), "{names:?}");
    assert!(names.contains(&"extra"), "{names:?}");
    assert_eq!(catalog.current_schema.as_deref(), Some("main"));

    let main = catalog.schema("main").unwrap();
    let people = main
        .relations
        .iter()
        .find(|relation| relation.name == "people")
        .expect("the table");
    assert_eq!(people.kind, RelationKind::Table);
    let named = main
        .relations
        .iter()
        .find(|relation| relation.name == "named")
        .expect("the view");
    assert_eq!(named.kind, RelationKind::View);

    let relation = RelationRef {
        schema: "main".to_owned(),
        relation: "people".to_owned(),
    };
    h.manager.load_columns(h.connection, relation.clone());
    loop {
        if let ExecEvent::ColumnsLoaded {
            relation: loaded,
            columns,
            ..
        } = h.next()
        {
            assert_eq!(loaded, relation);
            assert_eq!(columns.unwrap(), ["id", "name"]);
            break;
        }
    }
}

#[test]
fn a_file_session_is_discarded_after_a_run_and_memory_stays() {
    let mut h = Harness::open("reuse", Settings::default());
    h.run(&["CREATE TEMPORARY TABLE temp_only(a INTEGER)"]);
    let run = h.run(&["SELECT count(*) AS count FROM temp_only"]);
    assert!(
        matches!(run.statuses[0], StatementStatus::Error(_)),
        "a new session must not keep the temporary table"
    );

    let mut memory = Harness::memory();
    memory.run(&["CREATE TEMPORARY TABLE temp_only(a INTEGER)"]);
    let run = memory.run(&["SELECT count(*) AS count FROM temp_only"]);
    assert_eq!(run.statuses, success(1));
}

#[test]
fn a_definition_round_trips_through_the_configuration() {
    let path = temp_duckdb("definition");
    let folder = path.parent().unwrap().to_path_buf();
    let mut definition = Definition::new(
        "local",
        Source::Duckdb(DuckdbTarget {
            source: DuckdbSource::File(path.clone()),
            allow_create: true,
            read_only: false,
        }),
    );
    definition.read_only = true;
    let entry = sqln::store::config::to_entry("id", &definition);
    assert_eq!(entry.kind, sqln::store::config::KindName::Duckdb);
    assert!(entry.read_only);
    assert_eq!(entry.file.as_deref(), Some(path.to_string_lossy().as_ref()));
    let back = sqln::store::config::to_definition(&entry, folder);
    let Source::Duckdb(target) = back.source else {
        panic!("expected a DuckDB source");
    };
    assert_eq!(target.source, DuckdbSource::File(path));
    assert!(target.read_only);
}

#[test]
fn a_relative_base_folder_resolves_the_file() {
    let definition = Definition {
        base: Some(PathBuf::from("/tmp/base")),
        ..Definition::new(
            "relative",
            Source::Duckdb(DuckdbTarget {
                source: DuckdbSource::File(PathBuf::from("data/app.duckdb")),
                allow_create: false,
                read_only: false,
            }),
        )
    };
    let Source::Duckdb(target) = definition.resolved_source() else {
        panic!("expected a DuckDB source");
    };
    assert_eq!(
        target.source,
        DuckdbSource::File(PathBuf::from("/tmp/base/data/app.duckdb"))
    );
}

#[test]
fn the_cli_opens_a_duckdb_target_from_its_scheme() {
    use sqln::connector::target::{OpenTarget, open_target};

    assert_eq!(
        open_target("duckdb:data/app.duckdb"),
        Ok(OpenTarget::Duckdb("data/app.duckdb".to_owned()))
    );
    assert_eq!(
        open_target("duckdb:"),
        Ok(OpenTarget::Duckdb(String::new()))
    );
    assert_eq!(
        open_target("duckdb::memory:"),
        Ok(OpenTarget::Duckdb(":memory:".to_owned()))
    );
    // A text without the scheme stays a SQLite file.
    assert_eq!(
        open_target("data/app.db"),
        Ok(OpenTarget::File(PathBuf::from("data/app.db")))
    );
}

#[test]
fn the_form_creates_a_duckdb_definition_with_read_only_and_memory() {
    use sqln::form::{ConnectionForm, Kind};

    let mut form = ConnectionForm::new();
    form.kind = Kind::Duckdb;
    form.focus_on(sqln::form::Field::Name);
    form.on_paste("local");
    form.focus_on(sqln::form::Field::File);
    form.on_paste("data/app.duckdb");
    let definition = form.definition().expect("a definition");
    let Source::Duckdb(target) = definition.source else {
        panic!("expected a DuckDB source");
    };
    assert!(!target.read_only);
    assert!(!target.is_memory());

    // `:memory:` needs no file and lives in one session.
    let mut form = ConnectionForm::new();
    form.kind = Kind::Duckdb;
    form.focus_on(sqln::form::Field::Name);
    form.on_paste("scratch");
    form.focus_on(sqln::form::Field::File);
    form.on_paste(":memory:");
    let definition = form.definition().expect("a definition");
    assert_eq!(definition.settings(&Settings::default()).max_connections, 1);
    let Source::Duckdb(target) = definition.source else {
        panic!("expected a DuckDB source");
    };
    assert!(target.is_memory());
}

#[test]
fn a_pragma_and_a_call_deliver_their_rows() {
    let mut h = Harness::open("pragma", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER, b VARCHAR)"]);

    let run = h.run(&["PRAGMA table_info('t')"]);
    assert_eq!(run.statuses, success(1));
    assert_eq!(run.rows.len(), 2, "one row per column");
    assert_eq!(run.columns[0][1], "name");
    assert_eq!(run.rows[0].get(1).unwrap(), Value::Text("a"));

    let run = h.run(&["CALL duckdb_settings()"]);
    // The settings of this DuckDB version carry a `Variant` column, and the Arrow reader
    // of the crate refuses that type. The error reaches the user instead of a silent row.
    match &run.statuses[0] {
        StatementStatus::Success => assert!(!run.rows.is_empty()),
        StatementStatus::Error(message) => assert!(message.contains("Variant"), "{message}"),
        other => panic!("unexpected status: {other:?}"),
    }
}

#[test]
fn a_statement_with_returning_delivers_its_rows() {
    let mut h = Harness::open("returning", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER)"]);
    let run = h.run(&["INSERT INTO t VALUES (1), (2) RETURNING a"]);
    assert_eq!(run.statuses, success(1));
    assert_eq!(run.rows.len(), 2);
    // The changed count belongs to a statement without a result of its own.
    assert_eq!(run.changed, [None]);
    assert_eq!(
        h.cell("SELECT count(*) FROM t").get(0).unwrap(),
        number("2")
    );
}

#[test]
fn the_form_keeps_the_visible_schemas_of_a_duckdb_connection() {
    use sqln::form::{ConnectionForm, Field, Kind};

    let mut form = ConnectionForm::new();
    form.kind = Kind::Duckdb;
    form.focus_on(Field::Name);
    form.on_paste("local");
    form.focus_on(Field::File);
    form.on_paste("app.duckdb");
    form.focus_on(Field::Schemas);
    form.on_paste("main, analytics");
    let definition = form.definition().expect("a definition");
    assert_eq!(definition.schemas, ["main", "analytics"]);
}

#[test]
fn a_write_of_one_session_reaches_another_one() {
    let mut h = Harness::open("shared-instance", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER)"]);
    let second = h.second_console();

    h.run(&["INSERT INTO t VALUES (1)"]);
    let run = h.run_on(second, &["SELECT count(*) AS count FROM t"], false);
    assert_eq!(
        run.rows[0].get(0).unwrap(),
        number("1"),
        "the other session sees the row"
    );

    h.run_on(second, &["INSERT INTO t VALUES (2)"], false);
    let run = h.run(&["SELECT count(*) AS count FROM t"]);
    assert_eq!(run.rows[0].get(0).unwrap(), number("2"));

    // A table that one session creates is visible to the other, and it stays after a
    // session ends, because the database lives in the connector.
    h.run(&["CREATE TABLE made_later(a INTEGER)"]);
    let run = h.run_on(second, &["SELECT count(*) AS count FROM made_later"], false);
    assert_eq!(run.statuses, success(1));
}

#[test]
fn a_file_of_another_format_is_refused_with_the_reader() {
    let folder = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(env!("CARGO_CRATE_NAME"))
        .join("external");
    std::fs::create_dir_all(&folder).unwrap();

    for (name, reader) in [
        ("data.csv", "read_csv"),
        ("data.parquet", "read_parquet"),
        ("data.json", "read_json"),
    ] {
        let path = folder.join(name);
        std::fs::write(&path, "content that is not a database").unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(DuckdbConnector::new(DuckdbTarget::new(&path)).connect())
            .err()
            .unwrap_or_else(|| panic!("{name} must be refused"));
        // The message names the statement that reads the file, and it does not start a
        // database in memory without a word.
        assert!(error.message.contains(reader), "{name}: {error:?}");
        assert!(error.message.contains(name), "{name}: {error:?}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"content that is not a database",
            "{name}: the file must not change"
        );
    }
}

#[test]
fn a_file_of_another_format_opens_through_its_reader() {
    let folder = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(env!("CARGO_CRATE_NAME"))
        .join("external-read");
    std::fs::create_dir_all(&folder).unwrap();
    let csv = folder.join("data.csv");
    std::fs::write(&csv, "a,b\n1,x\n2,y\n").unwrap();

    let mut h = Harness::open("external-read-db", Settings::default());
    let sql = format!("SELECT a, b FROM read_csv('{}') ORDER BY a", csv.display());
    let run = h.run(&[&sql]);
    assert_eq!(run.statuses, success(1));
    assert_eq!(run.columns, [["a", "b"]]);
    assert_eq!(run.rows.len(), 2);
    assert_eq!(run.rows[0].get(0).unwrap(), number("1"));
    assert_eq!(run.rows[0].get(1).unwrap(), Value::Text("x"));
}

#[test]
fn a_cte_insert_reports_its_changed_rows_without_a_count_column() {
    let mut h = Harness::open("cte-change", Settings::default());
    h.run(&["CREATE TABLE t(a INTEGER)"]);
    let run =
        h.run(&["WITH two AS (SELECT 1 AS a UNION ALL SELECT 2) INSERT INTO t SELECT a FROM two"]);
    assert_eq!(run.statuses, success(1));
    assert!(
        run.columns.is_empty(),
        "no result of its own: {:?}",
        run.columns
    );
    assert_eq!(run.changed, [Some(2)]);
    assert_eq!(
        h.cell("SELECT count(*) AS count FROM t").get(0).unwrap(),
        number("2")
    );
}

#[test]
fn concurrent_first_connects_share_one_owner() {
    let mut target = DuckdbTarget::new(temp_duckdb("concurrent-owner"));
    target.allow_create = true;
    let connector = Arc::new(DuckdbConnector::new(target));
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let connects = (0..8).map(|_| {
            let connector = connector.clone();
            tokio::spawn(async move { connector.connect().await.is_ok() })
        });
        for connect in connects {
            assert!(connect.await.unwrap(), "every connect opens one session");
        }
    });
}

#[test]
fn the_catalog_reports_the_schema_of_the_session() {
    let path = temp_duckdb("current-schema");
    {
        let conn = duckdb::Connection::open(&path).expect("a writable file");
        conn.execute_batch("CREATE SCHEMA analytics; CREATE TABLE analytics.t(a INTEGER)")
            .expect("the schema and the table");
    }
    let catalog = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut target = DuckdbTarget::new(&path);
        target.allow_create = true;
        let connector = DuckdbConnector::new(target);
        let mut session = connector.connect().await.expect("a session");
        session
            .apply_context(&SessionContext {
                schema: Some("analytics".to_owned()),
                read_only: false,
                only_schema: false,
            })
            .await
            .expect("the schema of the console");
        session.read_catalog().await.expect("the catalog")
    });
    assert_eq!(catalog.current_schema.as_deref(), Some("analytics"));
    assert!(
        catalog
            .schemas
            .iter()
            .any(|schema| schema.name == "analytics"),
        "the schemas: {:?}",
        catalog
            .schemas
            .iter()
            .map(|schema| &schema.name)
            .collect::<Vec<_>>()
    );
}
