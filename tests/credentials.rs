mod common;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::temp_dir;
use keyring_core::api::CredentialStoreApi;
use sqln::connection::{ConnectionKey, Definition, Source};
use sqln::connector::sqlite::SqliteTarget;
use sqln::connector::target::{
    Address, ClientCertificate, Engine, NetworkTarget, Secret, Tls, TlsFiles,
};
use sqln::creds::{Credentials, Identity, PlainFile, SystemVault, Vault, VaultError};
use sqln::store::config::{self, Change};
use sqln::store::file::{self, Expect, WriteError};
use sqln::store::{Root, Scope, Store};
use sqln::trust::{Required, Trust, TrustError};
use zeroize::{Zeroize, ZeroizeOnDrop};

const ID: &str = "0f3726f5-16d2-46af-82f9-54736aaa81f8";
const PASSWORD: &str = "a-password-that-must-not-appear";

fn definition() -> Definition {
    Definition::new(
        "app",
        Source::Network(NetworkTarget {
            engine: Engine::Postgres,
            address: Address::Tcp {
                host: "db.example.test".into(),
                port: 5432,
            },
            user: "app_user".into(),
            password: Some(Secret::new(PASSWORD)),
            database: Some("app_db".into()),
            tls: Tls::Verify(TlsFiles::default()),
        }),
    )
}

fn target(definition: &mut Definition) -> &mut NetworkTarget {
    let Source::Network(target) = &mut definition.source else {
        panic!("Expected a network target")
    };
    target
}

fn identity(scope: Scope) -> Identity {
    Identity::of(
        &ConnectionKey {
            scope,
            id: ID.into(),
        },
        &definition(),
    )
    .unwrap()
}

fn changed_destinations() -> Vec<Definition> {
    let mut engine = definition();
    target(&mut engine).engine = Engine::MySql;
    let mut host = definition();
    target(&mut host).address = Address::Tcp {
        host: "other.example.test".into(),
        port: 5432,
    };
    let mut port = definition();
    target(&mut port).address = Address::Tcp {
        host: "db.example.test".into(),
        port: 5433,
    };
    let mut user = definition();
    target(&mut user).user = "other_user".into();
    let mut tls = definition();
    target(&mut tls).tls = Tls::Disabled;
    let mut ca = definition();
    target(&mut ca).tls = Tls::Verify(TlsFiles {
        ca: Some("company.pem".into()),
        client: None,
    });
    let mut client = definition();
    target(&mut client).tls = Tls::Verify(TlsFiles {
        ca: None,
        client: Some(ClientCertificate {
            certificate: "client.pem".into(),
            key: "client.key".into(),
        }),
    });
    let mut socket = definition();
    target(&mut socket).address = Address::Socket("/tmp/postgres.sock".into());
    vec![engine, host, port, user, tls, ca, client, socket]
}

#[test]
fn passwords_clear_memory_and_hide_debug_text() {
    fn drops_with_zeroize<T: ZeroizeOnDrop>() {}
    drops_with_zeroize::<Secret>();
    let mut password = Secret::new(PASSWORD);
    let copy = password.clone();
    assert!(!format!("{password:?} {copy:?}").contains(PASSWORD));
    password.zeroize();
    assert_eq!(password.expose(), "");
    assert_eq!(copy.expose(), PASSWORD);
}

#[test]
fn renaming_or_moving_a_project_does_not_change_the_credential_identity() {
    let original = identity(Scope::Project);
    let mut renamed = definition();
    renamed.name = "new name".into();
    renamed.base = Some("/a/moved/project".into());
    target(&mut renamed).password = Some(Secret::new("another password"));
    let changed = Identity::of(
        &ConnectionKey {
            scope: Scope::Project,
            id: ID.into(),
        },
        &renamed,
    )
    .unwrap();
    assert_eq!(original, changed);
    assert_eq!(original.account().unwrap(), changed.account().unwrap());
    assert!(!original.account().unwrap().contains(PASSWORD));
    assert!(!format!("{original:?}").contains(PASSWORD));
}

