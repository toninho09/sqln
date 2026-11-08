mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::{ctrl, key, temp_dir};
use crossterm::event::{Event, KeyCode};
use ratatui::{Terminal, backend::TestBackend};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::{ConnectionKey, Definition, Source};
use sqln::connector::target::{Address, Engine, NetworkTarget, Secret, Tls, parse_uri};
use sqln::console::Phase;
use sqln::creds::{Identity, PlainFile, Vault, VaultError};
use sqln::form::Field;
use sqln::store::config::{self, CredentialName};
use sqln::store::workspace::{self, OpenTab, OpenTabs};
use sqln::store::{Root, Scope, Store};
use sqln::trust::Trust;
use sqln::ui::theme::Theme;

const ID: &str = "0f3726f5-16d2-46af-82f9-54736aaa81f8";
const PASSWORD: &str = "private-password-α";

#[derive(Default)]
struct State {
    values: HashMap<Identity, Secret>,
    reads: usize,
    writes: usize,
    read_error: Option<VaultError>,
    write_error: Option<VaultError>,
}

#[derive(Default)]
struct FakeVault {
    state: Mutex<State>,
    blocked: Mutex<bool>,
    wake: Condvar,
}

impl FakeVault {
    fn release(&self) {
        *self.blocked.lock().unwrap() = false;
        self.wake.notify_all();
    }
}

impl Vault for FakeVault {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        self.state.lock().unwrap().reads += 1;
        let blocked = self.blocked.lock().unwrap();
        let (blocked, _) = self
            .wake
            .wait_timeout_while(blocked, Duration::from_secs(2), |blocked| *blocked)
            .unwrap();
        if *blocked {
            return Err(VaultError::Failed);
        }
        let state = self.state.lock().unwrap();
        if let Some(error) = state.read_error {
            return Err(error);
        }
        Ok(state.values.get(identity).cloned())
    }

    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        let mut state = self.state.lock().unwrap();
        state.writes += 1;
        if let Some(error) = state.write_error {
            return Err(error);
        }
        state.values.insert(identity.clone(), password.clone());
        Ok(())
    }

    fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        Ok(self.state.lock().unwrap().values.remove(identity).is_some())
    }
}

fn definition() -> Definition {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    Definition::new(
        "test-db",
        Source::Network(NetworkTarget {
            engine: Engine::Postgres,
            address: Address::Tcp {
                host: "127.0.0.1".into(),
                port,
            },
            user: "test_user".into(),
            password: None,
            database: None,
            tls: Tls::Disabled,
        }),
    )
}

struct Fixture {
    app: App,
    store: Store,
    base: PathBuf,
    vault: Arc<FakeVault>,
    definition: Definition,
}

impl Fixture {
    fn new(credential: CredentialName, trusted: bool) -> Self {
        Self::with_definition(definition(), credential, trusted)
    }

