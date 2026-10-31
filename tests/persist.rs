// The application with files: configuration, project, autosave, reopen, conflicts,
// and scripts without a connection.

mod common;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, temp_dir};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::script_file::Trouble;
use sqln::store::scripts::{self, ScriptMeta};
use sqln::store::{Root, Scope, Store, file};
use sqln::ui::theme::Theme;

struct Session {
    app: App,
    /// The simulated clock of the autosave.
    now: Instant,
}

fn sqlite_entry(id: &str, name: &str, file: &str) -> String {
    format!(r#"{{"id":"{id}","name":"{name}","kind":"sqlite","file":"{file}"}}"#)
}

fn write_config(root: &Root, preferences: &str, connections: &[String]) {
    fs::create_dir_all(&root.dir).unwrap();
    let text = format!(
        r#"{{"version":1,"preferences":{preferences},"connections":[{}]}}"#,
        connections.join(",")
    );
    fs::write(root.config_file(), text).unwrap();
}

/// A store with a global folder and a project folder.
fn store(name: &str) -> (Store, PathBuf) {
    let base = temp_dir(name);
    let project = Root::project(&base.join("project"));
    fs::create_dir_all(&project.dir).unwrap();
    (Store { global: Root::global(base.join("global")), project: Some(project) }, base)
}

/// A store without a project.
fn global_only(name: &str) -> (Store, PathBuf) {
    let base = temp_dir(name);
    (Store { global: Root::global(base.join("global")), project: None }, base)
}

fn database(base: &Path, name: &str, setup: &str) -> PathBuf {
    let path = base.join(format!("{name}.db"));
    rusqlite::Connection::open(&path).unwrap().execute_batch(setup).unwrap();
    path
}

impl Session {
    fn start(store: &Store) -> Self {
        Self::with(Options { store: Some(store.clone()), ..Options::default() })
    }

    fn with(options: Options) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(options, clipboard, Theme::plain());
        Self { app, now: Instant::now() }
    }

    fn press(&mut self, event: crossterm::event::KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn prefix(&mut self, c: char) {
        self.press(ctrl('w'));
        self.code(KeyCode::Char(c));
    }

    fn write(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    /// Moves the autosave clock and runs one step of the main loop.
    fn wait(&mut self, millis: u64) {
        self.now += Duration::from_millis(millis);
        self.app.tick_at(self.now);
    }

    /// Writes text and lets the autosave save it.
    fn type_and_save(&mut self, text: &str) {
        self.write(text);
        self.wait(0);
        self.wait(500);
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect();
                row.trim_end().to_owned() + "\n"
            })
            .collect()
    }

    fn assert_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(screen.contains(text), "{text:?} is not on the screen:\n{screen}");
    }

    fn assert_not_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(!screen.contains(text), "{text:?} is on the screen:\n{screen}");
    }

    fn names(&self) -> Vec<String> {
        self.app.consoles().iter().map(|console| console.name.clone()).collect()
    }

    fn text(&self) -> String {
        self.app.console().editor.text()
    }

    /// Selects a row of the navigator by the name of its connection.
    fn go_to_connection(&mut self, name: &str) {
        self.press(ctrl('w'));
        self.code(KeyCode::Char('h'));
        for _ in 0..20 {
            self.code(KeyCode::Up);
        }
        for _ in 0..20 {
            let rows = self.app.navigator().rows(self.app.connections());
            let current = self.app.navigator().current(self.app.connections());
            let on = current.is_some_and(|row| rows.contains(&row) && row.name(self.app.connections()) == name);
            if on {
                return;
            }
            self.code(KeyCode::Down);
        }
        panic!("no connection named {name}");
    }

    fn quit(&mut self) {
        self.prefix('q');
        self.app.shutdown();
    }
}

fn consoles_in(root: &Root, scope: Scope) -> Vec<scripts::ScriptEntry> {
    scripts::list(root, scope)
}

fn text_of(root: &Root, id: &str) -> String {
    scripts::read_text(root, id).unwrap().0
}

// ---- start and configuration ----

#[test]
fn the_connections_of_both_scopes_show_and_the_first_console_uses_the_first() {
    let (store, base) = store("p-scopes");
    let db = database(&base, "a", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "app", &db.display().to_string())]);
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[
            sqlite_entry("p1", "app", &db.display().to_string()),
            sqlite_entry("p2", "other", &db.display().to_string()),
        ],
    );
    let mut session = Session::start(&store);
    let names: Vec<_> = session.app.connections().iter().map(|c| c.name.clone()).collect();
    // A name that two scopes use shows its scope. The other name stays plain.
    assert_eq!(names, ["app (global)", "app (local)", "other"]);
    assert!(session.app.console().connection.is_some());
    session.assert_sees("app (global)");
    session.assert_sees("app (local)");
}

