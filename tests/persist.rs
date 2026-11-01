// The application with files: configuration, project, autosave, reopen, conflicts,
// and scripts without a connection.

mod common;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, temp_dir};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Source;
use sqln::export::{Delimiter, ExportChoices, ExportFormat};
use sqln::form::{Field, FormNote};
use sqln::input::SuggestionKeys;
use sqln::navigator::RowKind;
use sqln::script_file::Trouble;
use sqln::settings::SettingsForm;
use sqln::store::config::{self, ConnectionIcon, Preferences, ThemeName};
use sqln::store::scripts::{self, ScriptMeta};
use sqln::store::{Root, Scope, Store, file};
use sqln::ui::icons::Icons;
use sqln::ui::theme::{ColorEnv, Theme};

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
    (
        Store {
            global: Root::global(base.join("global")),
            project: Some(project),
        },
        base,
    )
}

/// Writes a database below the current folder of the process. It returns the full path
/// and the path from the current folder.
fn local_database(name: &str, setup: &str) -> (PathBuf, String) {
    let relative = format!("target/tmp/{name}.db");
    let full = std::env::current_dir().unwrap().join(&relative);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    let _ = fs::remove_file(&full);
    rusqlite::Connection::open(&full)
        .unwrap()
        .execute_batch(setup)
        .unwrap();
    (full, relative)
}

/// Returns the file of the first SQLite connection of the saved configuration.
fn saved_file(store: &Store) -> PathBuf {
    let config = sqln::store::config::load(store).config.unwrap();
    match &config.connections[0].definition.source {
        Source::Sqlite(target) => target.path.clone(),
        Source::Network(_) => panic!("this connection is not SQLite"),
    }
}

/// A store without a project.
fn global_only(name: &str) -> (Store, PathBuf) {
    let base = temp_dir(name);
    (
        Store {
            global: Root::global(base.join("global")),
            project: None,
        },
        base,
    )
}

fn database(base: &Path, name: &str, setup: &str) -> PathBuf {
    let path = base.join(format!("{name}.db"));
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(setup)
        .unwrap();
    path
}

impl Session {
    fn start(store: &Store) -> Self {
        Self::with(Options {
            store: Some(store.clone()),
            ..Options::default()
        })
    }

    fn with(options: Options) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(options, clipboard, Theme::plain());
        Self {
            app,
            now: Instant::now(),
        }
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
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row.trim_end().to_owned() + "\n"
            })
            .collect()
    }

    fn assert_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(
            screen.contains(text),
            "{text:?} is not on the screen:\n{screen}"
        );
    }

    fn assert_not_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(
            !screen.contains(text),
            "{text:?} is on the screen:\n{screen}"
        );
    }

    fn click(&mut self, column: u16, row: u16) {
        let kind = MouseEventKind::Down(MouseButton::Left);
        self.app.on_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Returns the column and the row of a text on the screen.
    fn locate(&mut self, text: &str) -> (u16, u16) {
        let screen = self.screen();
        for (y, row) in screen.lines().enumerate() {
            if let Some(byte) = row.find(text) {
                return (row[..byte].chars().count() as u16, y as u16);
            }
        }
        panic!("{text:?} is not on the screen:\n{screen}");
    }

    fn click_text(&mut self, text: &str) {
        let (column, row) = self.locate(text);
        self.click(column, row);
    }

    /// Opens the settings on the section of the results, with the focus on Page size.
    /// The store needs a project, because the focus goes past the scope.
    fn open_results_settings(&mut self) {
        self.prefix(',');
        self.code(KeyCode::Down);
        self.code(KeyCode::Down);
        self.code(KeyCode::Tab);
        self.code(KeyCode::Tab);
    }

    fn names(&self) -> Vec<String> {
        self.app
            .consoles()
            .iter()
            .map(|console| console.name.clone())
            .collect()
    }

    fn pump_sees(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.screen().contains(text) {
            self.app.drain_events();
            assert!(
                Instant::now() < deadline,
                "{text:?} did not appear:\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Opens the form of a new connection and writes a SQLite name and file.
    fn fill_sqlite_form(&mut self, name: &str, file: &str) {
        self.prefix('h');
        self.code(KeyCode::Char('a'));
        self.code(KeyCode::Left);
        self.code(KeyCode::Left);
        self.focus_form(Field::Name);
        for letter in name.chars() {
            self.code(KeyCode::Char(letter));
        }
        self.focus_form(Field::File);
        self.write(file);
    }

    fn focus_form(&mut self, field: Field) {
        for _ in 0..32 {
            if self.app.form().unwrap().focus == field {
                return;
            }
            self.code(KeyCode::Tab);
        }
        panic!("{field:?} is not in the form");
    }

    fn press_form_button(&mut self, field: Field) {
        self.focus_form(field);
        self.code(KeyCode::Enter);
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
            let on = current
                .is_some_and(|row| rows.contains(&row) && row.name(self.app.connections()) == name);
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "app", &db.display().to_string())],
    );
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[
            sqlite_entry("p1", "app", &db.display().to_string()),
            sqlite_entry("p2", "other", &db.display().to_string()),
        ],
    );
    let mut session = Session::start(&store);
    let names: Vec<_> = session
        .app
        .connections()
        .iter()
        .map(|c| c.name.clone())
        .collect();
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
    database(
        &project_root,
        "db/app",
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('from-project');",
    );
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[sqlite_entry("p1", "app", "db/app.db")],
    );
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
    write_config(
        &store.global,
        r#"{"keyboardMode":"conventional","rowCap":3}"#,
        &[],
    );
    write_config(
        store.project.as_ref().unwrap(),
        r#"{"keyboardMode":"vim"}"#,
        &[],
    );
    let session = Session::start(&store);
    assert_eq!(
        session.app.console().editor.keyboard(),
        sqln::editor::KeyboardMode::Vim
    );
}

#[test]
fn a_row_cap_of_the_file_limits_the_result() {
    let (store, base) = global_only("p-rowcap");
    let db = database(
        &base,
        "r",
        "CREATE TABLE t (v INTEGER); INSERT INTO t VALUES (1),(2),(3),(4),(5);",
    );
    write_config(
        &store.global,
        r#"{"rowCap":2}"#,
        &[sqlite_entry("g1", "r", &db.display().to_string())],
    );
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
    fs::write(
        store.global.config_file(),
        "{\n  \"version\": 1,\n  \"connections\": [,]\n}",
    )
    .unwrap();
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "fixed", &db.display().to_string())],
    );
    session.prefix('r');
    session.assert_sees("loaded again");
    let names: Vec<_> = session
        .app
        .connections()
        .iter()
        .map(|c| c.name.clone())
        .collect();
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
    write_config(
        &store.global,
        r#"{"keymap":{"conventional":{"run":["f6"],"stop":["f6"]}}}"#,
        &[],
    );
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
    write_config(
        &store.global,
        "{}",
        &[
            sqlite_entry("g1", "one", &path),
            sqlite_entry("g2", "two", &path),
        ],
    );
    session.wait(1_000);
    session.assert_sees("Configuration changed");
    // The default answer changes nothing.
    session.code(KeyCode::Enter);
    assert_eq!(session.app.connections().len(), 1);
    // The same content does not ask again.
    session.wait(1_000);
    session.assert_not_sees("Configuration changed");
    // A new change asks again, and Reload applies it.
    write_config(
        &store.global,
        "{}",
        &[
            sqlite_entry("g1", "one", &path),
            sqlite_entry("g3", "three", &path),
        ],
    );
    session.wait(1_000);
    session.assert_sees("Configuration changed");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    let names: Vec<_> = session
        .app
        .connections()
        .iter()
        .map(|c| c.name.clone())
        .collect();
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
    assert_eq!(
        session.app.connections().len(),
        1,
        "an active connection stays"
    );
    assert!(session.app.console().in_transaction());
}