    fn with_definition(definition: Definition, credential: CredentialName, trusted: bool) -> Self {
        let base = temp_dir(&format!("auth-{}", uuid::Uuid::new_v4()));
        let store = Store {
            global: Root::global(base.join("global")),
            project: None,
        };
        let vault = Arc::new(FakeVault::default());
        fs::create_dir_all(&store.global.dir).unwrap();
        let mut entry = config::to_entry(ID, &definition);
        entry.credential = credential;
        fs::write(
            store.global.config_file(),
            serde_json::to_vec(&serde_json::json!({"version":1,"connections":[entry]})).unwrap(),
        )
        .unwrap();
        let identity = Identity::of(
            &ConnectionKey {
                scope: Scope::Global,
                id: ID.into(),
            },
            &definition,
        )
        .unwrap();
        if trusted {
            Trust::new(&store).confirm(&identity, None).unwrap();
        }
        let app = App::new(
            Options {
                store: Some(store.clone()),
                vault: Some(vault.clone()),
                ..Options::default()
            },
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
        Self {
            app,
            store,
            base,
            vault,
            definition,
        }
    }

    fn copied_project(&mut self) {
        let old = self.base.join("original-project");
        let copied = Root::project(&self.base.join("copied-project"));
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&copied.dir).unwrap();
        let key = ConnectionKey {
            scope: Scope::Project,
            id: ID.into(),
        };
        let identity = Identity::of(&key, &self.definition).unwrap();
        Trust::new(&self.store)
            .confirm(&identity, Some(&old))
            .unwrap();
        fs::rename(self.store.global.config_file(), copied.config_file()).unwrap();
        self.store.project = Some(copied);
        self.app.shutdown();
        self.app = App::new(
            Options {
                store: Some(self.store.clone()),
                vault: Some(self.vault.clone()),
                ..Options::default()
            },
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
    }

    fn identity(&self) -> Identity {
        Identity::of(
            &ConnectionKey {
                scope: Scope::Global,
                id: ID.into(),
            },
            &self.definition,
        )
        .unwrap()
    }

    fn code(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }
    fn prefix(&mut self, c: char) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.code(KeyCode::Char(c));
    }
    fn paste(&mut self, value: &str) {
        self.app.on_event(Event::Paste(value.into()));
    }
    fn run(&mut self) {
        self.paste("SELECT 1;");
        self.code(KeyCode::F(5));
    }
    fn step(&mut self) {
        self.app.drain_events();
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    fn wait(&mut self, mut predicate: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            self.step();
            if predicate(self) {
                break;
            }
            assert!(Instant::now() < deadline, "{}", self.screen());
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn sees(&mut self, text: &str) {
        self.wait(|fixture| fixture.screen().contains(text));
    }
    fn confirm(&mut self) {
        self.code(KeyCode::Tab);
        self.code(KeyCode::Enter);
    }
    fn enter_password(&mut self, save: bool) {
        self.paste(PASSWORD);
        assert!(!self.screen().contains(PASSWORD));
        if save {
            self.code(KeyCode::Tab);
            self.code(KeyCode::Char(' '));
            self.code(KeyCode::Tab);
        }
        self.code(KeyCode::Enter);
    }
    fn idle(&mut self) {
        self.wait(|fixture| fixture.app.console().phase == Phase::Idle);
    }
    fn password(&self) -> Option<&str> {
        let Source::Network(target) = &self.app.connections()[0]
            .definition
            .as_ref()
            .unwrap()
            .source
        else {
            panic!("Network definition expected")
        };
        target.password.as_ref().map(Secret::expose)
    }
    fn focus(&mut self, field: Field) {
        common::focus_form_field(&mut self.app, field);
    }
    fn edit(&mut self) {
        self.prefix('h');
        self.code(KeyCode::Char('e'));
        assert!(self.app.form().is_some());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.vault.release();
        self.app.shutdown();
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn an_imported_destination_requires_confirmation_before_any_vault_read() {
    let mut f = Fixture::new(CredentialName::Vault, false);
    f.run();
    f.sees("Confirm destination");
    assert!(f.screen().contains("127.0.0.1"));
    assert!(f.screen().contains("test_user"));
    assert!(f.screen().contains("no TLS"));
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    f.code(KeyCode::Esc);
    f.idle();
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    assert!(f.password().is_none());
    assert!(!f.store.global.dir.join("trust.json").exists());
}

#[test]
fn enter_on_the_initial_trust_option_cancels_instead_of_sending_a_password() {
    let mut f = Fixture::new(CredentialName::Session, false);
    f.run();
    f.sees("Confirm destination");
    f.code(KeyCode::Enter);
    f.idle();
    assert!(f.password().is_none());
}

#[test]
fn confirmed_trust_persists_and_a_missing_vault_entry_opens_a_masked_password_field() {
    let mut f = Fixture::new(CredentialName::Vault, false);
    f.run();
    f.sees("Confirm destination");
    f.confirm();
    f.sees("Password · test-db");
    assert!(f.store.global.dir.join("trust.json").exists());
    assert_eq!(f.vault.state.lock().unwrap().reads, 1);
    f.paste(PASSWORD);
    assert!(!f.screen().contains(PASSWORD));
    assert!(f.screen().contains('•'));
    f.code(KeyCode::Esc);
    f.idle();
    assert!(f.password().is_none());
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
}

#[test]
fn a_session_password_is_not_saved_and_does_not_open_the_system_vault() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.sees("Password · test-db");
    f.enter_password(false);
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
    let state = f.vault.state.lock().unwrap();
    assert_eq!(state.reads, 0);
    assert_eq!(state.writes, 0);
    let text = fs::read_to_string(f.store.global.config_file()).unwrap();
    assert!(!text.contains(PASSWORD));
    assert!(text.contains("session"));
}

#[test]
fn saving_a_prompt_password_updates_only_the_credential_reference() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.sees("Password · test-db");
    f.enter_password(true);
    f.idle();
    assert_eq!(f.vault.state.lock().unwrap().writes, 1);
    assert_eq!(f.app.connections()[0].credential, CredentialName::Vault);
    assert_eq!(
        f.vault
            .state
            .lock()
            .unwrap()
            .values
            .get(&f.identity())
            .unwrap()
            .expose(),
        PASSWORD
    );
    let text = fs::read_to_string(f.store.global.config_file()).unwrap();
    assert!(text.contains("vault"));
    assert!(!text.contains(PASSWORD));
}

#[test]
fn a_blocked_vault_offers_retry_and_session_without_any_plaintext_write() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault.state.lock().unwrap().read_error = Some(VaultError::NoAccess);
    let before = fs::read(f.store.global.config_file()).unwrap();
    f.run();
    f.sees("locked or access is denied");
    f.code(KeyCode::Tab);
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.sees("Password · test-db");
    assert!(!f.screen().contains("Save in the system vault"));
    f.enter_password(false);
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
    assert_eq!(fs::read(f.store.global.config_file()).unwrap(), before);
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
}

#[test]
fn an_explicit_retry_reads_the_vault_again() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault.state.lock().unwrap().read_error = Some(VaultError::Failed);
    f.run();
    f.sees("system vault operation fails");
    f.vault.state.lock().unwrap().read_error = None;
    f.confirm();
    f.sees("Password · test-db");
    assert_eq!(f.vault.state.lock().unwrap().reads, 2);
    f.code(KeyCode::Esc);
}

