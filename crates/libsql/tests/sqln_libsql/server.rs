//! Tests with a real libSQL server (`sqld`). Start it with `compose.test.yaml` and load
//! `scripts/test-env.sh`. Without the variable, a test prints "skipped".

use sqln_core::catalog::RelationKind;
use sqln_core::grid::Value;
use sqln_libsql::LibsqlConnector;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{
    Connector, ErrorKind, ResetOutcome, Session, SessionContext, TransactionCommand,
    TransactionState,
};

use crate::{new_sink, received};

fn libsql() -> Option<LibsqlConnector> {
    match std::env::var("SQLN_TEST_LIBSQL") {
        Ok(uri) => Some(LibsqlConnector::new(parse_uri(&uri).unwrap())),
        Err(_) => {
            println!("skipped: SQLN_TEST_LIBSQL is not set");
            None
        }
    }
}

async fn run(session: &mut Box<dyn Session>, sql: &str) {
    session.run(sql, new_sink(1000).0).await.unwrap();
}

#[tokio::test]
async fn libsql_catalog_reads_tables_views_columns_and_keys() {
    let Some(connector) = libsql() else { return };
    let mut session = connector.connect().await.unwrap();
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS sqln_owner (id INTEGER PRIMARY KEY, name TEXT)",
    )
    .await;
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS sqln_pet (id INTEGER PRIMARY KEY, \
         owner INTEGER REFERENCES sqln_owner(id))",
    )
    .await;
    run(
        &mut session,
        "CREATE VIEW IF NOT EXISTS sqln_named AS SELECT name FROM sqln_owner",
    )
    .await;
    let catalog = session.read_catalog().await.unwrap();
    assert_eq!(catalog.current_schema.as_deref(), Some("main"));
    let main = &catalog.schemas[0];
    assert_eq!(main.name, "main");
    let kind = |name: &str| {
        main.relations
            .iter()
            .find(|relation| relation.name == name)
            .map(|relation| relation.kind)
    };
    assert_eq!(kind("sqln_owner"), Some(RelationKind::Table));
    assert_eq!(kind("sqln_named"), Some(RelationKind::View));
    let columns = session.read_columns("main", "sqln_owner").await.unwrap();
    assert_eq!(columns, ["id", "name"]);
    let keys = session.read_foreign_keys("main", "sqln_pet").await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].target.relation, "sqln_owner");
    let referencing = session
        .read_referencing_keys("main", "sqln_owner")
        .await
        .unwrap();
    assert_eq!(referencing.len(), 1);
}

#[tokio::test]
async fn libsql_values_keep_their_type() {
    let Some(connector) = libsql() else { return };
    let mut session = connector.connect().await.unwrap();
    let (sink, mut inbox) = new_sink(1000);
    session
        .run(
            "SELECT 7 AS n, 1.5 AS f, x'00ff' AS b, NULL AS z, 'ação' AS t",
            sink,
        )
        .await
        .unwrap();
    let (columns, _, rows) = received(&mut inbox);
    assert_eq!(columns, ["n", "f", "b", "z", "t"]);
    assert_eq!(rows[0].get(0), Some(Value::Number("7")));
    assert_eq!(rows[0].get(1), Some(Value::Number("1.5")));
    assert_eq!(rows[0].get(2), Some(Value::binary(&[0x00, 0xff])));
    assert_eq!(rows[0].get(3), Some(Value::Null));
    assert_eq!(rows[0].get(4), Some(Value::Text("ação")));
}

#[tokio::test]
async fn libsql_shows_a_transaction_only_while_one_is_open() {
    let Some(connector) = libsql() else { return };
    let mut session = connector.connect().await.unwrap();
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS sqln_tx (id INTEGER PRIMARY KEY)",
    )
    .await;
    assert_eq!(session.transaction_state().await, TransactionState::None);
    session.control(TransactionCommand::Begin).await.unwrap();
    run(&mut session, "INSERT INTO sqln_tx DEFAULT VALUES").await;
    assert_eq!(session.transaction_state().await, TransactionState::Open);
    session.control(TransactionCommand::Rollback).await.unwrap();
    assert_eq!(session.transaction_state().await, TransactionState::None);
}

#[tokio::test]
async fn libsql_cleanup_closes_the_stream_and_its_transaction() {
    let Some(connector) = libsql() else { return };
    let mut session = connector.connect().await.unwrap();
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS sqln_cleanup (id INTEGER)",
    )
    .await;
    run(&mut session, "DELETE FROM sqln_cleanup").await;
    session.control(TransactionCommand::Begin).await.unwrap();
    run(&mut session, "INSERT INTO sqln_cleanup VALUES (1)").await;
    assert_eq!(session.reset().await, ResetOutcome::Clean);
    assert_eq!(session.transaction_state().await, TransactionState::None);
    let (sink, mut inbox) = new_sink(10);
    session
        .run("SELECT count(*) FROM sqln_cleanup", sink)
        .await
        .unwrap();
    assert_eq!(received(&mut inbox).2[0].get(0), Some(Value::Number("0")));
}

#[tokio::test]
async fn libsql_read_only_refuses_a_write() {
    let Some(connector) = libsql() else { return };
    let mut session = connector.connect().await.unwrap();
    run(
        &mut session,
        "CREATE TABLE IF NOT EXISTS sqln_read_only (id INTEGER)",
    )
    .await;
    assert_eq!(session.reset().await, ResetOutcome::Clean);
    let context = SessionContext {
        read_only: true,
        ..SessionContext::default()
    };
    session.apply_context(&context).await.unwrap();
    let error = session
        .run("INSERT INTO sqln_read_only VALUES (1)", new_sink(10).0)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Permission, "{}", error.message);
    assert_eq!(error.message, sqln_core::text::LIBSQL_READ_ONLY);
    // The cleanup removes the rule.
    assert_eq!(session.reset().await, ResetOutcome::Clean);
    run(&mut session, "INSERT INTO sqln_read_only VALUES (1)").await;
}