#[test]
fn a_connection_that_leaves_the_file_leaves_the_application_when_it_is_idle() {
    let (store, base) = global_only("p-reload-remove");
    let db = database(&base, "a", "");
    let path = db.display().to_string();
    write_config(
        &store.global,
        "{}",
        &[
            sqlite_entry("g1", "one", &path),
            sqlite_entry("g2", "two", &path),
        ],
    );
    let mut session = Session::start(&store);
    write_config(&store.global, "{}", &[sqlite_entry("g1", "one", &path)]);
    session.prefix('r');
    let names: Vec<_> = session
        .app
        .connections()
        .iter()
        .map(|c| c.name.clone())
        .collect();
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
    assert_eq!(
        fs::read_to_string(folder.join(".sqln/.gitignore")).unwrap(),
        "workspace.json\nlocks/\n*.tmp\n"
    );
    assert!(!folder.join(".git").exists());
}

#[test]
fn declining_the_project_keeps_the_global_scope_only() {
    let (store, base) = global_only("p-decline");
    let folder = base.join("work");
    fs::create_dir_all(&folder).unwrap();
    let options = Options {
        store: Some(store),
        offer_project: Some(folder.clone()),
        ..Options::default()
    };
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
    session.press_form_button(Field::Save);
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
    session.press_form_button(Field::Save);
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
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[sqlite_entry("p1", "same", &path)],
    );
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
        session.press_form_button(Field::Save);
    };
    // The name is the same in the project, ignoring the case. The scope refuses it.
    save_sqlite(&mut session, false);
    session.assert_sees("Another connection has this name.");
    session.code(KeyCode::Esc);
    // The global scope accepts it, and both names show their scope.
    save_sqlite(&mut session, true);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let names: Vec<_> = session
        .app
        .connections()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["same (local)", "SAME (global)"]);
}

#[test]
fn removing_a_connection_removes_its_entry_and_keeps_its_scripts() {
    let (store, base) = global_only("p-remove");
    let db = database(&base, "d", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "gone", &db.display().to_string())],
    );
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
    assert_eq!(
        consoles_in(&store.global, Scope::Global).len(),
        1,
        "the script stays"
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "before", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.go_to_connection("before");
    session.code(KeyCode::Char('e'));
    erase_field(&mut session, "before".len());
    session.write("after");
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"id\": \"g1\""), "{text}");
    assert!(text.contains("\"name\": \"after\""), "{text}");
    assert!(!text.contains("before"), "{text}");
    assert_eq!(session.app.connections()[0].name, "after");
    // The script keeps pointing to the same identity.
    assert_eq!(
        consoles_in(&store.global, Scope::Global)[0]
            .meta
            .connection_id,
        "g1"
    );
}

#[test]
fn the_sql_limit_of_a_connection_saves_and_zero_means_no_limit() {
    let (store, base) = global_only("p-sql-limit");
    let db = database(&base, "e", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "one", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.go_to_connection("one");
    session.code(KeyCode::Char('e'));
    for _ in 0..7 {
        session.code(KeyCode::Tab);
    }
    session.assert_sees("SQL limit");
    erase_field(&mut session, 2);
    session.write("0");
    session.press_form_button(Field::Save);
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "one", &db.display().to_string())],
    );
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
    assert!(
        !fs::read_to_string(store.global.config_file())
            .unwrap()
            .contains("sqlTimeoutSeconds")
    );
}