#[test]
fn a_connection_with_a_relative_path_uses_the_folder_of_its_scope() {
    let (store, base) = store("p-relative");
    let project_root = base.join("project");
    fs::create_dir_all(project_root.join("db")).unwrap();
    database(&project_root, "db/app", "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('from-project');");
    write_config(store.project.as_ref().unwrap(), "{}", &[sqlite_entry("p1", "app", "db/app.db")]);
    let mut session = Session::start(&store);
    session.write("SELECT v FROM t;");
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.screen().contains("from-project") {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the query did not run:\n{}", session.screen());
}

#[test]
fn the_preferences_of_the_project_replace_only_the_values_that_it_declares() {
    let (store, _) = store("p-preferences");
    write_config(&store.global, r#"{"keyboardMode":"conventional","rowCap":3}"#, &[]);
    write_config(store.project.as_ref().unwrap(), r#"{"keyboardMode":"vim"}"#, &[]);
    let session = Session::start(&store);
    assert_eq!(session.app.console().editor.keyboard(), sqln::editor::KeyboardMode::Vim);
}

#[test]
fn a_row_cap_of_the_file_limits_the_result() {
    let (store, base) = global_only("p-rowcap");
    let db = database(&base, "r", "CREATE TABLE t (v INTEGER); INSERT INTO t VALUES (1),(2),(3),(4),(5);");
    write_config(&store.global, r#"{"rowCap":2}"#, &[sqlite_entry("g1", "r", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.write("SELECT v FROM t;");
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.screen().contains("row cap") || session.screen().contains("2 rows") {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the cap did not apply:\n{}", session.screen());
}

#[test]
fn a_keymap_of_the_file_changes_a_key() {
    let (store, base) = global_only("p-keymap");
    let db = database(&base, "k", "");
    write_config(
        &store.global,
        r#"{"keymap":{"conventional":{"run":["f9"]}}}"#,
        &[sqlite_entry("g1", "k", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.write("SELECT 1;");
    session.code(KeyCode::F(9));
    for _ in 0..2_000 {
        session.app.drain_events();
        if !session.app.console().results.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("F9 did not run:\n{}", session.screen());
}

#[test]
fn an_invalid_file_blocks_the_databases_until_it_is_fixed_and_reloaded() {
    let (store, base) = global_only("p-invalid");
    let db = database(&base, "i", "");
    fs::create_dir_all(&store.global.dir).unwrap();
    fs::write(store.global.config_file(), "{\n  \"version\": 1,\n  \"connections\": [,]\n}").unwrap();
    let mut session = Session::start(&store);
    session.assert_sees("Invalid configuration");
    session.assert_sees("line 3, column 19");
    session.assert_sees("File: ");
    assert!(session.app.connections().is_empty());
    // The dialog closes. The hint tells how to recover.
    session.code(KeyCode::Esc);
    session.assert_sees("Ctrl+W r");
    // A connection cannot start while the error stays.
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('a'));
    assert!(session.app.form().is_none());
    session.assert_sees("configuration has an error");
    // Reload with the same error asks again.
    session.prefix('r');
    session.assert_sees("Invalid configuration");
    session.code(KeyCode::Esc);
    // The user fixes the file outside the application, then reloads.
    write_config(&store.global, "{}", &[sqlite_entry("g1", "fixed", &db.display().to_string())]);
    session.prefix('r');
    session.assert_sees("loaded again");
    let names: Vec<_> = session.app.connections().iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["fixed"]);
    session.assert_not_sees("Ctrl+W r loads");
}

#[test]
fn after_a_failed_reload_the_open_connections_cannot_run_until_the_file_is_fixed() {
    let (store, base) = global_only("p-blocked-run");
    let db = database(&base, "b", "");
    let entry = sqlite_entry("g1", "b", &db.display().to_string());
    write_config(&store.global, "{}", std::slice::from_ref(&entry));
    let mut session = Session::start(&store);
    session.write("SELECT 7 * 6;");
    fs::write(store.global.config_file(), "{").unwrap();
    session.prefix('r');
    session.code(KeyCode::Esc);
    // The connection stays in the list, but nothing runs.
    assert_eq!(session.app.connections().len(), 1);
    session.press(common::shift(KeyCode::F(5)));
    session.assert_sees("configuration has an error");
    for _ in 0..40 {
        session.app.drain_events();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.app.console().results.is_empty());
    assert!(!session.app.console().is_busy());
    // The table view and the catalog read stay closed too.
    session.go_to_connection("b");
    session.code(KeyCode::Char('r'));
    session.assert_sees("configuration has an error");
    // The fix opens the connection again.
    write_config(&store.global, "{}", &[entry]);
    session.prefix('r');
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.screen().contains("42") {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the run did not work after the fix:\n{}", session.screen());
}

#[test]
fn the_error_of_a_keymap_conflict_names_the_file() {
    let (store, _) = global_only("p-keymap-error");
    write_config(&store.global, r#"{"keymap":{"conventional":{"run":["f6"],"stop":["f6"]}}}"#, &[]);
    let mut session = Session::start(&store);
    session.assert_sees("Invalid configuration");
    session.assert_sees("f6");
}

#[test]
fn a_change_of_the_file_by_another_program_asks_before_it_reloads() {
    let (store, base) = global_only("p-external-config");
    let db = database(&base, "e", "");
    let path = db.display().to_string();
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path)]);
    let mut session = Session::start(&store);
    session.wait(1_000);
    assert!(session.app.connections().len() == 1);
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path), sqlite_entry("g2", "two", &path)]);
    session.wait(1_000);
    session.assert_sees("Configuration changed");
    // The default answer changes nothing.
    session.code(KeyCode::Enter);
    assert_eq!(session.app.connections().len(), 1);
    // The same content does not ask again.
    session.wait(1_000);
    session.assert_not_sees("Configuration changed");
    // A new change asks again, and Reload applies it.
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path), sqlite_entry("g3", "three", &path)]);
    session.wait(1_000);
    session.assert_sees("Configuration changed");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    let names: Vec<_> = session.app.connections().iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["one", "three"]);
}

#[test]
fn a_reload_does_not_change_a_connection_with_an_open_transaction() {
    let (store, base) = global_only("p-reload-active");
    let db = database(&base, "a", "CREATE TABLE t (v INTEGER);");
    let path = db.display().to_string();
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path)]);
    let mut session = Session::start(&store);
    // A manual transaction keeps the session open.
    session.prefix('m');
    session.write("INSERT INTO t VALUES (1);");
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.app.console().in_transaction() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.app.console().in_transaction());
    // The file now has no connection, and the reload leaves the active one.
    write_config(&store.global, "{}", &[]);
    session.prefix('r');
    assert_eq!(session.app.connections().len(), 1, "an active connection stays");
    assert!(session.app.console().in_transaction());
}

#[test]
fn a_connection_that_leaves_the_file_leaves_the_application_when_it_is_idle() {
    let (store, base) = global_only("p-reload-remove");
    let db = database(&base, "a", "");
    let path = db.display().to_string();
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path), sqlite_entry("g2", "two", &path)]);
    let mut session = Session::start(&store);
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path)]);
    session.prefix('r');
    let names: Vec<_> = session.app.connections().iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["one"]);
}

#[test]
fn a_project_is_offered_and_created_on_request() {
    let (store, base) = global_only("p-offer");
    let folder = base.join("work");
    fs::create_dir_all(&folder).unwrap();
    let options = Options {
        store: Some(store.clone()),
        offer_project: Some(folder.clone()),
        ..Options::default()
    };
    let mut session = Session::with(options);
    session.assert_sees("Create project");
    // The first option never creates a folder.
    assert!(!folder.join(".sqln").exists());
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(folder.join(".sqln").is_dir());
    assert_eq!(fs::read_to_string(folder.join(".sqln/.gitignore")).unwrap(), "workspace.json\nlocks/\n*.tmp\n");
    assert!(!folder.join(".git").exists());
}

#[test]
fn declining_the_project_keeps_the_global_scope_only() {
    let (store, base) = global_only("p-decline");
    let folder = base.join("work");
    fs::create_dir_all(&folder).unwrap();
    let options = Options { store: Some(store), offer_project: Some(folder.clone()), ..Options::default() };
    let mut session = Session::with(options);
    session.code(KeyCode::Enter);
    assert!(!folder.join(".sqln").exists());
    // The form offers no project scope.
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('a'));
    session.assert_not_sees("Scope");
}

// ---- the connection form and the files ----

#[test]
fn a_new_connection_goes_to_the_project_file_by_default_without_a_password() {
    let (store, base) = store("p-form-save");
    let db = database(&base, "f", "");
    let mut session = Session::start(&store);
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('a'));
    session.assert_sees("Scope");
    session.assert_sees("Project");
    session.code(KeyCode::Right);
    session.code(KeyCode::Right);
    session.code(KeyCode::Tab);
    session.write("saved");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.write(&db.display().to_string());
    // File, foreign keys, lock limit, pool, read only, SQL limit, Test, then Save.
    for _ in 0..7 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let text = fs::read_to_string(store.project.as_ref().unwrap().config_file()).unwrap();
    assert!(text.contains("\"name\": \"saved\""), "{text}");
    assert!(text.contains("\"kind\": \"sqlite\""), "{text}");
    assert!(!store.global.config_file().exists());
    assert!(!text.contains("password"));
}