#[test]
fn a_failed_vault_save_keeps_the_password_for_a_session_without_an_automatic_retry() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.vault.state.lock().unwrap().write_error = Some(VaultError::Failed);
    f.run();
    f.sees("Password · test-db");
    f.enter_password(true);
    f.sees("password is not saved");
    assert_eq!(f.vault.state.lock().unwrap().writes, 1);
    assert_eq!(f.app.console().phase, Phase::Waiting);
    f.code(KeyCode::Tab);
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
    assert_eq!(f.vault.state.lock().unwrap().writes, 1);
    assert_eq!(f.app.connections()[0].credential, CredentialName::Session);
}

#[test]
fn a_failed_reference_write_does_not_erase_the_vault_password() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.sees("Password · test-db");
    fs::write(f.store.global.config_file(), "invalid configuration").unwrap();
    f.enter_password(true);
    f.sees("cannot save its reference");
    assert_eq!(f.vault.state.lock().unwrap().writes, 1);
    assert_eq!(
        f.vault
            .state
            .lock()
            .unwrap()
            .values
            .get(&f.identity())
            .unwrap()
            .expose(),
        PASSWORD
    );
    f.code(KeyCode::Esc);
    f.idle();
    assert!(f.password().is_none());
}

#[test]
fn a_damaged_trust_file_cannot_fall_back_to_a_session_password() {
    let mut f = Fixture::new(CredentialName::Session, false);
    fs::write(f.store.global.dir.join("trust.json"), "damaged").unwrap();
    f.run();
    f.sees("trust.json file is invalid");
    assert!(!f.screen().contains("Use for this session"));
    f.code(KeyCode::Esc);
    f.idle();
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    assert_eq!(
        fs::read_to_string(f.store.global.dir.join("trust.json")).unwrap(),
        "damaged"
    );
}

