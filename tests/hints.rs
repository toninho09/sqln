// The hint row: each state of the application shows the commands that act in that state.
// The tests use a real SQLite file. `tests/table.rs` covers the table tabs.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connector::TransactionState;
use sqln::editor::KeyboardMode;
use sqln::navigator::RowKind;
use sqln::ui::statusline::{HintItem, fit_hints, hint_text};
use sqln::ui::theme::Theme;

const SETUP: &str =
    "CREATE TABLE items(id INTEGER, label TEXT); INSERT INTO items VALUES (1, 'a');";

struct Screen {
    app: App,
}

impl Screen {
    fn start(options: Options) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
        }
    }

    fn new(name: &str, keyboard: KeyboardMode) -> Self {
        let path = temp_db(&format!("hints-{name}"));
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(SETUP)
            .unwrap();
        Self::start(Options {
            database: Some(path),
            keyboard,
            ..Options::default()
        })
    }

    fn conventional(name: &str) -> Self {
        Self::new(name, KeyboardMode::Conventional)
    }

    fn code(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    fn prefix(&mut self, letter: char) {
        self.app.on_event(Event::Key(ctrl('w')));
        self.code(KeyCode::Char(letter));
    }

    fn write(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    /// Runs the text of the editor and waits for its end.
    fn run(&mut self, sql: &str) {
        self.write(sql);
        self.code(KeyCode::F(5));
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if !self.app.console_mut().is_busy() {
                return;
            }
            assert!(Instant::now() < deadline, "the run did not end");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn select(&mut self, kind: RowKind, name: &str) {
        common::select_row(&mut self.app, 0, &[kind], name);
    }

    /// Returns the hint row of a screen with this width.
    fn hints_at(&mut self, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let last = buffer.area.height - 1;
        let row: String = (0..width).map(|x| buffer[(x, last)].symbol()).collect();
        row.trim().to_owned()
    }

    fn hints(&mut self) -> String {
        self.hints_at(120)
    }

    #[track_caller]
    fn assert_shows(&mut self, wanted: &[&str]) {
        let row = self.hints();
        for text in wanted {
            assert!(row.contains(text), "{text:?} is not in {row:?}");
        }
    }

    #[track_caller]
    fn assert_hides(&mut self, unwanted: &[&str]) {
        let row = self.hints();
        for text in unwanted {
            assert!(!row.contains(text), "{text:?} is in {row:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// The editor.

#[test]
fn an_idle_editor_shows_how_to_run_and_how_to_get_help() {
    let mut s = Screen::conventional("editor");
    s.assert_shows(&[
        "F5 run",
        "Shift+F5 script",
        "Ctrl+Space suggestions",
        "Ctrl+F find",
        "F1 keys",
    ]);
    s.assert_hides(&["results", "commit", "stop"]);
}

#[test]
fn a_console_with_results_shows_the_way_to_the_results() {
    let mut s = Screen::conventional("editor-results");
    s.run("SELECT 1 AS one;");
    s.assert_shows(&["Ctrl+W j results"]);
}

#[test]
fn the_way_to_the_results_fits_the_minimum_width_with_the_commands_to_run() {
    let mut s = Screen::conventional("editor-results-width");
    s.run("SELECT 1 AS one;");
    let row = s.hints_at(80);
    assert!(
        row.starts_with("F5 run  Shift+F5 script  Ctrl+W j results"),
        "{row}"
    );
}

#[test]
fn an_open_transaction_puts_its_commands_first_in_the_editor_and_in_the_results() {
    let mut s = Screen::conventional("transaction");
    s.prefix('m');
    s.run("INSERT INTO items VALUES (2, 'b');");
    assert!(s.app.console_mut().in_transaction());
    let row = s.hints_at(80);
    assert!(
        row.starts_with("Ctrl+W C commit  Ctrl+W R rollback"),
        "{row}"
    );
    s.prefix('j');
    let row = s.hints_at(80);
    assert!(
        row.starts_with("Ctrl+W C commit  Ctrl+W R rollback"),
        "{row}"
    );
}

#[test]
fn a_failed_transaction_offers_only_the_rollback() {
    let mut s = Screen::conventional("transaction-failed");
    s.prefix('m');
    s.run("INSERT INTO items VALUES (2, 'b');");
    s.app.console_mut().transaction = TransactionState::Failed;
    s.assert_shows(&["Ctrl+W R rollback"]);
    s.assert_hides(&["commit"]);
}

#[test]
fn no_transaction_hides_its_commands() {
    let mut s = Screen::conventional("no-transaction");
    s.prefix('m');
    s.assert_hides(&["commit", "rollback"]);
}

#[test]
fn a_console_without_a_database_shows_the_way_to_a_connection_and_not_the_run() {
    let mut s = Screen::start(Options::default());
    s.assert_shows(&["Ctrl+W h navigator"]);
    s.assert_hides(&["F5 run", "script"]);
}

#[test]
fn the_normal_mode_of_vim_hides_the_suggestions_and_the_insert_mode_shows_them() {
    let mut s = Screen::new("vim", KeyboardMode::Vim);
    s.assert_hides(&["suggestions"]);
    s.assert_shows(&["Ctrl+F find"]);
    s.code(KeyCode::Char('i'));
    s.assert_shows(&["Ctrl+Space suggestions"]);
}

// ---------------------------------------------------------------------------
// The results of a console.

#[test]
fn a_result_shows_the_commands_of_its_grid() {
    let mut s = Screen::conventional("results");
    s.run("SELECT 1 AS one;");
    s.prefix('j');
    s.assert_shows(&[
        "Ctrl+W k editor",
        "copy",
        "export",
        "value",
        "find",
        "] tab",
    ]);
}

#[test]
fn a_result_without_rows_hides_the_commands_of_a_row() {
    let mut s = Screen::conventional("results-empty");
    s.run("SELECT 1 AS one WHERE 1 = 0;");
    s.prefix('j');
    s.assert_shows(&["Ctrl+W k editor", "] tab"]);
    s.assert_hides(&["copy", "value", "find"]);
}

#[test]
fn the_messages_tab_hides_the_commands_of_a_grid() {
    let mut s = Screen::conventional("messages");
    s.run("SELECT 1 AS one;");
    s.prefix('j');
    s.code(KeyCode::Char(']'));
    assert!(s.app.console_mut().showing_messages());
    s.assert_shows(&["Ctrl+W k editor", "] tab"]);
    s.assert_hides(&["copy", "export", "value", "find"]);
}

#[test]
fn a_console_with_only_messages_has_no_other_tab_to_show() {
    let mut s = Screen::conventional("messages-only");
    s.run("CREATE TABLE more_items(id INTEGER);");
    s.prefix('j');
    s.assert_shows(&["Ctrl+W k editor"]);
    s.assert_hides(&["] tab", "export"]);
}

// ---------------------------------------------------------------------------
// The navigator.

#[test]
fn a_navigator_without_connections_shows_how_to_add_one() {
    let mut s = Screen::start(Options::default());
    s.prefix('h');
    s.assert_shows(&["a add"]);
    s.assert_hides(&["open", "struct", "edit", "refresh"]);
}

#[test]
fn a_connection_row_shows_the_commands_of_a_connection() {
    let mut s = Screen::conventional("connection");
    s.prefix('h');
    s.assert_shows(&[
        "Enter open",
        "Right expand",
        "i struct",
        "n new",
        "e edit",
        "r refresh",
        "Del drop",
    ]);
}

#[test]
fn a_sqlite_file_keeps_no_session_to_disconnect() {
    let mut s = Screen::conventional("disconnect");
    s.run("SELECT 1;");
    s.prefix('h');
    s.assert_hides(&["D disc"]);
}

#[test]
fn a_sqlite_file_has_no_saved_password_to_delete() {
    let mut s = Screen::conventional("password");
    s.prefix('h');
    s.assert_hides(&["p pwd"]);
}

#[test]
fn an_open_row_shows_the_key_that_closes_it() {
    let mut s = Screen::conventional("collapse");
    s.prefix('h');
    s.code(KeyCode::Right);
    s.assert_shows(&["Left collapse"]);
    s.assert_hides(&["Right expand"]);
}

#[test]
fn a_schema_row_shows_the_commands_of_its_tree() {
    let mut s = Screen::conventional("schema");
    s.select(RowKind::Schema, "main");
    s.assert_shows(&["i struct", "n new", "r refresh"]);
    s.assert_hides(&["Enter open", "e edit", "drop", "add", "scripts"]);
}

#[test]
fn a_table_row_shows_the_commands_of_a_table() {
    let mut s = Screen::conventional("table");
    s.select(RowKind::Table, "items");
    s.assert_shows(&["Enter open", "Right columns", "i struct", "n new"]);
    s.assert_hides(&["e edit", "drop", "disc", "add", "scripts"]);
}

#[test]
fn a_column_row_shows_its_parent_and_opens_nothing() {
    let mut s = Screen::conventional("column");
    s.select(RowKind::Table, "items");
    s.code(KeyCode::Right);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        s.app.drain_events();
        let rows = s.app.navigator().rows(s.app.connections());
        if rows.iter().any(|row| row.kind == RowKind::Column) {
            break;
        }
        assert!(Instant::now() < deadline, "the columns did not arrive");
        std::thread::sleep(Duration::from_millis(1));
    }
    s.code(KeyCode::Down);
    s.assert_shows(&["Left parent", "i struct"]);
    s.assert_hides(&["Enter open", "expand"]);
}

// ---------------------------------------------------------------------------
// The windows.

/// Moves through a form and reads the kinds of field that its hints name.
fn kinds_of_field(s: &mut Screen) -> Vec<&'static str> {
    let mut seen = Vec::new();
    for _ in 0..30 {
        let row = s.hints();
        if row.contains("Enter press") {
            seen.push("button");
        } else if row.contains("Ctrl+O browse") {
            seen.push("path");
        } else if row.contains("Left/Right change") {
            seen.push("choice");
        } else {
            seen.push("text");
        }
        s.code(KeyCode::Tab);
    }
    seen
}

#[test]
fn the_connection_form_shows_the_keys_of_the_focused_field() {
    let mut s = Screen::conventional("form");
    s.prefix('h');
    s.code(KeyCode::Char('a'));
    let seen = kinds_of_field(&mut s);
    for kind in ["text", "choice", "button"] {
        assert!(seen.contains(&kind), "{kind} is not in {seen:?}");
    }
}

#[test]
fn the_export_form_shows_the_keys_of_the_focused_field() {
    let mut s = Screen::conventional("export");
    s.run("SELECT 1 AS one;");
    s.prefix('j');
    s.code(KeyCode::Char('e'));
    let seen = kinds_of_field(&mut s);
    for kind in ["path", "choice", "button"] {
        assert!(seen.contains(&kind), "{kind} is not in {seen:?}");
    }
}

// ---------------------------------------------------------------------------
// The width of the row.

/// Returns the commands of a row.
fn commands(row: &str) -> Vec<String> {
    row.split("  ").map(str::to_owned).collect()
}

/// Checks that a narrow row holds only whole commands, in their order of relevance.
#[track_caller]
fn assert_whole_commands(s: &mut Screen) {
    let all = commands(&s.app.hint());
    for width in [80, 90, 100, 120] {
        let row = s.hints_at(width);
        assert!(
            row.chars().count() < usize::from(width),
            "{width}: {row:?} touches the edge"
        );
        let mut rest = all.iter();
        for command in commands(&row) {
            if command.is_empty() {
                continue;
            }
            assert!(
                rest.any(|item| *item == command),
                "{width}: {command:?} is cut or out of order in {row:?}"
            );
        }
    }
}

#[test]
fn a_narrow_row_keeps_whole_commands_in_each_state() {
    let mut s = Screen::conventional("width");
    assert_whole_commands(&mut s);
    s.run("SELECT 1 AS one;");
    assert_whole_commands(&mut s);
    s.prefix('j');
    assert_whole_commands(&mut s);
    s.prefix('h');
    assert_whole_commands(&mut s);
    s.select(RowKind::Schema, "main");
    assert_whole_commands(&mut s);
    s.select(RowKind::Table, "items");
    assert_whole_commands(&mut s);
    s.code(KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.app.table().is_none_or(|tab| tab.is_busy()) {
        s.app.drain_events();
        assert!(Instant::now() < deadline, "the table did not load");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_whole_commands(&mut s);
}

fn item(key: &str, label: &'static str) -> HintItem {
    HintItem {
        key: key.to_owned(),
        label,
    }
}

#[test]
fn a_command_that_does_not_fit_leaves_room_for_a_later_short_command() {
    let items = [
        item("a", "first"),
        item("b", "a very long command"),
        item("c", "last"),
    ];
    let kept = fit_hints(&items, 15);
    assert_eq!(hint_text(&kept), "a first  c last");
}

#[test]
fn a_row_can_end_exactly_at_its_width() {
    let items = [item("a", "first"), item("c", "last")];
    assert_eq!(fit_hints(&items, 15).len(), 2);
    assert_eq!(fit_hints(&items, 14).len(), 1);
}

#[test]
fn the_comment_command_needs_text() {
    let mut s = Screen::conventional("comment-text");
    s.assert_hides(&["comment"]);
    s.write("SELECT 1;");
    let row = s.hints_at(200);
    assert!(row.contains("Ctrl+W / comment"), "{row}");
}

#[test]
fn the_row_command_needs_rows() {
    let mut s = Screen::conventional("row-rows");
    s.run("SELECT 1 AS a WHERE 1 = 0;");
    s.prefix('j');
    assert!(!s.hints_at(200).contains("r row"));
    s.prefix('k');
    s.run("SELECT 1 AS a;");
    s.prefix('j');
    let row = s.hints_at(200);
    assert!(row.contains("r row"), "{row}");
}