#[test]
fn a_connected_connection_can_change_its_name_and_the_file_follows() {
    let (store, base) = global_only("p-rename");
    let db = database(&base, "e", "CREATE TABLE t (v INTEGER);");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "old", &db.display().to_string())],
    );
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
    erase_field(&mut session, "old".len());
    session.write("new");
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none());
    assert_eq!(session.app.connections()[0].name, "new");
    assert!(session.app.console().in_transaction());
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(
        text.contains("\"name\": \"new\"") && !text.contains("\"old\""),
        "{text}"
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.wait(0);
    session.wait(2_000);
    assert!(
        consoles_in(&store.global, Scope::Global).is_empty(),
        "no edit, no file"
    );
    session.write("SELECT 1;");
    session.wait(0);
    session.wait(499);
    assert!(
        consoles_in(&store.global, Scope::Global).is_empty(),
        "before 500 ms"
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[sqlite_entry("p1", "proj", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    assert_eq!(
        consoles_in(store.project.as_ref().unwrap(), Scope::Project).len(),
        1
    );
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
}

#[test]
fn ctrl_s_writes_at_once() {
    let (store, base) = global_only("p-ctrl-s");
    let db = database(&base, "s", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.press(ctrl('s'));
    session.assert_sees("no text to save");
    assert!(consoles_in(&store.global, Scope::Global).is_empty());
}

#[test]
fn closing_a_console_writes_the_pending_text() {
    let (store, base) = global_only("p-close-flush");
    let db = database(&base, "s", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.write("SELECT 1;\nSELECT 2;");
    session.quit();
    let saved = consoles_in(&store.global, Scope::Global);
    assert_eq!(
        text_of(&store.global, &saved[0].meta.id),
        "SELECT 1;\nSELECT 2;"
    );
    let state = sqln::store::workspace::load(&store.global);
    let cursor = state.consoles[&saved[0].meta.id].cursor;
    assert_eq!((cursor.row, cursor.col), (1, 9));
}

#[test]
fn a_new_console_takes_the_lowest_free_name_after_a_restart() {
    let (store, base) = global_only("p-names");
    let db = database(&base, "s", "");
    let preferences = r#"{"reopenTabs":false}"#;
    write_config(
        &store.global,
        preferences,
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.quit();
    drop(first);
    let mut second = Session::start(&store);
    assert_eq!(second.names(), ["console-2"], "console-1 is saved");
    second.type_and_save("SELECT 2;");
    let mut names: Vec<_> = consoles_in(&store.global, Scope::Global)
        .into_iter()
        .map(|entry| entry.meta.name)
        .collect();
    names.sort();
    assert_eq!(names, ["console-1", "console-2"]);
}

#[test]
fn the_alert_of_a_failed_write_stays_while_the_failure_lasts() {
    let (store, base) = global_only("p-failure-stays");
    let db = database(&base, "s", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
    fs::write(store.global.dir.join("consoles"), "x").unwrap();
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    // Each new attempt fails again, so 30 s do not end the alert.
    for _ in 0..15 {
        session.wait(2_000);
        session.assert_sees("Cannot save");
    }
    // The next attempt writes the text, and the alert has no new failure.
    fs::remove_file(store.global.dir.join("consoles")).unwrap();
    session.wait(5_000);
    session.wait(8_000);
    session.assert_not_sees("Cannot save");
}

#[test]
fn a_failed_write_keeps_the_text_shows_an_alert_and_blocks_the_close_until_confirmed() {
    let (store, base) = global_only("p-failure");
    let db = database(&base, "s", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    scripts::create(
        root,
        &ScriptMeta::new("a".into(), "investigate".into(), "g1".into(), Scope::Global),
        "SELECT 1;",
    )
    .unwrap();
    scripts::create(
        root,
        &ScriptMeta::new("b".into(), "other-db".into(), "g2".into(), Scope::Global),
        "SELECT 2;",
    )
    .unwrap();
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[sqlite_entry("p1", "app", &path)],
    );
    scripts::create(
        store.project.as_ref().unwrap(),
        &ScriptMeta::new(
            "pc".into(),
            "from-project".into(),
            "p1".into(),
            Scope::Project,
        ),
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
    let connection = session
        .app
        .connections()
        .iter()
        .find(|c| Some(c.id) == console.connection)
        .unwrap();
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "gone", &db.display().to_string())],
    );
    let mut session = Session::start(&store);
    session.go_to_connection("gone");
    session.code(KeyCode::Delete);
    session.assert_sees("Remove the definition of gone from");
    session.assert_sees("scripts of the connection stay");
    // Cancel removes nothing.
    session.code(KeyCode::Enter);
    assert_eq!(session.app.connections().len(), 1);
    assert!(
        fs::read_to_string(store.global.config_file())
            .unwrap()
            .contains("gone")
    );
}

// ---- changes by other programs ----

fn script_path(root: &Root, id: &str) -> PathBuf {
    root.consoles_dir().join(format!("{id}.sql"))
}

fn session_with_saved_console(name: &str) -> (Session, Store, String) {
    let (store, base) = global_only(name);
    let db = database(&base, "s", "");
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
    let now = std::time::SystemTime::now();
    for (id, seconds) in [
        ("fresh", 5),
        ("minutes", 5 * 60),
        ("hours", 3 * 3_600),
        ("days", 4 * 86_400),
    ] {
        scripts::create(
            &store.global,
            &ScriptMeta::new(id.into(), id.into(), "g1".into(), Scope::Global),
            "SELECT 1;",
        )
        .unwrap();
        let time = now - Duration::from_secs(seconds);
        fs::File::options()
            .write(true)
            .open(script_path(&store.global, id))
            .unwrap()
            .set_modified(time)
            .unwrap();
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
        .map(|name| {
            screen
                .find(&format!("{name} "))
                .unwrap_or_else(|| panic!("{name} is missing:\n{screen}"))
        })
        .collect();
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "{order:?}\n{screen}"
    );
}

#[test]
fn a_reload_with_another_keyboard_mode_keeps_the_text_and_the_saved_state() {
    let (store, base) = global_only("p-reload-keyboard");
    let db = database(&base, "s", "");
    let entry = sqlite_entry("g1", "s", &db.display().to_string());
    write_config(
        &store.global,
        r#"{"keyboardMode":"conventional"}"#,
        std::slice::from_ref(&entry),
    );
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;\nSELECT 2;");
    session.write("\nSELECT 3;");
    // The last text is not saved yet.
    write_config(&store.global, r#"{"keyboardMode":"vim"}"#, &[entry]);
    session.prefix('r');
    assert_eq!(
        session.app.console().editor.keyboard(),
        sqln::editor::KeyboardMode::Vim
    );
    assert_eq!(session.text(), "SELECT 1;\nSELECT 2;\nSELECT 3;");
    // The new editor is still newer than the file, so the autosave writes it.
    session.wait(0);
    session.wait(500);
    let id = consoles_in(&store.global, Scope::Global)[0].meta.id.clone();
    assert_eq!(
        text_of(&store.global, &id),
        "SELECT 1;\nSELECT 2;\nSELECT 3;"
    );
}

// ---- scripts without a connection ----

fn session_with_orphan(name: &str) -> (Session, Store, PathBuf) {
    let (store, base) = store(name);
    let db = database(
        &base,
        "s",
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('hello');",
    );
    let path = db.display().to_string();
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "global-db", &path)],
    );
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[sqlite_entry("p1", "project-db", &path)],
    );
    // A script of a connection that does not exist anymore.
    scripts::create(
        &store.global,
        &ScriptMeta::new(
            "orphan".into(),
            "old-report".into(),
            "removed".into(),
            Scope::Global,
        ),
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
        &ScriptMeta::new(
            "fine".into(),
            "has-connection".into(),
            "g1".into(),
            Scope::Global,
        ),
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    assert!(
        session.app.console().results.is_empty(),
        "the association runs no SQL"
    );
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry(
            "new-id",
            "same-name",
            &db.display().to_string(),
        )],
    );
    scripts::create(
        &store.global,
        &ScriptMeta::new(
            "old".into(),
            "script".into(),
            "old-id".into(),
            Scope::Global,
        ),
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
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(past)
        .unwrap();
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
    let db = database(&base, "s", "CREATE TABLE t (v INTEGER);")
        .display()
        .to_string();
    let other = database(&base, "o", "").display().to_string();
    write_config(
        &store.global,
        "{}",
        &[
            sqlite_entry("g1", "first", &db),
            sqlite_entry("g2", "second", &other),
        ],
    );
    write_config(
        store.project.as_ref().unwrap(),
        "{}",
        &[sqlite_entry("p1", "third", &other)],
    );
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
    scripts::create(
        &root,
        &ScriptMeta::new("a".into(), "Taken".into(), "g1".into(), Scope::Global),
        "",
    )
    .unwrap();
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
    scripts::create(
        &root,
        &ScriptMeta::new("a".into(), "old-name".into(), "g1".into(), Scope::Global),
        "SELECT 2;",
    )
    .unwrap();
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
    session.open_results_settings();
    session.assert_sees("Settings");
    session.write("77");
    session.assert_sees("77");
    session.code(KeyCode::Esc);
    assert_eq!(fs::read_to_string(project.config_file()).unwrap(), before);
}

#[test]
fn the_settings_write_only_the_scope_of_the_dialog() {
    let (store, _) = two_connections("p-settings-save");
    let global = fs::read_to_string(store.global.config_file()).unwrap();
    let project = store.project.clone().unwrap();
    let mut session = Session::start(&store);
    session.open_results_settings();
    session.write("77");
    for _ in 0..4 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    session.assert_sees("settings are saved");
    let text = fs::read_to_string(project.config_file()).unwrap();
    assert!(text.contains("\"pageSize\": 77"), "{text}");
    assert_eq!(
        fs::read_to_string(store.global.config_file()).unwrap(),
        global
    );
    // The connections and the other parts of the file stay.
    assert!(text.contains("third"), "{text}");
}

#[test]
fn an_empty_field_removes_the_project_value_and_shows_the_global_value() {
    let (store, base) = store("p-settings-remove");
    let db = database(&base, "s", "").display().to_string();
    write_config(
        &store.global,
        r#"{"pageSize":10}"#,
        &[sqlite_entry("g1", "s", &db)],
    );
    let project = store.project.clone().unwrap();
    write_config(
        &project,
        r#"{"pageSize":20,"keymap":{"conventional":{"quit":["ctrl+w q"]}}}"#,
        &[],
    );
    let mut session = Session::start(&store);
    session.open_results_settings();
    session.code(KeyCode::Backspace);
    session.code(KeyCode::Backspace);
    session.assert_sees("from global: 10");
    for _ in 0..4 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Enter);
    let text = fs::read_to_string(project.config_file()).unwrap();
    assert!(!text.contains("pageSize"), "{text}");
    assert!(text.contains("keymap"), "{text}");
}

#[test]
fn a_settings_value_of_zero_stays_in_the_dialog_with_a_note() {
    let (store, _) = two_connections("p-settings-zero");
    let mut session = Session::start(&store);
    session.open_results_settings();
    session.write("0");
    for _ in 0..4 {
        session.code(KeyCode::Tab);
    }
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
    session.code(KeyCode::Tab);
    session.code(KeyCode::Right);
    session.code(KeyCode::Right);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
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
    write_config(
        &store.global,
        "{}",
        &[sqlite_entry("g1", "s", &db.display().to_string())],
    );
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
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "SELECT 'other program';"
    );
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

#[test]
fn the_form_saves_the_full_path_that_its_test_used() {
    let (store, _) = global_only("p-form-path");
    let (full, relative) = local_database(
        "form-full-path",
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('from-current');",
    );
    let mut session = Session::start(&store);
    session.fill_sqlite_form("checked", &relative);

    session.press_form_button(Field::Test);
    session.pump_sees("The connection works.");
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none(), "{}", session.screen());
    assert_eq!(saved_file(&store), full);
    assert!(!store.global.base.join(&relative).exists());

    // The saved connection opens the file of the test, and not a file of the global folder.
    session.code(KeyCode::Char('n'));
    session.write("SELECT v FROM t;");
    session.code(KeyCode::F(5));
    session.pump_sees("from-current");
}

#[test]
fn a_project_connection_also_saves_the_full_path() {
    let (store, _) = store("p-form-project-path");
    let (full, relative) = local_database("form-project-path", "CREATE TABLE t (v TEXT);");
    let mut session = Session::start(&store);
    session.fill_sqlite_form("checked", &relative);
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none(), "{}", session.screen());
    assert_eq!(saved_file(&store), full);
}

#[test]
fn a_new_file_goes_to_the_current_folder_with_its_full_path() {
    let (store, _) = global_only("p-form-new-file");
    let (full, relative) = local_database("form-new-file", "");
    fs::remove_file(&full).unwrap();
    let mut session = Session::start(&store);
    session.fill_sqlite_form("created", &relative);

    session.press_form_button(Field::Test);
    let Some(FormNote::Error(note)) = &session.app.form().unwrap().note else {
        panic!("the test must report the missing file");
    };
    assert!(note.contains(&full.display().to_string()), "{note}");

    session.press_form_button(Field::Save);
    session.assert_sees("Create it?");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    assert!(session.app.form().is_none(), "{}", session.screen());
    assert_eq!(saved_file(&store), full);
}

