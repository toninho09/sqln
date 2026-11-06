//! Real metadata reads. Network cases require scripts/test-env.sh.
use sqln::{
    ai::{Snapshot, schema::Event},
    connector::{
        network_connector,
        sqlite::{SqliteConnector, SqliteTarget},
        target::parse_uri,
    },
    exec::{ExecEvent, ExecutionEnd, Manager, RunOptions, Settings},
    grid::Budget,
};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn run(
    manager: &Manager,
    events: &mut tokio::sync::mpsc::Receiver<ExecEvent>,
    console: u64,
    sql: Vec<String>,
) {
    let execution = manager.run(console, sql, RunOptions::default());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        while let Ok(event) = events.try_recv() {
            match event {
                ExecEvent::ExecutionFinished {
                    console: id,
                    execution: job,
                    end,
                    ..
                } if id == console && job == execution => {
                    assert_eq!(end, ExecutionEnd::Completed);
                    return;
                }
                ExecEvent::Rows { bytes, .. } => {
                    assert_eq!(bytes, 0);
                }
                _ => {}
            }
        }
        assert!(Instant::now() < deadline, "setup did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn read(manager: &Manager, connection: u64, schemas: Vec<String>) -> Arc<Snapshot> {
    let (sender, receiver) = mpsc::channel();
    manager.load_ai_schema(connection, 1, schemas, CancellationToken::new(), sender);
    loop {
        if let Event::Loaded(id, 1, result) = receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("metadata response")
        {
            assert_eq!(id, connection);
            return result.unwrap();
        }
    }
}
#[test]
fn sqlite_snapshot_has_views_keys_indexes_and_definitions_without_records() {
    let path = crate::common::temp_db("ai-schema");
    rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TABLE parent(a INTEGER, b INTEGER, PRIMARY KEY(a,b)); CREATE TABLE child(id INTEGER PRIMARY KEY, a INTEGER, b INTEGER, label TEXT CHECK(length(label)>0), FOREIGN KEY(a,b) REFERENCES parent(a,b)); CREATE INDEX child_label ON child(label); CREATE VIEW labels AS SELECT label FROM child; INSERT INTO child VALUES(1,NULL,NULL,'never_send_this_record');").unwrap();
    let (mut manager, _events) = Manager::new(Arc::new(Budget::new(64 * 1024 * 1024))).unwrap();
    let connection = manager.add_connection(
        Arc::new(SqliteConnector::new(SqliteTarget::new(path))),
        Settings::default(),
    );
    let snapshot = read(&manager, connection, vec!["main".into()]);
    assert!(snapshot.objects.iter().any(|o| o.name == "labels"));
    let child = snapshot.objects.iter().find(|o| o.name == "child").unwrap();
    let data = serde_json::to_string(child).unwrap();
    assert!(data.contains("child_label"));
    assert!(data.contains("CHECK"));
    assert_eq!(child.targets.len(), 2);
    assert!(!data.contains("never_send_this_record"));
    drop(snapshot);
    manager.shutdown();
}
fn network_schema(variable: &str) {
    let Ok(uri) = std::env::var(variable) else {
        eprintln!("skipped: {variable} is not set");
        return;
    };
    let target = parse_uri(&uri).unwrap();
    let schema = if variable == "SQLN_TEST_POSTGRES" {
        "public".to_owned()
    } else {
        target.database.clone().unwrap()
    };
    let connector = network_connector(target);
    let dialect = connector.dialect();
    let (mut manager, mut events) = Manager::new(Arc::new(Budget::new(128 * 1024 * 1024))).unwrap();
    let connection = manager.add_connection(connector, Settings::default());
    let console = manager.open_console(connection).unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let table = format!("ai_schema_{}", &suffix[..12]);
    let view = format!("{table}_view");
    let index = format!("{table}_idx");
    run(
        &manager,
        &mut events,
        console,
        vec![
            format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY, label VARCHAR(100))"),
            format!("CREATE INDEX {index} ON {table}(label)"),
            format!("CREATE VIEW {view} AS SELECT label FROM {table}"),
        ],
    );
    if dialect == sqln::sql::Dialect::Postgres {
        run(
            &manager,
            &mut events,
            console,
            vec![format!(
                "COMMENT ON COLUMN {table}.label IS 'AI test column'"
            )],
        );
    }
    let snapshot = read(&manager, connection, vec![schema]);
    let object = snapshot.objects.iter().find(|o| o.name == table).unwrap();
    let text = serde_json::to_string(object).unwrap();
    assert!(text.contains(&index));
    assert!(text.contains("primary_key"));
    assert!(
        snapshot
            .objects
            .iter()
            .any(|o| o.name == view && o.metadata.iter().any(|m| m["section"] == "columns"))
    );
    if dialect == sqln::sql::Dialect::Postgres {
        assert!(text.contains("AI test column"));
    }
    run(
        &manager,
        &mut events,
        console,
        vec![format!("DROP VIEW {view}"), format!("DROP TABLE {table}")],
    );
    drop(snapshot);
    manager.shutdown();
}
#[test]
fn postgres_snapshot_uses_the_real_metadata_queries() {
    network_schema("SQLN_TEST_POSTGRES");
}
#[test]
fn mysql_snapshot_uses_the_real_metadata_queries() {
    network_schema("SQLN_TEST_MYSQL");
}
