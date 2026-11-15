//! Access to the system vault. Errors contain no provider text, account data, or
//! password bytes. Vault calls can wait for the system. Do not call them from the
//! interface thread.

use std::fmt;
use std::sync::{Arc, OnceLock};

use keyring_core::api::CredentialStore;
use zeroize::Zeroize;

use sqln_session::connector::target::Secret;

/// Errors contain no provider text, account data, or password bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultError {
    Absent,
    NoAccess,
    Failed,
    Unsupported,
    /// `credentials.json` is not readable, is not valid, or has open permissions.
    PlainFile,
}

impl VaultError {
    /// Returns true when this computer has no vault, so plain text can be offered.
    pub fn no_vault(self) -> bool {
        matches!(self, Self::Absent | Self::Unsupported)
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Absent => "The system vault is not available.",
            Self::NoAccess => "The system vault is locked or access is denied.",
            Self::Failed => "The system vault operation fails.",
            Self::Unsupported => "This platform has no supported system vault.",
            Self::PlainFile => {
                "SQLN cannot use credentials.json. Check its content and its permissions."
            }
        })
    }
}

impl std::error::Error for VaultError {}

/// Returns true only for the answers that a test with the real service confirms.
/// Any other answer is an error, so it cannot permit the plaintext exception.
#[cfg(target_os = "linux")]
fn is_absent(cause: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    match cause.downcast_ref::<secret_service::Error>() {
        Some(secret_service::Error::Unavailable) => true,
        Some(secret_service::Error::Zbus(zbus::Error::Connection(error, _))) => {
            error.kind() == std::io::ErrorKind::NotFound
        }
        Some(secret_service::Error::Zbus(zbus::Error::MethodError(name, ..))) => {
            name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown"
        }
        _ => false,
    }
}

/// The Keychain is part of macOS, so a Keychain failure is never an absent vault.
#[cfg(not(target_os = "linux"))]
fn is_absent(_: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    false
}

pub fn vault_error(error: keyring_core::Error) -> VaultError {
    match error {
        keyring_core::Error::NoDefaultStore => VaultError::Absent,
        keyring_core::Error::NoStorageAccess(_) => VaultError::NoAccess,
        keyring_core::Error::PlatformFailure(cause) if is_absent(cause.as_ref()) => {
            VaultError::Absent
        }
        keyring_core::Error::BadEncoding(mut bytes) => {
            bytes.zeroize();
            VaultError::Failed
        }
        _ => VaultError::Failed,
    }
}

/// Uses a private store instance, never a process-wide default or a project reference.
#[derive(Default)]
pub struct SystemVault {
    store: OnceLock<Arc<CredentialStore>>,
}

impl SystemVault {
    /// Defers native access until an operation needs it. A failed start permits retry.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_api_key(&self, account: &str) -> Result<Option<Secret>, VaultError> {
        match self.service_entry("sqln-ai", account)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(vault_error(error)),
        }
    }

    pub fn write_api_key(&self, account: &str, key: &Secret) -> Result<(), VaultError> {
        self.service_entry("sqln-ai", account)?
            .set_password(key.expose())
            .map_err(vault_error)
    }

    /// Returns true after the native store starts.
    pub fn is_started(&self) -> bool {
        self.store.get().is_some()
    }

    /// Uses an explicit store. Tests can supply the keyring-core memory store.
    pub fn with_store(store: Arc<CredentialStore>) -> Self {
        Self {
            store: OnceLock::from(store),
        }
    }

    /// Returns the vault entry of an account of a service.
    pub fn service_entry(
        &self,
        service: &str,
        account: &str,
    ) -> Result<keyring_core::Entry, VaultError> {
        if self.store.get().is_none() {
            let store = native_store()?;
            // Another caller can initialize the store first.
            let _ = self.store.set(store);
        }
        self.store
            .get()
            .expect("A successful initialization sets the store.")
            .build(service, account, None)
            .map_err(vault_error)
    }
}

#[cfg(target_os = "macos")]
fn native_store() -> Result<Arc<CredentialStore>, VaultError> {
    apple_native_keyring_store::keychain::Store::new()
        .map(|store| store as Arc<CredentialStore>)
        .map_err(vault_error)
}

#[cfg(target_os = "linux")]
fn native_store() -> Result<Arc<CredentialStore>, VaultError> {
    zbus_secret_service_keyring_store::Store::new()
        .map(|store| store as Arc<CredentialStore>)
        .map_err(vault_error)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn native_store() -> Result<Arc<CredentialStore>, VaultError> {
    Err(VaultError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_never_retain_provider_text_or_secret_bytes() {
        let secret = "password-in-provider-error";
        let errors = [
            (keyring_core::Error::NoDefaultStore, VaultError::Absent),
            (
                keyring_core::Error::NoStorageAccess(Box::new(std::io::Error::other(secret))),
                VaultError::NoAccess,
            ),
            (
                keyring_core::Error::PlatformFailure(Box::new(std::io::Error::other(secret))),
                VaultError::Failed,
            ),
            (
                keyring_core::Error::Invalid(secret.into(), secret.into()),
                VaultError::Failed,
            ),
            (
                keyring_core::Error::BadEncoding(secret.as_bytes().to_vec()),
                VaultError::Failed,
            ),
        ];
        for (error, expected) in errors {
            let result = vault_error(error);
            assert_eq!(result, expected);
            assert!(!format!("{result:?}: {result}").contains(secret));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_confirmed_answers_mean_an_absent_vault() {
        let failure = |error| vault_error(keyring_core::Error::PlatformFailure(Box::new(error)));
        assert_eq!(
            failure(secret_service::Error::Unavailable),
            VaultError::Absent
        );
        assert_eq!(failure(secret_service::Error::Locked), VaultError::Failed);
        assert_eq!(failure(secret_service::Error::Prompt), VaultError::Failed);
    }
}
