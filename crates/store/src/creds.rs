//! Credential identities, session passwords, and access to the system vault.
//! Vault calls can wait for the system. Do not call them from the interface thread.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::connector::target::{Address, Engine, Secret, Tls};
use crate::store::connection::{ConnectionKey, Definition, Source};
use crate::store::file::{self, Expect};
use crate::store::{Root, Scope};

pub use crate::vault::{SystemVault, VaultError, vault_error};

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
            // The automatic mode keeps the identity of its first attempt, so a saved
            // password stays valid after this mode arrived.
            tls: target.tls.first_try(&target.address),
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
            Engine::Kafka => "kafka",
            Engine::Databricks => "databricks",
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
            // Writing to a String cannot fail.
            let _ = write!(result, "%{byte:02X}");
        }
    }
    result
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

/// Returns the vault entry of a connection password.
fn entry(vault: &SystemVault, identity: &Identity) -> Result<keyring_core::Entry, VaultError> {
    vault.service_entry(SERVICE, &identity.account()?)
}

impl Vault for SystemVault {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        match entry(self, identity)?.get_password() {
            Ok(password) => Ok(Some(Secret::new(password))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(vault_error(error)),
        }
    }

    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        entry(self, identity)?
            .set_password(password.expose())
            .map_err(vault_error)
    }

    fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        match entry(self, identity)?.delete_credential() {
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
                kafka: Default::default(),
                databricks: Default::default(),
            }),
        );
        let identity = Identity::of(&key, &definition).unwrap();
        let mut credentials = Credentials::new(SystemVault::new());
        credentials.use_for_session(&identity, Secret::new("session password"));
        assert_eq!(
            credentials.resolve(&identity).unwrap().unwrap().expose(),
            "session password"
        );
        assert!(!credentials.vault.is_started());
    }
}
