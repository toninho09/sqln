//! Credential identities, session passwords, and access to the system vault.
//! Vault calls can wait for the system. Do not call them from the interface thread.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use keyring_core::api::CredentialStore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::connection::{ConnectionKey, Definition, Source};
use crate::connector::target::{Address, Engine, Secret, Tls};
use crate::store::file::{self, Expect};
use crate::store::{Root, Scope};

const SERVICE: &str = "sqln";
const PLAIN_VERSION: u32 = 1;

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
            Engine::Redis => "redis",
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

fn vault_error(error: keyring_core::Error) -> VaultError {
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

    /// Uses an explicit store. Tests can supply the keyring-core memory store.
    pub fn with_store(store: Arc<CredentialStore>) -> Self {
        Self {
            store: OnceLock::from(store),
        }
    }

    fn entry(&self, identity: &Identity) -> Result<keyring_core::Entry, VaultError> {
        self.service_entry(SERVICE, &identity.account()?)
    }

    fn service_entry(
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlainPasswords {
    version: u32,
    /// The account of `Identity::account`, and its password.
    passwords: BTreeMap<String, String>,
}

impl Drop for PlainPasswords {
    fn drop(&mut self) {
        self.passwords.values_mut().for_each(Zeroize::zeroize);
    }
}

/// The plain text passwords that the user authorized on a computer without a vault.
/// The file is in the global folder, and `config.json` never receives a password.
pub struct PlainFile {
    path: PathBuf,
    lock: PathBuf,
}

impl PlainFile {
    pub fn new(global: &Root) -> Self {
        Self {
            path: global.credentials_file(),
            lock: global.locks_dir().join("credentials.lock"),
        }
    }

    /// Refuses a link or a file that others can read, as `ssh` does with a private key.
    fn load(&self) -> Result<Option<PlainPasswords>, VaultError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(VaultError::PlainFile),
        };
        if !metadata.is_file() {
            return Err(VaultError::PlainFile);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(VaultError::PlainFile);
            }
        }
        let mut bytes = fs::read(&self.path).map_err(|_| VaultError::PlainFile)?;
        let parsed = serde_json::from_slice::<PlainPasswords>(&bytes);
        bytes.zeroize();
        parsed
            .ok()
            .filter(|stored| stored.version == PLAIN_VERSION)
            .map(Some)
            .ok_or(VaultError::PlainFile)
    }

    pub fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        let account = identity.account()?;
        let stored = self.load()?;
        Ok(stored.and_then(|stored| stored.passwords.get(&account).map(Secret::new)))
    }

    fn change<T>(&self, change: impl FnOnce(&mut PlainPasswords) -> T) -> Result<T, VaultError> {
        let _lock = file::lock(&self.lock).map_err(|_| VaultError::PlainFile)?;
        let mut stored = self.load()?.unwrap_or_else(|| PlainPasswords {
            version: PLAIN_VERSION,
            passwords: BTreeMap::new(),
        });
        let result = change(&mut stored);
        if stored.passwords.is_empty() {
            return match fs::remove_file(&self.path) {
                Ok(()) => Ok(result),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(result),
                Err(_) => Err(VaultError::PlainFile),
            };
        }
        let mut text = serde_json::to_vec_pretty(&stored).map_err(|_| VaultError::PlainFile)?;
        let written = file::write_private_atomic(&self.path, &text, Expect::Any);
        text.zeroize();
        written.map(|_| result).map_err(|_| VaultError::PlainFile)
    }

    pub fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        let account = identity.account()?;
        self.change(|stored| {
            if let Some(mut old) = stored
                .passwords
                .insert(account, password.expose().to_owned())
            {
                old.zeroize();
            }
        })
    }

    pub fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        let account = identity.account()?;
        if self.read(identity)?.is_none() {
            return Ok(false);
        }
        self.change(|stored| {
            stored
                .passwords
                .remove(&account)
                .map(|mut old| old.zeroize())
                .is_some()
        })
    }
}

/// What a request finds for one connection, in the order of the plan.
#[derive(Debug, PartialEq, Eq)]
pub enum Lookup {
    Found(Secret),
    /// Nothing is stored. A save goes to the vault.
    Missing,
    /// This computer has no vault, and nothing is stored. A save needs plain text.
    NoVault,
    /// A vault exists, and only the plain text file holds this password.
    Migrate(Secret),
}

/// Session passwords stay in memory. No error causes a write to a configuration file.
pub struct Credentials<V: Vault> {
    vault: V,
    plain: Option<PlainFile>,
    session: HashMap<Identity, Secret>,
}

impl<V: Vault> Credentials<V> {
    pub fn new(vault: V) -> Self {
        Self {
            vault,
            plain: None,
            session: HashMap::new(),
        }
    }

    /// Adds the file of the authorized plain text passwords.
    pub fn with_plain(mut self, plain: Option<PlainFile>) -> Self {
        self.plain = plain;
        self
    }

    /// Finds a password. A connection without a vault reference reads the vault only
    /// when the plain text file holds a password, so that it does not touch the vault.
    pub fn lookup(&mut self, identity: &Identity, reference: bool) -> Result<Lookup, VaultError> {
        if let Some(password) = self.session.get(identity) {
            return Ok(Lookup::Found(password.clone()));
        }
        let plain = match &self.plain {
            Some(file) => file.read(identity)?,
            None => None,
        };
        if !reference && plain.is_none() {
            return Ok(Lookup::Missing);
        }
        let found = match (self.vault.read(identity), plain) {
            (Ok(Some(password)), _) => Lookup::Found(password),
            (Ok(None), Some(password)) => Lookup::Migrate(password),
            (Ok(None), None) => Lookup::Missing,
            (Err(error), Some(password)) if error.no_vault() => Lookup::Found(password),
            (Err(error), None) if error.no_vault() => Lookup::NoVault,
            (Err(error), _) => return Err(error),
        };
        if let Lookup::Found(password) = &found {
            self.session.insert(identity.clone(), password.clone());
        }
        Ok(found)
    }

    /// Only an explicit authorization after the plain text warning must call this.
    pub fn save_plain(&mut self, identity: &Identity, password: Secret) -> Result<(), VaultError> {
        self.use_for_session(identity, password);
        let file = self.plain.as_ref().ok_or(VaultError::PlainFile)?;
        file.write(identity, &self.session[identity])
    }

    /// Removes the plain text source. A migration calls this only after its check.
    pub fn remove_plain(&mut self, identity: &Identity) -> Result<bool, VaultError> {
        self.plain
            .as_ref()
            .map_or(Ok(false), |file| file.delete(identity))
    }

    /// Only an explicit, separately confirmed action must call this operation.
    pub fn delete_saved(&mut self, identity: &Identity) -> Result<bool, VaultError> {
        self.forget_session(identity);
        let plain = self.remove_plain(identity)?;
        match self.vault.delete(identity) {
            Ok(found) => Ok(found || plain),
            Err(error) if error.no_vault() => Ok(plain),
            Err(error) => Err(error),
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
