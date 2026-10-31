// The answer to a refused password, with real servers. Load `scripts/test-env.sh` first.
// Without the variables, these tests print "skipped".

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{key, temp_dir};
use crossterm::event::{Event, KeyCode};
use ratatui::{Terminal, backend::TestBackend};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::{CatalogState, ConnectionKey, Definition, Source};
use sqln::connector::target::{NetworkTarget, Secret, parse_uri};
use sqln::connector::{Connector, network_connector};
use sqln::creds::{Identity, Vault, VaultError};
use sqln::store::config::{self, CredentialName};
use sqln::store::{Root, Scope, Store};
use sqln::trust::Trust;
use sqln::ui::theme::Theme;

const ID: &str = "7f3726f5-16d2-46af-82f9-54736aaa81f9";
const WRONG: &str = "not-the-password";
const OTHER_WRONG: &str = "also-not-the-password";

fn servers() -> Vec<String> {
    let mut found = Vec::new();
    for name in ["SQLN_TEST_POSTGRES", "SQLN_TEST_MYSQL"] {
        match std::env::var(name) {
            Ok(uri) => found.push(uri),
            Err(_) => println!("skipped: {name} is not set"),
        }
    }
    found
}

#[derive(Default)]
struct MemoryVault {
    values: Mutex<HashMap<Identity, Secret>>,
}

impl Vault for MemoryVault {
    fn read(&self, identity: &Identity) -> Result<Option<Secret>, VaultError> {
        Ok(self.values.lock().unwrap().get(identity).cloned())
    }

    fn write(&self, identity: &Identity, password: &Secret) -> Result<(), VaultError> {
        self.values
            .lock()
            .unwrap()
            .insert(identity.clone(), password.clone());
        Ok(())
    }

    fn delete(&self, identity: &Identity) -> Result<bool, VaultError> {
        Ok(self.values.lock().unwrap().remove(identity).is_some())
    }
}

struct Fixture {
    app: App,
    base: PathBuf,
    vault: Arc<MemoryVault>,
    identity: Identity,
    password: String,
}

impl Fixture {
    fn new(uri: &str, credential: CredentialName, stored: Option<&str>) -> Self {
        let mut target: NetworkTarget = parse_uri(uri).unwrap();
        let password = target.password.take().unwrap().expose().to_owned();
        let definition = Definition::new("real", Source::Network(target));
        let base = temp_dir(&format!("refused-{}", uuid::Uuid::new_v4()));
        let store = Store {
            global: Root::global(base.join("global")),
            project: None,
        };
        fs::create_dir_all(&store.global.dir).unwrap();
        let mut entry = config::to_entry(ID, &definition);
        entry.credential = credential;
        fs::write(
            store.global.config_file(),
            serde_json::to_vec(&serde_json::json!({"version":1,"connections":[entry]})).unwrap(),
        )
        .unwrap();
        let key = ConnectionKey {
            scope: Scope::Global,
            id: ID.into(),
        };
        let identity = Identity::of(&key, &definition).unwrap();
        Trust::new(&store).confirm(&identity, None).unwrap();
        let vault = Arc::new(MemoryVault::default());
        if let Some(stored) = stored {
            vault.write(&identity, &Secret::new(stored)).unwrap();
        }
        let options = Options {
            store: Some(store),
            vault: Some(vault.clone()),
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(options, clipboard, Theme::plain());
        Self {
            app,
            base,
            vault,
            identity,
            password,
        }
    }

    fn code(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    fn run(&mut self) {
        self.code(KeyCode::F(5));
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
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

    fn sees(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            let screen = self.screen();
            if screen.contains(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{text:?} is not on the screen:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn enter(&mut self, password: &str, save: bool) {
        self.app.on_event(Event::Paste(password.to_owned()));
        if save {
            self.code(KeyCode::Tab);
            self.code(KeyCode::Char(' '));
            self.code(KeyCode::Tab);
        }
        self.code(KeyCode::Enter);
    }

    fn stored(&self) -> Option<String> {
        self.vault
            .values
            .lock()
            .unwrap()
            .get(&self.identity)
            .map(|value| value.expose().to_owned())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.shutdown();
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn only_a_wrong_password_counts_as_a_refusal() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for uri in servers() {
        let mut wrong = parse_uri(&uri).unwrap();
        wrong.password = Some(Secret::new(WRONG));
        let connector: Arc<dyn Connector> = network_connector(wrong);
        let error = runtime.block_on(connector.connect()).err().unwrap();
        assert!(error.password_refused, "{error:?}");

        let mut other_database = parse_uri(&uri).unwrap();
        other_database.database = Some("no_such_database".to_owned());
        let connector: Arc<dyn Connector> = network_connector(other_database);
        let error = runtime.block_on(connector.connect()).err().unwrap();
        assert!(!error.password_refused, "{error:?}");
    }
}

#[test]
fn a_refusal_asks_once_and_a_second_refusal_suspends_the_connection() {
    for uri in servers() {
        let mut f = Fixture::new(&uri, CredentialName::Session, None);
        f.app.on_event(Event::Paste("SELECT 41 + 1;".to_owned()));
        f.run();
        f.sees("Password · real");
        f.enter(WRONG, false);
        f.sees("refused the last password");

        f.enter(OTHER_WRONG, false);
        f.sees("Password refused");
        assert!(f.screen().contains("stops new attempts"));
        f.code(KeyCode::Enter);

        // The next action asks again, and the accepted password runs that action.
        f.run();
        f.sees("refused the last password");
        let password = f.password.clone();
        f.enter(&password, false);
        f.sees("42");
    }
}

#[test]
fn a_refused_vault_password_stays_until_a_new_password_passes_the_check() {
    for uri in servers() {
        let mut f = Fixture::new(&uri, CredentialName::Vault, Some(WRONG));
        f.app.on_event(Event::Paste("SELECT 41 + 1;".to_owned()));
        f.run();
        f.sees("refused the last password");
        assert_eq!(f.stored().as_deref(), Some(WRONG));

        // A refused password never reaches the vault, even with the save option marked.
        f.enter(OTHER_WRONG, true);
        f.sees("Password refused");
        assert_eq!(f.stored().as_deref(), Some(WRONG));
        f.code(KeyCode::Enter);

        f.run();
        f.sees("refused the last password");
        let password = f.password.clone();
        f.enter(&password, true);
        f.sees("42");
        assert_eq!(f.stored().as_deref(), Some(password.as_str()));
    }
}

#[test]
fn a_refusal_stops_the_catalog_until_an_action_of_the_user() {
    for uri in servers() {
        let mut f = Fixture::new(&uri, CredentialName::Session, None);
        f.app.on_event(Event::Paste("SELECT 1;".to_owned()));
        f.run();
        f.sees("Password · real");
        f.enter(WRONG, false);
        f.sees("refused the last password");
        f.code(KeyCode::Esc);
        // A key in the editor starts a catalog read, and that read needs no password.
        f.code(KeyCode::Char('x'));
        for _ in 0..50 {
            f.app.drain_events();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!f.screen().contains("refused the last password"));
        assert_eq!(f.app.connections()[0].catalog_state, CatalogState::Failed);
        // The navigator refresh is an action of the user, so it asks for a password.
        f.app.on_event(Event::Key(common::ctrl('w')));
        f.code(KeyCode::Char('h'));
        f.code(KeyCode::Char('r'));
        f.sees("refused the last password");
    }
}
