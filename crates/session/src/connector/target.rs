//! The address of a network database. A connection text enters here, so that the
//! form and the command line follow the same rules.

use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::sql::Dialect;
use crate::text;

/// A password. Debug hides its value, and drop clears its memory.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(***)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Postgres,
    MySql,
    Redis,
    Kafka,
    Databricks,
    ClickHouse,
    /// libSQL and Turso: SQLite over the Hrana protocol.
    Libsql,
}

impl Engine {
    /// Returns the dialect of the statements of this source.
    pub fn dialect(self) -> Dialect {
        match self {
            Engine::Postgres => Dialect::Postgres,
            Engine::MySql => Dialect::MySql,
            Engine::Redis => Dialect::Redis,
            Engine::Kafka => Dialect::Kafka,
            Engine::Databricks => Dialect::Databricks,
            Engine::ClickHouse => Dialect::ClickHouse,
            Engine::Libsql => Dialect::Sqlite,
        }
    }

    /// Returns the name of the source for the user.
    pub fn label(self) -> &'static str {
        match self {
            Engine::Postgres => text::VALUE_POSTGRES,
            Engine::MySql => text::VALUE_MYSQL,
            Engine::Redis => text::VALUE_REDIS,
            Engine::Kafka => text::VALUE_KAFKA,
            Engine::Databricks => text::VALUE_DATABRICKS,
            Engine::ClickHouse => text::VALUE_CLICKHOUSE,
            Engine::Libsql => text::VALUE_LIBSQL,
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Engine::Postgres => 5432,
            Engine::MySql => 3306,
            Engine::Redis => 6379,
            Engine::Kafka => 9092,
            Engine::Databricks | Engine::Libsql => 443,
            Engine::ClickHouse => 8123,
        }
    }

    /// Returns false for a source that works without a user name. Databricks and libSQL
    /// read only the token, in the password.
    pub fn needs_user(self) -> bool {
        !matches!(
            self,
            Engine::Redis | Engine::Kafka | Engine::Databricks | Engine::Libsql
        )
    }

    /// Returns true for a source whose password field holds a token and that has no user.
    pub fn uses_token(self) -> bool {
        matches!(self, Engine::Databricks | Engine::Libsql)
    }

    /// Returns true for a source that speaks HTTP, which needs a host and not a socket.
    pub fn uses_http(self) -> bool {
        matches!(
            self,
            Engine::Databricks | Engine::ClickHouse | Engine::Libsql
        )
    }

    /// Returns true when the command line asks for a password that the connection text
    /// does not hold. Databricks has no user, but it always needs its token.
    pub fn needs_password(self) -> bool {
        self.needs_user() || self == Engine::Databricks
    }

    /// Returns true when the database name has a valid form for this source. Kafka and
    /// libSQL have no databases. The database of Databricks is its start catalog.
    pub fn accepts_database(self, name: &str) -> bool {
        match self {
            Engine::Redis => name.parse::<u32>().is_ok(),
            Engine::Kafka | Engine::Libsql => false,
            Engine::Postgres | Engine::MySql | Engine::Databricks | Engine::ClickHouse => true,
        }
    }

    /// Returns the reason that `accepts_database` refuses a name.
    pub fn database_error(self) -> &'static str {
        match self {
            Engine::Kafka => "A Kafka connection has no database.",
            Engine::Libsql => text::LIBSQL_NO_DATABASE,
            _ => "The Redis database must be a number.",
        }
    }
}

/// The SASL mechanism of a Kafka connection with a user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sasl {
    #[serde(rename = "plain")]
    Plain,
    #[serde(rename = "scram-sha-256")]
    ScramSha256,
    #[default]
    #[serde(rename = "scram-sha-512")]
    ScramSha512,
}

impl Sasl {
    pub const ALL: [Sasl; 3] = [Sasl::Plain, Sasl::ScramSha256, Sasl::ScramSha512];

