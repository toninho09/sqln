//! `config.json`: the preferences and the connections of one scope.
//!
//! A file holds only the values that its scope declares. A load applies the
//! precedence of the plan: defaults, then the global file, then the project file.
//! A write reads the file again, takes a lock, and replaces the file in one step.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::error::Category;

use super::file::{self, Expect};
use super::{Root, Scope, Store};
use crate::connection::{
    DEFAULT_BUSY_SECONDS, DEFAULT_IDLE_SECONDS, DEFAULT_POOL_LIMIT, DEFAULT_SQL_SECONDS,
    Definition, Source,
};
use crate::connector::duckdb::DuckdbTarget;
use crate::connector::sqlite::{SqliteTarget, file_source};
use crate::connector::target::{Address, ClientCertificate, Engine, NetworkTarget, Tls, TlsFiles};
use crate::editor::{DEFAULT_UNDO_LIMIT, KeyboardMode};
use crate::export::ExportChoices;
use crate::import::ImportChoices;
use crate::input::{Keymap, SuggestionKeys};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub version: u32,
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    pub connections: Vec<ConnectionEntry>,
}

impl ConfigFile {
    pub fn empty() -> Self {
        Self {
            version: VERSION,
            preferences: Preferences::default(),
            connections: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardName {
    Conventional,
    Vim,
}

/// The start opens the tabs of the last session unless a file says otherwise.
pub const DEFAULT_REOPEN_TABS: bool = true;
pub const DEFAULT_QUERY_HISTORY: bool = true;
pub const DEFAULT_FORMAT_JSON: bool = true;
/// A plain read without its own limit receives `LIMIT` with the row cap.
pub const DEFAULT_CAP_IN_SQL: bool = true;
pub const DEFAULT_SHOW_SYSTEM_COMMANDS: bool = false;
/// A run of this time or more rings the bell at its end. The value 0 stops the bell.
pub const DEFAULT_BELL_AFTER_SECONDS: u64 = 10;
/// The instance opens its MCP endpoint unless a file says otherwise.
pub const DEFAULT_MCP: bool = true;
pub const DEFAULT_MCP_REMEMBER_CLIENTS: bool = true;

/// An access level of an MCP session. The names are the `access` values of `request_access`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentAccess {
    /// Reads, read-only queries, and edit proposals.
    Propose,
    /// Also direct edits in all consoles.
    Direct,
    /// Also database writes, with the existing run confirmations.
    #[default]
    Full,
}

impl AgentAccess {
    pub const ALL: [Self; 3] = [Self::Propose, Self::Direct, Self::Full];
}

/// What a new MCP session gets without a question. `Ask` shows the access request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentDefault {
    #[default]
    Ask,
    Propose,
    Direct,
    Full,
}

impl AgentDefault {
    pub const ALL: [Self; 4] = [Self::Ask, Self::Propose, Self::Direct, Self::Full];

    /// Returns the level that a new session gets without a question.
    pub fn access(self) -> Option<AgentAccess> {
        match self {
            Self::Ask => None,
            Self::Propose => Some(AgentAccess::Propose),
            Self::Direct => Some(AgentAccess::Direct),
            Self::Full => Some(AgentAccess::Full),
        }
    }
}

/// The MCP preferences, after the precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentSettings {
    /// False when the instance has no MCP endpoint.
    pub enabled: bool,
    pub default_access: AgentDefault,
    /// The highest level that a session can get.
    pub max_access: AgentAccess,
    /// False when each new session asks again, also for a known client.
    pub remember_clients: bool,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_MCP,
            default_access: AgentDefault::default(),
            max_access: AgentAccess::default(),
            remember_clients: DEFAULT_MCP_REMEMBER_CLIENTS,
        }
    }
}

/// The theme of the interface. `Auto` follows the terminal, and `Ansi` uses the colors
/// of the terminal profile. The other themes need a truecolor terminal. `Custom` names
/// a theme file of the `themes` folder; a missing file falls back to `Auto`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    Auto,
    Astrodark,
    Ansi,
    TokyoNight,
    CatppuccinMocha,
    CatppuccinLatte,
    Gruvbox,
    Nord,
    /// The id of a theme file, without `.json`.
    Custom(String),
}

