//! The executor keeps row checks and updates on one session until commit.

use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sqln::catalog::{Catalog, RelationRef};
use sqln::connector::sqlite::{SqliteConnector, SqliteTarget};
use sqln::connector::target::parse_uri;
use sqln::connector::{
    Canceller, ConnectError, Connector, ExecError, ResetOutcome, Session, SessionContext,
    StatementEnd, TransactionCommand, TransactionState, network_connector,
};
use sqln::data_edit::{Failure, Job, Metadata, Outcome, Patch, SavedRow};
use sqln::exec::{ExecEvent, ExecutionEnd, Manager, RunOptions, Settings, StatementStatus};
use sqln::grid::{Budget, Row, Value};
use sqln::sql::Dialect;
use sqln::ui::text;
use tokio::sync::mpsc::Receiver;

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    console: u64,
    budget: Arc<Budget>,
    dialect: Dialect,
}

impl Harness {
    fn new(connector: Arc<dyn Connector>, settings: Settings) -> Self {
        let dialect = connector.dialect();
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let (mut manager, events) = Manager::new(budget.clone()).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            console,
            budget,
            dialect,
        }
    }
    fn next(&mut self) -> ExecEvent {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(event) = self.events.try_recv() {
                return event;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn query(&mut self, sql: &str) -> Vec<Row> {
        let execution = self.manager.run(
            self.console,
            vec![sql.into()],
            RunOptions {
                row_cap: Some(100),
                ..RunOptions::default()
            },
        );
        let mut rows = Vec::new();
        loop {
            match self.next() {
                ExecEvent::Rows {
                    rows: found, bytes, ..
                } => {
                    self.budget.release(bytes);
                    rows.extend(found);
                }
                ExecEvent::StatementFinished {
                    status: StatementStatus::Error(message),
                    ..
                } => panic!("{sql}: {message}"),
                ExecEvent::ExecutionFinished {
                    execution: done,
                    end,
                    ..
                } if done == execution => {
                    assert_eq!(end, ExecutionEnd::Completed, "{sql}");
                    return rows;
                }
                _ => {}
            }
        }
    }
    fn collect_edit(&mut self, execution: u64) -> Result<Outcome, Failure> {
        let mut result = None;
        loop {
            match self.next() {
                ExecEvent::EditFinished {
                    execution: done,
                    result: found,
                    ..
                } if done == execution => result = Some(found),
                ExecEvent::ExecutionFinished {
                    execution: done, ..
                } if done == execution => return result.expect("an edit outcome"),
                ExecEvent::Rows { bytes, .. } => {
                    self.budget.release(bytes);
                    panic!("private rows reach the interface");
                }
                _ => {}
            }
        }
    }
    fn edit(&mut self, job: Job) -> Result<Outcome, Failure> {
        let execution = self.manager.edit(self.console, job);
        self.collect_edit(execution)
    }
    fn metadata(&mut self, schema: &str, table: &str) -> Metadata {
        match self
            .edit(Job::Inspect {
                relation: RelationRef {
                    schema: schema.into(),
                    relation: table.into(),
                },
                dialect: self.dialect,
            })
            .unwrap()
        {
            Outcome::Metadata(metadata) => metadata,
            other => panic!("{other:?}"),
        }
    }
    fn patch(&self, original: &Row, col: usize, value: Value<'_>) -> Patch {
        let current = original
            .cells()
            .enumerate()
            .map(|(index, old)| if index == col { value } else { old })
            .collect();
        Patch {
            original: SavedRow::reserve(original.clone(), &self.budget).unwrap(),
            current: SavedRow::reserve(current, &self.budget).unwrap(),
            columns: BTreeSet::from([col]),
        }
    }
}

#[test]
fn network_updates_preserve_values_reject_conflicts_and_accept_mysql_no_ops() {
    for variable in ["SQLN_TEST_POSTGRES", "SQLN_TEST_MYSQL"] {
        let Ok(uri) = std::env::var(variable) else {
            println!("skipped: {variable} is not set");
            continue;
        };
        let mut h = Harness::new(
            network_connector(parse_uri(&uri).unwrap()),
            Settings::default(),
        );
        let schema = h.query(if h.dialect == Dialect::Postgres {
            "SELECT current_schema()"
        } else {
            "SELECT DATABASE()"
        })[0]
            .text(0)
            .unwrap();
        let table = "sqln_grid_edit_rows";
        h.query(&format!("DROP TABLE IF EXISTS {table}"));
        let engine = if h.dialect == Dialect::MySql {
            " ENGINE=InnoDB"
        } else {
            ""
        };
        h.query(&format!("CREATE TABLE {table} (id INT NOT NULL, part VARCHAR(40) NOT NULL, value TEXT, amount DECIMAL(30,8), PRIMARY KEY (id, part)){engine}"));
        h.query(&format!(
            "INSERT INTO {table} VALUES (1, 'a', 'old', 1), (2, 'b', 'second', 2)"
        ));
        let metadata = h.metadata(&schema, table);
        let select = format!("SELECT * FROM {table} ORDER BY id");
        for value in [
            Value::Text("'\\\n雪; DROP TABLE x; --"),
            Value::Null,
            Value::Text(""),
            Value::Text("NULL"),
        ] {
            let rows = h.query(&select);
            let patch = h.patch(&rows[0], 2, value);
            assert!(matches!(
                h.edit(Job::Apply {
                    metadata: metadata.clone(),
                    patches: vec![patch]
                })
                .unwrap(),
                Outcome::Applied
            ));
            let rows = h.query(&select);
            assert_eq!(rows[0].get(2), Some(value));
        }
        let rows = h.query(&select);
        let patch = h.patch(&rows[0], 3, Value::Text("1"));
        assert!(matches!(
            h.edit(Job::Apply {
                metadata: metadata.clone(),
                patches: vec![patch]
            })
            .unwrap(),
            Outcome::Applied
        ));
        let rows = h.query(&select);
        let patches = vec![
            h.patch(&rows[0], 2, Value::Text("first")),
            h.patch(&rows[1], 2, Value::Text("mine")),
        ];
        h.query(&format!("UPDATE {table} SET amount = 9 WHERE id = 2"));
        let error = h.edit(Job::Apply { metadata, patches }).unwrap_err();
        assert_eq!(error.message, text::EDIT_CONFLICT);
        assert!(!error.uncertain);
        let rows = h.query(&select);
        assert_eq!(rows[0].text(2).as_deref(), Some("NULL"));
        assert_eq!(rows[1].text(2).as_deref(), Some("second"));
        h.query(&format!("DROP TABLE {table}"));
        h.manager.shutdown();
    }
}

#[test]
fn mysql_tables_without_transactional_storage_refuse_grid_editing() {
    let Ok(uri) = std::env::var("SQLN_TEST_MYSQL") else {
        println!("skipped: SQLN_TEST_MYSQL is not set");
        return;
    };
    let mut h = Harness::new(
        network_connector(parse_uri(&uri).unwrap()),
        Settings::default(),
    );
    let schema = h.query("SELECT DATABASE()")[0].text(0).unwrap();
    h.query("DROP TABLE IF EXISTS sqln_grid_edit_myisam");
    h.query("CREATE TABLE sqln_grid_edit_myisam (id INT PRIMARY KEY, value TEXT) ENGINE=MyISAM");
    let error = h
        .edit(Job::Inspect {
            relation: RelationRef {
                schema,
                relation: "sqln_grid_edit_myisam".into(),
            },
            dialect: Dialect::MySql,
        })
        .unwrap_err();
    assert_eq!(error.message, text::EDIT_ENGINE);
    h.query("DROP TABLE sqln_grid_edit_myisam");
    h.manager.shutdown();
}

struct FaultConnector {
    sqlite: SqliteConnector,
    fault: Fault,
    entered: Arc<AtomicBool>,
}
#[derive(Clone, Copy)]
enum Fault {
    Commit,
    Update,
    Rollback,
    Trigger,
}
struct FaultSession {
    inner: Box<dyn Session>,
    fault: Fault,
    entered: Arc<AtomicBool>,
}

#[async_trait]
impl Connector for FaultConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }
    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        Ok(Box::new(FaultSession {
            inner: self.sqlite.connect().await?,
            fault: self.fault,
            entered: self.entered.clone(),
        }))
    }
}