    /// Returns the name of the mechanism for librdkafka and for the connection text.
    pub fn name(self) -> &'static str {
        match self {
            Sasl::Plain => "PLAIN",
            Sasl::ScramSha256 => "SCRAM-SHA-256",
            Sasl::ScramSha512 => "SCRAM-SHA-512",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|sasl| sasl.name().eq_ignore_ascii_case(text))
    }
}

/// The Schema Registry of a Kafka connection. A saved password stays in the system vault,
/// and the connector reads it there when `password` is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    pub url: String,
    /// Empty for a registry without authentication.
    pub user: String,
    /// The password of this session, from the form.
    pub password: Option<Secret>,
}

/// The consumer group of the Kafka reads when the connection sets no group.
pub const KAFKA_READ_GROUP: &str = "sqln-read";

/// The settings that only a Kafka connection uses. Other engines keep the default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KafkaOptions {
    pub sasl: Sasl,
    pub registry: Option<Box<Registry>>,
    /// The consumer group id of the reads. `None` uses `KAFKA_READ_GROUP`.
    pub group: Option<String>,
    /// The patterns of the topics that the catalog shows. Empty shows all topics.
    pub topics: Vec<String>,
    /// The most messages that a read without its own `SCAN` examines. `None` keeps the
    /// value of the settings.
    pub max_scan: Option<u64>,
}

impl KafkaOptions {
    /// Returns the consumer group id of the reads.
    pub fn read_group(&self) -> &str {
        self.group.as_deref().unwrap_or(KAFKA_READ_GROUP)
    }
}

/// The settings that only a Databricks connection uses. Other engines keep the default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DatabricksOptions {
    /// The id of the SQL warehouse that runs the statements.
    pub warehouse: String,
}

/// Reads the warehouse id of a Databricks connection. The id goes into the path of a
/// request, so only ASCII letters and digits are valid.
pub fn warehouse_id(text: &str) -> Result<String, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Err(text::DATABRICKS_NO_WAREHOUSE);
    }
    if !text.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(text::DATABRICKS_BAD_WAREHOUSE);
    }
    Ok(text.to_owned())
}

/// Reads the consumer group id of a Kafka connection. The spaces at the ends go; an empty
/// text means the default group. A space or a control character inside is refused.
pub fn consumer_group(text: &str) -> Result<Option<String>, &'static str> {
    let text = text.trim();
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(crate::text::KAFKA_BAD_GROUP);
    }
    Ok(Some(text.to_owned()).filter(|text| !text.is_empty()))
}

/// Reads the topic patterns of a Kafka connection: a list with commas, such as
/// `orders.*, !*.dlq`. The spaces at the ends of each item go, and an empty item goes.
/// A space or a control character inside a pattern is refused, and `!` needs a pattern.
pub fn topic_filter(text: &str) -> Result<Vec<String>, &'static str> {
    let mut patterns = Vec::new();
    for item in text
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if item == "!" || item.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(crate::text::KAFKA_BAD_TOPICS);
        }
        patterns.push(item.to_owned());
    }
    Ok(patterns)
}

/// Returns true when the patterns show the topic. `*` matches any text, and the case of
/// the letters must match, as in Kafka. Without an include pattern, all topics match the
/// includes. A pattern that starts with `!` then removes its topics.
pub fn topic_visible(patterns: &[String], name: &str) -> bool {
    let (excludes, includes): (Vec<&str>, Vec<&str>) = patterns
        .iter()
        .map(String::as_str)
        .partition(|pattern| pattern.starts_with('!'));
    (includes.is_empty() || includes.iter().any(|pattern| glob(pattern, name)))
        && !excludes.iter().any(|pattern| glob(&pattern[1..], name))
}

/// Matches a name with a pattern where `*` is any text, also an empty text. A UTF-8
/// character of the pattern matches only the same character, so bytes are sufficient.
fn glob(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.as_bytes(), name.as_bytes());
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if pattern.get(p) == Some(&b'*') {
            star = Some((p, n));
            p += 1;
        } else if pattern.get(p) == Some(&name[n]) {
            p += 1;
            n += 1;
        } else if let Some((at, from)) = star {
            star = Some((at, from + 1));
            p = at + 1;
            n = from + 1;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|byte| *byte == b'*')
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Address {
    Tcp { host: String, port: u16 },
    Socket(PathBuf),
}