#[test]
fn a_form_for_a_network_connection_saves_no_password_in_the_file() {
    let (store, _) = global_only("p-form-password");
    let mut session = Session::start(&store);
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('a'));
    // The fields without a project: Kind, Name, Entry, Host, Port, Database, User, Password.
    session.code(KeyCode::Tab);
    session.write("remote");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.write("db.example.test");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.write("me");
    session.code(KeyCode::Tab);
    session.write("hunter2");
    // Then: credential, TLS, CA, certificate, key, pool, read only, connect, SQL limit, Test, Save.
    for _ in 0..11 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"host\": \"db.example.test\""), "{text}");
    assert!(text.contains("\"username\": \"me\""), "{text}");
    assert!(text.contains("\"credential\": \"session\""), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
    // The password stays in memory for the session.
    let definition = session.app.connections()[0].definition.as_ref().unwrap();
    assert!(format!("{definition:?}").contains("Secret(***)"));
}

#[test]
fn two_scopes_can_use_the_same_name_and_one_scope_cannot() {
    let (store, base) = store("p-form-names");
    let db = database(&base, "n", "");
    let path = db.display().to_string();
    write_config(store.project.as_ref().unwrap(), "{}", &[sqlite_entry("p1", "same", &path)]);
    let mut session = Session::start(&store);
    let save_sqlite = |session: &mut Session, scope_to_global: bool| {
        session.press(ctrl('w'));
        session.code(KeyCode::Char('h'));
        session.code(KeyCode::Char('a'));
        session.code(KeyCode::Right);
        session.code(KeyCode::Right);
        session.code(KeyCode::Tab);
        session.write("SAME");
        session.code(KeyCode::Tab);
        if scope_to_global {
            session.code(KeyCode::Right);
        }
        session.code(KeyCode::Tab);
        session.write(&path);
        for _ in 0..7 {
            session.code(KeyCode::Tab);
        }
        session.code(KeyCode::Enter);
    };
    // The name is the same in the project, ignoring the case. The scope refuses it.
    save_sqlite(&mut session, false);
    session.assert_sees("Another connection has this name.");
    session.code(KeyCode::Esc);
    // The global scope accepts it, and both names show their scope.
    save_sqlite(&mut session, true);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let names: Vec<_> = session.app.connections().iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["same (local)", "SAME (global)"]);
}

#[test]
fn removing_a_connection_removes_its_entry_and_keeps_its_scripts() {
    let (store, base) = global_only("p-remove");
    let db = database(&base, "d", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "gone", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
    // Close the console, then remove the connection.
    session.prefix('c');
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Delete);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(session.app.connections().is_empty());
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(!text.contains("gone"), "{text}");
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1, "the script stays");
}

fn erase_field(session: &mut Session, characters: usize) {
    for _ in 0..characters {
        session.code(KeyCode::Backspace);
    }
}

#[test]
fn editing_a_connection_replaces_its_entry_and_keeps_its_identity_and_scripts() {
    let (store, base) = global_only("p-edit");
    let db = database(&base, "e", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "before", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.go_to_connection("before");
    session.code(KeyCode::Char('e'));
    erase_field(&mut session, "before".len());
    session.write("after");
    // File, foreign keys, lock limit, pool, read only, SQL limit, Test, then Save.
    for _ in 0..8 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"id\": \"g1\""), "{text}");
    assert!(text.contains("\"name\": \"after\""), "{text}");
    assert!(!text.contains("before"), "{text}");
    assert_eq!(session.app.connections()[0].name, "after");
    // The script keeps pointing to the same identity.
    assert_eq!(consoles_in(&store.global, Scope::Global)[0].meta.connection_id, "g1");
}

#[test]
fn the_sql_limit_of_a_connection_saves_and_zero_means_no_limit() {
    let (store, base) = global_only("p-sql-limit");
    let db = database(&base, "e", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.go_to_connection("one");
    session.code(KeyCode::Char('e'));
    for _ in 0..6 {
        session.code(KeyCode::Tab);
    }
    session.assert_sees("SQL limit");
    erase_field(&mut session, 2);
    session.write("0");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"sqlTimeoutSeconds\": null"), "{text}");
    session.prefix('r');
    let definition = session.app.connections()[0].definition.as_ref().unwrap();
    assert_eq!(definition.sql_timeout, None);
}

#[test]
fn a_bad_sql_limit_keeps_the_form_open() {
    let (store, base) = global_only("p-sql-limit-bad");
    let db = database(&base, "e", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.go_to_connection("one");
    session.code(KeyCode::Char('e'));
    for _ in 0..6 {
        session.code(KeyCode::Tab);
    }
    erase_field(&mut session, 2);
    session.write("abc");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(session.app.form().is_some());
    session.assert_sees("SQL limit");
    // The file did not change.
    assert!(!fs::read_to_string(store.global.config_file()).unwrap().contains("sqlTimeoutSeconds"));
}

#[test]
fn a_connected_connection_can_change_its_name_only_and_the_file_follows() {
    let (store, base) = global_only("p-rename");
    let db = database(&base, "e", "CREATE TABLE t (v INTEGER);");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "old", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.prefix('m');
    session.write("INSERT INTO t VALUES (1);");
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.app.console().in_transaction() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    session.go_to_connection("old");
    session.code(KeyCode::Char('e'));
    session.assert_sees("Save name");
    erase_field(&mut session, "old".len());
    session.write("new");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(session.app.form().is_none());
    assert_eq!(session.app.connections()[0].name, "new");
    assert!(session.app.console().in_transaction());
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"name\": \"new\"") && !text.contains("\"old\""), "{text}");
}

#[test]
fn a_failed_write_of_the_configuration_keeps_the_form_open_with_the_error() {
    let (store, base) = global_only("p-save-error");
    let db = database(&base, "x", "");
    let mut session = Session::start(&store);
    // A file where the folder must be makes the write of the configuration fail.
    fs::create_dir_all(store.global.dir.parent().unwrap()).unwrap();
    fs::write(&store.global.dir, "not a folder").unwrap();
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('a'));
    session.code(KeyCode::Right);
    session.code(KeyCode::Right);
    session.code(KeyCode::Tab);
    session.write("x");
    session.code(KeyCode::Tab);
    session.write(&db.display().to_string());
    for _ in 0..7 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    assert!(session.app.connections().is_empty());
    assert!(session.app.form().is_some(), "{}", session.screen());
}

// ---- autosave ----

#[test]
fn an_edit_creates_the_files_after_500_ms_and_an_empty_console_creates_none() {
    let (store, base) = global_only("p-autosave");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.wait(0);
    session.wait(2_000);
    assert!(consoles_in(&store.global, Scope::Global).is_empty(), "no edit, no file");
    session.write("SELECT 1;");
    session.wait(0);
    session.wait(499);
    assert!(consoles_in(&store.global, Scope::Global).is_empty(), "before 500 ms");
    session.wait(1);
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].meta.name, "console-1");
    assert_eq!(saved[0].meta.connection_id, "g1");
    assert_eq!(saved[0].meta.connection_scope, Scope::Global);
    assert_eq!(text_of(&store.global, &saved[0].meta.id), "SELECT 1;");
}

#[test]
fn each_edit_restarts_the_wait() {
    let (store, base) = global_only("p-restart");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.write("SELECT");
    session.wait(0);
    session.wait(400);
    session.write(" 1;");
    session.wait(0);
    session.wait(400);
    // 800 ms after the first edit, but 400 ms after the last one.
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
    session.wait(100);
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
}