#[test]
fn a_slow_vault_does_not_stop_editor_input_and_stopping_discards_its_late_reply() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(f.identity(), Secret::new(PASSWORD));
    *f.vault.blocked.lock().unwrap() = true;
    f.run();
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    let start = Instant::now();
    f.paste(" -- editor stays available");
    assert!(start.elapsed() < Duration::from_millis(200));
    assert!(
        f.app
            .console()
            .editor
            .text()
            .contains("editor stays available")
    );
    f.prefix('x');
    assert_eq!(f.app.console().phase, Phase::Idle);
    f.vault.release();
    for _ in 0..20 {
        f.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(f.password().is_none());
}

#[test]
fn edits_during_a_credential_wait_do_not_mark_the_new_sql_as_executed() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(f.identity(), Secret::new(PASSWORD));
    *f.vault.blocked.lock().unwrap() = true;
    f.run();
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    f.app.on_event(Event::Key(ctrl('a')));
    f.paste("SELECT 22 AS changed;");
    f.vault.release();
    f.idle();
    assert!(f.app.console().editor.text().contains("SELECT 22"));
    assert!(f.app.console_mut().editor.marks().is_empty());
}

#[test]
fn canceled_queued_jobs_do_not_read_the_vault_after_a_slow_call_returns() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(f.identity(), Secret::new(PASSWORD));
    *f.vault.blocked.lock().unwrap() = true;
    f.run();
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    for _ in 0..8 {
        f.prefix('x');
        f.code(KeyCode::F(5));
        f.step();
    }
    f.prefix('x');
    f.vault.release();
    for _ in 0..30 {
        f.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(f.vault.state.lock().unwrap().reads, 1);
    assert!(f.password().is_none());
}

#[test]
fn repeated_run_during_authorization_does_not_add_a_second_execution() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.code(KeyCode::F(5));
    f.sees("Password · test-db");
    f.code(KeyCode::Esc);
    f.idle();
    for _ in 0..10 {
        f.step();
    }
    assert!(!f.screen().contains("Password · test-db"));
}

#[test]
fn opening_the_catalog_also_requires_trust_before_a_vault_read() {
    let mut f = Fixture::new(CredentialName::Vault, false);
    f.code(KeyCode::Char('s'));
    f.sees("Confirm destination");
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    f.code(KeyCode::Esc);
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
}

#[test]
fn a_changed_form_discards_a_password_reply_from_an_old_test() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(f.identity(), Secret::new(PASSWORD));
    *f.vault.blocked.lock().unwrap() = true;
    f.edit();
    f.focus(Field::Test);
    f.code(KeyCode::Enter);
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    f.focus(Field::Host);
    f.paste(".changed");
    f.step();
    assert!(f.app.form().unwrap().test.is_none());
    f.vault.release();
    for _ in 0..20 {
        f.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(f.password().is_none());
}

#[test]
fn testing_a_form_does_not_save_a_typed_password_in_the_vault() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.edit();
    f.focus(Field::Password);
    f.paste(PASSWORD);
    f.focus(Field::Test);
    f.code(KeyCode::Enter);
    f.wait(|f| f.app.form().unwrap().test.is_none());
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    assert!(f.password().is_none());
}

#[test]
fn a_local_form_save_registers_trust_without_a_destination_question() {
    let mut f = Fixture::new(CredentialName::Session, false);
    f.edit();
    f.focus(Field::Password);
    f.paste(PASSWORD);
    f.focus(Field::Save);
    f.code(KeyCode::Enter);
    f.wait(|f| f.store.global.dir.join("trust.json").exists());
    assert!(!f.screen().contains("Confirm destination"));
    assert!(f.app.form().is_none());
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
}

#[test]
fn choosing_the_vault_in_a_local_form_requires_an_explicit_save() {
    let mut f = Fixture::new(CredentialName::Session, false);
    f.edit();
    f.focus(Field::Password);
    f.paste(PASSWORD);
    f.focus(Field::Credential);
    f.code(KeyCode::Right);
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
    f.focus(Field::Save);
    f.code(KeyCode::Enter);
    f.wait(|f| f.vault.state.lock().unwrap().writes == 1);
    assert!(!f.screen().contains("Confirm destination"));
    assert!(
        !fs::read_to_string(f.store.global.config_file())
            .unwrap()
            .contains(PASSWORD)
    );
}