#[test]
fn every_destination_component_changes_the_credential_account() {
    let key = ConnectionKey {
        scope: Scope::Project,
        id: ID.into(),
    };
    let original = Identity::of(&key, &definition()).unwrap();
    for changed in changed_destinations() {
        let changed = Identity::of(&key, &changed).unwrap();
        assert_ne!(original, changed);
        assert_ne!(original.account().unwrap(), changed.account().unwrap());
    }
}

#[test]
fn scope_and_connection_id_separate_credentials() {
    let global = identity(Scope::Global);
    let project = identity(Scope::Project);
    let other = Identity::of(
        &ConnectionKey {
            scope: Scope::Project,
            id: "other".into(),
        },
        &definition(),
    )
    .unwrap();
    assert_ne!(global.account().unwrap(), project.account().unwrap());
    assert_ne!(project.account().unwrap(), other.account().unwrap());
}

#[test]
fn account_components_cannot_inject_a_path_separator_or_escape() {
    let mut first = definition();
    target(&mut first).user = "user/verify%2F".into();
    let mut second = first.clone();
    target(&mut second).user = "user%2Fverify%2F".into();
    let key = ConnectionKey {
        scope: Scope::Global,
        id: "id/with%2Fseparator".into(),
    };
    let first = Identity::of(&key, &first).unwrap().account().unwrap();
    let second = Identity::of(&key, &second).unwrap().account().unwrap();
    assert_eq!(first.split('/').count(), 6);
    assert_eq!(second.split('/').count(), 6);
    assert_ne!(first, second);
}

#[test]
fn moving_a_project_with_relative_transport_files_does_not_change_the_vault_identity() {
    let mut first = definition();
    target(&mut first).tls = Tls::Verify(TlsFiles {
        ca: Some("ca.pem".into()),
        client: None,
    });
    first.base = Some("/first".into());
    let mut second = first.clone();
    second.base = Some("/second".into());
    let key = ConnectionKey {
        scope: Scope::Project,
        id: ID.into(),
    };
    assert_eq!(Identity::of(&key, &first), Identity::of(&key, &second));
    assert_eq!(
        Identity::of(&key, &first).unwrap().account().unwrap(),
        Identity::of(&key, &second).unwrap().account().unwrap(),
    );
}

#[test]
fn sqlite_has_no_vault_identity() {
    let definition = Definition::sqlite(SqliteTarget::new("test.db"));
    let key = ConnectionKey {
        scope: Scope::Global,
        id: ID.into(),
    };
    assert!(Identity::of(&key, &definition).is_none());
}

#[test]
fn a_reload_never_reuses_a_password_after_a_transport_or_destination_change() {
    let old = definition();
    for mut changed in changed_destinations() {
        target(&mut changed).password = None;
        changed.keep_password_of(&old);
        assert!(target(&mut changed).password.is_none());
    }
    let mut same = old.without_password();
    same.name = "renamed".into();
    same.keep_password_of(&old);
    assert_eq!(
        target(&mut same).password.as_ref().unwrap().expose(),
        PASSWORD
    );
}

#[test]
fn a_reload_checks_resolved_transport_paths_before_reusing_a_password() {
    let mut old = definition();
    target(&mut old).tls = Tls::Verify(TlsFiles {
        ca: Some("ca.pem".into()),
        client: None,
    });
    old.base = Some("/first".into());
    let mut changed = old.without_password();
    changed.base = Some("/second".into());
    changed.keep_password_of(&old);
    assert!(target(&mut changed).password.is_none());
}

#[derive(Clone, Default)]
struct FakeVault(Arc<Mutex<VaultState>>);

#[derive(Default)]
struct VaultState {
    values: HashMap<Identity, Secret>,
    read_error: Option<VaultError>,
    write_error: Option<VaultError>,
    return_wrong_password: bool,
    reads: usize,
    writes: usize,
    deletes: usize,
}