#[async_trait]
impl Session for FaultSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        self.inner.canceller()
    }
    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.inner.apply_context(context).await
    }
    async fn run(
        &mut self,
        sql: &str,
        sink: sqln::exec::ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        if matches!(self.fault, Fault::Commit) && sql == "COMMIT" {
            self.inner.run(sql, sink).await?;
            return Err(ExecError::new("commit response lost"));
        }
        if matches!(self.fault, Fault::Rollback) && sql == "ROLLBACK" {
            return Err(ExecError::new("rollback response lost"));
        }
        if sql.starts_with("UPDATE ") {
            match self.fault {
                Fault::Update => {
                    self.entered.store(true, Ordering::SeqCst);
                    return std::future::pending().await;
                }
                Fault::Rollback => return Err(ExecError::new("update refused")),
                _ => {}
            }
        }
        self.inner.run(sql, sink).await
    }
    async fn transaction_state(&mut self) -> TransactionState {
        self.inner.transaction_state().await
    }
    async fn control(&mut self, command: TransactionCommand) -> Result<(), ExecError> {
        self.inner.control(command).await
    }
    async fn reset(&mut self) -> ResetOutcome {
        self.inner.reset().await
    }
    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        self.inner.read_catalog().await
    }
    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        self.inner.read_columns(schema, relation).await
    }
}

