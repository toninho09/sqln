// The query history: what it saves, its limits, and its window.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, temp_dir};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::settings::{Field, SettingsForm};
use sqln::sql::{Dialect, is_secret};
use sqln::store::config::Preferences;
use sqln::store::history::{self, Entry, LOAD_LIMIT, STATEMENT_LIMIT};
use sqln::store::{Root, Store};
use sqln::ui::theme::Theme;

const CONNECTION: &str = "g1";

struct Session {
    app: App,
    global: Root,
}

impl Session {
    /// Opens a saved SQLite connection, with `preferences` in its configuration.
    fn new(name: &str, preferences: &str) -> Self {
        let base = temp_dir(name);
        let global = Root::global(base.join("global"));
        fs::create_dir_all(&global.dir).unwrap();
        let database = base.join("data.db");
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute_batch("CREATE TABLE t (a INT); INSERT INTO t VALUES (1);")
            .unwrap();
        let text = format!(
            r#"{{"version":1,"preferences":{preferences},"connections":[{{"id":"{CONNECTION}","name":"one","kind":"sqlite","file":"{}"}}]}}"#,
            database.display()
        );
        fs::write(global.config_file(), text).unwrap();
        let app = open(global.clone());
        Self { app, global }
    }

    fn write(&mut self, sql: &str) {
        self.app.on_event(Event::Paste(sql.to_owned()));
    }

    fn press(&mut self, code: KeyCode) {
        self.send(key(code));
    }