#[test]
fn an_edit_keeps_a_relative_path_of_the_configuration() {
    let (store, _) = global_only("p-form-keep-relative");
    write_config(&store.global, "{}", &[sqlite_entry("a", "kept", "kept.db")]);
    database(&store.global.base, "kept", "CREATE TABLE t (v TEXT);");
    let mut session = Session::start(&store);
    session.go_to_connection("kept");
    session.code(KeyCode::Char('e'));
    assert_eq!(session.app.form().unwrap().text(Field::File), "kept.db");

    // The file is in the folder of the scope, and the current folder has no such file.
    session.press_form_button(Field::Test);
    session.pump_sees("The connection works.");
    session.focus_form(Field::Pool);
    session.code(KeyCode::Backspace);
    session.code(KeyCode::Char('2'));
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none(), "{}", session.screen());
    assert_eq!(saved_file(&store), PathBuf::from("kept.db"));
}

#[test]
fn a_changed_path_in_an_edit_starts_at_the_current_folder() {
    let (store, _) = global_only("p-form-change-path");
    write_config(&store.global, "{}", &[sqlite_entry("a", "moved", "old.db")]);
    database(&store.global.base, "old", "CREATE TABLE t (v TEXT);");
    let (full, relative) = local_database("form-changed-path", "CREATE TABLE t (v TEXT);");
    let mut session = Session::start(&store);
    session.go_to_connection("moved");
    session.code(KeyCode::Char('e'));
    session.focus_form(Field::File);
    for _ in "old.db".chars() {
        session.code(KeyCode::Backspace);
    }
    session.write(&relative);
    session.press_form_button(Field::Test);
    session.pump_sees("The connection works.");
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none(), "{}", session.screen());
    assert_eq!(saved_file(&store), full);
}

// ---- the tabs of the last session ----

const ITEMS: &str = "CREATE TABLE items(id INTEGER); INSERT INTO items VALUES (4242);";

/// A global folder with one SQLite connection that has a table.
fn reopening_store(name: &str, preferences: &str) -> (Store, String) {
    let (store, base) = global_only(name);
    let db = database(&base, "s", ITEMS).display().to_string();
    write_config(&store.global, preferences, &[sqlite_entry("g1", "s", &db)]);
    (store, db)
}

impl Session {
    fn new_console(&mut self) {
        self.go_to_connection("s");
        self.code(KeyCode::Char('n'));
        self.code(KeyCode::Enter);
    }

    fn top_row(&mut self) -> String {
        self.screen().lines().next().unwrap().to_owned()
    }
}

#[test]
fn the_tabs_of_the_last_session_reopen_in_order_with_the_same_active_tab() {
    let (store, _) = reopening_store("p-reopen", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.new_console();
    first.type_and_save("SELECT 2;");
    first.new_console();
    first.type_and_save("SELECT 3;");
    first.prefix('p');
    assert_eq!(first.app.console().name, "console-2");
    first.quit();
    drop(first);

    let mut second = Session::start(&store);
    assert_eq!(second.names(), ["console-1", "console-2", "console-3"]);
    assert_eq!(second.app.console().name, "console-2");
    assert_eq!(second.text(), "SELECT 2;");
    assert!(
        second.top_row().find("console-1").unwrap() < second.top_row().find("console-3").unwrap()
    );
}

#[test]
fn a_reopened_console_runs_no_sql_and_shows_no_result() {
    let (store, _) = reopening_store("p-reopen-result", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 4242;");
    first.press(key(KeyCode::F(5)));
    first.pump_sees("1 row");
    first.quit();
    drop(first);

    let mut second = Session::start(&store);
    assert_eq!(second.text(), "SELECT 4242;");
    assert!(second.app.console().results.is_empty());
    second.assert_sees("No messages.");
}

#[test]
fn an_empty_console_does_not_reopen() {
    let (store, _) = reopening_store("p-reopen-empty", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.new_console();
    assert_eq!(first.names(), ["console-1", "console-2"]);
    first.quit();
    drop(first);

    let second = Session::start(&store);
    assert_eq!(second.names(), ["console-1"]);
    assert_eq!(second.text(), "SELECT 1;");
}

#[test]
fn a_table_of_the_last_session_loads_at_its_selection() {
    let (store, _) = reopening_store("p-reopen-table", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    common::open_table(&mut first.app, 0, "items");
    first.pump_sees("4242");
    first.prefix('p');
    assert!(first.app.table().is_none());
    first.quit();
    drop(first);

    let mut second = Session::start(&store);
    assert!(
        second.top_row().contains("items (table)"),
        "{}",
        second.top_row()
    );
    assert!(second.app.table().is_none(), "the console stays on screen");
    assert_eq!(second.app.tables()[0].loaded(), 0);
    second.app.drain_events();
    second.assert_not_sees("4242");
    second.prefix('n');
    assert!(second.app.table().is_some());
    second.pump_sees("4242");
}

#[test]
fn the_list_of_consoles_offers_rename_and_remove_only_on_a_script() {
    let (store, _) = reopening_store("p-hint-picker", "{}");
    let mut session = Session::start(&store);
    session.type_and_save("SELECT 1;");
    session.go_to_connection("s");
    session.code(KeyCode::Char('n'));
    session.assert_sees("New console");
    let hint = session.app.hint();
    assert!(!hint.contains("rename"), "{hint}");
    assert!(!hint.contains("remove"), "{hint}");

    session.code(KeyCode::Down);
    let hint = session.app.hint();
    assert!(hint.contains("r rename"), "{hint}");
    assert!(hint.contains("Del remove a script"), "{hint}");
}

#[test]
fn the_settings_show_the_keys_of_the_focused_field() {
    let (store, _) = reopening_store("p-hint-settings", "{}");
    let mut session = Session::start(&store);
    let mut seen = Vec::new();
    // The first section has choices, and the third section has text fields.
    for section in [0, 2] {
        session.prefix(',');
        for _ in 0..section {
            session.code(KeyCode::Down);
        }
        for _ in 0..8 {
            let hint = session.app.hint();
            if hint.contains("An empty field removes the value") {
                assert!(!hint.contains("Left/Right"), "a text field: {hint}");
                seen.push("text");
            } else if hint.contains("Left/Right change") {
                seen.push("choice");
            } else if hint.contains("Enter press") {
                seen.push("button");
            }
            session.code(KeyCode::Tab);
        }
        session.code(KeyCode::Esc);
    }
    for kind in ["text", "choice", "button"] {
        assert!(seen.contains(&kind), "{kind} is not in {seen:?}");
    }
}

#[test]
fn a_structure_tab_does_not_reopen() {
    let (store, _) = reopening_store("p-reopen-structure", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    common::select_row(&mut first.app, 0, &[RowKind::Table], "items");
    first.code(KeyCode::Char('i'));
    first.pump_sees("items (structure)");
    first.quit();
    drop(first);

    let mut second = Session::start(&store);
    assert!(second.app.tables().is_empty());
    assert!(!second.top_row().contains("structure"));
}

#[test]
fn an_export_of_all_rows_reads_the_whole_structure() {
    let (store, base) = global_only("p-export-structure");
    let db = database(&base, "s", "CREATE TABLE wide(a INT, b INT, c INT, d INT);");
    let entry = sqlite_entry("g1", "s", &db.display().to_string());
    write_config(&store.global, r#"{"pageSize":2}"#, &[entry]);
    let output = base.join("files").join("wide.csv");
    let mut session = Session::start(&store);
    common::select_row(&mut session.app, 0, &[RowKind::Table], "wide");
    session.code(KeyCode::Char('i'));
    session.pump_sees("wide (structure)");
    let deadline = Instant::now() + Duration::from_secs(20);
    while session.app.table().is_none_or(|tab| tab.is_busy()) {
        session.app.drain_events();
        assert!(Instant::now() < deadline, "{}", session.screen());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(session.app.table().unwrap().loaded(), 2);

    session.code(KeyCode::Char('e'));
    session.assert_sees("All rows");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    for _ in 0..100 {
        session.code(KeyCode::Backspace);
    }
    session.write(&output.display().to_string());
    session.click_text("[Export]");
    while !output.exists() {
        session.app.drain_events();
        assert!(Instant::now() < deadline, "{}", session.screen());
        std::thread::sleep(Duration::from_millis(1));
    }
    let contents = fs::read_to_string(&output).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 5, "{contents}");
    assert!(lines[0].starts_with("name"), "{contents}");
    assert!(lines[4].starts_with('d'), "{contents}");
}

#[test]
fn a_table_on_screen_at_the_start_waits_for_a_key_and_has_no_empty_console() {
    let (store, _) = reopening_store("p-reopen-active", "{}");
    let mut first = Session::start(&store);
    common::open_table(&mut first.app, 0, "items");
    first.pump_sees("4242");
    first.quit();
    drop(first);

    let mut second = Session::start(&store);
    assert!(
        second.app.hint().starts_with("Enter load"),
        "{}",
        second.app.hint()
    );
    assert_eq!(
        second.app.table().map(|tab| tab.label()).as_deref(),
        Some("items (table)")
    );
    assert!(
        !second.top_row().contains("console-"),
        "{}",
        second.top_row()
    );
    second.assert_sees("Enter loads the rows.");
    second.assert_sees("Enter load  f where");
    for _ in 0..20 {
        second.app.drain_events();
    }
    assert_eq!(second.app.tables()[0].loaded(), 0);
    second.code(KeyCode::Enter);
    second.pump_sees("4242");
    second.assert_not_sees("Enter loads the rows.");
    second.assert_sees("Enter value");
}

#[test]
fn the_apply_button_also_loads_a_table_of_the_last_session() {
    let (store, _) = reopening_store("p-reopen-apply", "{}");
    let mut first = Session::start(&store);
    common::open_table(&mut first.app, 0, "items");
    first.pump_sees("4242");
    first.quit();
    drop(first);

    let mut second = Session::start(&store);
    second.prefix('k');
    second.code(KeyCode::Enter);
    second.pump_sees("4242");
}

#[test]
fn the_setting_off_leaves_the_tabs_closed_and_the_list_stays_for_later() {
    let (store, db) = reopening_store("p-reopen-off", r#"{"reopenTabs":false}"#);
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.quit();
    drop(first);

    let second = Session::start(&store);
    assert_eq!(second.names(), ["console-2"]);
    assert_eq!(second.text(), "");
    drop(second);

    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db)]);
    let third = Session::start(&store);
    assert_eq!(third.names(), ["console-1"]);
    assert_eq!(third.text(), "SELECT 1;");
}

#[test]
fn the_settings_choose_the_reopening_of_the_tabs() {
    let (store, _) = reopening_store("p-reopen-settings", "{}");
    let mut session = Session::start(&store);
    session.prefix(',');
    session.assert_sees("Reopen tabs");
    session.assert_sees("Default: On");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Right);
    session.assert_sees("< On >");
    session.code(KeyCode::Right);
    session.assert_sees("< Off >");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    session.assert_sees("settings are saved");
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"reopenTabs\": false"), "{text}");
}

#[test]
fn the_settings_work_with_the_mouse() {
    let (store, _) = two_connections("p-settings-mouse");
    let mut session = Session::start(&store);
    session.prefix(',');
    session.assert_sees("Settings");
    // A click on a choice changes it.
    session.click_text("< Default: On >");
    session.assert_sees("< On >");
    session.click_text("< On >");
    session.assert_sees("< Off >");
    // A click on Cancel closes the dialog without a write.
    session.click_text("[Cancel]");
    session.assert_not_sees("Settings");
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(!text.contains("reopenTabs"), "{text}");
}

#[test]
fn the_sidebar_shows_the_fields_of_one_section() {
    let (store, _) = two_connections("p-settings-sections");
    let mut session = Session::start(&store);
    session.prefix(',');
    session.assert_sees("Reopen tabs");
    session.assert_not_sees("Autocomplete");
    session.assert_not_sees("Page size");
    session.code(KeyCode::Down);
    session.assert_sees("Autocomplete");
    session.assert_not_sees("Reopen tabs");
    session.code(KeyCode::Down);
    session.assert_sees("Page size");
    session.assert_not_sees("Autocomplete");
    session.code(KeyCode::Down);
    session.assert_sees("Default theme");
    session.assert_not_sees("Page size");
    // The last section goes back to the first section.
    session.code(KeyCode::Down);
    session.assert_sees("Reopen tabs");
    session.code(KeyCode::Up);
    session.assert_sees("Default theme");
    // The scope stays in all sections.
    session.assert_sees("< Project >");
}

#[test]
fn the_tab_key_stays_in_the_open_section_and_goes_back_to_the_sidebar() {
    let (store, _) = two_connections("p-settings-tab");
    let mut session = Session::start(&store);
    session.prefix(',');
    // Scope, Keyboard, Reopen tabs, Query history, Save, Cancel, and the sidebar.
    for _ in 0..7 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Down);
    session.assert_sees("Autocomplete");
}

