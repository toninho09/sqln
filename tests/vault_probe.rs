//! A manual probe of the native vault. It prints the outcome of each step, and it uses
//! a fixed value that is not a secret. `spikes/RESULTS.md` explains how to run it.

use std::sync::Arc;

use keyring_core::api::CredentialStore;
use sqln::connection::{ConnectionKey, Definition, Source};
use sqln::connector::target::{Address, Engine, NetworkTarget, Secret, Tls, TlsFiles};
use sqln::creds::{Identity, SystemVault, Vault};
use sqln::store::Scope;

const ID: &str = "vault-probe";
const VALUE: &str = "probe-value";
const STEPS: &str = "read,write,read,delete,delete";

#[cfg(target_os = "macos")]
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecKeychainSetUserInteractionAllowed(allowed: u8) -> i32;
}

#[cfg(target_os = "macos")]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    apple_native_keyring_store::keychain::Store::new().map(|store| store as Arc<CredentialStore>)
}

#[cfg(target_os = "linux")]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    zbus_secret_service_keyring_store::Store::new().map(|store| store as Arc<CredentialStore>)
}

fn identity() -> Identity {
    let key = ConnectionKey {
        scope: Scope::Global,
        id: ID.to_owned(),
    };
    let definition = Definition::new(
        "probe",
        Source::Network(NetworkTarget {
            engine: Engine::Postgres,
            address: Address::Tcp {
                host: "db.example.test".into(),
                port: 5432,
            },
            user: "probe".into(),
            password: None,
            database: None,
            tls: Tls::Verify(TlsFiles::default()),
        }),
    );
    Identity::of(&key, &definition).expect("A network definition has an identity.")
}

/// Repeats a failed step on the native store, because `VaultError` hides the provider text.
fn raw_error(identity: &Identity, step: &str) -> String {
    let account = identity
        .account()
        .expect("The probe identity has an account.");
    let entry = match native_store().and_then(|store| store.build("sqln", &account, None)) {
        Ok(entry) => entry,
        Err(error) => return format!("{error:?}"),
    };
    let result = match step {
        "read" => entry.get_password().map(|_| ()),
        "write" => entry.set_password(VALUE),
        _ => entry.delete_credential(),
    };
    format!("{:?}", result.err())
}

#[test]
#[ignore = "Requires explicit SQLN_TEST_SYSTEM_VAULT=1 and access to the real system vault."]
fn native_vault_probe() {
    assert_eq!(std::env::var("SQLN_TEST_SYSTEM_VAULT").as_deref(), Ok("1"));
    // This call simulates a process without a window session.
    #[cfg(target_os = "macos")]
    if std::env::var_os("SQLN_VAULT_PROBE_NO_UI").is_some() {
        unsafe { SecKeychainSetUserInteractionAllowed(0) };
    }
    let steps = std::env::var("SQLN_VAULT_PROBE").unwrap_or_else(|_| STEPS.to_owned());
    let identity = identity();
    let vault = SystemVault::new();
    for step in steps.split(',') {
        let outcome = match step {
            "account" => identity.account().map(|account| account.to_string()),
            "read" => vault
                .read(&identity)
                .map(|value| format!("found={}", value.is_some())),
            "write" => vault
                .write(&identity, &Secret::new(VALUE))
                .map(|()| "written".to_owned()),
            "delete" => vault
                .delete(&identity)
                .map(|found| format!("found={found}")),
            other => panic!("Unknown step: {other}"),
        };
        match outcome {
            Ok(text) => println!("{step}: {text}"),
            Err(error) => println!("{step}: {error:?} raw={}", raw_error(&identity, step)),
        }
    }
}