#[test]
fn a_console_of_a_project_connection_saves_in_the_project() {
    let (store, base) = store("p-project-console");
    let db = database(&base, "s", "");
    write_config(store.project.as_ref().unwrap(), "{}", &[sqlite_entry("p1", "proj", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    assert_eq!(consoles_in(store.project.as_ref().unwrap(), Scope::Project).len(), 1);
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
}

#[test]
fn ctrl_s_writes_at_once() {
    let (store, base) = global_only("p-ctrl-s");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.write("SELECT 1;");
    session.press(ctrl('s'));
    session.assert_sees("Saved.");
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
}

#[test]
fn ctrl_s_in_an_empty_console_has_nothing_to_save() {
    let (store, base) = global_only("p-ctrl-s-empty");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.press(ctrl('s'));
    session.assert_sees("no text to save");
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
}

#[test]
fn closing_a_console_writes_the_pending_text() {
    let (store, base) = global_only("p-close-flush");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.write("SELECT 1;");
    session.prefix('c');
    // The close of the only console quits. The text is on disk before that.
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(saved.len(), 1);
    assert_eq!(text_of(&store.global, &saved[0].meta.id), "SELECT 1;");
}

#[test]
fn quitting_writes_the_pending_text_and_the_cursor() {
    let (store, base) = global_only("p-quit");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.write("SELECT 1;\nSELECT 2;");
    session.quit();
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(text_of(&store.global, &saved[0].meta.id), "SELECT 1;\nSELECT 2;");
    let state = sqln::store::workspace::load(&store.global);
    let cursor = state.consoles[&saved[0].meta.id].cursor;
    assert_eq!((cursor.row, cursor.col), (1, 9));
}

#[test]
fn a_new_console_takes_the_lowest_free_name_after_a_restart() {
    let (store, base) = global_only("p-names");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.quit();
    drop(first);
    let mut second = Session::start(&store);
    assert_eq!(second.names(), ["console-2"], "console-1 is saved");
    second.type_and_save("SELECT 2;");
    let mut names: Vec<_> =
        consoles_in(&store.global, Scope::Global).into_iter().map(|entry| entry.meta.name).collect();
    names.sort();
    assert_eq!(names, ["console-1", "console-2"]);
}

#[test]
fn a_failed_write_keeps_the_text_shows_an_alert_and_blocks_the_close_until_confirmed() {
    let (store, base) = global_only("p-failure");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    // A file in the place of the folder makes the write fail.
    fs::write(store.global.dir.join("consoles"), "x").unwrap();
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.assert_sees("Cannot save");
    session.assert_sees("console-1 !");
    assert_eq!(session.text(), "SELECT 1;");
    // The close asks. Cancel keeps the console.
    session.prefix('c');
    session.assert_sees("Text not saved");
    session.code(KeyCode::Enter);
    assert_eq!(session.names(), ["console-1"]);
    // The problem goes away, and Ctrl+S saves.
    fs::remove_file(store.global.dir.join("consoles")).unwrap();
    session.press(ctrl('s'));
    session.assert_sees("Saved.");
    session.assert_not_sees("console-1 !");
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
}

#[test]
fn closing_without_saving_after_a_failure_drops_only_the_unsaved_text() {
    let (store, base) = global_only("p-failure-discard");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    fs::write(store.global.dir.join("consoles"), "x").unwrap();
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.prefix('q');
    session.assert_sees("Text not saved");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(session.app.should_quit());
}

#[test]
fn a_failed_write_never_blocks_a_run() {
    let (store, base) = global_only("p-failure-run");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    fs::write(store.global.dir.join("consoles"), "x").unwrap();
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 41 + 1;");
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.screen().contains("42") {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the run did not work:\n{}", session.screen());
}

#[test]
fn an_unbound_console_with_text_asks_before_it_closes() {
    let (store, _) = global_only("p-unbound");
    let mut session = Session::start(&store);
    session.write("SELECT 1;");
    session.prefix('c');
    session.assert_sees("Text not saved");
    session.assert_sees("no connection");
    session.code(KeyCode::Enter);
    assert!(!session.app.should_quit());
}

#[test]
fn an_empty_unbound_console_closes_without_a_question() {
    let (store, _) = global_only("p-unbound-empty");
    let mut session = Session::start(&store);
    session.prefix('c');
    assert!(session.app.should_quit());
}

#[test]
fn a_crlf_file_keeps_its_line_ending_after_an_edit() {
    let (store, base) = global_only("p-crlf");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let root = &store.global;
    scripts::create(
        root,
        &ScriptMeta::new("s1".into(), "old".into(), "g1".into(), Scope::Global),
        "SELECT 1;\r\nSELECT 2;",
    )
    .unwrap();
    let mut session = Session::start(&store);
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.assert_sees("Consoles of s");
    session.code(KeyCode::Down);
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), "SELECT 1;\nSELECT 2;");
    session.code(KeyCode::Down);
    session.code(KeyCode::End);
    session.write("\nSELECT 3;");
    session.wait(0);
    session.wait(500);
    assert_eq!(text_of(root, "s1"), "SELECT 1;\r\nSELECT 2;\r\nSELECT 3;");
}

// ---- reopen ----

#[test]
fn the_list_offers_the_saved_consoles_of_the_connection_and_a_new_one() {
    let (store, base) = global_only("p-list");
    let db = database(&base, "s", "");
    write_config(
        &store.global,
        "{}",
        &[
            sqlite_entry("g1", "first", &db.display().to_string()),
            sqlite_entry("g2", "second", &db.display().to_string()),
        ],
    );
    let root = &store.global;
    scripts::create(root, &ScriptMeta::new("a".into(), "investigate".into(), "g1".into(), Scope::Global), "SELECT 1;").unwrap();
    scripts::create(root, &ScriptMeta::new("b".into(), "other-db".into(), "g2".into(), Scope::Global), "SELECT 2;").unwrap();
    let mut session = Session::start(&store);
    session.go_to_connection("first");
    session.code(KeyCode::Char('n'));
    session.assert_sees("Consoles of first");
    session.assert_sees("investigate");
    session.assert_sees("+ New console");
    // Only the consoles of the selected connection appear.
    session.assert_not_sees("other-db");
}

#[test]
fn choosing_a_saved_console_opens_it_without_running_it_and_restores_the_cursor() {
    let (store, base) = global_only("p-reopen");
    let db = database(&base, "s", "CREATE TABLE t (v INTEGER);");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut first = Session::start(&store);
    first.write("INSERT INTO t VALUES (1);\nSELECT 2;");
    first.code(KeyCode::Up);
    first.code(KeyCode::End);
    first.quit();
    drop(first);
    let mut second = Session::start(&store);
    second.go_to_connection("s");
    second.code(KeyCode::Char('n'));
    second.code(KeyCode::Down);
    second.code(KeyCode::Enter);
    assert_eq!(second.text(), "INSERT INTO t VALUES (1);\nSELECT 2;");
    let cursor = second.app.console().editor.cursor();
    assert_eq!((cursor.row, cursor.col), (0, 25));
    // Nothing ran.
    second.app.drain_events();
    let count: i64 = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row("SELECT count(*) FROM t", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn choosing_an_open_console_selects_its_tab() {
    let (store, base) = global_only("p-reopen-open");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.assert_sees("(open)");
    session.code(KeyCode::Down);
    session.code(KeyCode::Enter);
    assert_eq!(session.names(), ["console-1"], "no second tab");
}

#[test]
fn the_new_console_row_creates_another_console() {
    let (store, base) = global_only("p-new-row");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.code(KeyCode::Enter);
    assert_eq!(session.names(), ["console-1", "console-2"]);
}

#[test]
fn a_connection_without_saved_consoles_creates_a_console_without_the_list() {
    let (store, base) = global_only("p-no-list");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.write("x");
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.assert_not_sees("Consoles of");
    assert_eq!(session.names().len(), 2);
}

#[test]
fn a_second_instance_cannot_open_a_console_that_the_first_uses() {
    let (store, base) = global_only("p-lock");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    let mut second = Session::start(&store);
    second.go_to_connection("s");
    second.code(KeyCode::Char('n'));
    second.code(KeyCode::Down);
    second.code(KeyCode::Enter);
    second.assert_sees("open in another SQLN instance");
    assert_eq!(second.names().len(), 1);
    // The close of the first console frees the lock.
    first.press(ctrl('w'));
    first.code(KeyCode::Char('c'));
    first.app.shutdown();
    drop(first);
    second.code(KeyCode::Char('n'));
    second.code(KeyCode::Down);
    second.code(KeyCode::Enter);
    assert_eq!(second.text(), "SELECT 1;");
}

#[test]
fn a_console_reopens_with_the_connection_of_its_scope_when_two_names_collide() {
    let (store, base) = store("p-collide");
    let db = database(&base, "c", "");
    let path = db.display().to_string();
    write_config(&store.global, "{}", &[sqlite_entry("g1", "app", &path)]);
    write_config(store.project.as_ref().unwrap(), "{}", &[sqlite_entry("p1", "app", &path)]);
    scripts::create(
        store.project.as_ref().unwrap(),
        &ScriptMeta::new("pc".into(), "from-project".into(), "p1".into(), Scope::Project),
        "SELECT 'project';",
    )
    .unwrap();
    let mut session = Session::start(&store);
    session.go_to_connection("app (local)");
    session.code(KeyCode::Char('n'));
    session.assert_sees("Consoles of app (local) · Project");
    session.assert_sees("from-project");
    session.code(KeyCode::Down);
    session.code(KeyCode::Enter);
    let console = session.app.console();
    let connection = session.app.connections().iter().find(|c| Some(c.id) == console.connection).unwrap();
    assert_eq!(connection.name, "app (local)");
    // The global connection with the same name has no saved console.
    session.go_to_connection("app (global)");
    session.code(KeyCode::Char('n'));
    session.assert_not_sees("Consoles of");
}

#[test]
fn the_question_about_a_removal_names_the_file_and_the_scripts_that_stay() {
    let (store, base) = global_only("p-remove-text");
    let db = database(&base, "d", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "gone", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.go_to_connection("gone");
    session.code(KeyCode::Delete);
    session.assert_sees("Remove the definition of gone from");
    session.assert_sees("scripts of the connection stay");
    // Cancel removes nothing.
    session.code(KeyCode::Enter);
    assert_eq!(session.app.connections().len(), 1);
    assert!(fs::read_to_string(store.global.config_file()).unwrap().contains("gone"));
}

// ---- changes by other programs ----

fn script_path(root: &Root, id: &str) -> PathBuf {
    root.consoles_dir().join(format!("{id}.sql"))
}

fn session_with_saved_console(name: &str) -> (Session, Store, String) {
    let (store, base) = global_only(name);
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    let id = consoles_in(&store.global, Scope::Global)[0].meta.id.clone();
    (session, store, id)
}

#[test]
fn a_changed_script_asks_and_pauses_the_autosave_until_the_user_chooses() {
    let (mut session, store, id) = session_with_saved_console("p-conflict");
    fs::write(script_path(&store.global, &id), "SELECT 'theirs';").unwrap();
    session.wait(1_000);
    session.assert_sees("Scripts changed outside SQLN");
    session.assert_sees("changed");
    // The user types more. The file does not change while the question waits.
    session.code(KeyCode::Esc);
    session.write(" -- mine");
    session.wait(0);
    session.wait(2_000);
    assert_eq!(text_of(&store.global, &id), "SELECT 'theirs';");
    assert_eq!(session.text(), "SELECT 1; -- mine");
    // The question does not come back by itself for the same script.
    session.assert_not_sees("Scripts changed outside SQLN");
}

#[test]
fn reload_uses_the_file_and_runs_no_sql() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-reload");
    fs::write(script_path(&store.global, &id), "SELECT 'theirs';").unwrap();
    session.wait(1_000);
    session.code(KeyCode::Right); // choose the first action: Reload
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter); // Apply
    assert_eq!(session.text(), "SELECT 'theirs';");
    assert!(session.app.console().results.is_empty());
    // The editor is clean. The autosave works again with the new file.
    session.code(KeyCode::End);
    session.write(" -- more");
    session.wait(0);
    session.wait(500);
    assert_eq!(text_of(&store.global, &id), "SELECT 'theirs'; -- more");
}

#[test]
fn overwrite_writes_the_text_of_the_editor_over_the_file() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-overwrite");
    fs::write(script_path(&store.global, &id), "SELECT 'theirs';").unwrap();
    session.wait(1_000);
    session.code(KeyCode::Right);
    session.code(KeyCode::Right); // the second action: Overwrite
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert_eq!(text_of(&store.global, &id), "SELECT 1;");
    assert_eq!(session.text(), "SELECT 1;");
}

#[test]
fn apply_needs_a_choice_for_each_script() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-choose");
    fs::write(script_path(&store.global, &id), "SELECT 'theirs';").unwrap();
    session.wait(1_000);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter); // Apply without a choice
    session.assert_sees("Choose an action for each script.");
    assert_eq!(session.text(), "SELECT 1;");
    assert_eq!(text_of(&store.global, &id), "SELECT 'theirs';");
}