#[test]
fn the_vim_keys_change_the_section_in_the_sidebar() {
    let (store, _) = reopening_store("p-settings-vim", r#"{"keyboardMode":"vim"}"#);
    let mut session = Session::start(&store);
    session.prefix(',');
    session.code(KeyCode::Char('j'));
    session.assert_sees("Autocomplete");
    session.code(KeyCode::Char('k'));
    session.assert_sees("Reopen tabs");
}

impl Session {
    /// Opens the settings on the section of the themes, with the focus on the theme.
    /// The store has no project, so the scope is not a place of the focus.
    fn open_theme_settings(&mut self) {
        self.prefix(',');
        self.code(KeyCode::Up);
        self.code(KeyCode::Tab);
    }

    /// Returns the background of the explorer. The dialog does not cover this cell.
    fn panel_color(&mut self) -> Color {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal.backend().buffer()[(1, 20)].bg
    }
}

fn theme_panel(name: ThemeName) -> Color {
    Theme::named(name, ColorEnv::default()).panel.bg.unwrap()
}

#[test]
fn the_settings_show_a_theme_at_once_and_a_cancel_brings_back_the_saved_theme() {
    if ColorEnv::read().no_color {
        return;
    }
    let (store, _) = reopening_store("p-theme-preview", r#"{"theme":"gruvbox"}"#);
    let mut session = Session::start(&store);
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Gruvbox));
    session.open_theme_settings();
    session.assert_sees("< Gruvbox >");
    session.code(KeyCode::Right);
    session.assert_sees("< Nord >");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
    session.code(KeyCode::Esc);
    session.assert_not_sees("Default theme");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Gruvbox));
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("gruvbox"), "{text}");
}

#[test]
fn a_click_shows_a_theme_at_once_and_the_cancel_button_brings_back_the_saved_theme() {
    if ColorEnv::read().no_color {
        return;
    }
    let (store, _) = reopening_store("p-theme-mouse", r#"{"theme":"gruvbox"}"#);
    let mut session = Session::start(&store);
    session.open_theme_settings();
    session.click_text("< Gruvbox >");
    session.assert_sees("< Nord >");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
    session.click_text(" Cancel ");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Gruvbox));
}

#[test]
fn a_saved_theme_goes_to_the_file_and_comes_back_at_the_next_start() {
    if ColorEnv::read().no_color {
        return;
    }
    let (store, _) = reopening_store("p-theme-save", "{}");
    let mut session = Session::start(&store);
    session.open_theme_settings();
    session.assert_sees("Default: Auto");
    // `Left` goes from the first choice to the end of the list.
    session.code(KeyCode::Left);
    session.code(KeyCode::Left);
    session.code(KeyCode::Left);
    session.assert_sees("< Catppuccin Latte >");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    session.assert_sees("settings are saved");
    assert_eq!(
        session.panel_color(),
        theme_panel(ThemeName::CatppuccinLatte)
    );
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"theme\": \"catppuccin-latte\""), "{text}");
    drop(session);

    let mut session = Session::start(&store);
    assert_eq!(
        session.panel_color(),
        theme_panel(ThemeName::CatppuccinLatte)
    );
}

