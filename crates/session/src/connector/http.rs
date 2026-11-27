//! The HTTP client of the connectors that use an HTTP API, such as Databricks,
//! ClickHouse, and libSQL. The client uses the TLS rules of `tls`.

use std::time::Duration;

use super::target::{Address, NetworkTarget, Tls, TlsFiles};
use super::{ConnectError, ErrorKind, ExecError, describe, tls};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The time between two TCP keepalive probes.
const KEEPALIVE: Duration = Duration::from_secs(30);

/// Builds a client. Without files, the client has no TLS. The client does not follow
/// redirects, because a redirect can send the credentials to a different host.
/// `read_timeout` is the time limit between two parts of an answer. Without it, TCP
/// keepalive finds a stopped connection, and a long statement can wait for its first
/// part.
pub fn client(
    files: Option<&TlsFiles>,
    read_timeout: Option<Duration>,
) -> Result<reqwest::Client, ConnectError> {
    let config = match files {
        Some(files) => tls::client_config(files)?,
        None => tls::unused_client_config()?,
    };
    let builder = reqwest::Client::builder()
        .tls_backend_preconfigured(config)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .tcp_keepalive(KEEPALIVE);
    match read_timeout {
        Some(limit) => builder.read_timeout(limit),
        None => builder,
    }
    .build()
    .map_err(|error| ConnectError::new(describe(&error.without_url())))
}

/// The text of a network error. The URL stays out, because it can hold a secret.
pub fn network_error(error: reqwest::Error) -> ExecError {
    let kind = if error.is_timeout() {
        ErrorKind::Timeout
    } else if error.is_connect() || error.is_request() || error.is_body() {
        ErrorKind::Connection
    } else {
        ErrorKind::Other
    };
    ExecError::with_kind(kind, describe(&error.without_url()))
}

/// The base address of a target and its client. The automatic TLS mode uses only its
/// first attempt, because the HTTP client gives no clear signal of a server without TLS.
/// `source` names the database in the error of a socket address.
pub fn endpoint(
    target: &NetworkTarget,
    source: &str,
    read_timeout: Option<Duration>,
) -> Result<(String, reqwest::Client), ConnectError> {
    let Address::Tcp { host, port } = &target.address else {
        return Err(ConnectError::new(format!(
            "A {source} connection needs a host, not a socket."
        )));
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    let tls = target.tls.first_try(&target.address);
    let (scheme, files) = match &tls {
        Tls::Verify(files) => ("https", Some(files)),
        _ => ("http", None),
    };
    Ok((
        format!("{scheme}://{host}:{port}"),
        client(files, read_timeout)?,
    ))
}
