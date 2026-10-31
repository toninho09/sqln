//! Connection definitions and the connections of the running application.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::catalog::{Catalog, RelationRef};
use crate::connector::sqlite::{SqliteConnector, SqliteTarget};
use crate::connector::target::{Address, Engine, NetworkTarget, Tls};
use crate::connector::{Capabilities, Connector, network_connector};
use crate::exec::{ConnectionId, Settings};
use crate::sql::Dialect;
use crate::store::Scope;

pub const DEFAULT_POOL_LIMIT: usize = 5;
pub const DEFAULT_CONNECT_SECONDS: u64 = 30;
pub const DEFAULT_SQL_SECONDS: u64 = 30;
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
    /// The time limit of one statement. `None` runs without a limit.
    pub sql_timeout: Option<Duration>,
    /// The folder of the relative paths. Without a folder, the working folder applies.
    pub base: Option<PathBuf>,
}

impl Definition {
    pub fn new(name: impl Into<String>, source: Source) -> Self {
        Self {
            name: name.into(),
            source,
            read_only: false,
            max_connections: DEFAULT_POOL_LIMIT,
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_SECONDS),
            sql_timeout: Some(Duration::from_secs(DEFAULT_SQL_SECONDS)),
            base: None,
        }
    }

    fn resolve(&self, path: &std::path::Path) -> PathBuf {
        match &self.base {
            Some(base) => base.join(path),
            None => path.to_owned(),
        }
    }

    /// Returns the source with the relative paths of the files in the base folder.
    pub fn resolved_source(&self) -> Source {
        match &self.source {
            Source::Sqlite(target) => Source::Sqlite(SqliteTarget {
                path: self.resolve(&target.path),
                ..target.clone()
            }),
            Source::Network(target) => {
                let mut target = target.clone();
                if let Tls::Verify(files) = &mut target.tls {
                    files.ca = files.ca.as_deref().map(|path| self.resolve(path));
                    if let Some(client) = &mut files.client {
                        client.certificate = self.resolve(&client.certificate);
                        client.key = self.resolve(&client.key);
                    }
                }
                Source::Network(target)
            }
        }
    }

    /// Returns a definition for a file, with the file name as its name.
    pub fn sqlite(target: SqliteTarget) -> Self {
        let name = target.path.file_name().unwrap_or(target.path.as_os_str());
        let name = name.to_string_lossy().into_owned();
        Self::new(name, Source::Sqlite(target))
    }

    pub fn connector(&self) -> Arc<dyn Connector> {
        match self.resolved_source() {
            Source::Sqlite(target) => Arc::new(SqliteConnector::new(target)),
            Source::Network(target) => network_connector(target),
        }
    }

    /// Returns the limits of this connection on top of the application settings.
    pub fn settings(&self, base: &Settings) -> Settings {
        Settings {
            read_only: self.read_only || base.read_only,
            max_connections: self.max_connections,
            connect_timeout: self.connect_timeout,
            sql_timeout: self.sql_timeout,
            ..base.clone()
        }
    }

    /// Returns this definition without the password of the session.
    pub fn without_password(&self) -> Self {
        let mut definition = self.clone();
        if let Source::Network(target) = &mut definition.source {
            target.password = None;
        }
        definition
    }

    /// Keeps a session password only when the destination and transport context match.
    pub fn keep_password_of(&mut self, old: &Self) {
        let same = match (self.resolved_source(), old.resolved_source()) {
            (Source::Network(new), Source::Network(old)) => {
                new.engine == old.engine
                    && new.user == old.user
                    && new.address == old.address
                    && new.tls == old.tls
            }
            _ => false,
        };
        if same
            && let (Source::Network(new), Source::Network(old)) = (&mut self.source, &old.source)
            && new.password.is_none()
        {
            new.password.clone_from(&old.password);
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

/// The identity of a saved connection. It does not change when the name changes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionKey {
    pub scope: Scope,
    pub id: String,
}

/// One connection of the running application.
pub struct Connection {
    pub id: ConnectionId,
    /// The name of the screen. It holds the scope when two scopes use the same name.
    pub name: String,
    /// The place in the files. A connection of a test or of a ready connector has none.
    pub key: Option<ConnectionKey>,
    pub credential: crate::store::config::CredentialName,
    /// Changes when a saved definition changes, so old credential replies cannot apply.
    pub generation: u64,
    /// A test can give a ready connector, and that connection has no definition.
    pub definition: Option<Definition>,
    pub dialect: Dialect,
    pub capabilities: Capabilities,
    pub catalog: Option<Catalog>,
    /// The schema that a session reads without a selection.
    pub default_schema: Option<String>,
    pub catalog_state: CatalogState,
    /// The number of the newest read. The answer of an older read is out of date.
    pub catalog_read: u64,
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
            key: None,
            credential: crate::store::config::CredentialName::None,
            generation: 0,
            definition,
            dialect: connector.dialect(),
            capabilities: connector.capabilities(),
            catalog: None,
            default_schema: None,
            catalog_state: CatalogState::NotLoaded,
            catalog_read: 0,
            requested_columns: Vec::new(),
        }
    }

    /// Returns the name that the user chose, without the mark of the scope.
    pub fn plain_name(&self) -> &str {
        self.definition
            .as_ref()
            .map_or(&self.name, |definition| &definition.name)
    }

    pub fn scope(&self) -> Option<Scope> {
        self.key.as_ref().map(|key| key.scope)
    }

    pub fn set_catalog(&mut self, catalog: Catalog) {
        self.default_schema.clone_from(&catalog.current_schema);
        self.catalog = Some(catalog);
        self.catalog_state = CatalogState::Loaded;
        self.requested_columns.clear();
    }
}

/// Returns true when the name is in use in the scope, without a difference of case.
/// `except` is the connection that the form edits.
pub fn name_is_taken(
    connections: &[Connection],
    name: &str,
    scope: Option<Scope>,
    except: Option<ConnectionId>,
) -> bool {
    let name = name.trim().to_lowercase();
    connections
        .iter()
        .filter(|connection| Some(connection.id) != except && connection.scope() == scope)
        .any(|connection| connection.plain_name().to_lowercase() == name)
}

/// Writes the scope after the name of each connection whose name another scope uses.
pub fn refresh_labels(connections: &mut [Connection]) {
    let names: Vec<(String, Option<Scope>)> = connections
        .iter()
        .map(|item| (item.plain_name().to_lowercase(), item.scope()))
        .collect();
    for connection in connections.iter_mut() {
        let own = connection.plain_name().to_lowercase();
        let scope = connection.scope();
        let shared = names
            .iter()
            .any(|(name, other)| *name == own && *other != scope);
        connection.name = match (shared, scope) {
            (true, Some(Scope::Project)) => format!("{} (local)", connection.plain_name()),
            (true, Some(Scope::Global)) => format!("{} (global)", connection.plain_name()),
            _ => connection.plain_name().to_owned(),
        };
    }
}
