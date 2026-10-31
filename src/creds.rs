//! Credential identities, session passwords, and access to the system vault.
//! Vault calls can wait for the system. Do not call them from the interface thread.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use keyring_core::api::CredentialStore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::connection::{ConnectionKey, Definition, Source};
use crate::connector::target::{Address, Engine, Secret, Tls};
use crate::store::Scope;

const SERVICE: &str = "sqln";

/// The destination and transport context, without a password or a display name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Destination {
    engine: Engine,
    address: Address,
    user: String,
    tls: Tls,
}

impl Destination {
    pub fn of(definition: &Definition) -> Option<Self> {
        let Source::Network(target) = &definition.source else {
            return None;
        };
        Some(Self {
            engine: target.engine,
            address: target.address.clone(),
            user: target.user.clone(),
            tls: target.tls.clone(),
        })
    }
}

/// A credential belongs to a logical scope, connection ID, and destination.
/// Neither the project path nor the connection name changes this identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    scope: Scope,
    connection_id: String,
    destination: Destination,
}

impl Identity {
    pub fn of(key: &ConnectionKey, definition: &Definition) -> Option<Self> {
        Some(Self {
            scope: key.scope,
            connection_id: key.id.clone(),
            destination: Destination::of(definition)?,
        })
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn destination(&self) -> &Destination {
        &self.destination
    }

    /// Each component is escaped separately to prevent ambiguous account names.
    pub fn account(&self) -> Result<String, VaultError> {
        let scope = match self.scope {
            Scope::Global => "global",
            Scope::Project => "project",
        };
        let engine = match self.destination.engine {
            Engine::Postgres => "postgres",
            Engine::MySql => "mysql",
        };
        let address =
            serde_json::to_string(&self.destination.address).map_err(|_| VaultError::Failed)?;
        let tls = serde_json::to_string(&self.destination.tls).map_err(|_| VaultError::Failed)?;
        Ok(format!(
            "{scope}/{}/{engine}/{}/{}/{}",
            escape(&self.connection_id),
            escape(&address),
            escape(&self.destination.user),
            escape(&tls),
        ))
    }
}

fn escape(value: &str) -> String {
    use std::fmt::Write;

    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            result.push(char::from(byte));
        } else {
            write!(result, "%{byte:02X}").expect("Writing to a String cannot fail.");
        }
    }
    result
}

/// Errors contain no provider text, account data, or password bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultError {
    Absent,
    NoAccess,
    Failed,
    Unsupported,
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Absent => "The system vault is not available.",
            Self::NoAccess => "The system vault is locked or access is denied.",
            Self::Failed => "The system vault operation fails.",
            Self::Unsupported => "This platform has no supported system vault.",
        })
    }
}

impl std::error::Error for VaultError {}

fn vault_error(error: keyring_core::Error) -> VaultError {
    match error {
        keyring_core::Error::NoDefaultStore => VaultError::Absent,
        keyring_core::Error::NoStorageAccess(_) => VaultError::NoAccess,
        keyring_core::Error::BadEncoding(mut bytes) => {
            bytes.zeroize();
            VaultError::Failed
        }
        _ => VaultError::Failed,
    }
}

/// A missing credential is not an absent vault.
pub trait Vault: Send + Sync {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError>;
    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError>;
    fn delete(&self, identity: &Identity) -> Result<bool, VaultError>;
}

impl<V: Vault + ?Sized> Vault for Arc<V> {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        (**self).read(identity)
    }

    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        (**self).write(identity, password)
    }

    fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        (**self).delete(identity)
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

    /// Uses an explicit store. Tests can supply the keyring-core memory store.
    pub fn with_store(store: Arc<CredentialStore>) -> Self {
        Self {
            store: OnceLock::from(store),
        }
    }

    fn entry(&self, identity: &Identity) -> Result<keyring_core::Entry, VaultError> {
        let account = identity.account()?;
        if self.store.get().is_none() {
            let store = native_store()?;
            // Another caller can initialize the store first.
            let _ = self.store.set(store);
        }
        self.store
            .get()
            .expect("A successful initialization sets the store.")
            .build(SERVICE, &account, None)
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

impl Vault for SystemVault {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        match self.entry(identity)?.get_password() {
            Ok(password) => Ok(Some(Secret::new(password))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(vault_error(error)),
        }
    }

    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        self.entry(identity)?
            .set_password(password.expose())
            .map_err(vault_error)
    }

    fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        match self.entry(identity)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring_core::Error::NoEntry) => Ok(false),
            Err(error) => Err(vault_error(error)),
        }
    }
}

/// Session passwords stay in memory. No error causes a write to a configuration file.
pub struct Credentials<V: Vault> {
    vault: V,
    session: HashMap<Identity, Secret>,
}

impl<V: Vault> Credentials<V> {
    pub fn new(vault: V) -> Self {
        Self {
            vault,
            session: HashMap::new(),
        }
    }

    pub fn resolve(&mut self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        if let Some(password) = self.session.get(identity) {
            return Ok(Some(password.clone()));
        }
        let password = self.vault.read(identity)?;
        if let Some(password) = &password {
            self.session.insert(identity.clone(), password.clone());
        }
        Ok(password)
    }

    pub fn use_for_session(&mut self, identity: &Identity, password: Secret) {
        self.session.insert(identity.clone(), password);
    }

    /// A failed save keeps the password in the session for an explicit retry.
    pub fn save_to_vault(
        &mut self,
        identity: &Identity,
        password: Secret,
    ) -> Result<(), VaultError> {
        self.use_for_session(identity, password);
        self.vault.write(identity, &self.session[identity])
    }

    /// Migration must check this result before it offers removal of a plaintext source.
    pub fn save_verified(
        &mut self,
        identity: &Identity,
        password: Secret,
    ) -> Result<(), VaultError> {
        self.save_to_vault(identity, password)?;
        match self.vault.read(identity)? {
            Some(stored) if stored == self.session[identity] => Ok(()),
            _ => Err(VaultError::Failed),
        }
    }

    /// Forgets a rejected password without deleting anything from the vault.
    pub fn forget_session(&mut self, identity: &Identity) {
        self.session.remove(identity);
    }

    /// Only an explicit, separately confirmed action must call this operation.
    pub fn delete_from_vault(&self, identity: &Identity) -> Result<bool, VaultError> {
        self.vault.delete(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_password_does_not_initialize_the_native_vault() {
        let key = ConnectionKey {
            scope: Scope::Global,
            id: "connection".into(),
        };
        let definition = Definition::new(
            "test",
            Source::Network(crate::connector::target::NetworkTarget {
                engine: Engine::Postgres,
                address: Address::Tcp {
                    host: "localhost".into(),
                    port: 5432,
                },
                user: "test".into(),
                password: None,
                database: None,
                tls: Tls::Disabled,
            }),
        );
        let identity = Identity::of(&key, &definition).unwrap();
        let mut credentials = Credentials::new(SystemVault::new());
        credentials.use_for_session(&identity, Secret::new("session password"));
        assert_eq!(
            credentials.resolve(&identity).unwrap().unwrap().expose(),
            "session password"
        );
        assert!(credentials.vault.store.get().is_none());
    }

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
}