impl Address {
    /// Returns true for an address that can go without TLS.
    pub fn is_local(&self) -> bool {
        match self {
            Address::Socket(_) => true,
            Address::Tcp { host, .. } => {
                host.eq_ignore_ascii_case("localhost")
                    || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TlsFiles {
    /// Without a file, the system roots verify the server.
    pub ca: Option<PathBuf>,
    pub client: Option<ClientCertificate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientCertificate {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tls {
    Disabled,
    Verify(TlsFiles),
    /// The user made no choice. A local address tries without TLS and then with TLS
    /// when the server needs it. Another address tries with TLS and then without TLS
    /// only when the server answers that it has no TLS.
    Auto,
}

impl Tls {
    /// Returns the mode of the first attempt. Only `Auto` depends on the address.
    pub fn first_try(&self, address: &Address) -> Tls {
        match self {
            Tls::Auto if address.is_local() => Tls::Disabled,
            Tls::Auto => Tls::Verify(TlsFiles::default()),
            explicit => explicit.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkTarget {
    pub engine: Engine,
    pub address: Address,
    pub user: String,
    pub password: Option<Secret>,
    pub database: Option<String>,
    pub tls: Tls,
    pub kafka: KafkaOptions,
    pub databricks: DatabricksOptions,
}

/// What the command line opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    File(PathBuf),
    /// A DuckDB file or one database in memory. The text is empty for memory.
    Duckdb(String),
    /// Boxed, because a network target is much larger than a path. The options are
    /// the parameters of the connection text that only the command line accepts.
    Network {
        target: Box<NetworkTarget>,
        options: ConnectionOptions,
    },
}

/// A connection option of the command line. A flag or a parameter of the connection
/// text sets it. The connection form has its own fields for these options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionName {
    ReadOnly,
    Production,
    Pool,
    ConnectTimeout,
    SqlTimeout,
    IdleTimeout,
    ForeignKeys,
    BusyTimeout,
    RegistryUser,
    MaxScan,
}

impl OptionName {
    pub const ALL: [OptionName; 10] = [
        OptionName::ReadOnly,
        OptionName::Production,
        OptionName::Pool,
        OptionName::ConnectTimeout,
        OptionName::SqlTimeout,
        OptionName::IdleTimeout,
        OptionName::ForeignKeys,
        OptionName::BusyTimeout,
        OptionName::RegistryUser,
        OptionName::MaxScan,
    ];

    pub fn flag(self) -> &'static str {
        match self {
            OptionName::ReadOnly => "--read-only",
            OptionName::Production => "--production",
            OptionName::Pool => "--pool",
            OptionName::ConnectTimeout => "--connect-timeout",
            OptionName::SqlTimeout => "--sql-timeout",
            OptionName::IdleTimeout => "--idle-timeout",
            OptionName::ForeignKeys => "--foreign-keys",
            OptionName::BusyTimeout => "--busy-timeout",
            OptionName::RegistryUser => "--registry-user",
            OptionName::MaxScan => "--max-scan",
        }
    }

    /// Returns the parameter of the connection text. A SQLite option has no parameter,
    /// because a SQLite target is a path.
    pub fn param(self) -> Option<&'static str> {
        match self {
            OptionName::ReadOnly => Some("read_only"),
            OptionName::Production => Some("production"),
            OptionName::Pool => Some("pool"),
            OptionName::ConnectTimeout => Some("connect_timeout"),
            OptionName::SqlTimeout => Some("sql_timeout"),
            OptionName::IdleTimeout => Some("idle_timeout"),
            OptionName::ForeignKeys | OptionName::BusyTimeout => None,
            OptionName::RegistryUser => Some("registry_user"),
            OptionName::MaxScan => Some("max_scan"),
        }
    }

    /// Returns the name of the field of the connection form.
    pub fn title(self) -> &'static str {
        match self {
            OptionName::ReadOnly => "Read only",
            OptionName::Production => "Production",
            OptionName::Pool => "Pool",
            OptionName::ConnectTimeout => "Connect timeout",
            OptionName::SqlTimeout => "SQL timeout",
            OptionName::IdleTimeout => "Idle timeout",
            OptionName::ForeignKeys => "Foreign keys",
            OptionName::BusyTimeout => "Busy timeout",
            OptionName::RegistryUser => "Registry user",
            OptionName::MaxScan => "Max scan",
        }
    }

    /// Returns true for a flag without a value.
    pub fn is_switch(self) -> bool {
        matches!(self, OptionName::ReadOnly | OptionName::Production)
    }

    pub fn from_flag(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|name| name.flag() == text)
    }

    pub fn from_param(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|name| name.param() == Some(text))
    }
}

/// The connection options of the command line. `None` keeps the default of the
/// connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionOptions {
    pub read_only: Option<bool>,
    pub production: Option<bool>,
    pub pool: Option<usize>,
    pub connect_timeout: Option<Duration>,
    /// `Some(None)` runs the statements without a time limit.
    pub sql_timeout: Option<Option<Duration>>,
    /// `Some(None)` keeps the idle sessions open.
    pub idle_timeout: Option<Option<Duration>>,
    pub foreign_keys: Option<bool>,
    pub busy_timeout: Option<Duration>,
    pub registry_user: Option<String>,
    pub max_scan: Option<u64>,
}

fn switch(value: &str, label: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" => Ok(true),
        "false" | "off" => Ok(false),
        _ => Err(format!("{label} needs true, false, on, or off.")),
    }
}