/// A character without a color takes the color of the terminal, and that color can be
/// invisible on the background of a theme.
#[test]
fn a_theme_gives_a_color_to_each_character_of_the_screen() {
    if ColorEnv::read().no_color {
        return;
    }
    let (store, _) = reopening_store("p-theme-cells", r#"{"theme":"catppuccin-latte"}"#);
    let mut session = Session::start(&store);
    session.open_theme_settings();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| session.app.draw(frame)).unwrap();
    for cell in terminal.backend().buffer().content() {
        if !cell.symbol().trim().is_empty() {
            assert!(matches!(cell.fg, Color::Rgb(..)), "{cell:?}");
            assert!(matches!(cell.bg, Color::Rgb(..)), "{cell:?}");
        }
    }
}

#[test]
fn the_theme_of_a_project_has_priority_over_a_change_of_the_global_theme() {
    use sqln::settings::{Field, SettingsForm};
    let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
    let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
    let global = Preferences::default();
    let project = Preferences {
        theme: Some(ThemeName::Gruvbox),
        ..Preferences::default()
    };
    let mut form = SettingsForm::new(global, Some(project));
    assert_eq!(form.saved_theme(), Some(ThemeName::Gruvbox));

    form.focus_on(Field::Theme);
    form.on_key(left, false);
    assert_eq!(form.saved_theme(), Some(ThemeName::CatppuccinLatte));

    // A change of the global scope does not change what a project shows.
    form.focus_on(Field::Scope);
    form.on_key(right, false);
    form.focus_on(Field::Theme);
    form.on_key(left, false);
    assert_eq!(form.saved_theme(), Some(ThemeName::Gruvbox));
}

fn themed_entry(id: &str, name: &str, file: &str, theme: &str) -> String {
    format!(r#"{{"id":"{id}","name":"{name}","kind":"sqlite","file":"{file}","theme":"{theme}"}}"#)
}

fn icon_entry(id: &str, name: &str, file: &str, icon: &str) -> String {
    format!(r#"{{"id":"{id}","name":"{name}","kind":"sqlite","file":"{file}","icon":"{icon}"}}"#)
}

/// Starts with one connection `prod` that has the skull icon, and returns the screen.
fn screen_with_icon(name: &str, preferences: &str) -> String {
    let (store, base) = global_only(name);
    let db = database(&base, "i", "").display().to_string();
    write_config(
        &store.global,
        preferences,
        &[icon_entry("g1", "prod", &db, "skull")],
    );
    Session::start(&store).screen()
}

#[test]
fn the_icon_of_a_connection_stands_before_its_name_and_the_engine_stays_after_it() {
    if ColorEnv::read().no_color {
        return;
    }
    let unicode = screen_with_icon("p-icon-unicode", r#"{"theme":"astrodark"}"#);
    assert!(unicode.contains("☠ prod  sq"), "{unicode}");
    assert!(!unicode.contains(Icons::unicode().database), "{unicode}");

    let logos = Icons::nerd().sqlite;
    let nerd = screen_with_icon("p-icon-nerd", r#"{"theme":"astrodark","icons":"nerd"}"#);
    assert!(nerd.contains(&format!("☠ prod  {logos}")), "{nerd}");

    let plain = screen_with_icon("p-icon-plain", "{}");
    assert!(plain.contains("☠ prod  sq"), "{plain}");
}

#[test]
fn a_connected_connection_changes_its_icon_and_keeps_its_transaction() {
    let store = themed_connections("p-icon-connected");
    let mut session = Session::start(&store);
    session.console_of("prod");
    session.open_transaction();

    session.go_to_connection("prod");
    session.code(KeyCode::Char('e'));
    session.focus_form(Field::Icon);
    session.assert_sees("< Default >");
    session.code(KeyCode::Right);
    session.assert_sees("< ⚠ Warning >");
    session.press_form_button(Field::Save);

    assert!(session.app.form().is_none(), "{}", session.screen());
    assert!(session.app.console().in_transaction());
    session.assert_sees("⚠ prod");
    let saved = config::load(&store).config.unwrap();
    assert_eq!(
        saved.connections[1].definition.icon,
        Some(ConnectionIcon::Warning)
    );
    assert_eq!(saved.connections[0].definition.icon, None);

    // A change of the file also keeps the transaction.
    let file = store.global.config_file();
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("\"warning\"", "\"flag\"")).unwrap();
    session.prefix('r');
    session.assert_sees("⚑ prod");
    assert!(session.app.console().in_transaction());
}

/// A store with a default theme and two connections: `dev`, and `prod` with its own theme.
fn themed_connections(name: &str) -> Store {
    let (store, base) = global_only(name);
    let db = database(&base, "t", "CREATE TABLE items (v INTEGER);")
        .display()
        .to_string();
    write_config(
        &store.global,
        r#"{"theme":"gruvbox"}"#,
        &[
            sqlite_entry("g1", "dev", &db),
            themed_entry("g2", "prod", &db, "nord"),
        ],
    );
    store
}

impl Session {
    fn console_of(&mut self, connection: &str) {
        self.go_to_connection(connection);
        self.code(KeyCode::Char('n'));
        self.code(KeyCode::Enter);
    }

    /// Runs a statement that leaves a transaction open in the active console.
    fn open_transaction(&mut self) {
        self.prefix('m');
        self.write("INSERT INTO items VALUES (1);");
        self.press(common::shift(KeyCode::F(5)));
        for _ in 0..2_000 {
            self.app.drain_events();
            if self.app.console().in_transaction() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the transaction did not start");
    }
}

#[test]
fn the_theme_follows_the_connection_of_the_open_tab() {
    if ColorEnv::read().no_color {
        return;
    }
    let store = themed_connections("p-theme-tabs");
    let mut session = Session::start(&store);
    session.console_of("dev");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Gruvbox));
    session.console_of("prod");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
    session.prefix('p');
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Gruvbox));
    session.prefix('n');
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
}

#[test]
fn a_table_tab_shows_the_theme_of_its_connection() {
    if ColorEnv::read().no_color {
        return;
    }
    let store = themed_connections("p-theme-table");
    let mut session = Session::start(&store);
    session.console_of("dev");
    session.go_to_connection("prod");
    session.code(KeyCode::Enter);
    common::open_table(&mut session.app, 1, "items");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
}

#[test]
fn the_form_shows_a_theme_at_once_and_the_file_keeps_the_saved_theme() {
    if ColorEnv::read().no_color {
        return;
    }
    let store = themed_connections("p-theme-form");
    let mut session = Session::start(&store);
    let before = fs::read_to_string(store.global.config_file()).unwrap();
    session.go_to_connection("dev");
    session.code(KeyCode::Char('e'));
    session.focus_form(Field::Theme);
    session.assert_sees("< Default >");
    session.code(KeyCode::Left);
    session.assert_sees("< Nord >");
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
    session.code(KeyCode::Esc);
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Gruvbox));
    let after = fs::read_to_string(store.global.config_file()).unwrap();
    assert_eq!(before, after);

    session.code(KeyCode::Char('e'));
    session.focus_form(Field::Theme);
    session.code(KeyCode::Left);
    session.code(KeyCode::Left);
    session.assert_sees("< Gruvbox >");
    session.press_form_button(Field::Save);
    assert!(session.app.form().is_none(), "{}", session.screen());
    let saved = config::load(&store).config.unwrap();
    assert_eq!(
        saved.connections[0].definition.theme,
        Some(ThemeName::Gruvbox)
    );
    assert_eq!(saved.connections[1].definition.theme, Some(ThemeName::Nord));
}

#[test]
fn a_connected_connection_changes_its_theme_and_keeps_its_transaction() {
    if ColorEnv::read().no_color {
        return;
    }
    let store = themed_connections("p-theme-connected");
    let mut session = Session::start(&store);
    session.console_of("prod");
    session.open_transaction();

    session.go_to_connection("prod");
    session.code(KeyCode::Char('e'));
    assert!(session.app.form().unwrap().display_only);
    session.focus_form(Field::Theme);
    session.assert_sees("< Nord >");
    session.code(KeyCode::Left);
    session.assert_sees("< Gruvbox >");
    session.press_form_button(Field::Save);

    assert!(session.app.form().is_none(), "{}", session.screen());
    assert!(session.app.console().in_transaction());
    let saved = config::load(&store).config.unwrap();
    assert_eq!(
        saved.connections[1].definition.theme,
        Some(ThemeName::Gruvbox)
    );
}

