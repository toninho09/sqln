//! Connection definitions. The list stays in memory until the configuration stage.

use std::sync::Arc;
use std::time::Duration;

use crate::catalog::{Catalog, RelationRef};
use crate::connector::sqlite::{SqliteConnector, SqliteTarget};
use crate::connector::target::{Address, Engine, NetworkTarget, Tls};
use crate::connector::{Capabilities, Connector, network_connector};
use crate::exec::{ConnectionId, Settings};
use crate::sql::Dialect;

pub const DEFAULT_POOL_LIMIT: usize = 5;
pub const DEFAULT_CONNECT_SECONDS: u64 = 30;
pub const DEFAULT_BUSY_SECONDS: u64 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Sqlite(SqliteTarget),
    Network(NetworkTarget),
}

/// The configuration of one connection, without any session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub source: Source,
    pub read_only: bool,
    pub max_connections: usize,
    pub connect_timeout: Duration,
}

impl Definition {
    pub fn new(name: impl Into<String>, source: Source) -> Self {
        Self {
            name: name.into(),
            source,
            read_only: false,
            max_connections: DEFAULT_POOL_LIMIT,
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_SECONDS),
        }
    }

    /// Returns a definition for a file, with the file name as its name.
    pub fn sqlite(target: SqliteTarget) -> Self {
        let name = target.path.file_name().unwrap_or(target.path.as_os_str());
        let name = name.to_string_lossy().into_owned();
        Self::new(name, Source::Sqlite(target))
    }

    pub fn connector(&self) -> Arc<dyn Connector> {
        match &self.source {
            Source::Sqlite(target) => Arc::new(SqliteConnector::new(target.clone())),
            Source::Network(target) => network_connector(target.clone()),
        }
    }

    /// Returns the limits of this connection on top of the application settings.
    pub fn settings(&self, base: &Settings) -> Settings {
        Settings {
            read_only: self.read_only || base.read_only,
            max_connections: self.max_connections,
            connect_timeout: self.connect_timeout,
            ..base.clone()
        }
    }

    /// Returns true when a change of `other` needs a closed connection.
    pub fn same_access(&self, other: &Self) -> bool {
        self.source == other.source
            && self.read_only == other.read_only
            && self.max_connections == other.max_connections
            && self.connect_timeout == other.connect_timeout
    }

    /// Writes the destination, without the password.
    pub fn summary(&self) -> String {
        match &self.source {
            Source::Sqlite(target) => target.path.display().to_string(),
            Source::Network(target) => {
                let tls = match target.tls {
                    Tls::Disabled => "no TLS",
                    Tls::Verify(_) => "TLS",
                };
                let engine = match target.engine {
                    Engine::Postgres => "PostgreSQL",
                    Engine::MySql => "MySQL",
                };
                let place = match &target.address {
                    Address::Tcp { host, port } => format!("{host}:{port}"),
                    Address::Socket(path) => path.display().to_string(),
                };
                format!("{engine} {}@{place} · {tls}", target.user)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogState {
    NotLoaded,
    Loading,
    Loaded,
    /// The next try waits for an execution, so a key does not repeat a failed read.
    Failed,
}

/// One connection of the running application.
pub struct Connection {
    pub id: ConnectionId,
    pub name: String,
    /// A test can give a ready connector, and that connection has no definition.
    pub definition: Option<Definition>,
    pub dialect: Dialect,
    pub capabilities: Capabilities,
    pub catalog: Option<Catalog>,
    /// The schema that a session reads without a selection.
    pub default_schema: Option<String>,
    pub catalog_state: CatalogState,
    pub requested_columns: Vec<RelationRef>,
}

impl Connection {
    pub fn new(
        id: ConnectionId,
        name: String,
        definition: Option<Definition>,
        connector: &dyn Connector,
    ) -> Self {
        Self {
            id,
            name,
            definition,
            dialect: connector.dialect(),
            capabilities: connector.capabilities(),
            catalog: None,
            default_schema: None,
            catalog_state: CatalogState::NotLoaded,
            requested_columns: Vec::new(),
        }
    }

    pub fn set_catalog(&mut self, catalog: Catalog) {
        self.default_schema.clone_from(&catalog.current_schema);
        self.catalog = Some(catalog);
        self.catalog_state = CatalogState::Loaded;
        self.requested_columns.clear();
    }
}

/// Returns true when the name is in use, without a difference of case.
/// `except` is the connection that the form edits.
pub fn name_is_taken(
    connections: &[Connection],
    name: &str,
    except: Option<ConnectionId>,
) -> bool {
    let name = name.trim().to_lowercase();
    connections
        .iter()
        .filter(|connection| Some(connection.id) != except)
        .any(|connection| connection.name.to_lowercase() == name)
}
