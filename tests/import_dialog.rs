// The import dialog in the application: the keys, the sections, and the end of a run.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, select_row, shift, temp_db, temp_dir};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::navigator::RowKind;
use sqln::ui::theme::Theme;

const SETUP: &str = "\
    CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, photo BLOB);\
    CREATE VIEW names AS SELECT name FROM people;\
    INSERT INTO people (id, name) VALUES (100, 'first');";

struct Fixture {
    app: App,
    database: PathBuf,
    folder: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::with(name, |_| {})
    }

    fn with(name: &str, change: impl FnOnce(&mut Definition)) -> Self {
        let database = temp_db(name);
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute_batch(SETUP)
            .unwrap();
        let mut definition = Definition::sqlite(SqliteTarget::new(&database));
        change(&mut definition);
        let options = Options {
            connections: vec![definition],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
            database,
            folder: temp_dir(name),
        }
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn write(&mut self, text: &str) {
        for c in text.chars() {
            self.code(KeyCode::Char(c));
        }
    }

    fn screen_at(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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

    fn screen(&mut self) -> String {
        self.screen_at(120, 40)
    }

    fn sees(&mut self, text: &str) -> bool {
        self.screen().contains(text)
    }

    #[track_caller]
    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if self.sees(text) {
                return;
            }
            let screen = self.screen();
            assert!(
                Instant::now() < deadline,
                "{text:?} did not appear:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if self.app.table().is_some_and(|tab| !tab.is_busy()) {
                return;
            }
            assert!(Instant::now() < deadline, "the table is busy");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Opens a table with its first page.
    fn open(&mut self, name: &str) {
        crate::common::open_table(&mut self.app, 0, name);
        self.wait_idle();
    }

    fn import_key(&mut self) {
        self.press(shift(KeyCode::Char('I')));
    }

    /// Opens the dialog from the grid of the table `people`.
    fn open_dialog(&mut self) {
        self.open("people");
        self.press(ctrl('w'));
        self.code(KeyCode::Char('j'));
        self.import_key();
        self.wait_for("Import to people");
    }

    fn file(&self, name: &str, text: &[u8]) -> PathBuf {
        let path = self.folder.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Writes the path, and waits for the read of the file.
    fn choose(&mut self, path: &Path) {
        self.write(&path.display().to_string());
        self.code(KeyCode::Tab);
        self.wait_for("file columns.");
    }

    fn click_text(&mut self, text: &str) {
        let screen = self.screen();
        let (y, row) = screen
            .lines()
            .enumerate()
            .find(|(_, row)| row.contains(text))
            .unwrap_or_else(|| panic!("{text:?} is not on the screen:\n{screen}"));
        let byte = row.find(text).unwrap();
        self.app.on_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row[..byte].chars().count() as u16,
            row: y as u16,
            modifiers: KeyModifiers::NONE,
        }));
    }

    fn query(&self, sql: &str) -> Vec<String> {
        let connection = rusqlite::Connection::open(&self.database).unwrap();
        let mut statement = connection.prepare(sql).unwrap();
        let rows = statement.query_map([], |row| row.get::<_, String>(0));
        rows.unwrap().map(Result::unwrap).collect()
    }

    fn count(&self) -> String {
        self.query("SELECT CAST(count(*) AS TEXT) FROM people")
            .remove(0)
    }
}

#[test]
fn the_key_opens_the_dialog_from_the_grid_with_the_table_columns() {
    let mut f = Fixture::new("dialog-grid");
    f.open_dialog();
    f.click_text("Columns");
    let screen = f.screen();
    for text in ["id", "name", "age", "photo", "INTEGER", "BLOB"] {
        assert!(screen.contains(text), "{text}:\n{screen}");
    }
    assert_eq!(f.count(), "1");
}

#[test]
fn the_key_opens_the_table_and_the_dialog_from_the_navigator() {
    let mut f = Fixture::new("dialog-navigator");
    select_row(&mut f.app, 0, &[RowKind::Table], "people");
    assert!(crate::common::explorer_page(&f.app).contains(&"I Import a file".to_owned()));
    f.import_key();
    f.wait_for("Import to people");
    assert_eq!(f.app.table().unwrap().relation.relation, "people");
}