impl Vault for FakeVault {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        let mut state = self.0.lock().unwrap();
        state.reads += 1;
        if let Some(error) = state.read_error {
            return Err(error);
        }
        if state.return_wrong_password {
            return Ok(Some(Secret::new("wrong")));
        }
        Ok(state.values.get(identity).cloned())
    }

    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        let mut state = self.0.lock().unwrap();
        state.writes += 1;
        if let Some(error) = state.write_error {
            return Err(error);
        }
        state.values.insert(identity.clone(), password.clone());
        Ok(())
    }

    fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        let mut state = self.0.lock().unwrap();
        state.deletes += 1;
        Ok(state.values.remove(identity).is_some())
    }
}

#[test]
fn reading_a_missing_credential_does_not_write_anything() {
    let vault = FakeVault::default();
    let mut credentials = Credentials::new(vault.clone());
    assert_eq!(credentials.resolve(&identity(Scope::Global)), Ok(None));
    let state = vault.0.lock().unwrap();
    assert_eq!(state.reads, 1);
    assert_eq!(state.writes, 0);
    assert_eq!(state.deletes, 0);
}

#[test]
fn a_session_password_works_with_an_inaccessible_vault_without_saving_it() {
    let vault = FakeVault::default();
    vault.0.lock().unwrap().read_error = Some(VaultError::NoAccess);
    let id = identity(Scope::Global);
    let mut credentials = Credentials::new(vault.clone());
    assert_eq!(credentials.resolve(&id), Err(VaultError::NoAccess));
    credentials.use_for_session(&id, Secret::new(PASSWORD));
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
    let state = vault.0.lock().unwrap();
    assert_eq!(state.reads, 1);
    assert_eq!(state.writes, 0);
}

#[test]
fn a_failed_save_keeps_the_password_in_memory_for_an_explicit_retry() {
    let vault = FakeVault::default();
    vault.0.lock().unwrap().write_error = Some(VaultError::Failed);
    let id = identity(Scope::Global);
    let mut credentials = Credentials::new(vault.clone());
    assert_eq!(
        credentials.save_to_vault(&id, Secret::new(PASSWORD)),
        Err(VaultError::Failed)
    );
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
    assert_eq!(vault.0.lock().unwrap().writes, 1);
    vault.0.lock().unwrap().write_error = None;
    credentials
        .save_to_vault(&id, Secret::new(PASSWORD))
        .unwrap();
    assert_eq!(vault.0.lock().unwrap().writes, 2);
}

#[test]
fn a_vault_password_is_shared_by_the_connection_during_the_session() {
    let vault = FakeVault::default();
    let id = identity(Scope::Project);
    vault.write(&id, &Secret::new(PASSWORD)).unwrap();
    let mut credentials = Credentials::new(vault.clone());
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
    assert_eq!(vault.0.lock().unwrap().reads, 1);
    assert_eq!(credentials.resolve(&identity(Scope::Global)).unwrap(), None);
}

#[test]
fn a_session_password_does_not_survive_a_new_credentials_instance() {
    let vault = FakeVault::default();
    let id = identity(Scope::Project);
    let mut credentials = Credentials::new(vault.clone());
    credentials.use_for_session(&id, Secret::new(PASSWORD));
    drop(credentials);
    assert_eq!(Credentials::new(vault).resolve(&id), Ok(None));
}

#[test]
fn forgetting_a_rejected_session_password_does_not_delete_the_vault_entry() {
    let vault = FakeVault::default();
    let id = identity(Scope::Global);
    vault.write(&id, &Secret::new(PASSWORD)).unwrap();
    let mut credentials = Credentials::new(vault.clone());
    credentials.resolve(&id).unwrap();
    credentials.forget_session(&id);
    assert_eq!(vault.0.lock().unwrap().deletes, 0);
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
    assert_eq!(vault.0.lock().unwrap().reads, 2);
}