fn number(value: &str, label: &str) -> Result<u64, String> {
    value.parse().map_err(|_| text::form_needs_number(label))
}

fn positive(value: &str, label: &str) -> Result<u64, String> {
    match number(value, label)? {
        0 => Err(format!("{label} needs a number greater than 0.")),
        number => Ok(number),
    }
}

/// Reads a time limit in seconds. 0 means no limit.
fn limit(value: &str, label: &str) -> Result<Option<Duration>, String> {
    let seconds = number(value, label)?;
    Ok((seconds > 0).then(|| Duration::from_secs(seconds)))
}

impl ConnectionOptions {
    /// Returns true when the option has a value.
    pub fn has(&self, name: OptionName) -> bool {
        match name {
            OptionName::ReadOnly => self.read_only.is_some(),
            OptionName::Production => self.production.is_some(),
            OptionName::Pool => self.pool.is_some(),
            OptionName::ConnectTimeout => self.connect_timeout.is_some(),
            OptionName::SqlTimeout => self.sql_timeout.is_some(),
            OptionName::IdleTimeout => self.idle_timeout.is_some(),
            OptionName::ForeignKeys => self.foreign_keys.is_some(),
            OptionName::BusyTimeout => self.busy_timeout.is_some(),
            OptionName::RegistryUser => self.registry_user.is_some(),
            OptionName::MaxScan => self.max_scan.is_some(),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Sets one option from its text. `label` is the flag or the parameter that the user
    /// typed. The error never holds the value.
    pub fn set(&mut self, name: OptionName, value: &str, label: &str) -> Result<(), String> {
        if self.has(name) {
            return Err(format!("Use {label} only once."));
        }
        let value = value.trim();
        match name {
            OptionName::ReadOnly => self.read_only = Some(switch(value, label)?),
            OptionName::Production => self.production = Some(switch(value, label)?),
            OptionName::Pool => {
                self.pool = Some(usize::try_from(positive(value, label)?).unwrap_or(usize::MAX));
            }
            OptionName::ConnectTimeout => {
                self.connect_timeout = Some(Duration::from_secs(positive(value, label)?));
            }
            OptionName::SqlTimeout => self.sql_timeout = Some(limit(value, label)?),
            OptionName::IdleTimeout => self.idle_timeout = Some(limit(value, label)?),
            OptionName::ForeignKeys => self.foreign_keys = Some(switch(value, label)?),
            OptionName::BusyTimeout => {
                self.busy_timeout = Some(Duration::from_secs(number(value, label)?));
            }
            OptionName::RegistryUser => {
                if value.is_empty() || value.chars().any(char::is_control) {
                    return Err(format!("{label} needs a user name."));
                }
                self.registry_user = Some(value.to_owned());
            }
            OptionName::MaxScan => self.max_scan = Some(positive(value, label)?),
        }
        Ok(())
    }

    /// Joins the flags with the parameters of the connection text. One option in both
    /// places is refused, so no value changes without a message.
    pub fn merge(self, uri: Self) -> Result<Self, String> {
        if let Some(name) = OptionName::ALL
            .into_iter()
            .find(|name| self.has(*name) && uri.has(*name))
        {
            return Err(format!(
                "Set {} in the URI or in {}, not both.",
                name.param().unwrap_or(name.flag()),
                name.flag()
            ));
        }
        Ok(Self {
            read_only: self.read_only.or(uri.read_only),
            production: self.production.or(uri.production),
            pool: self.pool.or(uri.pool),
            connect_timeout: self.connect_timeout.or(uri.connect_timeout),
            sql_timeout: self.sql_timeout.or(uri.sql_timeout),
            idle_timeout: self.idle_timeout.or(uri.idle_timeout),
            foreign_keys: self.foreign_keys.or(uri.foreign_keys),
            busy_timeout: self.busy_timeout.or(uri.busy_timeout),
            registry_user: self.registry_user.or(uri.registry_user),
            max_scan: self.max_scan.or(uri.max_scan),
        })
    }
}

/// The scheme that marks a DuckDB target.
const DUCKDB_SCHEME: &str = "duckdb:";

/// Reads the target of the command line. A text with a scheme is a URI, a text with the
/// `duckdb:` scheme is a DuckDB target, and any other text is the path of a SQLite file.
pub fn open_target(text: &str) -> Result<OpenTarget, String> {
    if let Some(rest) = text.strip_prefix(DUCKDB_SCHEME) {
        // `duckdb:` names the empty text, and `duckdb::memory:` keeps `:memory:`. Both
        // open one database in memory.
        return Ok(OpenTarget::Duckdb(rest.to_owned()));
    }
    if text.contains("://") {
        parse_uri_options(text).map(|(target, options)| OpenTarget::Network {
            target: Box::new(target),
            options,
        })
    } else {
        Ok(OpenTarget::File(PathBuf::from(text)))
    }
}

impl NetworkTarget {
    /// Returns a name for the interface. It never holds the password.
    pub fn label(&self) -> String {
        let place = match &self.address {
            Address::Tcp { host, port } => format!("{host}:{port}"),
            Address::Socket(path) => path.display().to_string(),
        };
        match &self.database {
            Some(database) => format!("{place}/{database}"),
            None => place,
        }
    }
}

fn percent_decode(text: &str, name: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let code = bytes
            .get(index + 1..index + 3)
            .and_then(|code| std::str::from_utf8(code).ok())
            .and_then(|code| u8::from_str_radix(code, 16).ok())
            .ok_or_else(|| format!("The {name} has a bad percent code."))?;
        decoded.push(code);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| format!("The {name} is not valid UTF-8."))
}

/// The values of the query part of a connection text.
#[derive(Default)]
struct Parameters {
    tls_mode: Option<String>,
    ca: Option<PathBuf>,
    certificate: Option<PathBuf>,
    key: Option<PathBuf>,
    socket: Option<PathBuf>,
    sasl: Option<Sasl>,
    registry: Option<String>,
    group: Option<String>,
    topics: Vec<String>,
    warehouse: Option<String>,
    options: ConnectionOptions,
}

impl Parameters {
    fn parse(query: &str) -> Result<Self, String> {
        let mut parameters = Self::default();
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value, name)?;
            match name {
                "sslmode" => parameters.tls_mode = Some(value.to_ascii_lowercase()),
                "sslrootcert" => parameters.ca = Some(value.into()),
                "sslcert" => parameters.certificate = Some(value.into()),
                "sslkey" => parameters.key = Some(value.into()),
                "host" | "socket" if value.starts_with('/') => {
                    parameters.socket = Some(value.into());
                }
                "sasl" => {
                    parameters.sasl = Some(
                        Sasl::parse(&value)
                            .ok_or("sasl must be plain, scram-sha-256, or scram-sha-512.")?,
                    );
                }
                "registry" if !value.is_empty() => parameters.registry = Some(value),
                "group" => parameters.group = consumer_group(&value)?,
                "topics" => parameters.topics = topic_filter(&value)?,
                "warehouse" => parameters.warehouse = Some(warehouse_id(&value)?),
                other => match OptionName::from_param(other) {
                    Some(option) => parameters.options.set(option, &value, other)?,
                    None => return Err(format!("Unknown connection parameter: {other}")),
                },
            }
        }
        Ok(parameters)
    }

    /// Without `sslmode` and without TLS files, the mode is `Auto`. TLS files without
    /// `sslmode` ask for TLS.
    fn tls(self) -> Result<Tls, String> {
        let client = match (self.certificate, self.key) {
            (Some(certificate), Some(key)) => Some(ClientCertificate { certificate, key }),
            (None, None) => None,
            _ => return Err("A client certificate needs sslcert and sslkey.".to_owned()),
        };
        let wanted = match self.tls_mode.as_deref() {
            None if self.ca.is_none() && client.is_none() => return Ok(Tls::Auto),
            None => true,
            Some("disable") => false,
            Some("require" | "verify-ca" | "verify-full") => true,
            Some(mode) => {
                return Err(format!(
                    "sslmode={mode} can fall back to a connection without TLS after any \
                     TLS error. Use disable or verify-full, or omit sslmode."
                ));
            }
        };
        if wanted {
            return Ok(Tls::Verify(TlsFiles {
                ca: self.ca,
                client,
            }));
        }
        if self.ca.is_some() || client.is_some() {
            return Err("TLS files need a connection with TLS.".to_owned());
        }
        Ok(Tls::Disabled)
    }
}