fn faulty(name: &str, fault: Fault) -> (Harness, std::path::PathBuf, Arc<AtomicBool>) {
    let path = crate::common::temp_db(name);
    rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES (1, 'old'), (2, 'second');").unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let connector = Arc::new(FaultConnector {
        sqlite: SqliteConnector::new(SqliteTarget::new(&path)),
        fault,
        entered: entered.clone(),
    });
    let h = Harness::new(
        connector,
        Settings {
            stop_grace: Duration::from_millis(10),
            ..Settings::default()
        },
    );
    (h, path, entered)
}

#[test]
fn grid_operations_never_take_or_rollback_a_manual_console_transaction() {
    let (mut h, _, _) = faulty("edit-manual", Fault::Trigger);
    h.query("BEGIN");
    h.query("UPDATE items SET value = 'uncommitted' WHERE id = 1");
    let error = h
        .edit(Job::Inspect {
            relation: RelationRef {
                schema: "main".into(),
                relation: "items".into(),
            },
            dialect: Dialect::Sqlite,
        })
        .unwrap_err();
    assert_eq!(error.message, text::EDIT_MANUAL_TRANSACTION);
    assert_eq!(
        h.query("SELECT value FROM items WHERE id = 1")[0]
            .text(0)
            .as_deref(),
        Some("uncommitted")
    );
    h.query("ROLLBACK");
    assert_eq!(
        h.query("SELECT value FROM items WHERE id = 1")[0]
            .text(0)
            .as_deref(),
        Some("old")
    );
    h.manager.shutdown();
}

#[test]
fn a_lost_commit_response_is_unknown_and_never_retries() {
    let (mut h, path, _) = faulty("edit-lost-commit", Fault::Commit);
    let metadata = h.metadata("main", "items");
    let rows = h.query("SELECT * FROM items ORDER BY id");
    let patch = h.patch(&rows[0], 1, Value::Text("saved"));
    let error = h
        .edit(Job::Apply {
            metadata,
            patches: vec![patch],
        })
        .unwrap_err();
    assert!(error.uncertain);
    assert!(error.message.contains(text::EDIT_COMMIT_UNKNOWN));
    let value: String = rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT value FROM items WHERE id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "saved");
    h.manager.shutdown();
}

#[test]
fn stop_drops_an_unresponsive_update_and_rolls_back() {
    let (mut h, path, entered) = faulty("edit-stop", Fault::Update);
    let metadata = h.metadata("main", "items");
    let rows = h.query("SELECT * FROM items ORDER BY id");
    let patch = h.patch(&rows[0], 1, Value::Text("pending"));
    let execution = h.manager.edit(
        h.console,
        Job::Apply {
            metadata,
            patches: vec![patch],
        },
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !entered.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    h.manager.stop(h.console);
    let error = h.collect_edit(execution).unwrap_err();
    assert!(!error.uncertain);
    let value: String = rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT value FROM items WHERE id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "old");
    h.manager.shutdown();
}

#[test]
fn a_failed_rollback_drops_the_session_instead_of_reusing_it() {
    let (mut h, _, _) = faulty("edit-rollback", Fault::Rollback);
    let metadata = h.metadata("main", "items");
    let rows = h.query("SELECT * FROM items ORDER BY id");
    let patch = h.patch(&rows[0], 1, Value::Text("pending"));
    assert!(
        !h.edit(Job::Apply {
            metadata,
            patches: vec![patch]
        })
        .unwrap_err()
        .uncertain
    );
    assert_eq!(
        h.query("SELECT value FROM items WHERE id = 1")[0]
            .text(0)
            .as_deref(),
        Some("old")
    );
    h.manager.shutdown();
}

#[test]
fn a_trigger_cannot_silently_replace_another_pending_row() {
    let (mut h, _, _) = faulty("edit-trigger", Fault::Trigger);
    h.query("CREATE TRIGGER change_second AFTER UPDATE ON items WHEN NEW.id = 1 BEGIN UPDATE items SET value = 'trigger' WHERE id = 2; END;");
    let metadata = h.metadata("main", "items");
    let rows = h.query("SELECT * FROM items ORDER BY id");
    let patches = vec![
        h.patch(&rows[0], 1, Value::Text("first")),
        h.patch(&rows[1], 1, Value::Text("mine")),
    ];
    assert_eq!(
        h.edit(Job::Apply { metadata, patches })
            .unwrap_err()
            .message,
        text::EDIT_CONFLICT
    );
    let rows = h.query("SELECT * FROM items ORDER BY id");
    assert_eq!(rows[0].text(1).as_deref(), Some("old"));
    assert_eq!(rows[1].text(1).as_deref(), Some("second"));
    h.manager.shutdown();
}