#[test]
fn global_plaintext_is_not_silently_treated_as_a_passwordless_connection() {
    let mut f = Fixture::new(CredentialName::GlobalPlainText, true);
    f.run();
    f.sees("Password · test-db");
    assert_eq!(f.app.console().phase, Phase::Waiting);
    f.code(KeyCode::Esc);
    f.idle();
}

#[test]
fn no_tcp_connection_starts_before_trust_and_password_are_confirmed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut definition = definition();
    let Source::Network(target) = &mut definition.source else {
        unreachable!()
    };
    target.address = Address::Tcp {
        host: "127.0.0.1".into(),
        port: listener.local_addr().unwrap().port(),
    };
    let mut f = Fixture::with_definition(definition, CredentialName::Session, false);
    f.run();
    f.sees("Confirm destination");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    f.confirm();
    f.sees("Password · test-db");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    f.enter_password(false);
    let mut accepted = None;
    f.wait(|_| match listener.accept() {
        Ok((stream, _)) => {
            accepted = Some(stream);
            true
        }
        Err(_) => false,
    });
    drop(accepted);
    f.idle();
}

#[test]
fn a_copied_project_requires_confirmation_before_reusing_a_vault_entry() {
    let mut f = Fixture::new(CredentialName::Vault, false);
    f.copied_project();
    f.run();
    f.sees("Confirm project path");
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    f.code(KeyCode::Esc);
    f.idle();
    assert!(f.password().is_none());
}

#[test]
fn testing_an_existing_form_cannot_authorize_a_copied_project_implicitly() {
    let mut f = Fixture::new(CredentialName::Vault, false);
    f.copied_project();
    f.edit();
    f.focus(Field::Test);
    f.code(KeyCode::Enter);
    f.sees("Confirm project path");
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    f.code(KeyCode::Esc);
    assert!(f.app.form().unwrap().test.is_none());
}

#[test]
fn saving_an_existing_form_cannot_authorize_a_copied_project_implicitly() {
    let mut f = Fixture::new(CredentialName::Vault, false);
    f.copied_project();
    f.edit();
    f.focus(Field::Save);
    f.code(KeyCode::Enter);
    f.sees("Confirm project path");
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
    f.code(KeyCode::Esc);
}

#[test]
fn a_typed_test_password_does_not_replace_a_saved_vault_password() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(f.identity(), Secret::new("original-vault-password"));
    f.edit();
    f.focus(Field::Password);
    f.paste(PASSWORD);
    f.focus(Field::Test);
    f.code(KeyCode::Enter);
    f.wait(|f| f.app.form().unwrap().test.is_none());
    f.code(KeyCode::Esc);
    f.prefix('l');
    f.run();
    f.idle();
    assert_eq!(f.password(), Some("original-vault-password"));
    assert_eq!(f.vault.state.lock().unwrap().reads, 1);
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
}

#[test]
fn a_mouse_click_submits_only_the_masked_password_prompt() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.sees("Password · test-db");
    f.paste(PASSWORD);
    let screen = f.screen();
    assert!(!screen.contains(PASSWORD));
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("[Connect]"))
        .unwrap();
    let column = line.find("[Connect]").unwrap();
    f.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: column as u16,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    }));
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
}

#[test]
fn secret_line_input_never_exposes_its_value_in_debug_text() {
    let mut input = sqln::line_input::LineInput::default();
    input.insert(PASSWORD);
    assert!(!format!("{input:?}").contains(PASSWORD));
}

