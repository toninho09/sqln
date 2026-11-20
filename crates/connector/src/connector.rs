//! The contract of the connectors and the drivers, at the paths of the old
//! `connector` module. Each driver is a Cargo feature; a build without one opens an
//! [`Unavailable`] connector for its connections.

use std::sync::Arc;

use async_trait::async_trait;
pub use sqln_session::connector::*;
use sqln_sql::sql::Dialect;

use self::target::{Engine, NetworkTarget};
use sqln_store::store::connection::{Definition, Source};

#[cfg(feature = "postgres")]
pub mod postgres {
    pub use sqln_postgres::*;
}

#[cfg(feature = "mysql")]
pub mod mysql {
    pub use sqln_mysql::*;
}

pub mod sqlite {
    pub use sqln_session::connector::sqlite::*;
    #[cfg(feature = "sqlite")]
    pub use sqln_sqlite::*;
}

pub mod duckdb {
    #[cfg(feature = "duckdb")]
    pub use sqln_duckdb::*;
    pub use sqln_session::connector::duckdb::*;
}

pub mod redis {
    #[cfg(feature = "redis")]
    pub use sqln_redis::*;
    pub use sqln_session::connector::redis::*;
}

pub mod kafka {
    #[cfg(feature = "kafka")]
    pub use sqln_kafka::*;
    pub use sqln_session::connector::kafka::*;
}

#[cfg(feature = "databricks")]
pub mod databricks {
    pub use sqln_databricks::*;
}

/// The connector of a source whose driver is not in this build. It keeps the dialect of
/// the source, so the connection stays visible and editable, and each connection
/// attempt explains the missing feature.
pub struct Unavailable {
    dialect: Dialect,
    driver: &'static str,
    feature: &'static str,
}

impl Unavailable {
    pub fn new(dialect: Dialect, driver: &'static str, feature: &'static str) -> Self {
        Self {
            dialect,
            driver,
            feature,
        }
    }

    /// The text of each connection attempt.
    pub fn message(&self) -> String {
        format!(
            "This build of sqln has no {} driver. Install a build with the `{}` feature.",
            self.driver, self.feature
        )
    }
}

#[async_trait]
impl Connector for Unavailable {
    fn dialect(&self) -> Dialect {
        self.dialect
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        Err(ConnectError::new(self.message()))
    }
}

/// Opens the connector of a network address.
pub fn network_connector(target: NetworkTarget) -> Arc<dyn Connector> {
    match target.engine {
        #[cfg(feature = "postgres")]
        Engine::Postgres => Arc::new(postgres::PostgresConnector::new(target)),
        #[cfg(not(feature = "postgres"))]
        Engine::Postgres => Arc::new(Unavailable::new(
            Dialect::Postgres,
            "PostgreSQL",
            "postgres",
        )),
        #[cfg(feature = "mysql")]
        Engine::MySql => Arc::new(mysql::MySqlConnector::new(target)),
        #[cfg(not(feature = "mysql"))]
        Engine::MySql => Arc::new(Unavailable::new(Dialect::MySql, "MySQL", "mysql")),
        #[cfg(feature = "redis")]
        Engine::Redis => Arc::new(redis::RedisConnector::new(target)),
        #[cfg(not(feature = "redis"))]
        Engine::Redis => Arc::new(Unavailable::new(Dialect::Redis, "Redis", "redis")),
        #[cfg(feature = "kafka")]
        Engine::Kafka => Arc::new(kafka::KafkaConnector::new(target)),
        #[cfg(not(feature = "kafka"))]
        Engine::Kafka => Arc::new(Unavailable::new(Dialect::Kafka, "Kafka", "kafka")),
        #[cfg(feature = "databricks")]
        Engine::Databricks => Arc::new(databricks::DatabricksConnector::new(target)),
        #[cfg(not(feature = "databricks"))]
        Engine::Databricks => Arc::new(Unavailable::new(
            Dialect::Databricks,
            "Databricks",
            "databricks",
        )),
    }
}

/// Opens the connector of a saved definition.
pub fn open(definition: &Definition) -> Arc<dyn Connector> {
    match definition.resolved_source() {
        #[cfg(feature = "sqlite")]
        Source::Sqlite(target) => Arc::new(sqlite::SqliteConnector::new(target)),
        #[cfg(not(feature = "sqlite"))]
        Source::Sqlite(_) => Arc::new(Unavailable::new(Dialect::Sqlite, "SQLite", "sqlite")),
        #[cfg(feature = "duckdb")]
        Source::Duckdb(target) => Arc::new(duckdb::DuckdbConnector::new(duckdb::DuckdbTarget {
            // DuckDB applies read-only when the file opens, so the definition decides
            // it here, in one place.
            read_only: definition.read_only,
            ..target
        })),
        #[cfg(not(feature = "duckdb"))]
        Source::Duckdb(_) => Arc::new(Unavailable::new(Dialect::DuckDb, "DuckDB", "duckdb")),
        Source::Network(target) => network_connector(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::sqlite::SqliteTarget;

    fn connect_error(connector: &dyn Connector) -> String {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        match runtime.block_on(connector.connect()) {
            Ok(_) => panic!("the connector connected"),
            Err(error) => error.message,
        }
    }

    #[test]
    fn unavailable_keeps_dialect_and_names_the_feature() {
        let connector = Unavailable::new(Dialect::Kafka, "Kafka", "kafka");
        assert_eq!(connector.dialect(), Dialect::Kafka);
        let message = connect_error(&connector);
        assert!(message.contains("no Kafka driver"), "{message}");
        assert!(message.contains("`kafka` feature"), "{message}");
    }

    #[cfg(not(feature = "sqlite"))]
    #[test]
    fn build_without_sqlite_opens_unavailable_connector() {
        let definition = Definition::sqlite(SqliteTarget::new("missing.db"));
        let connector = open(&definition);
        assert_eq!(connector.dialect(), Dialect::Sqlite);
        let message = connect_error(connector.as_ref());
        assert!(message.contains("no SQLite driver"), "{message}");
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn build_with_sqlite_opens_the_driver() {
        let definition = Definition::sqlite(SqliteTarget::new("missing.db"));
        let connector = open(&definition);
        assert_eq!(connector.dialect(), Dialect::Sqlite);
        let message = connect_error(connector.as_ref());
        assert!(!message.contains("driver"), "{message}");
    }
}