impl ThemeName {
    /// The built-in themes, in the order of the settings.
    pub const ALL: [Self; 8] = [
        Self::Auto,
        Self::Astrodark,
        Self::Ansi,
        Self::TokyoNight,
        Self::CatppuccinMocha,
        Self::CatppuccinLatte,
        Self::Gruvbox,
        Self::Nord,
    ];

    /// The id of a custom theme.
    pub fn custom_id(&self) -> Option<&str> {
        match self {
            Self::Custom(id) => Some(id),
            _ => None,
        }
    }
}

/// An icon that a connection shows in place of the icon of its database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionIcon {
    Warning,
    Skull,
    Radiation,
    Flag,
    Crown,
    Flask,
    Star,
    Diamond,
    Tools,
    Gear,
    Home,
    Heart,
    Snowflake,
    Check,
}

impl ConnectionIcon {
    /// The icons, in the order of the form.
    pub const ALL: [Self; 14] = [
        Self::Warning,
        Self::Skull,
        Self::Radiation,
        Self::Flag,
        Self::Crown,
        Self::Flask,
        Self::Star,
        Self::Diamond,
        Self::Tools,
        Self::Gear,
        Self::Home,
        Self::Heart,
        Self::Snowflake,
        Self::Check,
    ];
}

/// The symbol set of the interface. `Nerd` needs a Nerd Font in the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconName {
    #[default]
    Unicode,
    Nerd,
}

/// The `keymap` of a file: the keys of each action, for each keyboard mode.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keymaps {
    #[serde(default)]
    pub conventional: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub vim: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_completion: Option<crate::ai::Preferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_mode: Option<KeyboardName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_cap: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap_in_sql: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_budget_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_limit_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keymap: Option<Keymaps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<IconName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopen_tabs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_history: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_json: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_system_commands: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bell_after_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion_keys: Option<SuggestionKeys>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_default_access: Option<AgentDefault>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_max_access: Option<AgentAccess>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_remember_clients: Option<bool>,
    /// The choices of the export dialog. Only the global file holds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<ExportChoices>,
    /// The choices of the import dialog. Only the global file holds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<ImportChoices>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindName {
    Postgres,
    Mysql,
    Sqlite,
    Duckdb,
    Redis,
}

impl KindName {
    /// Returns the engine of a network connection. SQLite and DuckDB have no engine.
    fn engine(self) -> Option<Engine> {
        match self {
            KindName::Postgres => Some(Engine::Postgres),
            KindName::Mysql => Some(Engine::MySql),
            KindName::Redis => Some(Engine::Redis),
            KindName::Sqlite | KindName::Duckdb => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialName {
    Vault,
    Session,
    GlobalPlainText,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    Verify,
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TlsEntry {
    pub mode: TlsMode,
    #[serde(default)]
    pub ca_file: Option<String>,
    #[serde(default)]
    pub client_cert: Option<String>,
    #[serde(default)]
    pub client_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PoolEntry {
    pub max_connections: usize,
}

impl Default for PoolEntry {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_POOL_LIMIT,
        }
    }
}

fn default_sql_timeout() -> Option<u64> {
    Some(DEFAULT_SQL_SECONDS)
}

fn default_idle_timeout() -> Option<u64> {
    Some(DEFAULT_IDLE_SECONDS)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConnectionEntry {
    pub id: String,
    pub name: String,
    pub kind: KindName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default)]
    pub credential: CredentialName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_keys: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy_timeout_millis: Option<u64>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub production: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsEntry>,
    #[serde(default)]
    pub pool: PoolEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_seconds: Option<u64>,
    /// A missing value means the default. A `null` value means no time limit.
    #[serde(default = "default_sql_timeout")]
    pub sql_timeout_seconds: Option<u64>,
    /// A missing value means the default. A `null` value keeps the idle sessions open.
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<ConnectionIcon>,
    /// The schemas, or the Redis databases, that the explorer shows. Empty shows all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<String>,
}

/// The configuration of a scope that cannot load. The text names the file, and the
/// position of the error when the file has one. It never holds a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub file: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.file.display(), self.message)
    }
}

/// A connection of a file, with its place.
#[derive(Clone, Debug)]
pub struct Stored {
    pub scope: Scope,
    pub id: String,
    pub definition: Definition,
    pub credential: CredentialName,
}