#[test]
fn deletion_requires_a_separate_vault_operation() {
    let vault = FakeVault::default();
    let id = identity(Scope::Global);
    let mut credentials = Credentials::new(vault.clone());
    credentials
        .save_to_vault(&id, Secret::new(PASSWORD))
        .unwrap();
    drop(credentials);
    assert_eq!(vault.0.lock().unwrap().deletes, 0);
    let credentials = Credentials::new(vault.clone());
    assert_eq!(credentials.delete_from_vault(&id), Ok(true));
    assert_eq!(credentials.delete_from_vault(&id), Ok(false));
}

#[test]
fn migration_verifies_the_saved_password_and_retains_the_session_after_a_mismatch() {
    let vault = FakeVault::default();
    let id = identity(Scope::Global);
    let mut credentials = Credentials::new(vault.clone());
    vault.0.lock().unwrap().return_wrong_password = true;
    assert_eq!(
        credentials.save_verified(&id, Secret::new(PASSWORD)),
        Err(VaultError::Failed)
    );
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
    vault.0.lock().unwrap().return_wrong_password = false;
    credentials
        .save_verified(&id, Secret::new(PASSWORD))
        .unwrap();
}

#[test]
fn migration_does_not_report_success_if_the_vault_cannot_read_back_the_password() {
    let vault = FakeVault::default();
    vault.0.lock().unwrap().read_error = Some(VaultError::NoAccess);
    let mut credentials = Credentials::new(vault);
    let id = identity(Scope::Global);
    assert_eq!(
        credentials.save_verified(&id, Secret::new(PASSWORD)),
        Err(VaultError::NoAccess)
    );
    assert_eq!(
        credentials.resolve(&id).unwrap().unwrap().expose(),
        PASSWORD
    );
}

#[test]
fn the_system_adapter_uses_only_sqln_without_a_process_wide_default() {
    let store = keyring_core::mock::Store::new().unwrap();
    let vault = SystemVault::with_store(store.clone());
    let id = identity(Scope::Project);
    let other_app = store
        .build("another-application", &id.account().unwrap(), None)
        .unwrap();
    other_app
        .set_password("other application password")
        .unwrap();
    assert_eq!(vault.read(&id), Ok(None));
    vault.write(&id, &Secret::new(PASSWORD)).unwrap();
    assert_eq!(vault.read(&id).unwrap().unwrap().expose(), PASSWORD);
    assert_eq!(vault.read(&identity(Scope::Global)), Ok(None));
    assert_eq!(vault.delete(&id), Ok(true));
    assert_eq!(vault.delete(&id), Ok(false));
    assert_eq!(
        other_app.get_password().unwrap(),
        "other application password"
    );
}

#[test]
fn the_system_adapter_distinguishes_missing_entry_from_access_failure() {
    let store = keyring_core::mock::Store::new().unwrap();
    let vault = SystemVault::with_store(store.clone());
    let id = identity(Scope::Global);
    let entry = store.build("sqln", &id.account().unwrap(), None).unwrap();
    let credential = entry
        .as_any()
        .downcast_ref::<keyring_core::mock::Cred>()
        .unwrap();
    credential.set_error(keyring_core::Error::NoStorageAccess(Box::new(
        std::io::Error::other(PASSWORD),
    )));
    assert_eq!(vault.read(&id), Err(VaultError::NoAccess));
    assert_eq!(vault.read(&id), Ok(None));
}

struct Fixture {
    base: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let base = temp_dir(&format!("credentials-{}", uuid::Uuid::new_v4()));
        let global = Root::global(base.join("global"));
        let project = Root::project(&base.join("project"));
        fs::create_dir_all(&project.dir).unwrap();
        Self {
            base,
            store: Store {
                global,
                project: Some(project),
            },
        }
    }

    fn project(&self) -> &Path {
        &self.store.project.as_ref().unwrap().base
    }

    fn trust_path(&self) -> PathBuf {
        self.store.global.dir.join("trust.json")
    }

    fn write_trust(&self, value: &str) {
        fs::create_dir_all(&self.store.global.dir).unwrap();
        fs::write(self.trust_path(), value).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn checking_trust_creates_no_file_and_requires_both_first_confirmations() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    assert_eq!(
        trust
            .required(&identity(Scope::Project), Some(fixture.project()))
            .unwrap(),
        Required {
            destination: true,
            project_path: true,
        }
    );
    assert!(!fixture.trust_path().exists());
    assert!(!fixture.store.global.dir.exists());
}

