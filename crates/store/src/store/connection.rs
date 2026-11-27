//! The saved definition of a connection and the limits of its executions.

use std::path::PathBuf;
use std::time::Duration;

use super::Scope;
use super::config::DEFAULT_CAP_IN_SQL;
use crate::connector::duckdb::{DuckdbSource, DuckdbTarget};
use crate::connector::sqlite::SqliteTarget;
use crate::connector::target::{Address, NetworkTarget, Tls};
use crate::grid::DEFAULT_LIMIT;

pub const DEFAULT_POOL_LIMIT: usize = 5;
pub const DEFAULT_CONNECT_SECONDS: u64 = 30;
pub const DEFAULT_SQL_SECONDS: u64 = 30;
/// An idle session of the pool closes after this time.
pub const DEFAULT_IDLE_SECONDS: u64 = 300;
pub const DEFAULT_BUSY_SECONDS: u64 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "The application keeps few definitions, and a box would change each use of the network target."
)]
pub enum Source {
    Sqlite(SqliteTarget),
    Duckdb(DuckdbTarget),
    Network(NetworkTarget),
}

/// The configuration of one connection, without any session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub source: Source,
    pub read_only: bool,
    /// Shows a mark on screen, and a run that can change data asks first.
    pub production: bool,
    pub max_connections: usize,
    pub connect_timeout: Duration,
    /// The time limit of one statement. `None` runs without a limit.
    pub sql_timeout: Option<Duration>,
    /// An idle session of the pool closes after this time. `None` keeps it open.
    pub idle_timeout: Option<Duration>,
    /// The folder of the relative paths. Without a folder, the working folder applies.
    pub base: Option<PathBuf>,
    /// The theme of the tabs of this connection. `None` uses the default theme.
    pub theme: Option<super::config::ThemeName>,
    /// The icon in the list of connections. `None` uses the icon of the database.
    pub icon: Option<super::config::ConnectionIcon>,
    /// The folder of the connection in the explorer. `None` puts it at the top level.
    pub folder: Option<String>,
    /// The schemas, or the Redis databases, that the catalog keeps. An empty list keeps
    /// all. The current schema always stays.
    pub schemas: Vec<String>,
}

impl Definition {
    pub fn new(name: impl Into<String>, source: Source) -> Self {
        Self {
            name: name.into(),
            source,
            read_only: false,
            production: false,
            max_connections: DEFAULT_POOL_LIMIT,
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_SECONDS),
            sql_timeout: Some(Duration::from_secs(DEFAULT_SQL_SECONDS)),
            idle_timeout: Some(Duration::from_secs(DEFAULT_IDLE_SECONDS)),
            base: None,
            theme: None,
            icon: None,
            folder: None,
            schemas: Vec::new(),
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
                source: match &target.source {
                    crate::connector::sqlite::SqliteSource::File(path) => {
                        crate::connector::sqlite::SqliteSource::File(self.resolve(path))
                    }
                    memory => memory.clone(),
                },
                ..target.clone()
            }),
            Source::Duckdb(target) => Source::Duckdb(DuckdbTarget {
                source: match &target.source {
                    DuckdbSource::File(path) => DuckdbSource::File(self.resolve(path)),
                    memory => memory.clone(),
                },
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
        let name = match &target.source {
            crate::connector::sqlite::SqliteSource::File(path) => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            crate::connector::sqlite::SqliteSource::Memory => ":memory:".to_owned(),
        };
        Self::new(name, Source::Sqlite(target))
    }

    /// Returns a definition for a DuckDB file, with the file name as its name.
    pub fn duckdb(target: DuckdbTarget) -> Self {
        let name = match &target.source {
            DuckdbSource::File(path) => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            DuckdbSource::Memory => ":memory:".to_owned(),
        };
        Self::new(name, Source::Duckdb(target))
    }

    /// Returns the limits of this connection on top of the application settings.
    /// A database in memory lives in one session, so the pool holds one place and no
    /// idle timeout closes it.
    pub fn settings(&self, base: &Settings) -> Settings {
        let memory = match &self.source {
            Source::Sqlite(target) => target.is_memory(),
            Source::Duckdb(target) => target.is_memory(),
            Source::Network(_) => false,
        };
        Settings {
            read_only: self.read_only || base.read_only,
            max_connections: if memory { 1 } else { self.max_connections },
            connect_timeout: self.connect_timeout,
            sql_timeout: self.sql_timeout,
            idle_timeout: if memory { None } else { self.idle_timeout },
            ..base.clone()
        }
    }

    /// Returns this definition without the password of the session.
    pub fn without_password(&self) -> Self {
        let mut definition = self.clone();
        if let Source::Network(target) = &mut definition.source {
            target.password = None;
            if let Some(registry) = &mut target.kafka.registry {
                registry.password = None;
            }
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
        // The registry password follows the same rule with its own destination.
        if let (Source::Network(new), Source::Network(old)) = (&mut self.source, &old.source)
            && let (Some(new), Some(old)) = (&mut new.kafka.registry, &old.kafka.registry)
            && new.url == old.url
            && new.user == old.user
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
            Source::Sqlite(target) => match &target.source {
                crate::connector::sqlite::SqliteSource::File(path) => path.display().to_string(),
                crate::connector::sqlite::SqliteSource::Memory => {
                    ":memory: (until disconnect)".to_owned()
                }
            },
            Source::Duckdb(target) => match &target.source {
                DuckdbSource::File(path) => format!("DuckDB {}", path.display()),
                DuckdbSource::Memory => "DuckDB :memory: (until disconnect)".to_owned(),
            },
            Source::Network(target) => {
                let tls = match target.tls {
                    Tls::Disabled => "no TLS",
                    Tls::Verify(_) => "TLS",
                    Tls::Auto => "auto TLS",
                };
                let engine = target.engine.label();
                let place = match &target.address {
                    Address::Tcp { host, port } => format!("{host}:{port}"),
                    Address::Socket(path) => path.display().to_string(),
                };
                match target.user.as_str() {
                    "" => format!("{engine} {place} · {tls}"),
                    user => format!("{engine} {user}@{place} · {tls}"),
                }
            }
        }
    }
}

/// The identity of a saved connection. It does not change when the name changes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionKey {
    pub scope: Scope,
    pub id: String,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub row_cap: usize,
    /// The rows of one page in a table tab.
    pub page_size: usize,
    /// `None` runs without a time limit.
    pub sql_timeout: Option<Duration>,
    /// An idle session of the pool closes after this time. `None` keeps it open.
    pub idle_timeout: Option<Duration>,
    pub connect_timeout: Duration,
    /// The time that a statement has to answer after a cancellation.
    pub stop_grace: Duration,
    pub max_connections: usize,
    pub budget_bytes: usize,
    pub read_only: bool,
    /// True adds `LIMIT` with the row cap to a plain read without its own limit.
    pub cap_in_sql: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            row_cap: 1_000,
            page_size: 100,
            sql_timeout: Some(Duration::from_secs(30)),
            idle_timeout: Some(Duration::from_secs(DEFAULT_IDLE_SECONDS)),
            connect_timeout: Duration::from_secs(30),
            stop_grace: Duration::from_secs(5),
            max_connections: 5,
            budget_bytes: DEFAULT_LIMIT,
            read_only: false,
            cap_in_sql: DEFAULT_CAP_IN_SQL,
        }
    }
}