/// The configuration of both scopes, after the precedence.
#[derive(Debug)]
pub struct Config {
    pub ai_completion: crate::ai::Preferences,
    pub keyboard: KeyboardMode,
    pub page_size: Option<usize>,
    pub row_cap: Option<usize>,
    /// True when a plain read without its own limit receives `LIMIT` with the row cap.
    pub cap_in_sql: bool,
    pub budget_bytes: Option<usize>,
    /// The limit of the undo history of one console, in the conventional mode.
    pub undo_limit_bytes: usize,
    pub keymap: Keymap,
    pub theme: Option<ThemeName>,
    pub icons: Option<IconName>,
    /// True when the start opens the tabs of the last session.
    pub reopen_tabs: bool,
    /// True when each statement of a console goes to the history file.
    pub query_history: bool,
    /// True when the value window opens a JSON value in its formatted form.
    pub format_json: bool,
    /// True when Messages shows internal and shared commands.
    pub show_system_commands: bool,
    /// A run of this time or more rings the bell at its end. The value 0 stops the bell.
    pub bell_after_seconds: u64,
    /// The keys that accept a suggestion.
    pub suggestion_keys: SuggestionKeys,
    pub agent: AgentSettings,
    /// The last choices of the export dialog.
    pub export: ExportChoices,
    /// The last choices of the import dialog.
    pub import: ImportChoices,
    /// The global connections first, then the project connections.
    pub connections: Vec<Stored>,
}

/// The result of a load. The hashes let the application find a later change of the
/// files, even when the load failed.
#[derive(Debug)]
pub struct Loaded {
    pub hashes: Vec<(Scope, Option<u64>)>,
    pub config: Result<Config, ConfigError>,
}

/// Writes the error of the JSON reader without the values that it saw.
fn describe(error: &serde_json::Error) -> String {
    let place = format!("line {}, column {}", error.line(), error.column());
    let text = error.to_string();
    let text = text
        .rsplit_once(" at line ")
        .map_or(text.as_str(), |(start, _)| start);
    let wrong_value = text.starts_with("invalid type") || text.starts_with("invalid value");
    let what = match error.classify() {
        Category::Io => "The file cannot be read.",
        Category::Eof => "The file ends too soon.",
        Category::Data if wrong_value => "A value has the wrong type or an invalid value.",
        Category::Data | Category::Syntax => text,
    };
    format!("{what} ({place})")
}

