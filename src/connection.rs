//! Connection definitions and the connections of the running application.

use crate::catalog::{Catalog, RelationRef};
use crate::connector::{Capabilities, Connector};
use crate::exec::{ConnectionId, ConsoleId};
use crate::sql::Dialect;
use crate::store::Scope;
use crate::store::scripts::ScriptEntry;

pub use crate::store::connection::{
    ConnectionKey, DEFAULT_BUSY_SECONDS, DEFAULT_CONNECT_SECONDS, DEFAULT_IDLE_SECONDS,
    DEFAULT_POOL_LIMIT, DEFAULT_SQL_SECONDS, Definition, Source,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogState {
    NotLoaded,
    Loading,
    Loaded,
    /// The next try waits for an execution, so a key does not repeat a failed read.
    Failed,
}

/// One console of a connection in the navigator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeConsole {
    pub name: String,
    /// The tab of an open console.
    pub tab: Option<ConsoleId>,
    /// The file of a saved console. An open console without text has no file.
    pub script: Option<ScriptEntry>,
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
    /// The console rows of the navigator. Only a connection open in the tree keeps them
    /// up to date, because the list reads the disk.
    pub consoles: Vec<TreeConsole>,
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
            consoles: Vec::new(),
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

    pub fn is_production(&self) -> bool {
        self.definition
            .as_ref()
            .is_some_and(|definition| definition.production)
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
