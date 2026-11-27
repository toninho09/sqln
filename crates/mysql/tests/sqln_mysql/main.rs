#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
//! Tests with servers that speak the MySQL protocol. Start them with
//! `compose.test.yaml` and load `scripts/test-env.sh`. Without the variable, a test
//! prints "skipped".

use sqln_mysql::MySqlConnector;
use sqln_session::connector::target::parse_uri;
use sqln_session::connector::{Connector, ResetOutcome, TransactionState};

fn tidb() -> Option<MySqlConnector> {
    match std::env::var("SQLN_TEST_TIDB") {
        Ok(uri) => Some(MySqlConnector::new(parse_uri(&uri).unwrap())),
        Err(_) => {
            println!("skipped: SQLN_TEST_TIDB is not set");
            None
        }
    }
}

/// TiDB gives the system schemas in capital letters and compares names by case.
#[tokio::test]
async fn tidb_catalog_hides_the_system_schemas() {
    let Some(connector) = tidb() else { return };
    let mut session = connector.connect().await.unwrap();
    let catalog = session.read_catalog().await.unwrap();
    let names: Vec<String> = catalog
        .schemas
        .iter()
        .map(|s| s.name.to_ascii_lowercase())
        .collect();
    assert!(names.contains(&"test".to_owned()), "{names:?}");
    for system in [
        "information_schema",
        "mysql",
        "performance_schema",
        "metrics_schema",
    ] {
        assert!(!names.contains(&system.to_owned()), "{names:?}");
    }
}

#[tokio::test]
async fn tidb_sessions_return_clean_to_the_pool() {
    let Some(connector) = tidb() else { return };
    let mut session = connector.connect().await.unwrap();
    assert_eq!(session.transaction_state().await, TransactionState::None);
    assert_eq!(session.reset().await, ResetOutcome::Clean);
}