#[test]
fn a_global_destination_does_not_need_a_project_path_confirmation() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    assert_eq!(
        trust.required(&identity(Scope::Global), None).unwrap(),
        Required {
            destination: true,
            project_path: false,
        }
    );
    trust.confirm(&identity(Scope::Global), None).unwrap();
    assert!(
        !trust
            .required(&identity(Scope::Global), None)
            .unwrap()
            .any()
    );
}

#[test]
fn trust_is_global_and_never_contains_a_password() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Project);
    trust.confirm(&id, Some(fixture.project())).unwrap();
    assert!(!trust.required(&id, Some(fixture.project())).unwrap().any());
    let text = fs::read_to_string(fixture.trust_path()).unwrap();
    assert!(!text.contains(PASSWORD));
    assert!(!text.contains("password"));
    assert!(
        !fixture
            .store
            .project
            .as_ref()
            .unwrap()
            .dir
            .join("trust.json")
            .exists()
    );
    assert!(
        !Trust::new(&fixture.store)
            .required(&id, Some(fixture.project()))
            .unwrap()
            .any()
    );
}

#[test]
fn a_new_project_path_requires_confirmation_even_for_a_known_destination() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Project);
    trust.confirm(&id, Some(fixture.project())).unwrap();
    let copied = fixture.base.join("copied");
    fs::create_dir(&copied).unwrap();
    assert_eq!(
        trust.required(&id, Some(&copied)).unwrap(),
        Required {
            destination: false,
            project_path: true,
        }
    );
    trust.confirm(&id, Some(&copied)).unwrap();
    assert!(!trust.required(&id, Some(&copied)).unwrap().any());
    assert!(!trust.required(&id, Some(fixture.project())).unwrap().any());
}

#[test]
fn a_changed_destination_requires_confirmation_without_forgetting_the_known_project_path() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Project);
    trust.confirm(&id, Some(fixture.project())).unwrap();
    let key = ConnectionKey {
        scope: Scope::Project,
        id: ID.into(),
    };
    for definition in changed_destinations() {
        let changed = Identity::of(&key, &definition).unwrap();
        assert_eq!(
            trust.required(&changed, Some(fixture.project())).unwrap(),
            Required {
                destination: true,
                project_path: false,
            }
        );
    }
}

#[test]
fn a_known_destination_does_not_authorize_an_unseen_project_connection() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    trust.confirm(&identity(Scope::Global), None).unwrap();
    assert_eq!(
        trust
            .required(&identity(Scope::Project), Some(fixture.project()))
            .unwrap(),
        Required {
            destination: false,
            project_path: true,
        }
    );
}

#[test]
fn a_project_identity_requires_a_real_project_folder() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Project);
    assert_eq!(trust.required(&id, None), Err(TrustError::ProjectPath));
    assert_eq!(
        trust.confirm(&id, Some(&fixture.base.join("missing"))),
        Err(TrustError::ProjectPath)
    );
    assert!(!fixture.trust_path().exists());
}

#[test]
fn damaged_or_unknown_trust_files_never_authorize_access_or_get_overwritten() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Global);
    for text in [
        "not json",
        "{\"version\":2}",
        "{\"version\":1,\"unknown\":true}",
        "{\"password\":\"secret\"}",
    ] {
        fixture.write_trust(text);
        assert_eq!(trust.required(&id, None), Err(TrustError::Invalid));
        assert_eq!(trust.confirm(&id, None), Err(TrustError::Invalid));
        assert_eq!(fs::read_to_string(fixture.trust_path()).unwrap(), text);
    }
}