#[test]
fn a_removed_script_asks_and_keeps_the_text() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-removed");
    fs::remove_file(script_path(&store.global, &id)).unwrap();
    session.wait(1_000);
    session.assert_sees("removed");
    session.assert_sees("Scripts changed outside SQLN");
    session.code(KeyCode::Esc);
    assert_eq!(session.text(), "SELECT 1;");
    // The file does not come back by itself.
    session.write(" ");
    session.wait(0);
    session.wait(2_000);
    assert!(!script_path(&store.global, &id).exists());
}

#[test]
fn recreate_writes_the_removed_script_again() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-recreate");
    scripts::remove(&store.global, &id).unwrap();
    session.wait(1_000);
    session.code(KeyCode::Right); // Recreate
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert_eq!(text_of(&store.global, &id), "SELECT 1;");
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
}

#[test]
fn close_after_a_removal_closes_the_console_without_writing() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-close");
    // Another console keeps the application open.
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.code(KeyCode::Enter);
    assert_eq!(session.names().len(), 2);
    scripts::remove(&store.global, &id).unwrap();
    session.wait(1_000);
    session.code(KeyCode::Right);
    session.code(KeyCode::Right); // Close
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert_eq!(session.names().len(), 1);
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
}

#[test]
fn several_changed_scripts_share_one_question_with_a_choice_for_each() {
    let (store, base) = global_only("p-conflict-batch");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.type_and_save("ONE;");
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.code(KeyCode::Enter);
    session.type_and_save("TWO;");
    let mut entries = consoles_in(&store.global, Scope::Global);
    entries.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    for entry in &entries {
        fs::write(script_path(&store.global, &entry.meta.id), "EXTERNAL;").unwrap();
    }
    session.wait(1_000);
    session.assert_sees("console-1");
    session.assert_sees("console-2");
    // Two rows, two choices, one Apply.
    session.code(KeyCode::Right); // row 1: Reload
    session.code(KeyCode::Down);
    session.code(KeyCode::Right);
    session.code(KeyCode::Right); // row 2: Overwrite
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert_eq!(text_of(&store.global, &entries[0].meta.id), "EXTERNAL;");
    assert_eq!(text_of(&store.global, &entries[1].meta.id), "TWO;");
}