fn split_port(place: &str) -> Result<(&str, Option<u16>), String> {
    let (host, port) = match place.strip_prefix('[') {
        Some(rest) => {
            let (host, after) = rest.split_once(']').ok_or("The IPv6 address has no end.")?;
            (host, after.strip_prefix(':'))
        }
        None => match place.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (place, None),
        },
    };
    let port = port
        .filter(|port| !port.is_empty())
        .map(|port| {
            port.parse()
                .map_err(|_| "The port is not valid.".to_owned())
        })
        .transpose()?;
    Ok((host, port))
}

/// Replaces each character of the password of a connection text, so that a screen can
/// show the text. The cursor of a field stays valid, because the length does not change.
pub fn hide_uri_password(uri: &str, mask: char) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_owned();
    };
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let Some((credentials, place)) = authority.rsplit_once('@') else {
        return match authority.split_once(':') {
            // The user can be in the middle of the password, before the `@`.
            Some((user, typed)) if !typed.is_empty() && !tail.starts_with('/') => {
                let hidden: String = typed.chars().map(|_| mask).collect();
                format!("{scheme}://{user}:{hidden}{tail}")
            }
            _ => uri.to_owned(),
        };
    };
    let Some((user, password)) = credentials.split_once(':') else {
        return uri.to_owned();
    };
    let hidden: String = password.chars().map(|_| mask).collect();
    format!("{scheme}://{user}:{hidden}@{place}{tail}")
}