#[test]
fn an_unreadable_trust_file_is_not_an_empty_trust_file() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.trust_path()).unwrap();
    assert_eq!(
        Trust::new(&fixture.store).required(&identity(Scope::Global), None),
        Err(TrustError::Read)
    );
}

#[test]
fn trust_errors_do_not_include_file_content() {
    let fixture = Fixture::new();
    fixture.write_trust(PASSWORD);
    let error = Trust::new(&fixture.store)
        .required(&identity(Scope::Global), None)
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains(PASSWORD));
}

#[test]
fn independent_trust_writers_keep_each_others_confirmations() {
    let fixture = Fixture::new();
    let mut threads = Vec::new();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    for n in 0..4 {
        let store = fixture.store.clone();
        let barrier = barrier.clone();
        let mut definition = definition();
        target(&mut definition).user = format!("user-{n}");
        let id = Identity::of(
            &ConnectionKey {
                scope: Scope::Global,
                id: ID.into(),
            },
            &definition,
        )
        .unwrap();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            Trust::new(&store).confirm(&id, None).unwrap();
            id
        }));
    }
    let trust = Trust::new(&fixture.store);
    for thread in threads {
        let id = thread.join().unwrap();
        assert!(!trust.required(&id, None).unwrap().any());
    }
}

#[test]
fn failed_trust_writes_do_not_grant_trust() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.store.global.dir).unwrap();
    fs::write(fixture.store.global.locks_dir(), "not a directory").unwrap();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Global);
    assert_eq!(trust.confirm(&id, None), Err(TrustError::Write));
    assert!(trust.required(&id, None).unwrap().destination);
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_known_project_does_not_create_a_second_location() {
    let fixture = Fixture::new();
    let trust = Trust::new(&fixture.store);
    let id = identity(Scope::Project);
    trust.confirm(&id, Some(fixture.project())).unwrap();
    let alias = fixture.base.join("project-alias");
    std::os::unix::fs::symlink(fixture.project(), &alias).unwrap();
    assert!(!trust.required(&id, Some(&alias)).unwrap().any());
}

#[cfg(unix)]
#[test]
fn private_atomic_writes_and_config_updates_have_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let path = fixture.base.join("private.json");
    file::write_private_atomic(&path, b"first", Expect::Missing).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    file::write_private_atomic(&path, b"second", Expect::Any).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let entry = config::to_entry(ID, &definition());
    config::change_connections(
        &fixture.store.global,
        Scope::Global,
        Change::Upsert(Box::new(entry)),
    )
    .unwrap();
    assert_eq!(
        fs::metadata(fixture.store.global.config_file())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Trust::new(&fixture.store)
        .confirm(&identity(Scope::Global), None)
        .unwrap();
    assert_eq!(
        fs::metadata(fixture.trust_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn a_failed_private_write_preserves_the_destination_and_removes_its_temporary_file() {
    let fixture = Fixture::new();
    let path = fixture.base.join("private.json");
    fs::write(&path, b"old").unwrap();
    assert_eq!(
        file::write_private_atomic(&path, b"new", Expect::Hash(file::hash(b"other"))),
        Err(WriteError::Changed)
    );
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert!(!fs::read_dir(&fixture.base).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "tmp")
    }));
}

#[cfg(unix)]
#[test]
fn a_private_write_never_follows_a_preexisting_temporary_symlink() {
    let fixture = Fixture::new();
    let path = fixture.base.join("private.json");
    let protected = fixture.base.join("other-file");
    fs::write(&protected, b"unchanged").unwrap();
    std::os::unix::fs::symlink(&protected, fixture.base.join("private.json.tmp")).unwrap();
    file::write_private_atomic(&path, b"private content", Expect::Missing).unwrap();
    assert_eq!(fs::read(&protected).unwrap(), b"unchanged");
}

