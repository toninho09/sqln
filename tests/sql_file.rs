// The open of an external `.sql` file as a console: the question of the copy or the
// link, the writes of a linked console, and the questions of a close.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use super::common::{ctrl, key, temp_db, temp_dir};
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::console::Focus;
use sqln::editor::KeyboardMode;
use sqln::exec::Settings;
use sqln::ui::theme::Theme;

struct Fixture {
    app: App,
    sql: PathBuf,
}

impl Fixture {
    /// Starts the application with a database and a `.sql` file that asks to open.
    fn new(name: &str, text: &str) -> Self {
        let path = temp_db(&format!("sql-file-{name}"));
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t(a);")
            .unwrap();
        let sql = temp_dir(&format!("sql-file-{name}")).join("query.sql");
        fs::write(&sql, text).unwrap();
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(
            Options {
                database: Some(path),
                script: Some(sql.clone()),
                keyboard: KeyboardMode::Conventional,
                settings: Settings::default(),
                ..Options::default()
            },
            clipboard,
            Theme::plain(),
        );
        Self { app, sql }
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    /// Accepts the option of the open dialog at `steps` moves from the first.
    fn choose(&mut self, steps: usize) {
        for _ in 0..steps {
            self.code(KeyCode::Tab);
        }
        self.code(KeyCode::Enter);
    }

    /// Replaces the text of the editor of the active console.
    fn write(&mut self, text: &str) {
        self.app.console_mut().focus = Focus::Editor;
        self.press(ctrl('a'));
        self.code(KeyCode::Delete);
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    fn save(&mut self) {
        self.press(ctrl('s'));
    }

    fn screen(&mut self) -> String {
        screen(&mut self.app)
    }

    fn assert_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(
            screen.contains(text),
            "{text:?} is not on the screen:\n{screen}"
        );
    }
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
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

#[test]
fn escape_cancels_the_open_without_creating_a_console() {
    let mut f = Fixture::new("cancel-open", "SELECT 1;\n");
    let count = f.app.consoles().len();
    assert!(f.screen().contains("[Copy]"));
    f.code(KeyCode::Esc);
    assert_eq!(f.app.consoles().len(), count);
    assert!(!f.screen().contains("[Copy]"));
    assert!(f.app.console().editor.is_blank());
}

#[test]
fn canceling_a_conflict_during_close_cancels_the_close() {
    let mut f = Fixture::new("cancel-conflict-close", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    fs::write(&f.sql, "SELECT other;\n").unwrap();
    f.press(ctrl('w'));
    f.code(KeyCode::Char('c'));
    f.choose(1);
    f.assert_sees("File changed outside SQLN");
    f.code(KeyCode::Esc);
    assert!(!f.app.console().closing);
    assert_eq!(f.app.console().editor.text(), "SELECT 2;\n");
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT other;\n");
}

#[test]
fn canceling_a_conflict_during_exit_cancels_the_exit() {
    let mut f = Fixture::new("cancel-conflict-exit", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    fs::write(&f.sql, "SELECT other;\n").unwrap();
    f.press(ctrl('w'));
    f.code(KeyCode::Char('q'));
    f.choose(1);
    f.assert_sees("File changed outside SQLN");
    f.code(KeyCode::Esc);
    assert!(!f.app.console().closing);
    // Closing this console later must not resume the canceled application exit.
    f.press(ctrl('w'));
    f.code(KeyCode::Char('c'));
    f.choose(2);
    assert!(!f.app.should_quit());
}

#[test]
fn an_open_as_a_copy_creates_a_console_with_the_text_of_the_file() {
    let mut f = Fixture::new("copy", "SELECT 1;\n");
    f.assert_sees("Open a SQL file");
    f.choose(1);
    let console = f.app.console();
    assert!(console.external.is_none());
    assert_eq!(console.name, "query");
    assert_eq!(console.editor.text(), "SELECT 1;\n");
    // A copy never writes the file.
    f.write("SELECT 2;\n");
    f.save();
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 1;\n");
}

#[test]
fn an_open_as_a_link_writes_the_file_on_each_explicit_save() {
    let mut f = Fixture::new("linked", "SELECT 1;\n");
    f.choose(2);
    assert!(f.app.console().external.is_some());
    f.write("SELECT 2;\n");
    f.save();
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 2;\n");
}

#[test]
fn a_link_refuses_to_write_a_file_that_another_program_changed() {
    let mut f = Fixture::new("changed", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    fs::write(&f.sql, "SELECT other;\n").unwrap();
    f.save();
    f.assert_sees("File changed outside SQLN");
    // Esc keeps the file of the other program and the text of the console.
    f.code(KeyCode::Esc);
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT other;\n");
    assert_eq!(f.app.console().editor.text(), "SELECT 2;\n");
    // The second option overwrites the file with the text of the console.
    f.save();
    f.choose(2);
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 2;\n");
}

#[test]
fn a_removed_file_asks_and_can_come_back() {
    let mut f = Fixture::new("removed", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    fs::remove_file(&f.sql).unwrap();
    f.save();
    f.assert_sees("File removed");
    f.choose(1);
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 2;\n");
}

#[test]
fn the_reload_of_a_changed_file_takes_the_new_text() {
    let mut f = Fixture::new("reload", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    fs::write(&f.sql, "SELECT other;\n").unwrap();
    f.save();
    f.choose(1);
    assert_eq!(f.app.console().editor.text(), "SELECT other;\n");
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT other;\n");
}

#[test]
fn a_close_with_changes_asks_before_it_writes() {
    let mut f = Fixture::new("close", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('c'));
    f.assert_sees("Write and close");
    f.choose(1);
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 2;\n");
}

#[test]
fn a_close_without_writing_leaves_the_file_as_it_is() {
    let mut f = Fixture::new("close-keep", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('c'));
    f.choose(2);
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 1;\n");
}

#[test]
fn a_clean_link_closes_without_a_question() {
    let mut f = Fixture::new("close-clean", "SELECT 1;\n");
    f.choose(2);
    f.press(ctrl('w'));
    f.code(KeyCode::Char('c'));
    assert_eq!(fs::read_to_string(&f.sql).unwrap(), "SELECT 1;\n");
    assert!(!f.app.console().external.is_some());
}

#[test]
fn a_file_that_cannot_open_shows_the_reason() {
    let path = temp_db("sql-file-broken");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE t(a);")
        .unwrap();
    let sql = temp_dir("sql-file-broken").join("broken.sql");
    fs::write(&sql, b"\xff\xfeS\x00").unwrap();
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(
        Options {
            database: Some(path),
            script: Some(sql),
            keyboard: KeyboardMode::Conventional,
            settings: Settings::default(),
            ..Options::default()
        },
        clipboard,
        Theme::plain(),
    );
    let screen = screen(&mut app);
    assert!(screen.contains("Cannot open"), "{screen}");
}

#[test]
fn the_command_opens_the_file_browser_and_escape_cancels() {
    use sqln::input::Keymap;
    let path = temp_db("sql-file-browser");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE t(a);")
        .unwrap();
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let overrides = [("openSqlFile".to_owned(), vec!["ctrl+o".to_owned()])]
        .into_iter()
        .collect();
    let app = App::new(
        Options {
            database: Some(path),
            keyboard: KeyboardMode::Conventional,
            keymap: Some(Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap()),
            settings: Settings::default(),
            ..Options::default()
        },
        clipboard,
        Theme::plain(),
    );
    let mut f = Fixture {
        app,
        sql: PathBuf::new(),
    };
    f.press(ctrl('o'));
    f.assert_sees("Choose a file");
    f.code(KeyCode::Esc);
    let screen = f.screen();
    assert!(!screen.contains("Choose a file"), "{screen}");
}

#[test]
fn a_console_of_a_link_never_becomes_a_script_of_the_store() {
    let mut f = Fixture::new("no-store", "SELECT 1;\n");
    f.choose(2);
    f.write("SELECT 2;\n");
    // The tick of the autosave does not create the files of a script.
    f.app.tick();
    assert!(f.app.console().file.is_none());
    assert!(f.app.console().external.is_some());
}

#[test]
fn the_palette_offers_the_command_and_it_opens_the_browser() {
    let mut f = Fixture::new("palette", "SELECT 1;\n");
    f.choose(1);
    // The command has no default key, so the palette is its way in.
    f.press(ctrl('p'));
    f.app.on_event(Event::Paste("sql file".to_owned()));
    let screen = f.screen();
    assert!(screen.contains("Open a SQL file as a console"), "{screen}");
    f.code(KeyCode::Enter);
    f.assert_sees("Choose a file");
}