#[test]
fn a_reload_changes_the_theme_of_a_connection_in_use_and_keeps_its_transaction() {
    if ColorEnv::read().no_color {
        return;
    }
    let store = themed_connections("p-theme-reload");
    let mut session = Session::start(&store);
    session.console_of("prod");
    session.open_transaction();
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));

    let file = store.global.config_file();
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("nord", "catppuccin-latte")).unwrap();
    session.prefix('r');

    assert_eq!(
        session.panel_color(),
        theme_panel(ThemeName::CatppuccinLatte)
    );
    assert!(session.app.console().in_transaction());
}

#[test]
fn the_settings_show_the_default_theme_and_the_connection_keeps_its_own_theme() {
    if ColorEnv::read().no_color {
        return;
    }
    let store = themed_connections("p-theme-default");
    let mut session = Session::start(&store);
    session.console_of("prod");
    session.open_theme_settings();
    session.code(KeyCode::Left);
    session.assert_sees("< Catppuccin Latte >");
    assert_eq!(
        session.panel_color(),
        theme_panel(ThemeName::CatppuccinLatte)
    );
    // The connection keeps its own theme after the dialog.
    session.code(KeyCode::Esc);
    assert_eq!(session.panel_color(), theme_panel(ThemeName::Nord));
}

#[test]
fn a_save_writes_the_changes_of_all_sections() {
    let (store, _) = two_connections("p-settings-all");
    let project = store.project.clone().unwrap();
    let mut session = Session::start(&store);
    session.open_results_settings();
    session.write("77");
    // Back to the sidebar: Row cap, Data, Format JSON, Save, Cancel, and the sidebar.
    for _ in 0..6 {
        session.code(KeyCode::Tab);
    }
    session.code(KeyCode::Up);
    // Autocomplete, after the scope.
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Right);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    session.assert_sees("settings are saved");
    let text = fs::read_to_string(project.config_file()).unwrap();
    assert!(text.contains("\"pageSize\": 77"), "{text}");
    assert!(text.contains("\"suggestionKeys\": \"tab\""), "{text}");
}

#[test]
fn a_bad_value_of_a_closed_section_shows_the_note() {
    let (store, _) = two_connections("p-settings-closed");
    let mut session = Session::start(&store);
    session.open_results_settings();
    session.write("0");
    for _ in 0..6 {
        session.code(KeyCode::Tab);
    }
    // The sidebar opens the first section. Then the focus goes to Save.
    session.code(KeyCode::Down);
    session.assert_not_sees("Page size");
    session.code(KeyCode::BackTab);
    session.code(KeyCode::BackTab);
    session.code(KeyCode::Enter);
    session.assert_sees("Page size needs a number of 1 or more");
}

#[test]
fn a_click_on_the_sidebar_opens_the_section() {
    let (store, _) = two_connections("p-settings-click");
    let mut session = Session::start(&store);
    session.prefix(',');
    session.click_text("Editor ");
    session.assert_sees("Autocomplete");
    session.assert_not_sees("Reopen tabs");
    // The focus is on the sidebar, so `Down` opens the next section.
    session.code(KeyCode::Down);
    session.assert_sees("Page size");
}

#[test]
fn the_project_value_of_the_reopening_comes_before_the_global_value() {
    let (store, base) = store("p-reopen-scopes");
    let db = database(&base, "s", "").display().to_string();
    let project = store.project.clone().unwrap();
    let reopen = |store: &Store| sqln::store::config::load(store).config.unwrap().reopen_tabs;
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db)]);
    assert!(reopen(&store), "the default reopens the tabs");
    write_config(
        &store.global,
        r#"{"reopenTabs":false}"#,
        &[sqlite_entry("g1", "s", &db)],
    );
    assert!(!reopen(&store));
    write_config(&project, r#"{"reopenTabs":true}"#, &[]);
    assert!(reopen(&store));
}

#[test]
fn a_console_in_use_by_another_instance_stays_closed_and_keeps_its_place_in_the_list() {
    let (store, _) = reopening_store("p-reopen-lock", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.quit();
    drop(first);

    let holder = Session::start(&store);
    assert_eq!(holder.text(), "SELECT 1;");
    let mut other = Session::start(&store);
    assert_eq!(other.text(), "");
    other.assert_sees("console-1");
    other.assert_sees("is open in another");
    other.quit();
    drop(other);
    drop(holder);

    let last = Session::start(&store);
    assert_eq!(last.text(), "SELECT 1;");
}

#[test]
fn the_tabs_of_a_removed_connection_do_not_reopen() {
    let (store, base) = global_only("p-reopen-removed");
    let db = database(&base, "s", ITEMS).display().to_string();
    let other = database(&base, "t", "").display().to_string();
    write_config(
        &store.global,
        "{}",
        &[
            sqlite_entry("g1", "s", &db),
            sqlite_entry("g2", "t", &other),
        ],
    );
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    common::open_table(&mut first.app, 0, "items");
    first.pump_sees("4242");
    first.quit();
    drop(first);

    write_config(&store.global, "{}", &[sqlite_entry("g2", "t", &other)]);
    let second = Session::start(&store);
    assert_eq!(second.text(), "");
    assert!(second.app.tables().is_empty());
    assert_eq!(second.app.consoles().len(), 1);
}

#[test]
fn an_invalid_configuration_keeps_the_list_of_tabs_for_the_next_start() {
    let (store, db) = reopening_store("p-reopen-invalid", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.quit();
    drop(first);

    let good = fs::read_to_string(store.global.config_file()).unwrap();
    fs::write(store.global.config_file(), "{ not json").unwrap();
    let mut broken = Session::start(&store);
    assert_eq!(broken.text(), "");
    broken.app.shutdown();
    drop(broken);

    fs::write(store.global.config_file(), good).unwrap();
    let last = Session::start(&store);
    assert_eq!(last.text(), "SELECT 1;");
    assert!(!db.is_empty());
}

#[test]
fn a_signal_or_a_panic_also_keeps_the_list_of_tabs() {
    let (store, _) = reopening_store("p-reopen-signal", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.app.save_without_questions();
    drop(first);

    let second = Session::start(&store);
    assert_eq!(second.text(), "SELECT 1;");
}

#[test]
fn the_last_tab_ends_the_application_and_comes_back_at_the_next_start() {
    let (store, _) = reopening_store("p-reopen-last", "{}");
    let mut first = Session::start(&store);
    first.type_and_save("SELECT 1;");
    first.prefix('c');
    assert!(first.app.should_quit());
    first.app.shutdown();
    drop(first);

    let second = Session::start(&store);
    assert_eq!(second.names(), ["console-1"]);
    assert_eq!(second.text(), "SELECT 1;");
}

#[test]
fn a_workspace_file_without_a_list_of_tabs_starts_as_before() {
    let (store, _) = reopening_store("p-reopen-old", "{}");
    fs::write(
        store.global.workspace_file(),
        r#"{"version":1,"layout":{"navigatorCollapsed":true,"navigatorWidth":30,"editorRatio":0.5}}"#,
    )
    .unwrap();
    let mut session = Session::start(&store);
    session.assert_not_sees("cannot read workspace.json");
    assert_eq!(session.app.consoles().len(), 1);
    assert_eq!(session.text(), "");
    assert!(session.app.navigator().collapsed);
}

// ---- the keys of the suggestion list ----

const TYPED: &str = "SELECT * FROM it";
const ACCEPTED: &str = "SELECT * FROM items";

impl Session {
    /// Replaces the text with the start of a name, and waits for the list.
    fn open_suggestions(&mut self) {
        self.press(ctrl('a'));
        self.code(KeyCode::Delete);
        self.write(TYPED);
        self.press(ctrl(' '));
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.app.console().editor.suggestions().is_none() {
            self.app.drain_events();
            assert!(
                Instant::now() < deadline,
                "the list did not open:\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn by_default_tab_accepts_and_enter_breaks_the_line() {
    let (store, _) = reopening_store("p-keys-default", "{}");
    let mut session = Session::start(&store);
    session.open_suggestions();
    session.assert_sees("Tab accept");
    session.assert_sees("Enter new line");
    session.code(KeyCode::Tab);
    assert_eq!(session.text(), ACCEPTED);

    session.open_suggestions();
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), format!("{TYPED}\n"));
}

#[test]
fn the_enter_choice_accepts_with_enter_and_tab_keeps_its_own_effect() {
    let (store, _) = reopening_store("p-keys-enter", r#"{"suggestionKeys":"enter"}"#);
    let mut session = Session::start(&store);
    session.open_suggestions();
    session.assert_sees("Enter accept");
    session.assert_not_sees("Enter new line");
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), ACCEPTED);

    session.open_suggestions();
    session.code(KeyCode::Tab);
    assert!(session.text().starts_with(TYPED), "{}", session.text());
    assert_ne!(session.text(), ACCEPTED);
    assert!(session.app.console().editor.suggestions().is_none());
}

#[test]
fn the_choice_of_both_accepts_with_each_key() {
    let (store, _) = reopening_store("p-keys-both", r#"{"suggestionKeys":"both"}"#);
    let mut session = Session::start(&store);
    session.open_suggestions();
    session.assert_sees("Tab/Enter accept");
    session.code(KeyCode::Tab);
    assert_eq!(session.text(), ACCEPTED);
    session.open_suggestions();
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), ACCEPTED);
}

#[test]
fn a_key_with_a_modifier_does_not_accept_a_suggestion() {
    let (store, _) = reopening_store("p-keys-modifier", r#"{"suggestionKeys":"both"}"#);
    let mut session = Session::start(&store);
    session.open_suggestions();
    session.press(crossterm::event::KeyEvent::new(
        KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    assert_ne!(session.text(), ACCEPTED);
}

#[test]
fn the_vim_keyboard_uses_the_same_choice_of_keys() {
    let preferences = r#"{"keyboardMode":"vim","suggestionKeys":"enter"}"#;
    let (store, _) = reopening_store("p-keys-vim", preferences);
    let mut session = Session::start(&store);
    session.code(KeyCode::Char('i'));
    session.open_suggestions();
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), ACCEPTED);
}

#[test]
fn the_settings_choose_the_keys_of_the_list_and_the_change_works_at_once() {
    let (store, _) = reopening_store("p-keys-settings", "{}");
    let mut session = Session::start(&store);
    session.prefix(',');
    session.code(KeyCode::Down);
    session.assert_sees("Autocomplete");
    session.assert_sees("Default: Tab");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Right);
    session.assert_sees("< Tab >");
    session.code(KeyCode::Right);
    session.assert_sees("< Enter >");
    session.code(KeyCode::Right);
    session.assert_sees("< Tab or Enter >");
    session.code(KeyCode::Tab);
    session.code(KeyCode::Enter);
    session.assert_sees("settings are saved");
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"suggestionKeys\": \"both\""), "{text}");

    session.open_suggestions();
    session.code(KeyCode::Enter);
    assert_eq!(session.text(), ACCEPTED);
}

