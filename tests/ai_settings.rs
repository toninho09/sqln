use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use sqln::store::{Root, Store};
use std::{
    any::Any,
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

// ---------------------------------------------------------------------------
// The autocomplete section of the settings: the connection test and the vault key.

/// A credential store that refuses every entry while `fail` is true.
#[derive(Debug)]
struct FlakyStore {
    fail: AtomicBool,
    inner: Arc<keyring_core::mock::Store>,
}

impl keyring_core::api::CredentialStoreApi for FlakyStore {
    fn vendor(&self) -> String {
        "flaky".into()
    }
    fn id(&self) -> String {
        "flaky".into()
    }
    fn build(
        &self,
        service: &str,
        user: &str,
        modifiers: Option<&HashMap<&str, &str>>,
    ) -> keyring_core::Result<keyring_core::Entry> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(keyring_core::Error::NoStorageAccess(
                "the vault is locked".into(),
            ));
        }
        self.inner.build(service, user, modifiers)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct SettingsSession {
    app: App,
    store: Store,
}

impl SettingsSession {
    /// Starts with a global configuration whose autocomplete block is `ai`.
    fn new(name: &str, ai: Value, vault: Arc<FlakyStore>) -> Self {
        let base = crate::common::temp_dir!(name);
        let store = Store {
            global: Root::global(base.join("global")),
            project: None,
        };
        std::fs::create_dir_all(&store.global.dir).unwrap();
        let config = json!({"version": 1, "preferences": {"aiCompletion": ai}, "connections": []});
        std::fs::write(store.global.config_file(), config.to_string()).unwrap();
        let app = App::new(
            Options {
                store: Some(store.clone()),
                ai_vault: Some(Arc::new(sqln::creds::SystemVault::with_store(vault))),
                ..Options::default()
            },
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
        Self { app, store }
    }

    fn rows(&mut self) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(110, 34)).unwrap();
        terminal.draw(|f| self.app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    fn screen(&mut self) -> String {
        self.rows().join("\n")
    }

    fn click(&mut self, label: &str) {
        let rows = self.rows();
        let (y, row) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| row.contains(label))
            .unwrap_or_else(|| panic!("{label:?} is not on the screen:\n{}", rows.join("\n")));
        let column = row[..row.find(label).unwrap()].width() as u16;
        self.app.on_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: y as u16,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Opens the settings on the autocomplete section.
    fn open_autocomplete(&mut self) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.app.on_event(Event::Key(key(KeyCode::Char(','))));
        for _ in 0..5 {
            self.app.on_event(Event::Key(key(KeyCode::Down)));
        }
        let screen = self.screen();
        assert!(screen.contains(text::FIELD_AI_SOURCE), "{screen}");
    }

    fn wait_for(&mut self, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            self.app.drain_events();
            let screen = self.screen();
            if screen.contains(text) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "{text:?} did not appear:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Types a vault key: the source becomes the system vault, and the key field gets the text.
    fn type_vault_key(&mut self, value: &str) {
        self.click(text::FIELD_AI_SOURCE);
        let screen = self.screen();
        assert!(screen.contains("System vault"), "{screen}");
        self.click(text::FIELD_AI_KEY);
        self.app.on_event(Event::Paste(value.into()));
    }

    fn saved_credential(&self) -> Value {
        let config: Value =
            serde_json::from_slice(&std::fs::read(self.store.global.config_file()).unwrap())
                .unwrap();
        config["preferences"]["aiCompletion"]["provider"]["credential"].clone()
    }
}

fn flaky(fail: bool) -> Arc<FlakyStore> {
    Arc::new(FlakyStore {
        fail: AtomicBool::new(fail),
        inner: keyring_core::mock::Store::new().unwrap(),
    })
}

fn provider_block(url: &str) -> Value {
    json!({"enabled": true, "provider": {
        "kind": "compatible", "model": "test-model", "baseUrl": url,
        "credential": {"source": "none"}
    }})
}

#[test]
fn the_connection_test_reports_an_endpoint_that_answers() {
    let server = Server::new("SELECT 1", Duration::ZERO);
    let mut s = SettingsSession::new(
        "ai-settings-test-ok",
        provider_block(&server.url),
        flaky(false),
    );
    s.open_autocomplete();
    s.click(text::BUTTON_TEST);
    s.wait_for("The endpoint accepts the connection.");
    // The test sends a request and writes no file.
    assert!(!server.requests.lock().unwrap().is_empty());
    assert_eq!(s.saved_credential()["source"], "none");
}

#[test]
fn a_failed_connection_test_leaves_a_note_in_the_dialog() {
    let server = Server::response(500, "{}".into(), Duration::ZERO);
    let mut s = SettingsSession::new(
        "ai-settings-test-fail",
        provider_block(&server.url),
        flaky(false),
    );
    s.open_autocomplete();
    s.click(text::BUTTON_TEST);
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        s.app.drain_events();
        let screen = s.screen();
        if screen.contains(text::BUTTON_TEST) && !screen.contains("Working…") {
            assert!(!screen.contains("The endpoint accepts"), "{screen}");
            assert!(screen.contains(text::FIELD_AI_SOURCE), "{screen}");
            break;
        }
        assert!(Instant::now() < deadline, "{screen}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_vault_key_is_written_before_the_settings_save() {
    let server = Server::new("SELECT 1", Duration::ZERO);
    let vault = flaky(false);
    let mut s = SettingsSession::new("ai-settings-vault", provider_block(&server.url), vault);
    s.open_autocomplete();
    s.type_vault_key("sk-test");
    s.click(text::BUTTON_SAVE);
    s.wait_for(text::SETTINGS_SAVED);
    let credential = s.saved_credential();
    assert_eq!(credential["source"], "vault", "{credential}");
    // The key itself never reaches the file.
    let file = std::fs::read_to_string(s.store.global.config_file()).unwrap();
    assert!(!file.contains("sk-test"), "{file}");
}

#[test]
fn a_vault_failure_offers_a_retry_that_saves_once_the_vault_works() {
    let server = Server::new("SELECT 1", Duration::ZERO);
    let vault = flaky(true);
    let mut s = SettingsSession::new(
        "ai-settings-retry",
        provider_block(&server.url),
        vault.clone(),
    );
    s.open_autocomplete();
    s.type_vault_key("sk-test");
    s.click(text::BUTTON_SAVE);
    let screen = s.wait_for("Cannot save the API key");
    assert!(screen.contains("The draft is not saved."), "{screen}");
    assert_eq!(s.saved_credential()["source"], "none");
    vault.fail.store(false, Ordering::SeqCst);
    s.click("[Retry]");
    s.wait_for(text::SETTINGS_SAVED);
    assert_eq!(s.saved_credential()["source"], "vault");
}

#[test]
fn a_vault_failure_can_keep_the_key_for_this_session_only() {
    let server = Server::new("SELECT 1", Duration::ZERO);
    let mut s = SettingsSession::new(
        "ai-settings-session",
        provider_block(&server.url),
        flaky(true),
    );
    s.open_autocomplete();
    s.type_vault_key("sk-test");
    s.click(text::BUTTON_SAVE);
    s.wait_for("Cannot save the API key");
    s.click("[Use in this session]");
    s.wait_for("The API key stays in this session only.");
    let file = std::fs::read_to_string(s.store.global.config_file()).unwrap();
    assert!(!file.contains("sk-test"), "{file}");
}

#[test]
fn closing_the_settings_during_a_vault_write_saves_nothing() {
    let server = Server::new("SELECT 1", Duration::ZERO);
    let mut s = SettingsSession::new(
        "ai-settings-close",
        provider_block(&server.url),
        flaky(true),
    );
    s.open_autocomplete();
    s.type_vault_key("sk-test");
    s.click(text::BUTTON_SAVE);
    s.app.on_event(Event::Key(key(KeyCode::Esc)));
    // A late answer of the vault finds no dialog and no pending save.
    for _ in 0..50 {
        s.app.drain_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    let screen = s.screen();
    assert!(!screen.contains("Cannot save the API key"), "{screen}");
    assert!(!screen.contains(text::FIELD_AI_SOURCE), "{screen}");
    assert_eq!(s.saved_credential()["source"], "none");
}

#[test]
fn a_vault_source_without_a_key_names_the_missing_key() {
    let server = Server::new("SELECT 1", Duration::ZERO);
    let mut s = SettingsSession::new(
        "ai-settings-no-key",
        provider_block(&server.url),
        flaky(false),
    );
    s.open_autocomplete();
    s.click(text::FIELD_AI_SOURCE);
    s.click(text::BUTTON_TEST);
    let screen = s.screen();
    assert!(
        screen.contains("Insert a key for the system vault."),
        "{screen}"
    );
    s.click(text::BUTTON_SAVE);
    let screen = s.screen();
    assert!(
        screen.contains("Insert a key for the system vault."),
        "{screen}"
    );
    assert_eq!(s.saved_credential()["source"], "none");
}