#[test]
fn a_key_on_a_table_that_is_open_opens_its_dialog() {
    let mut f = Fixture::new("dialog-open-table");
    f.open("people");
    select_row(&mut f.app, 0, &[RowKind::Table], "people");
    f.import_key();
    f.wait_for("Import to people");
}

#[test]
fn a_view_has_no_import() {
    let mut f = Fixture::new("dialog-view");
    select_row(&mut f.app, 0, &[RowKind::View], "names");
    assert!(!crate::common::explorer_page(&f.app).contains(&"I Import a file".to_owned()));
    f.import_key();
    f.app.drain_events();
    assert!(!f.sees("Import to"));
    f.open("names");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('j'));
    f.import_key();
    f.app.drain_events();
    assert!(!f.sees("Import to"));
}

#[test]
fn a_read_only_connection_has_no_import() {
    let mut f = Fixture::with("dialog-read-only", |item| item.read_only = true);
    f.open("people");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('j'));
    f.import_key();
    f.app.drain_events();
    assert!(!f.sees("Import to"));
}

#[test]
fn a_console_grid_has_no_import() {
    let mut f = Fixture::new("dialog-console");
    f.import_key();
    assert!(!f.sees("Import to"));
}

#[test]
fn the_dialog_reads_the_file_and_finds_the_mapping() {
    let mut f = Fixture::new("dialog-read");
    f.open_dialog();
    let path = f.file("people.csv", b"ID;Name;other\n1;Ana;x\n2;Rui;y\n");
    f.choose(&path);
    f.wait_for("3 file columns. 2 of 4 table columns get values.");
    f.click_text("Preview");
    let screen = f.screen();
    assert!(screen.contains("Ana"), "{screen}");
    assert!(screen.contains("Rui"), "{screen}");
    assert!(!screen.contains(" x "), "{screen}");
}

#[test]
fn a_missing_file_shows_the_reason_and_stops_the_button() {
    let mut f = Fixture::new("dialog-missing");
    f.open_dialog();
    let path = f.folder.join("none.csv");
    f.choose(&path);
    f.click_text("[Import]");
    assert!(f.sees("Import to people"));
    assert!(f.sees("No such file"), "{}", f.screen());
    assert_eq!(f.count(), "1");
}

#[test]
fn the_button_needs_a_path() {
    let mut f = Fixture::new("dialog-no-path");
    f.open_dialog();
    f.click_text("[Import]");
    f.wait_for("Write the path of the file.");
}

#[test]
fn the_button_needs_a_mapped_column() {
    let mut f = Fixture::new("dialog-no-mapping");
    f.open_dialog();
    let path = f.file("other.csv", b"x,y\n1,2\n");
    f.choose(&path);
    f.click_text("[Import]");
    f.wait_for("Choose a file column for one table column or more.");
}

#[test]
fn the_button_imports_the_file_and_reloads_the_tab() {
    let mut f = Fixture::new("dialog-import");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name,age\n1,Ana,30\n2,Rui,\n");
    f.choose(&path);
    f.click_text("[Import]");
    f.wait_for("Import complete");
    assert!(f.sees("Rows inserted: 2."), "{}", f.screen());
    f.code(KeyCode::Enter);
    f.wait_idle();
    assert_eq!(f.count(), "3");
    assert_eq!(f.app.table().unwrap().result.table.rows.len(), 3);
    assert_eq!(
        f.query("SELECT CAST(age AS TEXT) FROM people WHERE id = 1"),
        ["30"]
    );
}

#[test]
fn the_list_changes_the_file_column_of_a_table_column() {
    let mut f = Fixture::new("dialog-mapping");
    f.open_dialog();
    let path = f.file("people.csv", b"id,nome\n1,Ana\n");
    f.choose(&path);
    f.wait_for("2 file columns. 1 of 4 table columns get values.");
    f.click_text("Columns");
    f.code(KeyCode::Tab);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.code(KeyCode::Right);
    let screen = f.screen();
    assert!(screen.contains("< nome >"), "{screen}");
    f.click_text("[Import]");
    f.wait_for("Import complete");
    assert_eq!(f.query("SELECT name FROM people WHERE id = 1"), ["Ana"]);
}