#[test]
fn a_vault_password_is_used_with_real_postgres_and_the_sql_snapshot_stays_fixed() {
    let Ok(uri) = std::env::var("SQLN_TEST_POSTGRES") else {
        println!("skipped: SQLN_TEST_POSTGRES is not set");
        return;
    };
    let mut target = parse_uri(&uri).unwrap();
    let password = target.password.take().unwrap();
    let definition = Definition::new("real-postgres", Source::Network(target));
    let mut f = Fixture::with_definition(definition, CredentialName::Vault, true);
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(f.identity(), password);
    *f.vault.blocked.lock().unwrap() = true;
    f.paste("SELECT 11 AS original;");
    f.code(KeyCode::F(5));
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    f.app.on_event(Event::Key(ctrl('a')));
    f.paste("SELECT 22 AS changed;");
    f.vault.release();
    f.idle();
    assert_eq!(
        f.app.console().results[0].table.rows[0].get(0).unwrap(),
        sqln::grid::Value::Number("11")
    );
    assert!(f.app.console().editor.text().contains("SELECT 22"));
    assert!(f.app.console_mut().editor.marks().is_empty());
    assert_eq!(f.vault.state.lock().unwrap().reads, 1);
}

fn plain_file(f: &Fixture) -> PlainFile {
    PlainFile::new(&f.store.global)
}

/// Moves the focus of an open dialog to one option, and presses it.
fn choose(f: &mut Fixture, option: usize) {
    for _ in 0..option {
        f.code(KeyCode::Tab);
    }
    f.code(KeyCode::Enter);
}

#[test]
fn without_a_vault_the_save_option_warns_and_writes_only_the_global_file() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault.state.lock().unwrap().read_error = Some(VaultError::Absent);
    f.run();
    f.sees("Save in plain text in credentials.json");
    f.enter_password(true);
    f.idle();
    let secrets = f.store.global.credentials_file();
    assert!(fs::read_to_string(&secrets).unwrap().contains(PASSWORD));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&secrets).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let config = fs::read_to_string(f.store.global.config_file()).unwrap();
    assert!(!config.contains(PASSWORD));
    assert!(config.contains("globalPlainText"));
    assert_eq!(
        f.app.connections()[0].credential,
        CredentialName::GlobalPlainText
    );
    assert_eq!(f.vault.state.lock().unwrap().writes, 0);
}

#[test]
fn an_unmarked_save_option_writes_nothing_without_a_vault() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.vault.state.lock().unwrap().read_error = Some(VaultError::Absent);
    f.run();
    f.sees("Save in plain text in credentials.json");
    f.enter_password(false);
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
    assert!(!f.store.global.credentials_file().exists());
}

#[test]
fn a_plain_text_password_answers_without_a_prompt_when_no_vault_exists() {
    let mut f = Fixture::new(CredentialName::GlobalPlainText, true);
    f.vault.state.lock().unwrap().read_error = Some(VaultError::Absent);
    plain_file(&f)
        .write(&f.identity(), &Secret::new(PASSWORD))
        .unwrap();
    f.run();
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
}

#[test]
fn a_session_reference_does_not_touch_the_vault_without_a_plain_text_password() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.sees("Password · test-db");
    assert_eq!(f.vault.state.lock().unwrap().reads, 0);
}

#[test]
fn a_locked_vault_never_uses_the_plain_text_password() {
    for error in [VaultError::NoAccess, VaultError::Failed] {
        let mut f = Fixture::new(CredentialName::Vault, true);
        f.vault.state.lock().unwrap().read_error = Some(error);
        plain_file(&f)
            .write(&f.identity(), &Secret::new(PASSWORD))
            .unwrap();
        f.run();
        f.sees("System vault");
        assert!(!f.screen().contains("plain text"));
        assert!(f.password().is_none());
    }
}

#[test]
fn a_failed_write_offers_plain_text_only_when_no_vault_exists() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.vault.state.lock().unwrap().write_error = Some(VaultError::Failed);
    f.run();
    f.sees("Password · test-db");
    f.enter_password(true);
    f.sees("The password is not saved");
    assert!(!f.screen().contains("Save in plain text"));
    assert!(!f.store.global.credentials_file().exists());

    let mut f = Fixture::new(CredentialName::Session, true);
    f.vault.state.lock().unwrap().write_error = Some(VaultError::Absent);
    f.run();
    f.sees("Password · test-db");
    f.enter_password(true);
    f.sees("Save in plain text");
    assert!(f.screen().contains("That file stores it in plain text"));
    assert!(!f.store.global.credentials_file().exists());
    choose(&mut f, 3);
    f.idle();
    assert!(f.store.global.credentials_file().exists());
    assert_eq!(
        f.app.connections()[0].credential,
        CredentialName::GlobalPlainText
    );
}

