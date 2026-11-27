//! Tests with a real ClickHouse server. Start it with `compose.test.yaml` and load
//! `scripts/test-env.sh`. Without the variable, a test prints "skipped".

use std::time::{Duration, Instant};

use sqln_clickhouse::ClickHouseConnector;
use sqln_core::catalog::RelationKind;
use sqln_core::grid::Value;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{Connector, ErrorKind, ResetOutcome, Session, SessionContext};

use crate::{new_sink, received};

fn clickhouse() -> Option<ClickHouseConnector> {
    match std::env::var("SQLN_TEST_CLICKHOUSE") {
        Ok(uri) => Some(ClickHouseConnector::new(parse_uri(&uri).unwrap())),
        Err(_) => {
            println!("skipped: SQLN_TEST_CLICKHOUSE is not set");
            None
        }
    }
}

async fn run(session: &mut Box<dyn Session>, sql: &str) {
    session.run(sql, new_sink(1000).0).await.unwrap();
}

#[tokio::test]
async fn clickhouse_catalog_hides_the_system_databases() {
    let Some(connector) = clickhouse() else {
        return;
    };
    let mut session = connector.connect().await.unwrap();
    run(&mut session, "CREATE DATABASE IF NOT EXISTS sqln_catalog").await;
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS sqln_catalog.orders (id UInt64) ENGINE = MergeTree ORDER BY id",
    )
    .await;
    run(
        &mut session,
        "CREATE VIEW IF NOT EXISTS sqln_catalog.big AS SELECT id FROM sqln_catalog.orders",
    )
    .await;
    let catalog = session.read_catalog().await.unwrap();
    let names: Vec<&str> = catalog.schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"default"), "{names:?}");
    for system in ["system", "INFORMATION_SCHEMA", "information_schema"] {
        assert!(!names.contains(&system), "{names:?}");
    }
    assert_eq!(catalog.current_schema.as_deref(), Some("default"));
    let schema = catalog
        .schemas
        .iter()
        .find(|schema| schema.name == "sqln_catalog")
        .unwrap();
    let kinds: Vec<(&str, RelationKind)> = schema
        .relations
        .iter()
        .map(|relation| (relation.name.as_str(), relation.kind))
        .collect();
    assert_eq!(
        kinds,
        [("big", RelationKind::View), ("orders", RelationKind::Table)]
    );
    let columns = session
        .read_columns("sqln_catalog", "orders")
        .await
        .unwrap();
    assert_eq!(columns, ["id"]);
}

#[tokio::test]
async fn clickhouse_values_keep_their_type() {
    let Some(connector) = clickhouse() else {
        return;
    };
    let mut session = connector.connect().await.unwrap();
    let (sink, mut inbox) = new_sink(1000);
    session
        .run(
            "SELECT toUInt64(7) AS n, 'a\\tb' AS s, NULL AS z, unhex('00FF') AS b",
            sink,
        )
        .await
        .unwrap();
    let (columns, types, rows) = received(&mut inbox);
    assert_eq!(columns, ["n", "s", "z", "b"]);
    assert_eq!(types[0].as_deref(), Some("UInt64"));
    assert_eq!(rows[0].get(0), Some(Value::Number("7")));
    assert_eq!(rows[0].get(1), Some(Value::Text("a\tb")));
    assert_eq!(rows[0].get(2), Some(Value::Null));
    assert_eq!(rows[0].get(3), Some(Value::binary(&[0x00, 0xff])));
}

#[tokio::test]
async fn clickhouse_own_format_shows_the_text_of_the_answer() {
    let Some(connector) = clickhouse() else {
        return;
    };
    let mut session = connector.connect().await.unwrap();
    let (sink, mut inbox) = new_sink(1000);
    session
        .run("SELECT 1 AS n FORMAT JSONEachRow", sink)
        .await
        .unwrap();
    let (columns, _, rows) = received(&mut inbox);
    assert_eq!(columns, ["result"]);
    assert_eq!(rows[0].get(0), Some(Value::Text("{\"n\":1}")));
}

#[tokio::test]
async fn clickhouse_cancel_stops_the_statement_on_the_server() {
    let Some(connector) = clickhouse() else {
        return;
    };
    let mut session = connector.connect().await.unwrap();
    let canceller = session.canceller();
    let started = Instant::now();
    let run = tokio::spawn(async move {
        session
            .run(
                "SELECT sleepEachRow(1) FROM numbers(30) SETTINGS max_block_size = 1",
                new_sink(1000).0,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    canceller.cancel().await.unwrap();
    let error = run.await.unwrap().unwrap_err();
    assert_eq!(error.kind, ErrorKind::Canceled, "{}", error.message);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn clickhouse_read_only_refuses_a_write() {
    let Some(connector) = clickhouse() else {
        return;
    };
    let mut session = connector.connect().await.unwrap();
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS default.sqln_read_only (id UInt8) ENGINE = Memory",
    )
    .await;
    let context = SessionContext {
        read_only: true,
        ..SessionContext::default()
    };
    session.apply_context(&context).await.unwrap();
    let error = session
        .run(
            "INSERT INTO default.sqln_read_only VALUES (1)",
            new_sink(10).0,
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Permission, "{}", error.message);
}

#[tokio::test]
async fn clickhouse_sessions_keep_their_state_until_the_cleanup() {
    let Some(connector) = clickhouse() else {
        return;
    };
    let mut session = connector.connect().await.unwrap();
    run(
        &mut session,
        "CREATE TEMPORARY TABLE sqln_scratch (id UInt8) ENGINE = Memory",
    )
    .await;
    run(&mut session, "SELECT * FROM sqln_scratch").await;
    assert_eq!(session.reset().await, ResetOutcome::Clean);
    let error = session
        .run("SELECT * FROM sqln_scratch", new_sink(10).0)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Syntax, "{}", error.message);
}