#[test]
fn closing_a_console_with_a_conflict_asks_before_it_drops_the_text() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-close-ask");
    fs::write(script_path(&store.global, &id), "SELECT 'theirs';").unwrap();
    session.wait(1_000);
    session.code(KeyCode::Esc);
    session.write(" -- mine");
    session.prefix('c');
    session.assert_sees("Text not saved");
    session.assert_sees("changed the script");
    session.code(KeyCode::Enter); // Cancel
    assert!(!session.app.should_quit());
    assert_eq!(text_of(&store.global, &id), "SELECT 'theirs';");
}

#[test]
fn ctrl_s_opens_the_question_again_for_a_script_in_conflict() {
    let (mut session, store, id) = session_with_saved_console("p-conflict-ctrl-s");
    fs::write(script_path(&store.global, &id), "SELECT 'theirs';").unwrap();
    session.wait(1_000);
    session.code(KeyCode::Esc);
    session.assert_not_sees("Scripts changed outside SQLN");
    session.press(ctrl('s'));
    session.assert_sees("Scripts changed outside SQLN");
}

#[test]
fn the_list_shows_how_long_ago_each_console_changed() {
    let (store, base) = global_only("p-ages");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let now = std::time::SystemTime::now();
    for (id, seconds) in [("fresh", 5), ("minutes", 5 * 60), ("hours", 3 * 3_600), ("days", 4 * 86_400)] {
        scripts::create(
            &store.global,
            &ScriptMeta::new(id.into(), id.into(), "g1".into(), Scope::Global),
            "SELECT 1;",
        )
        .unwrap();
        let time = now - Duration::from_secs(seconds);
        fs::File::options().write(true).open(script_path(&store.global, id)).unwrap().set_modified(time).unwrap();
    }
    let mut session = Session::start(&store);
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.assert_sees("just now");
    session.assert_sees("5 min ago");
    session.assert_sees("3 h ago");
    session.assert_sees("4 d ago");
    // The newest console comes first.
    let screen = session.screen();
    let order: Vec<usize> = ["fresh", "minutes", "hours", "days"]
        .iter()
        .map(|name| screen.find(&format!("{name} ")).unwrap_or_else(|| panic!("{name} is missing:\n{screen}")))
        .collect();
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}\n{screen}");
}

#[test]
fn a_reload_with_another_keyboard_mode_keeps_the_text_and_the_saved_state() {
    let (store, base) = global_only("p-reload-keyboard");
    let db = database(&base, "s", "");
    let entry = sqlite_entry("g1", "s", &db.display().to_string());
    write_config(&store.global, r#"{"keyboardMode":"conventional"}"#, std::slice::from_ref(&entry));
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;\nSELECT 2;");
    session.write("\nSELECT 3;");
    // The last text is not saved yet.
    write_config(&store.global, r#"{"keyboardMode":"vim"}"#, &[entry]);
    session.prefix('r');
    assert_eq!(session.app.console().editor.keyboard(), sqln::editor::KeyboardMode::Vim);
    assert_eq!(session.text(), "SELECT 1;\nSELECT 2;\nSELECT 3;");
    // The new editor is still newer than the file, so the autosave writes it.
    session.wait(0);
    session.wait(500);
    let id = consoles_in(&store.global, Scope::Global)[0].meta.id.clone();
    assert_eq!(text_of(&store.global, &id), "SELECT 1;\nSELECT 2;\nSELECT 3;");
}

// ---- scripts without a connection ----

fn session_with_orphan(name: &str) -> (Session, Store, PathBuf) {
    let (store, base) = store(name);
    let db = database(&base, "s", "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('hello');");
    let path = db.display().to_string();
    write_config(&store.global, "{}", &[sqlite_entry("g1", "global-db", &path)]);
    write_config(store.project.as_ref().unwrap(), "{}", &[sqlite_entry("p1", "project-db", &path)]);
    // A script of a connection that does not exist anymore.
    scripts::create(
        &store.global,
        &ScriptMeta::new("orphan".into(), "old-report".into(), "removed".into(), Scope::Global),
        "SELECT v FROM t;",
    )
    .unwrap();
    (Session::start(&store), store, base)
}

#[test]
fn the_list_of_scripts_without_a_connection_shows_only_those_scripts() {
    let (mut session, store, _) = session_with_orphan("p-orphans");
    scripts::create(
        &store.global,
        &ScriptMeta::new("fine".into(), "has-connection".into(), "g1".into(), Scope::Global),
        "SELECT 1;",
    )
    .unwrap();
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.assert_sees("Scripts without a connection");
    session.assert_sees("old-report");
    session.assert_not_sees("has-connection");
}

#[test]
fn the_list_is_empty_when_every_script_has_a_connection() {
    let (store, base) = global_only("p-no-orphans");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    let mut session = Session::start(&store);
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.assert_sees("No scripts without a connection");
}

#[test]
fn an_orphan_opens_for_edit_and_saves_but_the_run_asks_for_a_connection_first() {
    let (mut session, store, _) = session_with_orphan("p-orphan-edit");
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), "SELECT v FROM t;");
    assert!(session.app.console().connection.is_none());
    // The editor works, and the autosave keeps the script in its place.
    session.code(KeyCode::End);
    session.write(" -- edit");
    session.wait(0);
    session.wait(500);
    assert_eq!(text_of(&store.global, "orphan"), "SELECT v FROM t; -- edit");
    // The run asks for a connection. It does not run anything.
    session.press(common::shift(KeyCode::F(5)));
    session.assert_sees("Choose a connection");
    assert!(session.app.console().results.is_empty());
}