#[test]
#[ignore = "Requires explicit SQLN_TEST_SYSTEM_VAULT=1 and access to the real system vault."]
fn native_vault_round_trip() {
    assert_eq!(std::env::var("SQLN_TEST_SYSTEM_VAULT").as_deref(), Ok("1"));
    let key = ConnectionKey {
        scope: Scope::Global,
        id: uuid::Uuid::new_v4().to_string(),
    };
    let id = Identity::of(&key, &definition()).unwrap();
    let vault = SystemVault::new();
    struct Cleanup<'a>(&'a SystemVault, &'a Identity);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = self.0.delete(self.1);
        }
    }
    assert!(vault.read(&id).unwrap().is_none());
    vault.write(&id, &Secret::new(PASSWORD)).unwrap();
    let _cleanup = Cleanup(&vault, &id);
    assert_eq!(vault.read(&id).unwrap().unwrap().expose(), PASSWORD);
    assert!(vault.delete(&id).unwrap());
    assert!(vault.read(&id).unwrap().is_none());
}

fn plain_fixture(name: &str) -> (PathBuf, Root, Identity) {
    let base = temp_dir(&format!("{name}-{}", uuid::Uuid::new_v4()));
    let root = Root::global(base.clone());
    let key = ConnectionKey {
        scope: Scope::Global,
        id: ID.into(),
    };
    (base, root, Identity::of(&key, &definition()).unwrap())
}

#[test]
fn the_plain_file_holds_one_entry_for_each_identity_and_only_the_owner_reads_it() {
    let (_base, root, identity) = plain_fixture("plain-entry");
    let file = PlainFile::new(&root);
    file.write(&identity, &Secret::new(PASSWORD)).unwrap();
    assert_eq!(file.read(&identity).unwrap().unwrap().expose(), PASSWORD);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.credentials_file())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let mut other = definition();
    target(&mut other).user = "another_user".into();
    let key = ConnectionKey {
        scope: Scope::Global,
        id: ID.into(),
    };
    let changed = Identity::of(&key, &other).unwrap();
    assert!(
        file.read(&changed).unwrap().is_none(),
        "a new destination finds no password"
    );
}

#[test]
fn the_last_deletion_removes_the_plain_file() {
    let (_base, root, identity) = plain_fixture("plain-delete");
    let file = PlainFile::new(&root);
    assert_eq!(file.delete(&identity), Ok(false));
    file.write(&identity, &Secret::new(PASSWORD)).unwrap();
    assert_eq!(file.delete(&identity), Ok(true));
    assert!(!root.credentials_file().exists());
}

#[test]
fn a_damaged_plain_file_is_an_error_and_stays_as_it_is() {
    let (_base, root, identity) = plain_fixture("plain-damaged");
    fs::create_dir_all(&root.dir).unwrap();
    fs::write(root.credentials_file(), "{ not json").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.credentials_file(), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let file = PlainFile::new(&root);
    assert_eq!(file.read(&identity), Err(VaultError::PlainFile));
    assert_eq!(
        file.write(&identity, &Secret::new(PASSWORD)),
        Err(VaultError::PlainFile)
    );
    assert_eq!(
        fs::read_to_string(root.credentials_file()).unwrap(),
        "{ not json"
    );
}

#[cfg(unix)]
#[test]
fn a_plain_file_that_others_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let (_base, root, identity) = plain_fixture("plain-open");
    let file = PlainFile::new(&root);
    file.write(&identity, &Secret::new(PASSWORD)).unwrap();
    fs::set_permissions(root.credentials_file(), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(file.read(&identity), Err(VaultError::PlainFile));
}

#[cfg(unix)]
#[test]
fn a_plain_file_that_is_a_link_is_refused() {
    let (base, root, identity) = plain_fixture("plain-link");
    let file = PlainFile::new(&root);
    file.write(&identity, &Secret::new(PASSWORD)).unwrap();
    let real = base.join("elsewhere.json");
    fs::rename(root.credentials_file(), &real).unwrap();
    std::os::unix::fs::symlink(&real, root.credentials_file()).unwrap();
    assert_eq!(file.read(&identity), Err(VaultError::PlainFile));
}
