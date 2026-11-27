#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
//! Tests with servers that speak the PostgreSQL protocol. Start them with
//! `compose.test.yaml` and load `scripts/test-env.sh`. Without the variable, a test
//! prints "skipped".

use sqln_postgres::PostgresConnector;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{Connector, ResetOutcome, TransactionCommand, TransactionState};

fn cockroach() -> Option<PostgresConnector> {
    match std::env::var("SQLN_TEST_COCKROACH") {
        Ok(uri) => Some(PostgresConnector::new(parse_uri(&uri).unwrap())),
        Err(_) => {
            println!("skipped: SQLN_TEST_COCKROACH is not set");
            None
        }
    }
}

/// CockroachDB gives two different clocks in each statement. The probe must not read
/// that difference as an open transaction.
#[tokio::test]
async fn cockroach_shows_a_transaction_only_while_one_is_open() {
    let Some(connector) = cockroach() else { return };
    let mut session = connector.connect().await.unwrap();
    assert_eq!(session.transaction_state().await, TransactionState::None);
    session.control(TransactionCommand::Begin).await.unwrap();
    assert_eq!(session.transaction_state().await, TransactionState::Open);
    session.control(TransactionCommand::Rollback).await.unwrap();
    assert_eq!(session.transaction_state().await, TransactionState::None);
}

/// CockroachDB refuses `DISCARD PLANS`. The pool must keep the session all the same.
#[tokio::test]
async fn cockroach_sessions_return_clean_to_the_pool() {
    let Some(connector) = cockroach() else { return };
    let mut session = connector.connect().await.unwrap();
    assert_eq!(session.reset().await, ResetOutcome::Clean);
    assert_eq!(session.transaction_state().await, TransactionState::None);
}

#[tokio::test]
async fn cockroach_catalog_hides_the_internal_schema() {
    let Some(connector) = cockroach() else { return };
    let mut session = connector.connect().await.unwrap();
    let catalog = session.read_catalog().await.unwrap();
    let names: Vec<&str> = catalog.schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"public"), "{names:?}");
    assert!(!names.contains(&"crdb_internal"), "{names:?}");
}