#[test]
fn associating_in_the_same_scope_confirms_links_and_does_not_run() {
    let (mut session, store, _) = session_with_orphan("p-associate-same");
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Enter);
    session.press(common::shift(KeyCode::F(5)));
    session.assert_sees("global-db");
    session.assert_sees("project-db");
    session.code(KeyCode::Enter); // global-db is the first row
    session.assert_sees("Associate old-report with global-db?");
    session.assert_sees("The files stay in");
    // Cancel is the default and links nothing.
    session.code(KeyCode::Enter);
    assert!(session.app.console().connection.is_none());
    session.press(common::shift(KeyCode::F(5)));
    session.code(KeyCode::Enter);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(session.app.console().connection.is_some());
    assert!(session.app.console().results.is_empty(), "the association runs no SQL");
    let entry = &consoles_in(&store.global, Scope::Global)[0];
    assert_eq!(entry.meta.connection_id, "g1");
    assert_eq!(entry.meta.connection_scope, Scope::Global);
    // Now the run works.
    session.press(common::shift(KeyCode::F(5)));
    for _ in 0..2_000 {
        session.app.drain_events();
        if session.screen().contains("hello") {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the run did not work:\n{}", session.screen());
}

#[test]
fn associating_with_a_connection_of_another_scope_moves_the_files() {
    let (mut session, store, _) = session_with_orphan("p-associate-move");
    let project = store.project.as_ref().unwrap();
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Enter);
    session.code(KeyCode::End);
    session.write(" -- edited");
    session.press(common::shift(KeyCode::F(5)));
    session.code(KeyCode::Down); // project-db
    session.code(KeyCode::Enter);
    session.assert_sees("moves the files");
    session.assert_sees("Project scope");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    // The pending text moved with the script. The old place has nothing left.
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
    let moved = consoles_in(project, Scope::Project);
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].meta.connection_id, "p1");
    assert_eq!(moved[0].meta.connection_scope, Scope::Project);
    assert_eq!(text_of(project, "orphan"), "SELECT v FROM t; -- edited");
    // The lock moved too: another instance cannot take the script.
    assert!(scripts::lock(project, "orphan").unwrap().is_none());
    assert!(session.app.console().connection.is_some());
}

#[test]
fn a_script_of_a_removed_connection_does_not_attach_to_a_connection_with_the_same_name() {
    let (store, base) = global_only("p-no-name-match");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("new-id", "same-name", &db.display().to_string())]);
    scripts::create(
        &store.global,
        &ScriptMeta::new("old".into(), "script".into(), "old-id".into(), Scope::Global),
        "SELECT 1;",
    )
    .unwrap();
    let mut session = Session::start(&store);
    session.go_to_connection("same-name");
    session.code(KeyCode::Char('n'));
    // The connection has no saved console, so no list opens. The script stays an orphan.
    session.assert_not_sees("Consoles of");
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.assert_sees("script");
}

#[test]
fn removing_a_script_from_the_list_asks_and_cancel_keeps_it() {
    let (mut session, store, _) = session_with_orphan("p-delete-script");
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Delete);
    session.assert_sees("Remove console");
    session.assert_sees("cannot be undone");
    session.code(KeyCode::Enter); // Cancel
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Delete);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
    assert!(!script_path(&store.global, "orphan").exists());
}

#[test]
fn a_script_that_is_open_cannot_be_removed() {
    let (mut session, store, _) = session_with_orphan("p-delete-open");
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Enter);
    session.press(ctrl('w'));
    session.code(KeyCode::Char('h'));
    session.code(KeyCode::Char('s'));
    session.code(KeyCode::Delete);
    session.assert_sees("Close the console before you remove its script.");
    assert_eq!(consoles_in(&store.global, Scope::Global).len(), 1);
}

// ---- layout ----

#[test]
fn the_layout_of_the_session_comes_back_at_the_next_start() {
    let (store, _) = global_only("p-layout");
    let mut first = Session::start(&store);
    first.prefix('b'); // collapse the navigator
    first.quit();
    drop(first);
    let second = Session::start(&store);
    assert!(second.app.navigator().collapsed);
}

#[test]
fn old_temporary_files_go_at_the_start() {
    let (store, _) = global_only("p-tmp");
    fs::create_dir_all(store.global.consoles_dir()).unwrap();
    let old = store.global.consoles_dir().join("x.sql.tmp");
    fs::write(&old, "x").unwrap();
    let past = std::time::SystemTime::now() - Duration::from_secs(3_600);
    fs::File::options().write(true).open(&old).unwrap().set_modified(past).unwrap();
    let _session = Session::start(&store);
    assert!(!old.exists());
}

#[test]
fn a_session_without_a_store_loads_and_saves_nothing() {
    let mut session = Session::with(Options::default());
    session.write("SELECT 1;");
    session.press(ctrl('s'));
    session.assert_sees("no text to save");
    session.wait(5_000);
    assert!(!session.app.tick_at(session.now));
    // The trouble of a file is a value of the console, not of the application.
    assert!(session.app.console().file.is_none());
    let _ = file::hash(b"");
    let _ = Trouble::Changed;
}

fn erase_name(session: &mut Session) {
    for _ in 0..40 {
        session.code(KeyCode::Backspace);
    }
}

fn two_connections(name: &str) -> (Store, PathBuf) {
    let (store, base) = store(name);
    let db = database(&base, "s", "CREATE TABLE t (v INTEGER);").display().to_string();
    let other = database(&base, "o", "").display().to_string();
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "first", &db), sqlite_entry("g2", "second", &other)],
    );
    write_config(store.project.as_ref().unwrap(), "{}", &[sqlite_entry("p1", "third", &other)]);
    (store, base)
}

fn run_and_wait(session: &mut Session) {
    session.code(KeyCode::F(5));
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.app.console().is_busy() {
        session.app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_new_name_keeps_the_identity_the_files_and_the_connection() {
    let (store, _) = two_connections("p-rename");
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    let id = consoles_in(&store.global, Scope::Global)[0].meta.id.clone();
    session.press(ctrl('w'));
    session.code(KeyCode::Char('N'));
    session.assert_sees("Rename console");
    erase_name(&mut session);
    session.write("report");
    session.code(KeyCode::Enter);
    assert_eq!(session.app.console().name, "report");
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].meta.id, id);
    assert_eq!(saved[0].meta.name, "report");
    assert_eq!(saved[0].meta.connection_id, "g1");
    assert_eq!(text_of(&store.global, &id), "SELECT 1;");
}

#[test]
fn a_name_must_not_be_empty_or_repeated_in_the_connection() {
    let (store, _) = two_connections("p-rename-refused");
    let root = store.global.clone();
    scripts::create(&root, &ScriptMeta::new("a".into(), "Taken".into(), "g1".into(), Scope::Global), "").unwrap();
    let mut session = Session::start(&store);
    let before = session.app.console().name.clone();
    session.press(ctrl('w'));
    session.code(KeyCode::Char('N'));
    erase_name(&mut session);
    session.code(KeyCode::Enter);
    session.assert_sees("needs a name");
    session.write("taken");
    session.code(KeyCode::Enter);
    session.assert_sees("has this name");
    assert_eq!(session.app.console().name, before);
    session.code(KeyCode::Esc);
    assert_eq!(session.app.console().name, before);
}