    fn send(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn prefix(&mut self, c: char) {
        self.send(ctrl('w'));
        self.press(KeyCode::Char(c));
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        self.app.on_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Clears the editor and opens the window of the history.
    fn open_window(&mut self) {
        self.send(ctrl('a'));
        self.press(KeyCode::Backspace);
        self.prefix('H');
    }

    fn idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.console().is_busy() {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the run did not end");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
    }

    /// Replaces the text of the editor and runs it.
    fn run(&mut self, sql: &str) {
        self.send(ctrl('a'));
        self.write(sql);
        self.press(KeyCode::F(5));
        self.idle();
    }

    fn entries(&self) -> Vec<Entry> {
        history::load(&self.global, CONNECTION, LOAD_LIMIT)
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal.backend().buffer().content().chunks(120);
        rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn open(global: Root) -> App {
    let options = Options {
        store: Some(Store {
            global,
            project: None,
        }),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::plain())
}

fn entry(connection: &str, sql: &str) -> Entry {
    Entry {
        at: 1,
        connection: connection.to_owned(),
        ok: true,
        sql: sql.to_owned(),
    }
}

fn write_entries(root: &Root, entries: &[Entry]) {
    let mut lines = Vec::new();
    for item in entries {
        history::push_line(&mut lines, item);
    }
    history::append(root, &lines).unwrap();
}

fn empty_root(name: &str) -> Root {
    Root::global(temp_dir(name).join("global"))
}

#[test]
fn a_word_of_a_secret_keeps_a_statement_out() {
    for sql in [
        "ALTER USER a WITH PASSWORD 'x'",
        "alter user a identified by 'x'",
        "SET PASSWORD = 'x'",
        "SELECT passwd FROM t",
    ] {
        assert!(is_secret(sql, Dialect::Postgres), "{sql}");
    }
    assert!(!is_secret("SELECT 1", Dialect::Postgres));
    assert!(!is_secret("SELECT 'password'", Dialect::Postgres));
}

#[test]
fn the_commands_of_redis_with_a_secret_stay_out() {
    for sql in [
        "AUTH secret",
        "auth user secret",
        "ACL SETUSER a on >secret",
        "HELLO 3 AUTH user secret",
        "CONFIG SET requirepass secret",
        "CONFIG SET masterauth secret",
    ] {
        assert!(is_secret(sql, Dialect::Redis), "{sql}");
    }
    assert!(!is_secret("GET key", Dialect::Redis));
    assert!(!is_secret("SELECT AUTH", Dialect::Postgres));
}

#[test]
fn load_returns_one_connection_with_the_newest_first() {
    let root = empty_root("history-load");
    write_entries(
        &root,
        &[entry("a", "one"), entry("b", "other"), entry("a", "two")],
    );
    let sql: Vec<String> = history::load(&root, "a", 10)
        .into_iter()
        .map(|item| item.sql)
        .collect();
    assert_eq!(sql, ["two", "one"]);
    assert_eq!(history::load(&root, "a", 1)[0].sql, "two");
}

#[test]
fn load_skips_a_line_that_does_not_read() {
    let root = empty_root("history-bad-line");
    write_entries(&root, &[entry("a", "one")]);
    history::append(&root, b"{\"at\":1,\"conn").unwrap();
    assert_eq!(history::load(&root, "a", 10).len(), 1);
}

#[test]
fn load_of_a_missing_file_is_empty() {
    assert_eq!(history::load(&empty_root("history-none"), "a", 10), []);
}

#[cfg(unix)]
#[test]
fn only_the_owner_reads_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let root = empty_root("history-mode");
    write_entries(&root, &[entry("a", "one")]);
    let mode = fs::metadata(root.history_file())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn trim_keeps_the_newest_whole_lines_of_a_large_file() {
    let root = empty_root("history-trim");
    let sql = "x".repeat(1024);
    let entries: Vec<Entry> = (0..9_000).map(|_| entry("a", &sql)).collect();
    write_entries(&root, &entries);
    write_entries(&root, &[entry("a", "newest")]);
    let before = fs::metadata(root.history_file()).unwrap().len();
    assert!(before > 8 * 1024 * 1024);

    history::trim(&root).unwrap();

    let after = fs::metadata(root.history_file()).unwrap().len();
    assert!(after <= 4 * 1024 * 1024, "{after}");
    let text = fs::read_to_string(root.history_file()).unwrap();
    assert!(text.starts_with('{'), "the first line is whole");
    assert_eq!(history::load(&root, "a", 1)[0].sql, "newest");
}

#[test]
fn trim_leaves_a_small_file() {
    let root = empty_root("history-small");
    write_entries(&root, &[entry("a", "one")]);
    let before = fs::read(root.history_file()).unwrap();
    history::trim(&root).unwrap();
    assert_eq!(fs::read(root.history_file()).unwrap(), before);
}

#[test]
fn a_plan_stays_out_of_the_history() {
    let mut session = Session::new("history-plan", "{}");
    session.write("SELECT a FROM t");
    session.send(ctrl('w'));
    session.send(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT));
    session.idle();
    assert!(session.app.console().results[0].plan);
    // The next run proves that the history works and holds only that statement.
    session.run("SELECT 1");
    let entries = session.entries();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].sql, "SELECT 1");
}

#[test]
fn a_run_notes_each_statement_with_its_result() {
    let mut session = Session::new("history-run", "{}");
    session.send(ctrl('a'));
    session.write("SELECT 1; SELECT * FROM missing; SELECT 2;");
    session
        .app
        .on_event(Event::Key(crate::common::shift(KeyCode::F(5))));
    session.idle();
    let entries = session.entries();
    assert_eq!(entries.len(), 2, "the third statement did not run");
    assert_eq!(entries[0].sql, "SELECT * FROM missing");
    assert!(!entries[0].ok);
    assert_eq!(entries[1].sql, "SELECT 1");
    assert!(entries[1].ok);
    assert!(entries[1].at > 0);
}

#[test]
fn a_repetition_stays_out_and_a_new_text_goes_in() {
    let mut session = Session::new("history-repeat", "{}");
    session.run("SELECT 1");
    session.run("SELECT 1");
    session.run("SELECT 2");
    session.run("SELECT 1");
    assert_eq!(session.entries().len(), 3);
}

#[test]
fn a_statement_with_a_secret_stays_out() {
    let mut session = Session::new("history-secret", "{}");
    session.run("SELECT password FROM t");
    assert_eq!(session.entries(), []);
}

#[test]
fn a_long_statement_stays_out() {
    let mut session = Session::new("history-long", "{}");
    let sql = format!("SELECT '{}'", "x".repeat(STATEMENT_LIMIT));
    session.run(&sql);
    assert_eq!(session.entries(), []);
}

#[test]
fn the_setting_stops_the_notes_and_the_window() {
    let mut session = Session::new("history-off", r#"{"queryHistory":false}"#);
    session.run("SELECT 1");
    assert_eq!(session.entries(), []);
    session.prefix('H');
    assert!(!session.screen().contains("Query history"));
}

#[test]
fn the_settings_stop_the_notes_at_once() {
    let mut session = Session::new("history-live", "{}");
    session.run("SELECT 1");
    session.prefix(',');
    // Keyboard, Reopen tabs, and Query history. The store has no project.
    for _ in 0..3 {
        session.press(KeyCode::Tab);
    }
    session.press(KeyCode::Right);
    session.press(KeyCode::Right);
    // Bell seconds, then Save.
    session.press(KeyCode::Tab);
    session.press(KeyCode::Tab);
    session.press(KeyCode::Enter);
    let text = fs::read_to_string(session.global.config_file()).unwrap();
    assert!(text.contains("\"queryHistory\": false"), "{text}");
    session.run("SELECT 2");
    assert_eq!(session.entries().len(), 1);
}

#[test]
fn a_connection_without_a_saved_definition_leaves_no_notes() {
    let base = temp_dir("history-session");
    let global = Root::global(base.join("global"));
    let definition = Definition::sqlite(SqliteTarget::new(base.join("data.db")));
    let options = Options {
        connections: vec![definition],
        session_target: true,
        store: Some(Store {
            global: global.clone(),
            project: None,
        }),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut session = Session {
        app: App::new(options, clipboard, Theme::plain()),
        global: global.clone(),
    };
    fs::write(base.join("data.db"), "").unwrap();
    session.run("SELECT 1");
    assert!(!global.history_file().exists());
    session.prefix('H');
    assert!(!session.screen().contains("Query history"));
}

#[test]
fn a_table_tab_leaves_no_notes() {
    let mut session = Session::new("history-table", "{}");
    session.run("SELECT 1");
    let before = session.entries().len();
    crate::common::open_table(&mut session.app, 0, "t");
    session.idle();
    assert_eq!(session.entries().len(), before);
}

#[test]
fn the_window_lists_the_connection_with_the_newest_first() {
    let mut session = Session::new("history-window", "{}");
    session.run("SELECT 1");
    session.run("SELECT 2");
    session.prefix('H');
    let screen = session.screen();
    let first = screen.find("SELECT 2").unwrap();
    assert!(screen.contains("Query history"), "{screen}");
    assert!(first < screen.rfind("SELECT 1").unwrap(), "{screen}");
}

#[test]
fn an_empty_history_opens_no_window() {
    let mut session = Session::new("history-empty", "{}");
    session.prefix('H');
    assert!(session.screen().contains("This connection has no history."));
}

#[test]
fn enter_inserts_the_statement_at_the_cursor_and_does_not_run_it() {
    let mut session = Session::new("history-enter", "{}");
    session.run("SELECT 1");
    session.send(ctrl('a'));
    session.write("-- note\n");
    session.prefix('H');
    session.press(KeyCode::Enter);
    session.idle();
    assert_eq!(session.app.console().editor.text(), "-- note\nSELECT 1");
    assert_eq!(session.entries().len(), 1);
    assert!(!session.screen().contains("Query history"));
}

#[test]
fn the_filter_keeps_the_rows_with_each_word() {
    let mut session = Session::new("history-filter", "{}");
    session.run("SELECT a FROM t");
    session.run("SELECT 2");
    session.send(ctrl('a'));
    session.press(KeyCode::Backspace);
    session.prefix('H');
    session.write("FROM SELECT");
    let screen = session.screen();
    assert!(screen.contains("SELECT a FROM t"), "{screen}");
    assert!(!screen.contains("SELECT 2"), "{screen}");
    session.press(KeyCode::Enter);
    assert_eq!(session.app.console().editor.text(), "SELECT a FROM t");
}

#[test]
fn a_click_on_a_row_inserts_its_statement_and_does_not_run_it() {
    let mut session = Session::new("history-click", "{}");
    session.run("SELECT 1");
    session.run("SELECT 2");
    session.open_window();
    let screen = session.screen();
    let (y, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("SELECT 1"))
        .unwrap();
    let column = line[..line.find("SELECT 1").unwrap()].chars().count();
    let left = MouseEventKind::Down(MouseButton::Left);

    session.mouse(left, 0, 0);
    assert!(session.screen().contains("Query history"), "outside");
    session.mouse(left, column as u16, y as u16);
    session.idle();
    assert!(!session.screen().contains("Query history"));
    assert_eq!(session.app.console().editor.text(), "SELECT 1");
    assert_eq!(session.entries().len(), 2);
}

#[test]
fn the_wheel_moves_the_cursor_of_the_window() {
    let mut session = Session::new("history-wheel", "{}");
    session.run("SELECT 1");
    session.run("SELECT 2");
    session.open_window();
    session.mouse(MouseEventKind::ScrollDown, 0, 0);
    session.press(KeyCode::Enter);
    assert_eq!(session.app.console().editor.text(), "SELECT 1");

    session.open_window();
    session.mouse(MouseEventKind::ScrollDown, 0, 0);
    session.mouse(MouseEventKind::ScrollUp, 0, 0);
    session.press(KeyCode::Enter);
    assert_eq!(session.app.console().editor.text(), "SELECT 2");
}

#[test]
fn escape_closes_the_window_and_the_text_stays() {
    let mut session = Session::new("history-escape", "{}");
    session.run("SELECT 1");
    session.prefix('H');
    session.press(KeyCode::Esc);
    assert!(!session.screen().contains("Query history"));
    assert_eq!(session.app.console().editor.text(), "SELECT 1");
}

#[test]
fn the_file_of_a_second_session_keeps_the_old_entries() {
    let mut session = Session::new("history-reopen", "{}");
    session.run("SELECT 1");
    let global = session.global.clone();
    session.app.shutdown();
    let mut next = Session {
        app: open(global.clone()),
        global,
    };
    next.prefix('H');
    assert!(next.screen().contains("SELECT 1"));
}

#[test]
fn the_settings_show_the_field_in_the_first_section() {
    let form = SettingsForm::new(Preferences::default(), None);
    let section = &sqln::settings::SECTIONS[0];
    assert!(section.fields.contains(&Field::QueryHistory));
    assert_eq!(form.query_history(), None);
    assert!(form.inherited(Field::QueryHistory).contains("On"));
}

#[test]
fn a_path_for_the_history_stays_in_the_global_folder() {
    let root = Root::global(PathBuf::from("/tmp/sqln"));
    assert_eq!(
        root.history_file(),
        PathBuf::from("/tmp/sqln/history.jsonl")
    );
}

#[test]
fn the_setting_opens_a_json_value_in_its_original_form() {
    let mut session = Session::new("json-original", r#"{"formatJson":false}"#);
    session.run(r#"SELECT '{"b":1}' AS doc"#);
    session.prefix('j');
    session.press(KeyCode::Enter);
    let screen = session.screen();
    assert!(screen.contains(r#"{"b":1}"#), "{screen}");
    assert!(screen.contains("JSON, original"), "{screen}");
    session.press(KeyCode::Char('f'));
    let screen = session.screen();
    assert!(screen.contains("\"b\": 1"), "{screen}");
}

#[test]
fn a_project_value_of_the_json_setting_wins_over_the_global_value() {
    let preferences = |text: &str| serde_json::from_str::<Preferences>(text).unwrap();
    let form = SettingsForm::new(
        preferences(r#"{"formatJson":false}"#),
        Some(preferences("{}")),
    );
    assert_eq!(form.format_json(), None);
    assert!(form.inherited(Field::FormatJson).contains("Off"));
    let form = SettingsForm::new(
        preferences(r#"{"formatJson":false}"#),
        Some(preferences(r#"{"formatJson":true}"#)),
    );
    assert_eq!(form.format_json(), Some(true));
    assert_eq!(
        form.preferences().unwrap().format_json,
        Some(true),
        "the save keeps the value"
    );
}
