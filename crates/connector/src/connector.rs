//! The contract of the connectors and the drivers, at the paths of the old
//! `connector` module.

use std::sync::Arc;

pub use sqln_session::connector::*;

use self::databricks::DatabricksConnector;
use self::duckdb::{DuckdbConnector, DuckdbTarget};
use self::kafka::KafkaConnector;
use self::mysql::MySqlConnector;
use self::postgres::PostgresConnector;
use self::redis::RedisConnector;
use self::sqlite::SqliteConnector;
use self::target::{Engine, NetworkTarget};
use sqln_store::store::connection::{Definition, Source};

pub mod postgres {
    pub use sqln_postgres::*;
}

pub mod mysql {
    pub use sqln_mysql::*;
}

pub mod sqlite {
    pub use sqln_session::connector::sqlite::*;
    pub use sqln_sqlite::*;
}

pub mod duckdb {
    pub use sqln_duckdb::*;
    pub use sqln_session::connector::duckdb::*;
}

pub mod redis {
    pub use sqln_redis::*;
    pub use sqln_session::connector::redis::*;
}

pub mod kafka {
    pub use sqln_kafka::*;
    pub use sqln_session::connector::kafka::*;
}

pub mod databricks {
    pub use sqln_databricks::*;
}

/// Opens the connector of a network address.
pub fn network_connector(target: NetworkTarget) -> Arc<dyn Connector> {
    match target.engine {
        Engine::Postgres => Arc::new(PostgresConnector::new(target)),
        Engine::MySql => Arc::new(MySqlConnector::new(target)),
        Engine::Redis => Arc::new(RedisConnector::new(target)),
        Engine::Kafka => Arc::new(KafkaConnector::new(target)),
        Engine::Databricks => Arc::new(DatabricksConnector::new(target)),
    }
}

/// Opens the connector of a saved definition.
pub fn open(definition: &Definition) -> Arc<dyn Connector> {
    match definition.resolved_source() {
        Source::Sqlite(target) => Arc::new(SqliteConnector::new(target)),
        Source::Duckdb(target) => Arc::new(DuckdbConnector::new(DuckdbTarget {
            // DuckDB applies read-only when the file opens, so the definition decides
            // it here, in one place.
            read_only: definition.read_only,
            ..target
        })),
        Source::Network(target) => network_connector(target),
    }
}