#[test]
fn the_list_renames_a_saved_console_that_no_tab_shows() {
    let (store, _) = two_connections("p-rename-list");
    let root = store.global.clone();
    scripts::create(&root, &ScriptMeta::new("a".into(), "old-name".into(), "g1".into(), Scope::Global), "SELECT 2;").unwrap();
    let mut session = Session::start(&store);
    session.go_to_connection("first");
    session.code(KeyCode::Char('n'));
    session.assert_sees("old-name");
    session.code(KeyCode::Down);
    session.code(KeyCode::Char('r'));
    erase_name(&mut session);
    session.write("new-name");
    session.code(KeyCode::Enter);
    let saved = consoles_in(&root, Scope::Global);
    assert_eq!(saved[0].meta.name, "new-name");
    assert_eq!(text_of(&root, "a"), "SELECT 2;");
}

#[test]
fn a_change_of_connection_keeps_the_text_and_the_name_and_ends_the_results() {
    let (store, _) = two_connections("p-change");
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    run_and_wait(&mut session);
    assert!(!session.app.console().results.is_empty());
    let name = session.app.console().name.clone();
    let old = session.app.console().connection;
    session.prefix('a');
    session.assert_sees("Change connection");
    session.code(KeyCode::Enter);
    session.assert_sees("from first to second");
    session.assert_sees("runs no SQL");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    let console = session.app.console();
    assert_ne!(console.connection, old);
    assert_eq!(console.name, name);
    assert_eq!(console.editor.text(), "SELECT 1;");
    assert!(console.results.is_empty());
    assert!(console.messages.is_empty());
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(saved[0].meta.connection_id, "g2");
}

#[test]
fn a_change_to_another_scope_moves_the_files_and_says_so() {
    let (store, _) = two_connections("p-change-scope");
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.prefix('a');
    session.code(KeyCode::Down);
    session.code(KeyCode::Enter);
    session.assert_sees("The files move to");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    let project = store.project.as_ref().unwrap();
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
    let moved = consoles_in(project, Scope::Project);
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].meta.connection_id, "p1");
    assert_eq!(text_of(project, &moved[0].meta.id), "SELECT 1;");
}

#[test]
fn a_change_of_connection_waits_for_the_end_of_a_transaction() {
    let (store, _) = two_connections("p-change-busy");
    let mut session = Session::start(&store);
    session.write("BEGIN;");
    run_and_wait(&mut session);
    let old = session.app.console().connection;
    session.prefix('a');
    session.assert_sees("End the execution and the transaction");
    assert_eq!(session.app.console().connection, old);
}

#[test]
fn the_settings_write_nothing_before_the_save() {
    let (store, _) = two_connections("p-settings-cancel");
    let project = store.project.clone().unwrap();
    let before = fs::read_to_string(project.config_file()).unwrap();
    let mut session = Session::start(&store);
    session.prefix(',');
    session.assert_sees("Settings");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.write("77");
    session.code(KeyCode::Esc);
    assert_eq!(fs::read_to_string(project.config_file()).unwrap(), before);
}

#[test]
fn the_settings_write_only_the_scope_of_the_dialog() {
    let (store, _) = two_connections("p-settings-save");
    let global = fs::read_to_string(store.global.config_file()).unwrap();
    let project = store.project.clone().unwrap();
    let mut session = Session::start(&store);
    session.prefix(',');
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.write("77");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    session.assert_sees("settings are saved");
    let text = fs::read_to_string(project.config_file()).unwrap();
    assert!(text.contains("\"pageSize\": 77"), "{text}");
    assert_eq!(fs::read_to_string(store.global.config_file()).unwrap(), global);
    // The connections and the other parts of the file stay.
    assert!(text.contains("third"), "{text}");
}

#[test]
fn an_empty_field_removes_the_project_value_and_shows_the_global_value() {
    let (store, base) = store("p-settings-remove");
    let db = database(&base, "s", "").display().to_string();
    write_config(&store.global, r#"{"pageSize":10}"#, &[sqlite_entry("g1", "s", &db)]);
    let project = store.project.clone().unwrap();
    write_config(&project, r#"{"pageSize":20,"keymap":{"conventional":{"quit":["ctrl+w q"]}}}"#, &[]);
    let mut session = Session::start(&store);
    session.prefix(',');
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Backspace);
    session.code(KeyCode::Backspace);
    session.assert_sees("from global: 10");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    let text = fs::read_to_string(project.config_file()).unwrap();
    assert!(!text.contains("pageSize"), "{text}");
    assert!(text.contains("keymap"), "{text}");
}

#[test]
fn a_settings_value_of_zero_stays_in_the_dialog_with_a_note() {
    let (store, _) = two_connections("p-settings-zero");
    let mut session = Session::start(&store);
    session.prefix(',');
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.write("0");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    session.assert_sees("needs a number of 1 or more");
}

#[test]
fn a_change_of_keyboard_in_the_settings_keeps_the_text_the_cursor_and_the_history() {
    let (store, _) = two_connections("p-settings-keyboard");
    let mut session = Session::start(&store);
    session.write("SELECT 1;");
    let cursor = session.app.console().editor.cursor();
    session.prefix(',');
    session.code(KeyCode::Tab);
    session.code(KeyCode::Right);
    session.code(KeyCode::Right);
    for _ in 0..4 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    let editor = &session.app.console().editor;
    assert_eq!(editor.keyboard(), sqln::editor::KeyboardMode::Vim);
    assert_eq!(editor.text(), "SELECT 1;");
    assert_eq!(editor.cursor().row, cursor.row);
    session.code(KeyCode::Char('u'));
    assert_eq!(session.text(), "");
}

#[test]
fn a_damaged_workspace_file_warns_without_a_block() {
    let (store, base) = global_only("p-workspace");
    let db = database(&base, "s", "");
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db.display().to_string())]);
    fs::write(store.global.workspace_file(), "{ not json").unwrap();
    let mut session = Session::start(&store);
    session.assert_sees("cannot read workspace.json");
    session.write("SELECT 1;");
    assert_eq!(session.text(), "SELECT 1;");
}

#[test]
fn a_signal_saves_the_pending_text_and_asks_nothing() {
    let (store, _) = two_connections("p-signal");
    let mut session = Session::start(&store);
    session.write("SELECT 1;");
    session.app.save_without_questions();
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(text_of(&store.global, &saved[0].meta.id), "SELECT 1;");
    session.assert_not_sees("Close without saving");
}

#[test]
fn a_signal_leaves_a_file_that_another_program_changed() {
    let (mut session, store, id) = session_with_saved_console("p-signal-changed");
    let path = store.global.consoles_dir().join(format!("{id}.sql"));
    fs::write(&path, "SELECT 'other program';").unwrap();
    session.wait(1_000);
    session.code(KeyCode::Esc);
    session.write("more text");
    session.app.save_without_questions();
    assert_eq!(fs::read_to_string(&path).unwrap(), "SELECT 'other program';");
}

#[test]
fn the_close_of_a_removed_script_names_the_loss_of_the_text() {
    let (mut session, store, id) = session_with_saved_console("p-close-removed");
    fs::remove_file(script_path(&store.global, &id)).unwrap();
    session.wait(1_000);
    session.code(KeyCode::Esc);
    session.prefix('c');
    session.assert_sees("Close and discard the text");
    session.assert_not_sees("Close without saving");
}