/// Reads `postgres://user:password@host:port/database?sslmode=verify-full`. The
/// connection form uses it, so the options of the command line are refused.
/// The error text never holds the text of the input, because it can hold a password.
pub fn parse_uri(uri: &str) -> Result<NetworkTarget, String> {
    let (target, options) = parse_uri_options(uri)?;
    match OptionName::ALL.into_iter().find(|name| options.has(*name)) {
        Some(name) => Err(format!(
            "{} is valid only on the command line. Use the form fields.",
            name.param().unwrap_or(name.flag())
        )),
        None => Ok(target),
    }
}

/// Reads a connection text of the command line, with its connection options.
pub fn parse_uri_options(uri: &str) -> Result<(NetworkTarget, ConnectionOptions), String> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or("The connection text needs a scheme.")?;
    let scheme = scheme.to_ascii_lowercase();
    let engine = match scheme.as_str() {
        "postgres" | "postgresql" => Engine::Postgres,
        "mysql" | "mariadb" => Engine::MySql,
        "redis" | "rediss" | "valkey" | "valkeys" => Engine::Redis,
        "kafka" | "kafkas" => Engine::Kafka,
        "databricks" => Engine::Databricks,
        "clickhouse" => Engine::ClickHouse,
        "libsql" => Engine::Libsql,
        _ => {
            return Err(
                "The scheme must be postgres, mysql, redis, kafka, databricks, \
                 clickhouse, or libsql."
                    .to_owned(),
            );
        }
    };
    let (location, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = location.split_once('/').unwrap_or((location, ""));
    // A password can hold `@` only in percent form, so the last one ends the user part.
    let (credentials, place) = authority.rsplit_once('@').unwrap_or(("", authority));
    let (user, password) = match credentials.split_once(':') {
        Some((user, password)) => (user, Some(password)),
        None => (credentials, None),
    };
    let user = percent_decode(user, "user")?;
    if user.is_empty() && engine.needs_user() {
        return Err("The connection text needs a user.".to_owned());
    }
    let password = password
        .map(|password| percent_decode(password, "password"))
        .transpose()?
        .map(Secret::new);
    let database = (!path.is_empty())
        .then(|| percent_decode(path, "database"))
        .transpose()?;
    if database
        .as_deref()
        .is_some_and(|name| !engine.accepts_database(name))
    {
        return Err(engine.database_error().to_owned());
    }

    let mut parameters = Parameters::parse(query)?;
    if engine != Engine::Kafka
        && (parameters.sasl.is_some()
            || parameters.registry.is_some()
            || parameters.group.is_some()
            || !parameters.topics.is_empty())
    {
        return Err("sasl, registry, group, and topics are valid only for kafka.".to_owned());
    }
    if engine == Engine::Kafka && parameters.socket.is_some() {
        return Err("A Kafka connection needs a host, not a socket.".to_owned());
    }
    let databricks = match (engine, parameters.warehouse.take()) {
        (Engine::Databricks, Some(warehouse)) => DatabricksOptions { warehouse },
        (Engine::Databricks, None) => return Err(text::DATABRICKS_NO_WAREHOUSE.to_owned()),
        (_, Some(_)) => return Err("warehouse is valid only for databricks.".to_owned()),
        (_, None) => DatabricksOptions::default(),
    };
    if engine.uses_http() && parameters.socket.is_some() {
        return Err(format!(
            "A {} connection needs a host, not a socket.",
            engine.label()
        ));
    }
    let kafka = KafkaOptions {
        sasl: parameters.sasl.take().unwrap_or_default(),
        registry: parameters.registry.take().map(|url| {
            Box::new(Registry {
                url,
                user: String::new(),
                password: None,
            })
        }),
        group: parameters.group.take(),
        topics: std::mem::take(&mut parameters.topics),
        max_scan: None,
    };
    if matches!(scheme.as_str(), "rediss" | "valkeys" | "kafkas") {
        if parameters.tls_mode.as_deref() == Some("disable") {
            return Err("This scheme always uses TLS, so sslmode=disable is not valid.".to_owned());
        }
        parameters.tls_mode = Some("verify-full".to_owned());
    }
    let (host, port) = split_port(place)?;
    let host = percent_decode(host, "host")?;
    let address = match parameters.socket.take() {
        Some(path) if host.is_empty() => Address::Socket(path),
        Some(_) => return Err("Use a host or a socket, not both.".to_owned()),
        None if host.is_empty() => return Err("The connection text needs a host.".to_owned()),
        None => Address::Tcp {
            host,
            port: port.unwrap_or(engine.default_port()),
        },
    };
    let options = std::mem::take(&mut parameters.options);
    let tls = parameters.tls()?;
    let target = NetworkTarget {
        engine,
        address,
        user,
        password,
        database,
        tls,
        kafka,
        databricks,
    };
    Ok((target, options))
}