#[test]
fn a_project_connection_stores_the_plain_password_only_in_the_global_folder() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.copied_project();
    f.vault.state.lock().unwrap().read_error = Some(VaultError::Absent);
    f.run();
    f.sees("Confirm");
    f.confirm();
    f.sees("Save in plain text in credentials.json");
    f.enter_password(true);
    f.idle();
    let project = f.store.project.clone().unwrap();
    let config = fs::read_to_string(project.config_file()).unwrap();
    assert!(!config.contains(PASSWORD));
    assert!(!config.contains("globalPlainText"));
    assert_eq!(f.app.connections()[0].credential, CredentialName::Vault);
    assert!(
        fs::read_to_string(f.store.global.credentials_file())
            .unwrap()
            .contains(PASSWORD)
    );
}

#[test]
fn a_migration_removes_the_source_only_after_the_vault_answers() {
    let mut f = Fixture::new(CredentialName::GlobalPlainText, true);
    plain_file(&f)
        .write(&f.identity(), &Secret::new(PASSWORD))
        .unwrap();
    f.run();
    f.sees("Move the password to the vault");
    choose(&mut f, 1);
    f.sees("read it back");
    assert!(f.store.global.credentials_file().exists());
    assert_eq!(f.vault.state.lock().unwrap().writes, 1);
    choose(&mut f, 1);
    f.idle();
    assert!(!f.store.global.credentials_file().exists());
    assert_eq!(f.app.connections()[0].credential, CredentialName::Vault);
    assert!(
        f.vault
            .state
            .lock()
            .unwrap()
            .values
            .contains_key(&f.identity())
    );
}

#[test]
fn a_failed_migration_keeps_the_source_and_says_so() {
    let mut f = Fixture::new(CredentialName::GlobalPlainText, true);
    f.vault.state.lock().unwrap().write_error = Some(VaultError::Failed);
    plain_file(&f)
        .write(&f.identity(), &Secret::new(PASSWORD))
        .unwrap();
    f.run();
    f.sees("Move the password to the vault");
    choose(&mut f, 1);
    f.sees("The migration is not complete");
    choose(&mut f, 0);
    f.idle();
    assert_eq!(f.password(), Some(PASSWORD));
    assert!(
        fs::read_to_string(f.store.global.credentials_file())
            .unwrap()
            .contains(PASSWORD)
    );
}

#[test]
fn a_migration_can_keep_the_plain_text_copy() {
    let mut f = Fixture::new(CredentialName::GlobalPlainText, true);
    plain_file(&f)
        .write(&f.identity(), &Secret::new(PASSWORD))
        .unwrap();
    f.run();
    f.sees("Move the password to the vault");
    choose(&mut f, 1);
    f.sees("read it back");
    choose(&mut f, 0);
    f.idle();
    assert!(f.store.global.credentials_file().exists());
    assert!(
        f.vault
            .state
            .lock()
            .unwrap()
            .values
            .contains_key(&f.identity())
    );
}

#[test]
fn the_separate_deletion_asks_and_removes_the_saved_password() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    let identity = f.identity();
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(identity.clone(), Secret::new(PASSWORD));
    f.prefix('h');
    f.code(KeyCode::Char('p'));
    f.sees("Delete saved password");
    assert!(f.screen().contains("Other copies of the project"));
    choose(&mut f, 0);
    assert!(f.vault.state.lock().unwrap().values.contains_key(&identity));

    f.code(KeyCode::Char('p'));
    f.sees("Delete saved password");
    choose(&mut f, 1);
    f.wait(|f| !f.vault.state.lock().unwrap().values.contains_key(&identity));
    f.sees("The saved password is deleted");
}

#[test]
fn the_separate_deletion_also_removes_a_plain_text_password() {
    let mut f = Fixture::new(CredentialName::GlobalPlainText, true);
    plain_file(&f)
        .write(&f.identity(), &Secret::new(PASSWORD))
        .unwrap();
    f.prefix('h');
    f.code(KeyCode::Char('p'));
    f.sees("Delete saved password");
    choose(&mut f, 1);
    f.sees("The saved password is deleted");
    assert!(!f.store.global.credentials_file().exists());
}