#[test]
fn a_click_on_a_row_of_the_list_selects_it() {
    let mut f = Fixture::new("dialog-click-list");
    f.open_dialog();
    let path = f.file("people.csv", b"id,nome\n1,Ana\n");
    f.choose(&path);
    f.click_text("Columns");
    f.click_text("TEXT");
    f.code(KeyCode::Right);
    f.code(KeyCode::Right);
    assert!(f.sees("< nome >"), "{}", f.screen());
}

#[test]
fn without_a_header_the_mapping_follows_the_position() {
    let mut f = Fixture::new("dialog-position");
    f.open_dialog();
    let path = f.file("people.csv", b"1,Ana\n2,Rui\n");
    f.choose(&path);
    f.click_text("Format");
    f.code(KeyCode::Tab);
    f.code(KeyCode::Tab);
    f.code(KeyCode::Right);
    f.wait_for_section("File", "2 file columns. 2 of 4 table columns get values.");
    f.click_text("[Import]");
    f.wait_for("Import complete");
    assert_eq!(f.count(), "3");
}

impl Fixture {
    /// Opens a section and waits for a text.
    fn wait_for_section(&mut self, section: &str, text: &str) {
        self.click_text(section);
        self.wait_for(text);
    }
}

#[test]
fn a_failure_undoes_the_rows_and_names_the_reason() {
    let mut f = Fixture::new("dialog-failure");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n1,Rui\n");
    f.choose(&path);
    f.click_text("[Import]");
    f.wait_for("Import not complete");
    let screen = f.screen();
    assert!(screen.contains("Rows inserted: 0."), "{screen}");
    assert!(screen.contains("lines 2 to 3"), "{screen}");
    assert_eq!(f.count(), "1");
}

#[test]
fn log_errors_writes_the_log_and_shows_its_path() {
    let mut f = Fixture::new("dialog-log");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n1,again\n2,Rui\n");
    f.choose(&path);
    f.click_text("Options");
    f.code(KeyCode::Tab);
    f.code(KeyCode::Right);
    f.code(KeyCode::Right);
    f.wait_for("Log file");
    f.click_text("[Import]");
    f.wait_for("Import complete");
    let screen = f.screen();
    assert!(screen.contains("Rows inserted: 2."), "{screen}");
    assert!(screen.contains("Rows with errors: 1."), "{screen}");
    assert!(screen.contains("-errors-"), "{screen}");
    assert_eq!(f.count(), "3");
    let logs: Vec<_> = std::fs::read_dir(&f.folder)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect();
    assert_eq!(logs.len(), 1);
    let log = std::fs::read_to_string(&logs[0]).unwrap();
    assert!(log.contains("Line 3:"), "{log}");
}

#[test]
fn the_mode_starts_as_all_or_nothing_in_each_dialog() {
    let mut f = Fixture::new("dialog-mode");
    f.open_dialog();
    f.click_text("Options");
    assert!(f.sees("All or nothing"), "{}", f.screen());
    f.code(KeyCode::Tab);
    f.code(KeyCode::Right);
    assert!(f.sees("By batch"), "{}", f.screen());
    f.code(KeyCode::Esc);
    f.import_key();
    f.wait_for("Import to people");
    f.click_text("Options");
    assert!(f.sees("All or nothing"), "{}", f.screen());
}

#[test]
fn production_asks_and_cancel_returns_to_the_dialog() {
    let mut f = Fixture::with("dialog-production", |item| item.production = true);
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n");
    f.choose(&path);
    f.click_text("[Import]");
    f.wait_for("Import to production");
    assert!(f.sees("people.csv"), "{}", f.screen());
    assert_eq!(f.count(), "1");
    f.code(KeyCode::Enter);
    assert!(f.sees("Import to people"), "{}", f.screen());
    assert_eq!(f.count(), "1");
    f.click_text("[Import]");
    f.wait_for("Import to production");
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.wait_for("Import complete");
    assert_eq!(f.count(), "2");
}

