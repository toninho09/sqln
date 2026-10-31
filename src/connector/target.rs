//! The address of a network database. A connection text enters here, so that the
//! form and the command line follow the same rules.

use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

/// A password. Its debug text hides the value, so a log cannot show it.
#[derive(Clone, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Postgres,
    MySql,
}

impl Engine {
    fn default_port(self) -> u16 {
        match self {
            Engine::Postgres => 5432,
            Engine::MySql => 3306,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TlsFiles {
    /// Without a file, the system roots verify the server.
    pub ca: Option<PathBuf>,
    pub client: Option<ClientCertificate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientCertificate {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tls {
    Disabled,
    Verify(TlsFiles),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkTarget {
    pub engine: Engine,
    pub address: Address,
    pub user: String,
    pub password: Option<Secret>,
    pub database: Option<String>,
    pub tls: Tls,
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
                other => return Err(format!("Unknown connection parameter: {other}")),
            }
        }
        Ok(parameters)
    }

    /// Without `sslmode`, a local address uses no TLS and another address verifies.
    fn tls(self, address: &Address) -> Result<Tls, String> {
        let wanted = match self.tls_mode.as_deref() {
            None => !address.is_local(),
            Some("disable") => false,
            Some("require" | "verify-ca" | "verify-full") => true,
            Some(mode) => {
                return Err(format!(
                    "sslmode={mode} can fall back to a connection without TLS. \
                     Use disable or verify-full."
                ));
            }
        };
        let client = match (self.certificate, self.key) {
            (Some(certificate), Some(key)) => Some(ClientCertificate { certificate, key }),
            (None, None) => None,
            _ => return Err("A client certificate needs sslcert and sslkey.".to_owned()),
        };
        if wanted {
            return Ok(Tls::Verify(TlsFiles { ca: self.ca, client }));
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
        .map(|port| port.parse().map_err(|_| "The port is not valid.".to_owned()))
        .transpose()?;
    Ok((host, port))
}

/// Reads `postgres://user:password@host:port/database?sslmode=verify-full`.
/// The error text never holds the text of the input, because it can hold a password.
pub fn parse_uri(uri: &str) -> Result<NetworkTarget, String> {
    let (scheme, rest) = uri.split_once("://").ok_or("The connection text needs a scheme.")?;
    let engine = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => Engine::Postgres,
        "mysql" | "mariadb" => Engine::MySql,
        _ => return Err("The scheme must be postgres or mysql.".to_owned()),
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
    if user.is_empty() {
        return Err("The connection text needs a user.".to_owned());
    }
    let password = password
        .map(|password| percent_decode(password, "password"))
        .transpose()?
        .map(Secret::new);
    let database = (!path.is_empty())
        .then(|| percent_decode(path, "database"))
        .transpose()?;

    let mut parameters = Parameters::parse(query)?;
    let (host, port) = split_port(place)?;
    let host = percent_decode(host, "host")?;
    let address = match parameters.socket.take() {
        Some(path) if host.is_empty() => Address::Socket(path),
        Some(_) => return Err("Use a host or a socket, not both.".to_owned()),
        None if host.is_empty() => return Err("The connection text needs a host.".to_owned()),
        None => Address::Tcp { host, port: port.unwrap_or(engine.default_port()) },
    };
    let tls = parameters.tls(&address)?;
    Ok(NetworkTarget { engine, address, user, password, database, tls })
}
