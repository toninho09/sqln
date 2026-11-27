//! A failure with the configuration change that can correct it. A driver reads the
//! cause from a code, an error type, or a setting, never from the error text.

use crate::text;

/// A configuration that can cause a failure. Each value names the cause and the field
/// that the user can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// The TLS mode needs TLS, and the server has no TLS.
    TlsOff,
    /// The TLS mode is off, and the server needs TLS.
    TlsOn,
    /// SQLN does not trust the authority of the server certificate.
    CertificateIssuer,
    /// The server certificate is for another name.
    CertificateName,
    /// The server certificate is expired or not valid yet.
    CertificateDate,
    /// The database does not exist.
    Database,
    /// SQLN cannot find or reach the host.
    Host,
    /// Nothing accepts connections at the port.
    Port,
    /// The socket file does not exist.
    Socket,
    /// SQLN cannot open the database file.
    File,
    /// A rule of the server refuses the user, the host, or the TLS mode.
    ServerRule,
    /// The server has no free connection.
    PoolLimit,
    /// The connection time limit ended.
    ConnectLimit,
    /// The `Read only` setting of the connection refused a write.
    ReadOnly,
    /// The server, the file, or the transaction refused a write.
    ServerReadOnly,
    /// The user of the connection lacks a privilege.
    Privilege,
    /// The statement time limit of the connection ended.
    SqlTimeout,
    /// Another process holds the lock of the database file.
    LockLimit,
}

impl Fix {
    /// Returns the line that tells the user what to change.
    pub fn hint(self) -> String {
        text::fix_hint(self)
    }
}

/// An error text and the configuration change that can correct it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub fix: Option<Fix>,
}

impl Failure {
    pub fn new(message: impl Into<String>, fix: Option<Fix>) -> Self {
        Self {
            message: message.into(),
            fix,
        }
    }

    /// Returns the hint line, when the failure has a known cause.
    pub fn hint(&self) -> Option<String> {
        self.fix.map(Fix::hint)
    }

    /// Returns the message and the hint on one line, for a place with one line only.
    pub fn one_line(&self) -> String {
        match self.hint() {
            Some(hint) => format!("{} {hint}", self.message),
            None => self.message.clone(),
        }
    }

    /// Returns the message and the hint on two lines.
    pub fn two_lines(&self) -> String {
        match self.hint() {
            Some(hint) => format!("{}\n{hint}", self.message),
            None => self.message.clone(),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::new(message, None)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::new(message, None)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