#[test]
fn a_change_of_format_reads_the_file_again() {
    let mut f = Fixture::new("dialog-format");
    f.open_dialog();
    let path = f.file("people.txt", b"[{\"id\": 1, \"name\": \"Ana\"}]");
    f.choose(&path);
    f.code(KeyCode::Right);
    f.code(KeyCode::Right);
    f.wait_for("2 file columns. 2 of 4 table columns get values.");
    f.click_text("[Import]");
    f.wait_for("Import complete");
    assert_eq!(f.query("SELECT name FROM people WHERE id = 1"), ["Ana"]);
}

#[test]
fn the_extension_chooses_the_format() {
    let mut f = Fixture::new("dialog-json");
    f.open_dialog();
    let path = f.file(
        "people.jsonl",
        b"{\"id\": 1, \"name\": \"Ana\", \"x\": 1}\n",
    );
    f.choose(&path);
    assert!(f.sees("< JSON Lines >"), "{}", f.screen());
    f.click_text("Format");
    assert!(
        f.sees("JSON files have no format options."),
        "{}",
        f.screen()
    );
    f.click_text("[Import]");
    f.wait_for("Import complete");
    assert_eq!(f.count(), "2");
}

#[test]
fn a_preview_row_names_its_problem() {
    let mut f = Fixture::new("dialog-bad-row");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\nwrong\n");
    f.choose(&path);
    f.click_text("Preview");
    assert!(f.sees("Line 3: This row has 1 fields."), "{}", f.screen());
}

#[test]
fn the_preview_scrolls_its_rows_and_columns() {
    let mut f = Fixture::new("dialog-scroll");
    f.open_dialog();
    let mut file = String::from("id,name,age,photo\n");
    for row in 0..40 {
        file.push_str(&format!("{row},name{row},{row},0x00\n"));
    }
    let path = f.file("people.csv", file.as_bytes());
    f.choose(&path);
    f.click_text("Preview");
    f.code(KeyCode::Tab);
    assert!(f.sees("name0"), "{}", f.screen());
    f.code(KeyCode::Down);
    assert!(!f.sees("name0"), "{}", f.screen());
    assert!(f.sees("name1"), "{}", f.screen());
    f.code(KeyCode::Right);
    assert!(!f.screen().contains("  id  "), "{}", f.screen());
}

#[test]
fn the_dialog_fits_a_small_terminal() {
    let mut f = Fixture::new("dialog-small");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n");
    f.choose(&path);
    for section in ["File", "Format", "Columns", "Options", "Preview"] {
        f.click_text(section);
        let screen = f.screen_at(80, 24);
        assert!(screen.contains("[Import]"), "{section}:\n{screen}");
        assert!(screen.contains("[Cancel]"), "{section}:\n{screen}");
    }
}

#[test]
fn the_rows_of_the_grid_stay_during_the_read_of_the_columns() {
    let mut f = Fixture::new("dialog-grid-rows");
    f.open_dialog();
    assert_eq!(f.app.table().unwrap().result.table.rows.len(), 1);
    f.code(KeyCode::Esc);
    assert!(!f.sees("Import to people"));
    assert_eq!(f.app.table().unwrap().result.table.rows.len(), 1);
}

#[test]
fn the_hint_row_follows_the_focus() {
    let mut f = Fixture::new("dialog-hints");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n");
    f.choose(&path);
    f.click_text("Columns");
    assert!(f.sees("section"), "{}", f.screen());
    f.code(KeyCode::Tab);
    assert!(f.sees("file column"), "{}", f.screen());
}

#[test]
fn stop_undoes_a_run_and_keeps_the_tab_usable() {
    let mut f = Fixture::new("dialog-stop");
    f.open_dialog();
    let mut file = String::from("id,name\n");
    for row in 1000..400_000 {
        file.push_str(&format!("{row},x\n"));
    }
    let path = f.file("people.csv", file.as_bytes());
    f.choose(&path);
    f.click_text("Options");
    f.code(KeyCode::Tab);
    f.code(KeyCode::Tab);
    for _ in 0..3 {
        f.code(KeyCode::Backspace);
    }
    f.write("1");
    f.click_text("[Import]");
    f.wait_for("Importing:");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('x'));
    f.wait_for("Import not complete");
    assert!(f.sees("The import stopped."), "{}", f.screen());
    assert_eq!(f.count(), "1");
}

