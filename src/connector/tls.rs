//! TLS of the network connectors. A server without a valid certificate fails.
//! This module has no path that skips the verification.

use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use rustls::client::WantsClientCert;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, ConfigBuilder as RustlsBuilder, RootCertStore};

use super::ConnectError;
use super::target::TlsFiles;

type ConfigBuilder = RustlsBuilder<ClientConfig, WantsClientCert>;

/// Reads the roots of the system once, because the read can take 100 ms.
/// The MySQL driver stops at a root that it cannot read, so this list keeps valid roots.
pub fn system_roots() -> &'static [CertificateDer<'static>] {
    static ROOTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut store = RootCertStore::empty();
        rustls_native_certs::load_native_certs()
            .certs
            .into_iter()
            .filter(|root| store.add(root.clone()).is_ok())
            .collect()
    })
}

fn file_error(path: &Path, error: impl std::fmt::Display) -> ConnectError {
    ConnectError::new(format!("Cannot read {}: {error}", path.display()))
}

/// Reads all certificates of a PEM file.
pub fn read_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, ConnectError> {
    let data = fs::read(path).map_err(|error| file_error(path, error))?;
    let certificates = CertificateDer::pem_slice_iter(&data)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| file_error(path, error))?;
    if certificates.is_empty() {
        return Err(file_error(path, "the file has no certificate"));
    }
    Ok(certificates)
}

fn roots(files: &TlsFiles) -> Result<RootCertStore, ConnectError> {
    let mut store = RootCertStore::empty();
    if let Some(ca) = &files.ca {
        for certificate in read_certificates(ca)? {
            store
                .add(certificate)
                .map_err(|error| file_error(ca, error))?;
        }
        return Ok(store);
    }
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    store.add_parsable_certificates(system_roots().iter().cloned());
    Ok(store)
}

fn builder(store: RootCertStore) -> Result<ConfigBuilder, ConnectError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| ConnectError::new(error.to_string()))?
        .with_root_certificates(store))
}

/// Returns a configuration without roots. It accepts no server.
/// The driver needs one value even when it does not use TLS.
pub fn unused_client_config() -> Result<ClientConfig, ConnectError> {
    Ok(builder(RootCertStore::empty())?.with_no_client_auth())
}

/// Returns the client configuration of the PostgreSQL connector.
pub fn client_config(files: &TlsFiles) -> Result<ClientConfig, ConnectError> {
    let builder = builder(roots(files)?)?;
    match &files.client {
        None => Ok(builder.with_no_client_auth()),
        Some(client) => {
            let chain = read_certificates(&client.certificate)?;
            let key = PrivateKeyDer::from_pem_file(&client.key)
                .map_err(|error| file_error(&client.key, error))?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|error| ConnectError::new(error.to_string()))
        }
    }
}