fn validate(file: &ConfigFile, scope: Scope) -> Result<(), String> {
    if file.version != VERSION {
        return Err(format!("The version must be {VERSION}."));
    }
    let preferences = &file.preferences;
    for (name, value) in [
        ("pageSize", preferences.page_size),
        ("rowCap", preferences.row_cap),
        ("gridBudgetBytes", preferences.grid_budget_bytes),
        ("undoLimitBytes", preferences.undo_limit_bytes),
    ] {
        if value == Some(0) {
            return Err(format!("{name} needs 1 or more."));
        }
    }
    if let Some(export) = &preferences.export {
        if scope == Scope::Project {
            return Err("export is valid only in the global file.".to_owned());
        }
        if export.batch == 0 {
            return Err("export.batch needs 1 or more.".to_owned());
        }
    }
    if let Some(import) = &preferences.import {
        if scope == Scope::Project {
            return Err("import is valid only in the global file.".to_owned());
        }
        if import.batch == 0 {
            return Err("import.batch needs 1 or more.".to_owned());
        }
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for entry in &file.connections {
        let name = entry.name.trim().to_lowercase();
        if entry.id.trim().is_empty() || name.is_empty() {
            return Err("A connection needs an id and a name.".to_owned());
        }
        if !ids.insert(entry.id.clone()) {
            return Err(format!("Two connections use the id {}.", entry.id));
        }
        if !names.insert(name) {
            return Err(format!("Two connections use the name {}.", entry.name));
        }
        validate_entry(entry, scope)
            .map_err(|reason| format!("Connection {}: {reason}", entry.name))?;
    }
    Ok(())
}

fn non_empty(value: Option<&String>) -> bool {
    value.is_some_and(|text| !text.trim().is_empty())
}

fn validate_entry(entry: &ConnectionEntry, scope: Scope) -> Result<(), String> {
    if entry.credential == CredentialName::GlobalPlainText && scope == Scope::Project {
        return Err("globalPlainText is valid only in the global file.".to_owned());
    }
    if entry.pool.max_connections == 0 {
        return Err("pool.maxConnections needs 1 or more.".to_owned());
    }
    if [
        entry.sql_timeout_seconds,
        entry.connect_timeout_seconds,
        entry.idle_timeout_seconds,
    ]
    .contains(&Some(0))
    {
        return Err("A timeout needs 1 or more seconds.".to_owned());
    }
    match entry.kind.engine() {
        None => {
            if !non_empty(entry.file.as_ref()) {
                return Err(match entry.kind {
                    KindName::Duckdb => "A DuckDB connection needs a file.".to_owned(),
                    _ => "A SQLite connection needs a file.".to_owned(),
                });
            }
        }
        Some(engine) => {
            if !non_empty(entry.host.as_ref())
                || (engine.needs_user() && !non_empty(entry.username.as_ref()))
            {
                return Err("A network connection needs host and username.".to_owned());
            }
            if entry
                .database
                .as_deref()
                .is_some_and(|name| !name.is_empty() && !engine.accepts_database(name))
            {
                return Err("The Redis database must be a number.".to_owned());
            }
            if entry
                .schemas
                .iter()
                .any(|name| !engine.accepts_database(name))
            {
                return Err("Each Redis database in schemas must be a number.".to_owned());
            }
            if let Some(tls) = &entry.tls
                && tls.mode == TlsMode::Disabled
                && (tls.ca_file.is_some() || tls.client_cert.is_some() || tls.client_key.is_some())
            {
                return Err("TLS files need a connection with TLS.".to_owned());
            }
            let client = entry
                .tls
                .as_ref()
                .map(|tls| (&tls.client_cert, &tls.client_key));
            if client.is_some_and(|(cert, key)| cert.is_some() != key.is_some()) {
                return Err("A client certificate needs clientCert and clientKey.".to_owned());
            }
        }
    }
    Ok(())
}

fn seconds(value: Option<u64>) -> Option<Duration> {
    value.map(Duration::from_secs)
}

/// Builds the definition of a validated entry. `base` is the folder of the scope.
pub fn to_definition(entry: &ConnectionEntry, base: PathBuf) -> Definition {
    let source = match entry.kind.engine() {
        None if entry.kind == KindName::Duckdb => {
            let file = entry.file.clone().unwrap_or_default();
            let mut target = DuckdbTarget {
                source: crate::connector::duckdb::file_source(&file),
                ..DuckdbTarget::new(PathBuf::new())
            };
            target.read_only = entry.read_only;
            Source::Duckdb(target)
        }
        None => {
            let file = entry.file.clone().unwrap_or_default();
            let mut target = SqliteTarget {
                source: file_source(&file),
                ..SqliteTarget::new(PathBuf::new())
            };
            target.foreign_keys = entry.foreign_keys;
            target.busy_timeout = Duration::from_millis(
                entry
                    .busy_timeout_millis
                    .unwrap_or(DEFAULT_BUSY_SECONDS * 1_000),
            );
            Source::Sqlite(target)
        }
        Some(engine) => {
            let host = entry.host.clone().unwrap_or_default();
            let address = if host.starts_with('/') {
                Address::Socket(PathBuf::from(host))
            } else {
                Address::Tcp {
                    host,
                    port: entry.port.unwrap_or(engine.default_port()),
                }
            };
            let tls = match &entry.tls {
                Some(tls) if tls.mode == TlsMode::Disabled => Tls::Disabled,
                Some(tls) => Tls::Verify(TlsFiles {
                    ca: tls.ca_file.as_ref().map(PathBuf::from),
                    client: tls.client_cert.as_ref().zip(tls.client_key.as_ref()).map(
                        |(certificate, key)| ClientCertificate {
                            certificate: certificate.into(),
                            key: key.into(),
                        },
                    ),
                }),
                None if address.is_local() => Tls::Disabled,
                None => Tls::Verify(TlsFiles::default()),
            };
            Source::Network(NetworkTarget {
                engine,
                address,
                user: entry.username.clone().unwrap_or_default(),
                password: None,
                database: entry.database.clone().filter(|name| !name.is_empty()),
                tls,
            })
        }
    };
    let mut definition = Definition::new(entry.name.trim(), source);
    definition.read_only = entry.read_only;
    definition.production = entry.production;
    definition.max_connections = entry.pool.max_connections;
    if let Some(connect) = seconds(entry.connect_timeout_seconds) {
        definition.connect_timeout = connect;
    }
    definition.sql_timeout = seconds(entry.sql_timeout_seconds);
    definition.idle_timeout = seconds(entry.idle_timeout_seconds);
    definition.base = Some(base);
    definition.theme = entry.theme.clone();
    definition.icon = entry.icon;
    definition.schemas.clone_from(&entry.schemas);
    definition
}

fn path_text(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// Builds the entry of a definition. The password never goes to the entry.
pub fn to_entry(id: &str, definition: &Definition) -> ConnectionEntry {
    let mut entry = ConnectionEntry {
        id: id.to_owned(),
        name: definition.name.clone(),
        kind: KindName::Sqlite,
        host: None,
        port: None,
        database: None,
        username: None,
        credential: CredentialName::None,
        file: None,
        foreign_keys: None,
        busy_timeout_millis: None,
        read_only: definition.read_only,
        production: definition.production,
        tls: None,
        pool: PoolEntry {
            max_connections: definition.max_connections,
        },
        connect_timeout_seconds: None,
        sql_timeout_seconds: definition.sql_timeout.map(|time| time.as_secs()),
        idle_timeout_seconds: definition.idle_timeout.map(|time| time.as_secs()),
        theme: definition.theme.clone(),
        icon: definition.icon,
        schemas: definition.schemas.clone(),
    };
    match &definition.source {
        Source::Sqlite(target) => {
            entry.kind = KindName::Sqlite;
            entry.file = Some(match &target.source {
                crate::connector::sqlite::SqliteSource::File(path) => path_text(path),
                crate::connector::sqlite::SqliteSource::Memory => ":memory:".to_owned(),
            });
            entry.foreign_keys = target.foreign_keys;
            entry.busy_timeout_millis =
                Some(u64::try_from(target.busy_timeout.as_millis()).unwrap_or(u64::MAX));
        }
        Source::Duckdb(target) => {
            entry.kind = KindName::Duckdb;
            entry.file = Some(match &target.source {
                crate::connector::duckdb::DuckdbSource::File(path) => path_text(path),
                crate::connector::duckdb::DuckdbSource::Memory => ":memory:".to_owned(),
            });
        }
        Source::Network(target) => {
            entry.kind = match target.engine {
                Engine::Postgres => KindName::Postgres,
                Engine::MySql => KindName::Mysql,
                Engine::Redis => KindName::Redis,
            };
            match &target.address {
                Address::Tcp { host, port } => {
                    entry.host = Some(host.clone());
                    entry.port = Some(*port);
                }
                Address::Socket(path) => entry.host = Some(path_text(path)),
            }
            entry.database.clone_from(&target.database);
            entry.username = Some(target.user.clone()).filter(|user| !user.is_empty());
            entry.connect_timeout_seconds = Some(definition.connect_timeout.as_secs());
            entry.tls = Some(match &target.tls {
                Tls::Disabled => TlsEntry {
                    mode: TlsMode::Disabled,
                    ca_file: None,
                    client_cert: None,
                    client_key: None,
                },
                Tls::Verify(files) => TlsEntry {
                    mode: TlsMode::Verify,
                    ca_file: files.ca.as_deref().map(path_text),
                    client_cert: files
                        .client
                        .as_ref()
                        .map(|client| path_text(&client.certificate)),
                    client_key: files.client.as_ref().map(|client| path_text(&client.key)),
                },
            });
            if target.password.is_some() {
                entry.credential = CredentialName::Session;
            }
        }
    }
    entry
}

/// Reads one file. A missing file is an empty configuration.
fn read_scope(root: &Root, scope: Scope) -> (Option<u64>, Result<ConfigFile, ConfigError>) {
    let path = root.config_file();
    let error = |message: String| ConfigError {
        file: path.clone(),
        message,
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(read) if read.kind() == std::io::ErrorKind::NotFound => {
            return (None, Ok(ConfigFile::empty()));
        }
        Err(_) => return (None, Err(error("The file cannot be read.".to_owned()))),
    };
    let hash = Some(file::hash(&bytes));
    let parsed = serde_json::from_slice::<ConfigFile>(&bytes)
        .map_err(|failure| describe(&failure))
        .and_then(|config| validate(&config, scope).map(|()| config))
        .map_err(error);
    (hash, parsed)
}

fn keymap_overrides(preferences: &Preferences, mode: KeyboardMode) -> HashMap<String, Vec<String>> {
    let keymaps = preferences.keymap.clone().unwrap_or_default();
    let section = if mode == KeyboardMode::Vim {
        keymaps.vim
    } else {
        keymaps.conventional
    };
    section.into_iter().collect()
}

fn combined_overrides(
    global: &Preferences,
    project: Option<&Preferences>,
    mode: KeyboardMode,
) -> HashMap<String, Vec<String>> {
    let mut overrides = keymap_overrides(global, mode);
    if let Some(project) = project {
        overrides.extend(keymap_overrides(project, mode));
    }
    overrides
}

/// Checks both modes before the settings dialog writes a file.
pub(crate) fn validate_keymaps(
    global: &Preferences,
    project: Option<&Preferences>,
) -> Result<(), String> {
    for (mode, name) in [
        (KeyboardMode::Conventional, "Conventional"),
        (KeyboardMode::Vim, "Vim"),
    ] {
        for (scope, preferences) in [("Global", Some(global)), ("Project", project)] {
            if let Some(preferences) = preferences {
                Keymap::with_overrides(mode, &keymap_overrides(preferences, mode))
                    .map_err(|message| format!("{name} {scope}: {message}"))?;
            }
        }
        Keymap::with_overrides(mode, &combined_overrides(global, project, mode))
            .map_err(|message| format!("{name} Global + Project: {message}"))?;
    }
    Ok(())
}

fn keyboard_of(preferences: &Preferences) -> Option<KeyboardMode> {
    preferences.keyboard_mode.map(|name| match name {
        KeyboardName::Conventional => KeyboardMode::Conventional,
        KeyboardName::Vim => KeyboardMode::Vim,
    })
}

/// Loads both scopes. The project declares only the values that it changes.
pub fn load(store: &Store) -> Loaded {
    let mut hashes = Vec::new();
    let mut files = Vec::new();
    for (scope, root) in store.scopes() {
        let (hash, result) = read_scope(root, scope);
        hashes.push((scope, hash));
        files.push((scope, root, result));
    }
    let config = combine(files);
    Loaded { hashes, config }
}

fn combine(
    files: Vec<(Scope, &Root, Result<ConfigFile, ConfigError>)>,
) -> Result<Config, ConfigError> {
    let mut parsed = Vec::new();
    for (scope, root, result) in files {
        parsed.push((scope, root, result?));
    }
    let global = &parsed[0].2.preferences;
    let project = parsed.get(1).map(|(_, _, file)| &file.preferences);
    let pick = |value: fn(&Preferences) -> Option<usize>| {
        project.and_then(value).or_else(|| value(global))
    };
    let keyboard = project
        .and_then(keyboard_of)
        .or_else(|| keyboard_of(global))
        .unwrap_or(KeyboardMode::Conventional);
    // Each file must hold a valid map alone, so that the error names its file.
    for (_, root, file) in &parsed {
        Keymap::with_overrides(keyboard, &keymap_overrides(&file.preferences, keyboard)).map_err(
            |message| ConfigError {
                file: root.config_file(),
                message,
            },
        )?;
    }
    let overrides = combined_overrides(global, project, keyboard);
    let last_file = parsed
        .last()
        .map(|(_, root, _)| root.config_file())
        .unwrap_or_default();
    let keymap = Keymap::with_overrides(keyboard, &overrides).map_err(|message| ConfigError {
        file: last_file,
        message,
    })?;
    let connections = parsed
        .iter()
        .flat_map(|(scope, root, file)| {
            file.connections.iter().map(|entry| Stored {
                scope: *scope,
                id: entry.id.clone(),
                definition: to_definition(entry, root.base.clone()),
                credential: entry.credential,
            })
        })
        .collect();
    Ok(Config {
        ai_completion: crate::ai::Preferences::resolve(
            global.ai_completion.as_ref(),
            project.and_then(|p| p.ai_completion.as_ref()),
        ),
        keyboard,
        page_size: pick(|preferences| preferences.page_size),
        row_cap: pick(|preferences| preferences.row_cap),
        cap_in_sql: project
            .and_then(|preferences| preferences.cap_in_sql)
            .or(global.cap_in_sql)
            .unwrap_or(DEFAULT_CAP_IN_SQL),
        budget_bytes: pick(|preferences| preferences.grid_budget_bytes),
        undo_limit_bytes: pick(|preferences| preferences.undo_limit_bytes)
            .unwrap_or(DEFAULT_UNDO_LIMIT),
        keymap,
        theme: project
            .and_then(|preferences| preferences.theme.clone())
            .or_else(|| global.theme.clone()),
        icons: project
            .and_then(|preferences| preferences.icons)
            .or(global.icons),
        reopen_tabs: project
            .and_then(|preferences| preferences.reopen_tabs)
            .or(global.reopen_tabs)
            .unwrap_or(DEFAULT_REOPEN_TABS),
        query_history: project
            .and_then(|preferences| preferences.query_history)
            .or(global.query_history)
            .unwrap_or(DEFAULT_QUERY_HISTORY),
        format_json: project
            .and_then(|preferences| preferences.format_json)
            .or(global.format_json)
            .unwrap_or(DEFAULT_FORMAT_JSON),
        show_system_commands: project
            .and_then(|preferences| preferences.show_system_commands)
            .or(global.show_system_commands)
            .unwrap_or(DEFAULT_SHOW_SYSTEM_COMMANDS),
        bell_after_seconds: project
            .and_then(|preferences| preferences.bell_after_seconds)
            .or(global.bell_after_seconds)
            .unwrap_or(DEFAULT_BELL_AFTER_SECONDS),
        suggestion_keys: project
            .and_then(|preferences| preferences.suggestion_keys)
            .or(global.suggestion_keys)
            .unwrap_or_default(),
        agent: AgentSettings {
            enabled: project
                .and_then(|preferences| preferences.mcp)
                .or(global.mcp)
                .unwrap_or(DEFAULT_MCP),
            default_access: project
                .and_then(|preferences| preferences.mcp_default_access)
                .or(global.mcp_default_access)
                .unwrap_or_default(),
            max_access: project
                .and_then(|preferences| preferences.mcp_max_access)
                .or(global.mcp_max_access)
                .unwrap_or_default(),
            remember_clients: project
                .and_then(|preferences| preferences.mcp_remember_clients)
                .or(global.mcp_remember_clients)
                .unwrap_or(DEFAULT_MCP_REMEMBER_CLIENTS),
        },
        export: global.export.clone().unwrap_or_default(),
        import: global.import.clone().unwrap_or_default(),
        connections,
    })
}

/// A change of the connections of one file.
pub enum Change {
    Upsert(Box<ConnectionEntry>),
    Remove(String),
}

/// Applies a change to the file of a scope. It reads the file again under a lock,
/// so that a change of another instance stays. It returns the hash of the new file.
pub fn change_connections(root: &Root, scope: Scope, change: Change) -> Result<u64, String> {
    change_file(root, scope, |config| match change {
        Change::Upsert(entry) => {
            match config
                .connections
                .iter_mut()
                .find(|item| item.id == entry.id)
            {
                Some(item) => *item = *entry,
                None => config.connections.push(*entry),
            }
        }
        Change::Remove(id) => config.connections.retain(|item| item.id != id),
    })
}

/// Reads the preferences that the file of a scope declares.
pub fn preferences(root: &Root, scope: Scope) -> Result<Preferences, String> {
    let (_, result) = read_scope(root, scope);
    result
        .map(|config| config.preferences)
        .map_err(|error| error.to_string())
}

/// Replaces the preferences of a scope. It returns the hash of the new file.
pub fn change_preferences(
    root: &Root,
    scope: Scope,
    preferences: Preferences,
) -> Result<u64, String> {
    change_file(root, scope, |config| config.preferences = preferences)
}

/// Writes the choices of the import dialog. The other values of the file stay.
pub fn change_import(root: &Root, import: ImportChoices) -> Result<u64, String> {
    change_file(root, Scope::Global, |config| {
        config.preferences.import = Some(import);
    })
}

/// Writes the choices of the export dialog. The other values of the file stay.
pub fn change_export(root: &Root, export: ExportChoices) -> Result<u64, String> {
    change_file(root, Scope::Global, |config| {
        config.preferences.export = Some(export);
    })
}

fn change_file(
    root: &Root,
    scope: Scope,
    change: impl FnOnce(&mut ConfigFile),
) -> Result<u64, String> {
    let _lock =
        file::lock(&root.locks_dir().join("config.lock")).map_err(|error| error.to_string())?;
    let path = root.config_file();
    let (hash, result) = read_scope(root, scope);
    let mut config = result.map_err(|error| error.to_string())?;
    change(&mut config);
    validate(&config, scope)?;
    let mut text = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    text.push('\n');
    let expect = hash.map_or(Expect::Missing, Expect::Hash);
    file::write_private_atomic(&path, text.as_bytes(), expect).map_err(|error| error.to_string())
}
