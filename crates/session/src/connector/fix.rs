//! Finds the configuration that can correct a failure of the network or of TLS. It
//! reads error types and kinds, never the error text.

use std::error::Error;
use std::io;

use rustls::CertificateError;

use super::Fix;
use super::target::Address;

/// Returns the fix of a failure of the socket or of the server certificate. It reads
/// each cause of the error, and the error inside an `io::Error`.
pub fn network_fix(error: &(dyn Error + 'static), address: &Address) -> Option<Fix> {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(fix) = certificate_fix(error) {
            return Some(fix);
        }
        if let Some(io) = error.downcast_ref::<io::Error>() {
            // `io::Error::source` skips the inner error, so read it directly.
            if let Some(fix) = io.get_ref().and_then(|inner| network_fix(inner, address)) {
                return Some(fix);
            }
            if let Some(fix) = io_fix(io.kind(), address) {
                return Some(fix);
            }
        }
        cause = error.source();
    }
    None
}

fn certificate_fix(error: &(dyn Error + 'static)) -> Option<Fix> {
    let rustls::Error::InvalidCertificate(certificate) = error.downcast_ref::<rustls::Error>()?
    else {
        return None;
    };
    match certificate {
        CertificateError::UnknownIssuer => Some(Fix::CertificateIssuer),
        CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
            Some(Fix::CertificateName)
        }
        CertificateError::Expired
        | CertificateError::ExpiredContext { .. }
        | CertificateError::NotValidYet
        | CertificateError::NotValidYetContext { .. } => Some(Fix::CertificateDate),
        _ => None,
    }
}

fn io_fix(kind: io::ErrorKind, address: &Address) -> Option<Fix> {
    match kind {
        io::ErrorKind::ConnectionRefused => Some(Fix::Port),
        io::ErrorKind::NotFound if matches!(address, Address::Socket(_)) => Some(Fix::Socket),
        io::ErrorKind::HostUnreachable | io::ErrorKind::NetworkUnreachable => Some(Fix::Host),
        io::ErrorKind::TimedOut => Some(Fix::ConnectLimit),
        _ => None,
    }
}

/// Returns `Fix::Host` when the host name of a TCP address does not resolve. Call it
/// only after a failure, because it asks the name service.
pub async fn host_fix(address: &Address) -> Option<Fix> {
    let Address::Tcp { host, port } = address else {
        return None;
    };
    if host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let resolved = tokio::net::lookup_host((host.as_str(), *port)).await;
    match resolved.map(|mut addresses| addresses.next()) {
        Ok(Some(_)) => None,
        _ => Some(Fix::Host),
    }
}

/// The wait for each address in `port_closed`.
const PORT_PROBE: std::time::Duration = std::time::Duration::from_secs(1);

/// Returns true when each address of the host refuses the port. A driver reports the
/// error of one address only, such as `::1` of `localhost`, while another address can
/// accept the connection and fail for another cause. Call it only after a failure.
pub async fn port_closed(address: &Address) -> bool {
    let Address::Tcp { host, port } = address else {
        return true;
    };
    let Ok(addresses) = tokio::net::lookup_host((host.as_str(), *port)).await else {
        return false;
    };
    for address in addresses {
        let refused = matches!(
            tokio::time::timeout(PORT_PROBE, tokio::net::TcpStream::connect(address)).await,
            Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused
        );
        if !refused {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn tcp() -> Address {
        Address::Tcp {
            host: "db.example.test".to_owned(),
            port: 5432,
        }
    }

    #[test]
    fn a_refused_port_asks_for_the_port() {
        let error = io::Error::from(io::ErrorKind::ConnectionRefused);
        assert_eq!(network_fix(&error, &tcp()), Some(Fix::Port));
    }

    #[test]
    fn a_missing_socket_asks_for_the_socket() {
        let error = io::Error::from(io::ErrorKind::NotFound);
        let socket = Address::Socket(PathBuf::from("/tmp/.s.PGSQL.5432"));
        assert_eq!(network_fix(&error, &socket), Some(Fix::Socket));
        assert_eq!(network_fix(&error, &tcp()), None);
    }

    #[test]
    fn an_unreachable_host_asks_for_the_host() {
        for kind in [
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::NetworkUnreachable,
        ] {
            assert_eq!(network_fix(&io::Error::from(kind), &tcp()), Some(Fix::Host));
        }
    }

    #[test]
    fn a_certificate_error_inside_an_io_error_gives_its_fix() {
        let cases = [
            (CertificateError::UnknownIssuer, Fix::CertificateIssuer),
            (CertificateError::NotValidForName, Fix::CertificateName),
            (CertificateError::Expired, Fix::CertificateDate),
            (CertificateError::NotValidYet, Fix::CertificateDate),
        ];
        for (certificate, fix) in cases {
            let inner = rustls::Error::InvalidCertificate(certificate);
            let error = io::Error::new(io::ErrorKind::InvalidData, inner);
            assert_eq!(network_fix(&error, &tcp()), Some(fix));
        }
    }

    /// A wrapper error, as a driver error with the socket error as its cause.
    #[derive(Debug)]
    struct Wrapper(io::Error);

    impl std::fmt::Display for Wrapper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("error connecting to server")
        }
    }

    impl Error for Wrapper {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn a_cause_of_a_driver_error_gives_its_fix() {
        let error = Wrapper(io::Error::from(io::ErrorKind::ConnectionRefused));
        assert_eq!(network_fix(&error, &tcp()), Some(Fix::Port));
    }

    #[test]
    fn another_error_has_no_fix() {
        let error = io::Error::other("closed");
        assert_eq!(network_fix(&error, &tcp()), None);
    }

    #[test]
    fn a_port_is_closed_only_when_each_address_refuses_it() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let open = Address::Tcp {
                host: "127.0.0.1".to_owned(),
                port: listener.local_addr().unwrap().port(),
            };
            assert!(!port_closed(&open).await);
            drop(listener);
            // The port of the closed listener refuses connections.
            assert!(port_closed(&open).await);
        });
    }

    #[test]
    fn an_ip_address_does_not_ask_the_name_service() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let address = Address::Tcp {
            host: "127.0.0.1".to_owned(),
            port: 1,
        };
        assert_eq!(runtime.block_on(host_fix(&address)), None);
    }
}