#[test]
fn the_project_choice_of_keys_comes_before_the_global_choice() {
    let (store, base) = store("p-keys-scopes");
    let db = database(&base, "s", "").display().to_string();
    let project = store.project.clone().unwrap();
    let keys = |store: &Store| {
        sqln::store::config::load(store)
            .config
            .unwrap()
            .suggestion_keys
    };
    write_config(&store.global, "{}", &[sqlite_entry("g1", "s", &db)]);
    assert_eq!(keys(&store), SuggestionKeys::Tab);
    write_config(
        &store.global,
        r#"{"suggestionKeys":"enter"}"#,
        &[sqlite_entry("g1", "s", &db)],
    );
    assert_eq!(keys(&store), SuggestionKeys::Enter);
    write_config(&project, r#"{"suggestionKeys":"both"}"#, &[]);
    assert_eq!(keys(&store), SuggestionKeys::Both);
}

#[test]
fn an_unknown_choice_of_keys_is_a_configuration_error() {
    let (store, _) = reopening_store("p-keys-unknown", r#"{"suggestionKeys":"space"}"#);
    assert!(sqln::store::config::load(&store).config.is_err());
}

#[test]
fn the_export_choices_stay_in_the_global_file_only() {
    let (store, _) = two_connections("p-export-global");
    let project = fs::read_to_string(store.project.as_ref().unwrap().config_file()).unwrap();
    let choices = ExportChoices {
        format: ExportFormat::Json,
        delimiter: Delimiter::Semicolon,
        batch: 50,
        folder: Some(PathBuf::from("/data/out")),
        ..ExportChoices::default()
    };
    config::change_export(&store.global, choices.clone()).unwrap();
    assert_eq!(
        fs::read_to_string(store.project.as_ref().unwrap().config_file()).unwrap(),
        project
    );
    let text = fs::read_to_string(store.global.config_file()).unwrap();
    assert!(text.contains("\"export\""), "{text}");
    assert!(text.contains("first"), "the connections stay: {text}");
    assert_eq!(config::load(&store).config.unwrap().export, choices);
}

#[test]
fn a_file_without_export_choices_gives_the_defaults() {
    let (store, _) = two_connections("p-export-default");
    assert_eq!(
        config::load(&store).config.unwrap().export,
        ExportChoices::default()
    );
}

#[test]
fn export_in_a_project_file_is_a_configuration_error() {
    let (store, _) = two_connections("p-export-project");
    write_config(
        store.project.as_ref().unwrap(),
        r#"{"export":{"format":"tsv"}}"#,
        &[],
    );
    let error = config::load(&store).config.unwrap_err().to_string();
    assert!(error.contains("export"), "{error}");
}

#[test]
fn a_batch_of_zero_is_a_configuration_error() {
    let (store, _) = two_connections("p-export-batch");
    write_config(&store.global, r#"{"export":{"batch":0}}"#, &[]);
    let error = config::load(&store).config.unwrap_err().to_string();
    assert!(error.contains("batch"), "{error}");
}

#[test]
fn the_settings_keep_the_export_choices() {
    let choices = ExportChoices {
        format: ExportFormat::Tsv,
        ..ExportChoices::default()
    };
    let preferences = Preferences {
        export: Some(choices.clone()),
        ..Preferences::default()
    };
    let form = SettingsForm::new(preferences, None);
    assert_eq!(form.preferences().unwrap().export, Some(choices));
}

#[test]
fn an_export_saves_its_choices_without_the_change_question() {
    let (store, base) = two_connections("p-export-session");
    let output = base.join("files").join("out.csv");
    let mut session = Session::start(&store);
    session.write("SELECT 1 AS a;");
    session.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    let deadline = Instant::now() + Duration::from_secs(20);
    while session.app.console_mut().is_busy() || session.app.console_mut().results.is_empty() {
        session.app.drain_events();
        assert!(Instant::now() < deadline, "{}", session.screen());
        std::thread::sleep(Duration::from_millis(1));
    }
    session.prefix('j');
    session.code(KeyCode::Char('e'));
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    for _ in 0..100 {
        session.code(KeyCode::Backspace);
    }
    session.write(&output.display().to_string());
    session.code(KeyCode::Tab);
    session.code(KeyCode::Tab);
    session.code(KeyCode::Right);
    session.click_text("[Export]");
    while session.app.console_mut().is_busy() || !output.exists() {
        session.app.drain_events();
        assert!(Instant::now() < deadline, "{}", session.screen());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fs::read_to_string(&output).unwrap(), "a\n1\n");
    let saved = config::load(&store).config.unwrap().export;
    assert_eq!(saved.delimiter, Delimiter::Semicolon);
    assert_eq!(saved.folder.as_deref(), output.parent());

    session.now += Duration::from_secs(10);
    session.app.tick_at(session.now);
    session.assert_not_sees("Reload");
}