/// The stop races with the connection, so ten runs reach the wait for it.
#[test]
fn a_stop_before_the_connection_reports_the_stop() {
    for run in 0..10 {
        let mut f = Fixture::new(&format!("dialog-early-stop-{run}"));
        f.open_dialog();
        let mut file = String::from("id,name\n");
        for row in 1000..20_000 {
            file.push_str(&format!("{row},x\n"));
        }
        let path = f.file("people.csv", file.as_bytes());
        f.choose(&path);
        f.click_text("[Import]");
        f.press(ctrl('w'));
        f.code(KeyCode::Char('x'));
        f.wait_for("Import not complete");
        assert!(f.sees("The import stopped."), "run {run}:\n{}", f.screen());
    }
}

impl Fixture {
    /// Selects the log mode, and puts the focus on the path of the log.
    fn focus_log(&mut self) {
        self.click_text("Options");
        self.code(KeyCode::Tab);
        self.code(KeyCode::Right);
        self.code(KeyCode::Right);
        self.wait_for("Log file");
        self.code(KeyCode::Tab);
        self.code(KeyCode::Tab);
    }
}

#[test]
fn the_file_browser_fills_the_path_and_reads_the_file() {
    let mut f = Fixture::new("dialog-browse-file");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n");
    f.write(&format!("{}/", f.folder.display()));
    assert!(f.sees("Ctrl+O browse"), "{}", f.screen());
    f.press(ctrl('o'));
    f.wait_for("Choose a file");
    assert!(!f.sees("Use this folder"), "{}", f.screen());
    f.write("people");
    f.code(KeyCode::Enter);
    f.wait_for("2 file columns.");
    assert!(f.sees("people.csv"), "{}: {}", path.display(), f.screen());
}

#[test]
fn the_file_browser_chooses_the_folder_of_the_log() {
    let mut f = Fixture::new("dialog-browse-log");
    f.open_dialog();
    let folder = f.folder.join("logs");
    std::fs::create_dir(&folder).unwrap();
    let path = f.file("people.csv", b"id,name\n1,Ana\n1,again\n");
    f.choose(&path);
    f.focus_log();
    assert!(f.sees("Ctrl+O browse"), "{}", f.screen());
    f.press(ctrl('o'));
    f.wait_for("Use this folder");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.code(KeyCode::Enter);
    assert!(!f.sees("Choose a file"), "{}", f.screen());
    f.click_text("[Import]");
    f.wait_for("Import complete");
    let names: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("people-errors-"), "{names:?}");
}

#[test]
fn the_button_of_the_log_opens_the_file_browser() {
    let mut f = Fixture::new("dialog-browse-button");
    f.open_dialog();
    let path = f.file("people.csv", b"id,name\n1,Ana\n");
    f.choose(&path);
    f.focus_log();
    f.code(KeyCode::BackTab);
    f.code(KeyCode::BackTab);
    let small = f.screen_at(80, 24);
    assert!(small.contains("[...]"), "{small}");
    assert!(small.contains(".log"), "{small}");
    f.click_text("[...]");
    f.wait_for("Use this folder");
    f.code(KeyCode::Esc);
    assert!(!f.sees("Choose a file"), "{}", f.screen());
    assert!(f.sees("Ctrl+O browse"), "{}", f.screen());
}

#[test]
fn a_new_file_keeps_a_log_path_from_the_browser() {
    let mut f = Fixture::new("dialog-browse-keep");
    f.open_dialog();
    let folder = f.folder.join("logs");
    std::fs::create_dir(&folder).unwrap();
    let first = f.file("first.csv", b"id,name\n1,Ana\n");
    f.choose(&first);
    f.focus_log();
    f.press(ctrl('o'));
    f.wait_for("Use this folder");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.code(KeyCode::Enter);
    let log = f.screen();
    assert!(log.contains("logs/first-errors-"), "{log}");
    let second = f.file("second.csv", b"id,name\n2,Rui\n");
    f.click_text("File");
    f.code(KeyCode::Tab);
    for _ in 0..first.display().to_string().chars().count() {
        f.code(KeyCode::Backspace);
    }
    f.choose(&second);
    f.click_text("Options");
    assert!(f.sees("logs/first-errors-"), "{}", f.screen());
}