#[test]
fn removing_a_connection_offers_the_deletion_and_keeps_the_password_by_default() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    let identity = f.identity();
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(identity.clone(), Secret::new(PASSWORD));
    f.prefix('h');
    f.code(KeyCode::Delete);
    f.sees("Remove connection");
    choose(&mut f, 1);
    f.sees("Delete saved password");
    assert!(f.app.connections().is_empty());
    f.code(KeyCode::Enter);
    assert!(f.vault.state.lock().unwrap().values.contains_key(&identity));
}

const WAIT_MESSAGE: &str = "Preparing credentials";

#[test]
fn the_credential_message_lasts_as_long_as_the_wait_and_ends_with_it() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    let identity = f.identity();
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(identity, Secret::new(PASSWORD));
    *f.vault.blocked.lock().unwrap() = true;
    f.run();
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    f.sees(WAIT_MESSAGE);
    // A key and a long time do not end the wait, so the message stays.
    f.code(KeyCode::Char(' '));
    f.app.tick_at(Instant::now());
    f.app.tick_at(Instant::now() + Duration::from_secs(60));
    assert!(f.screen().contains(WAIT_MESSAGE), "{}", f.screen());
    f.vault.release();
    f.wait(|f| !f.screen().contains(WAIT_MESSAGE));
}

#[test]
fn the_credential_message_gives_way_to_the_hint_of_the_password_prompt() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    f.run();
    f.sees("Enter connects");
    assert!(!f.screen().contains(WAIT_MESSAGE), "{}", f.screen());
}

#[test]
fn a_table_of_the_last_session_reads_no_password_until_the_user_asks() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    let identity = f.identity();
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(identity, Secret::new(PASSWORD));
    let table = OpenTab::Table {
        scope: Scope::Global,
        connection: ID.to_owned(),
        schema: "public".to_owned(),
        relation: "users".to_owned(),
        view: false,
    };
    let tabs = OpenTabs {
        tabs: vec![table],
        active: Some(0),
    };
    workspace::update(&f.store.global, |file| file.open_tabs = Some(tabs)).unwrap();

    f.app = App::new(
        Options {
            store: Some(f.store.clone()),
            vault: Some(f.vault.clone()),
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    for _ in 0..20 {
        f.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        f.screen().contains("Enter loads the rows."),
        "{}",
        f.screen()
    );
    assert_eq!(
        f.vault.state.lock().unwrap().reads,
        0,
        "the start reads no password"
    );

    f.code(KeyCode::Enter);
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
}

#[test]
fn the_time_of_a_run_does_not_count_the_wait_for_credentials() {
    let mut f = Fixture::new(CredentialName::Vault, true);
    let identity = f.identity();
    f.vault
        .state
        .lock()
        .unwrap()
        .values
        .insert(identity, Secret::new(PASSWORD));
    *f.vault.blocked.lock().unwrap() = true;
    f.run();
    f.wait(|f| f.vault.state.lock().unwrap().reads == 1);
    f.app.tick_at(Instant::now() + Duration::from_secs(65));
    let screen = f.screen();
    assert!(screen.contains("Waiting"), "{screen}");
    assert!(!screen.contains("1m 0"), "{screen}");
    f.prefix('x');
    f.vault.release();
}

/// Asks the open palette for the technical name of the action.
fn paste_palette(f: &mut Fixture, query: &str) {
    f.app.on_event(Event::Key(ctrl('p')));
    f.paste(query);
    assert!(
        !f.screen().contains("No command matches"),
        "{query}: {}",
        f.screen()
    );
    f.code(KeyCode::Esc);
}

#[test]
fn the_palette_offers_the_command_that_deletes_the_saved_password() {
    let mut f = Fixture::new(CredentialName::Session, true);
    f.run();
    f.sees("Password · test-db");
    f.enter_password(true);
    f.idle();
    f.prefix('h');
    paste_palette(&mut f, "deletePassword");
}
