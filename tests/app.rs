// End-to-end tests: keys enter the application, SQL runs in a real SQLite file,
// and the screen shows the result.

mod common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, shift, temp_db};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use edtui::Index2;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::console::Focus;
use sqln::editor::KeyboardMode;
use sqln::exec::Settings;
use sqln::ui::theme::Theme;

const LONG: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT count(*) FROM c;";

struct Fixture {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
    path: PathBuf,
}

impl Fixture {
    fn at(path: &Path, keyboard: KeyboardMode, settings: Settings) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let options = Options {
            keyboard,
            database: Some(path.to_owned()),
            settings,
            ..Options::default()
        };
        let app = App::new(options, clipboard.clone(), Theme::plain());
        Self { app, clipboard, path: path.to_owned() }
    }

    /// Opens a database that exists and that holds the text of `setup`.
    fn with(name: &str, keyboard: KeyboardMode, settings: Settings, setup: &str) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path).unwrap().execute_batch(setup).unwrap();
        Self::at(&path, keyboard, settings)
    }

    fn new(name: &str) -> Self {
        Self::with(name, KeyboardMode::Conventional, Settings::default(), "")
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn prefix(&mut self, c: char) {
        self.press(ctrl('w'));
        self.press(key(KeyCode::Char(c)));
    }

    fn write(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(key(KeyCode::Char(c)));
        }
    }

    fn pump_until(&mut self, condition: impl Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if condition(self) {
                return;
            }
            assert!(Instant::now() < deadline, "the condition did not happen:\n{}", self.screen());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn idle(&mut self) {
        self.pump_until(|f| !f.app.console_mut().is_busy());
        self.app.drain_events();
    }

    /// Runs the whole text of the editor.
    fn run_script(&mut self, sql: &str) {
        self.select_all_and_write(sql);
        self.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
        self.idle();
    }

    fn select_all_and_write(&mut self, sql: &str) {
        self.app.console_mut().focus = Focus::Editor;
        self.press(ctrl('a'));
        self.press(key(KeyCode::Delete));
        self.write(sql);
    }

    fn buffer(&mut self) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn screen(&mut self) -> String {
        let buffer = self.buffer();
        (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect();
                row.trim_end().to_owned() + "\n"
            })
            .collect()
    }

    fn sees(&mut self, text: &str) -> bool {
        self.screen().contains(text)
    }

    fn assert_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(screen.contains(text), "{text:?} is not on the screen:\n{screen}");
    }

    fn file_query(&self, sql: &str) -> i64 {
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }
}

#[test]
fn a_missing_file_asks_first_and_cancel_is_the_default() {
    let path = temp_db("create-cancel");
    let mut f = Fixture::at(&path, KeyboardMode::Conventional, Settings::default());
    f.assert_sees("Create it?");
    f.press(key(KeyCode::Enter));
    assert!(!path.exists());
    f.write("SELECT 1;");
    f.press(key(KeyCode::F(5)));
    f.assert_sees("No database");
}

#[test]
fn escape_also_keeps_the_file_absent() {
    let path = temp_db("create-escape");
    let mut f = Fixture::at(&path, KeyboardMode::Conventional, Settings::default());
    f.press(key(KeyCode::Esc));
    assert!(!path.exists());
}

#[test]
fn an_explicit_choice_creates_the_file() {
    let path = temp_db("create-yes");
    let mut f = Fixture::at(&path, KeyboardMode::Conventional, Settings::default());
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Enter));
    f.run_script("CREATE TABLE t(a);");
    assert!(path.exists());
    assert_eq!(f.file_query("SELECT count(*) FROM sqlite_schema"), 1);
}

#[test]
fn a_memory_path_is_refused() {
    let mut f = Fixture::at(Path::new(":memory:"), KeyboardMode::Conventional, Settings::default());
    f.assert_sees("not valid");
}

#[test]
fn the_start_opens_no_connection() {
    let mut f = Fixture::new("no-connection");
    f.assert_sees("Pool 0/5");
}

#[test]
fn a_long_file_name_leaves_the_state_visible() {
    let name = "a-very-long-database-file-name-that-does-not-fit-on-the-top-row-of-the-screen-at-all";
    let mut f = Fixture::new(name);
    let screen = f.screen();
    let top = screen.lines().nth(1).unwrap();
    assert!(top.contains('…'), "{top}");
    assert!(top.contains("Auto · No transaction · Idle"), "{top}");
    assert!(top.contains("Pool 0/5"), "{top}");
}

#[test]
fn f5_runs_the_statement_at_the_cursor_and_the_grid_shows_it() {
    let mut f = Fixture::with("run", KeyboardMode::Conventional, Settings::default(),
        "CREATE TABLE users(id, name); INSERT INTO users VALUES (1, 'Ana'), (2, 'Bruno');");
    f.write("SELECT 1;\nSELECT id, name FROM users ORDER BY id;\nSELECT 3;");
    f.app.console_mut().editor.set_cursor(Index2::new(1, 4));
    f.press(key(KeyCode::F(5)));
    f.idle();

    f.assert_sees("id");
    f.assert_sees("name");
    f.assert_sees("Ana");
    f.assert_sees("Bruno");
    f.assert_sees("2 rows");
    assert_eq!(f.app.console_mut().results.len(), 1, "only one statement ran");
}

#[test]
fn ctrl_enter_runs_and_shift_enter_runs_the_script() {
    let mut f = Fixture::new("run-keys");
    f.write("SELECT 1;\nSELECT 2;");
    f.press(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    f.idle();
    assert_eq!(f.app.console_mut().results.len(), 1);
    f.press(shift(KeyCode::Enter));
    f.idle();
    assert_eq!(f.app.console_mut().results.len(), 2);
}

#[test]
fn the_prefix_runs_too() {
    let mut f = Fixture::new("run-prefix");
    f.write("SELECT 1;\nSELECT 2;");
    f.prefix('e');
    f.idle();
    assert_eq!(f.app.console_mut().results.len(), 1);
    f.prefix('s');
    f.idle();
    assert_eq!(f.app.console_mut().results.len(), 2);
}

#[test]
fn a_selection_has_priority_over_the_statement() {
    let mut f = Fixture::new("run-selection");
    f.write("SELECT 1, 2;");
    f.app.console_mut().editor.set_cursor(Index2::new(0, 0));
    for _ in 0..8 {
        f.press(shift(KeyCode::Right));
    }
    f.press(key(KeyCode::F(5)));
    f.idle();
    assert_eq!(f.app.console_mut().results[0].table.columns, ["1"]);
}

#[test]
fn a_cursor_outside_a_statement_runs_nothing_and_keeps_the_results() {
    let mut f = Fixture::new("outside");
    f.write("SELECT 1;\n\nSELECT 2;");
    f.app.console_mut().editor.set_cursor(Index2::new(0, 3));
    f.press(key(KeyCode::F(5)));
    f.idle();
    assert_eq!(f.app.console_mut().results.len(), 1);

    f.app.console_mut().editor.set_cursor(Index2::new(1, 0));
    f.press(key(KeyCode::F(5)));
    f.assert_sees("outside a statement");
    assert_eq!(f.app.console_mut().results.len(), 1, "the refusal keeps the results");
}

#[test]
fn an_empty_script_reports_that_it_has_no_statement() {
    let mut f = Fixture::new("empty-script");
    f.write("-- only a comment");
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    f.assert_sees("no statement");
}

#[test]
fn the_margin_shows_each_statement_state_and_the_error_message() {
    let mut f = Fixture::new("marks");
    f.run_script("SELECT 1;\nSELECT * FROM missing;\nSELECT 3;");
    let screen = f.screen();
    assert!(screen.contains("1✓SELECT 1;"), "{screen}");
    assert!(screen.contains("2✗SELECT * FROM missing;"), "{screen}");
    assert!(screen.contains("3—SELECT 3;"), "{screen}");
    f.assert_sees("Statement 2: no such table: missing");
    assert!(f.app.console_mut().showing_messages());
}

#[test]
fn an_edit_removes_the_mark_of_that_statement_only() {
    let mut f = Fixture::new("mark-edit");
    f.run_script("SELECT 1;\nSELECT 2;");
    f.app.console_mut().editor.set_cursor(Index2::new(1, 7));
    f.type_text("9");
    let screen = f.screen();
    assert!(screen.contains("1✓SELECT 1;"), "{screen}");
    assert!(screen.contains("2 SELECT 92;") || screen.contains("2 SELECT 9"), "{screen}");
    assert!(!screen.contains("2✓"), "{screen}");
}

#[test]
fn a_new_row_above_moves_the_mark_with_its_statement() {
    let mut f = Fixture::new("mark-move");
    f.run_script("SELECT 1;");
    f.app.console_mut().editor.set_cursor(Index2::new(0, 0));
    f.press(key(KeyCode::Enter));
    let screen = f.screen();
    assert!(screen.contains("2✓SELECT 1;"), "{screen}");
}

#[test]
fn a_new_execution_clears_results_and_frees_the_budget() {
    let settings = Settings::default();
    let mut f = Fixture::with("clear", KeyboardMode::Conventional, settings, "");
    f.run_script("SELECT 'a'; SELECT 'b'; SELECT 'c';");
    assert_eq!(f.app.console_mut().results.len(), 3);
    f.run_script("SELECT 'a';");
    assert_eq!(f.app.console_mut().results.len(), 1);
    f.run_script("CREATE TABLE t(a);");
    assert!(f.app.console_mut().results.is_empty());
    f.assert_sees("Data 0/256 MiB");
}

#[test]
fn a_result_without_rows_shows_columns_and_zero_rows() {
    let mut f = Fixture::new("zero");
    f.run_script("SELECT 1 AS first_column WHERE 0;");
    f.assert_sees("first_column");
    f.assert_sees("0 rows");
}

#[test]
fn the_row_cap_shows_its_message() {
    let settings = Settings { row_cap: 5, ..Settings::default() };
    let mut f = Fixture::with("cap", KeyboardMode::Conventional, settings, "");
    f.run_script("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 100) SELECT x FROM c;");
    f.assert_sees("5 rows read. Row cap reached. The result can have more rows.");
    assert!(f.screen().contains("1⋯"), "{}", f.screen());
}

#[test]
fn changes_report_rows_in_messages() {
    let mut f = Fixture::new("changes");
    f.run_script("CREATE TABLE t(a);\nINSERT INTO t VALUES (1), (2);");
    f.assert_sees("Statement 2: 2 rows changed.");
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 2);
}

#[test]
fn a_second_run_during_an_execution_keeps_everything() {
    let mut f = Fixture::new("busy");
    f.write(LONG);
    f.press(key(KeyCode::F(5)));
    f.pump_until(|f| f.sees("Running"));
    f.press(key(KeyCode::F(5)));
    f.assert_sees("in progress");
    f.prefix('x');
    f.idle();
}

#[test]
fn the_editor_accepts_text_during_an_execution() {
    let mut f = Fixture::new("edit-running");
    f.write(LONG);
    f.press(key(KeyCode::F(5)));
    f.pump_until(|f| f.sees("Running"));
    f.press(ctrl('a'));
    f.type_text("x");
    assert_eq!(f.app.console_mut().editor.text(), "x");
    f.prefix('x');
    f.idle();
}

#[test]
fn a_stop_interrupts_and_shows_the_state() {
    let mut f = Fixture::new("stop");
    f.write(LONG);
    f.press(key(KeyCode::F(5)));
    f.pump_until(|f| f.sees("1●"));
    f.prefix('x');
    f.idle();
    f.assert_sees("Statement 1: stopped.");
    assert!(f.screen().contains("1■"), "{}", f.screen());
}

#[test]
fn the_time_limit_shows_its_state() {
    let settings = Settings { sql_timeout: Some(Duration::from_millis(50)), ..Settings::default() };
    let mut f = Fixture::with("timeout", KeyboardMode::Conventional, settings, "");
    f.run_script(LONG);
    f.assert_sees("the time limit ended");
    assert!(f.screen().contains("1◷"), "{}", f.screen());
}

#[test]
fn read_only_refuses_a_change() {
    let settings = Settings { read_only: true, ..Settings::default() };
    let mut f = Fixture::with("read-only", KeyboardMode::Conventional, settings, "CREATE TABLE t(a);");
    f.run_script("INSERT INTO t VALUES (1);");
    f.assert_sees("readonly");
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 0);
}

#[test]
fn grid_keys_move_select_and_copy_tsv() {
    let mut f = Fixture::new("grid");
    f.run_script("SELECT 1 AS a, 'x' AS b UNION ALL SELECT 2, NULL UNION ALL SELECT 3, 'tab\there';");
    f.prefix('j');
    assert_eq!(f.app.console_mut().focus, Focus::Results);

    f.press(key(KeyCode::Down));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "2\n", "the cursor cell counts as the target");

    f.press(shift(KeyCode::Right));
    f.press(shift(KeyCode::Down));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "2\t\n3\t\"tab\there\"\n");

    f.press(ctrl('a'));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "1\tx\n2\t\n3\t\"tab\there\"\n");
    f.assert_sees("Copied.");
}

#[test]
fn alt_arrows_reach_the_limits_of_the_grid() {
    let mut f = Fixture::new("grid-limits");
    f.run_script("SELECT 1 AS a, 2 AS b UNION ALL SELECT 3, 4;");
    f.prefix('j');
    f.press(common::alt(KeyCode::Down));
    f.press(common::alt(KeyCode::Right));
    let view = f.app.console_mut().results[0].view;
    assert_eq!((view.row, view.col), (1, 1));
    f.press(common::alt(KeyCode::Up));
    f.press(common::alt(KeyCode::Left));
    let view = f.app.console_mut().results[0].view;
    assert_eq!((view.row, view.col), (0, 0));
}

#[test]
fn vim_keys_work_in_the_grid() {
    let mut f = Fixture::with("grid-vim", KeyboardMode::Vim, Settings::default(), "");
    f.write("SELECT 1 AS a, 2 AS b UNION ALL SELECT 3, 4 UNION ALL SELECT 5, 6;");
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    f.idle();
    f.prefix('j');
    f.type_text("Gl");
    let view = f.app.console_mut().results[0].view;
    assert_eq!((view.row, view.col), (2, 1));
    f.type_text("gg0");
    let view = f.app.console_mut().results[0].view;
    assert_eq!((view.row, view.col), (0, 0));
    f.type_text("vjy");
    assert_eq!(f.clipboard.borrow().text, "1\n3\n");
}

#[test]
fn brackets_switch_between_results_and_messages() {
    let mut f = Fixture::new("tabs");
    f.run_script("SELECT 'one' AS a; SELECT 'two' AS b;");
    f.assert_sees("two");
    f.prefix('j');
    f.press(key(KeyCode::Char('[')));
    f.assert_sees("one");
    f.press(key(KeyCode::Char('[')));
    assert!(f.app.console_mut().showing_messages());
    f.press(key(KeyCode::Char(']')));
    f.assert_sees("one");
}

#[test]
fn enter_opens_the_complete_value_and_esc_closes_it() {
    let long = "x".repeat(300);
    let mut f = Fixture::new("detail");
    f.run_script(&format!("SELECT 'line one\nline two {long}' AS bio;"));
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    f.assert_sees("bio · row 1");
    f.assert_sees("line two");
    f.assert_sees("318 characters · text");
    f.press(ctrl('c'));
    assert!(f.clipboard.borrow().text.starts_with("line one\nline two"));
    f.press(key(KeyCode::Esc));
    assert!(!f.sees("characters"));
}

#[test]
fn data_cannot_send_terminal_sequences() {
    let mut f = Fixture::new("escape");
    f.run_script("SELECT char(27) || '[31mred' || char(10) || 'next' || char(7) AS \"na\u{1b}me\";");
    let buffer = f.buffer();
    for cell in buffer.content() {
        assert!(!cell.symbol().chars().any(char::is_control), "{cell:?}");
    }
    f.assert_sees("[31mred↵next");
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    let buffer = f.buffer();
    for cell in buffer.content() {
        assert!(!cell.symbol().chars().any(char::is_control), "{cell:?}");
    }
}

#[test]
fn database_errors_cannot_send_terminal_sequences() {
    let mut f = Fixture::new("escape-error");
    f.run_script("SELECT * FROM \"a\u{1b}[31mb\";");
    let buffer = f.buffer();
    for cell in buffer.content() {
        assert!(!cell.symbol().chars().any(char::is_control), "{cell:?}");
    }
}

#[test]
fn a_click_selects_a_cell_and_a_tab() {
    let mut f = Fixture::new("mouse-grid");
    f.prefix('b');
    f.run_script("SELECT 1 AS a, 2 AS b UNION ALL SELECT 3, 4; SELECT 9 AS c;");
    f.buffer();
    let press = |column, row, kind| {
        Event::Mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE })
    };
    let left = MouseEventKind::Down(MouseButton::Left);

    // The tabs row sits below the editor. "Result 1" starts at the first column.
    let tabs_row = (0..30).find(|y| {
        let buffer = f.buffer();
        (0..8).map(|x| buffer[(x, *y)].symbol().to_owned()).collect::<String>() == "Result 1"
    });
    let tabs_row = tabs_row.expect("the tab row is on the screen");
    f.app.on_event(press(1, tabs_row, left));
    assert_eq!(f.app.console_mut().active, 0);

    f.buffer();
    f.app.on_event(press(6, tabs_row + 3, left));
    assert_eq!(f.app.console_mut().focus, Focus::Results);
    let view = f.app.console_mut().results[0].view;
    assert_eq!((view.row, view.col), (1, 1));
}

#[test]
fn the_wheel_moves_the_grid() {
    let mut f = Fixture::new("mouse-wheel");
    f.prefix('b');
    f.run_script("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 50) SELECT x FROM c;");
    f.buffer();
    let results_row = 25;
    let wheel = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 3,
        row: results_row,
        modifiers: KeyModifiers::NONE,
    });
    f.app.on_event(wheel);
    assert_eq!(f.app.console_mut().results[0].view.row, 3);
}

#[test]
fn a_transaction_by_sql_shows_its_state_and_the_close_asks() {
    let mut f = Fixture::with("tx-close", KeyboardMode::Conventional, Settings::default(), "CREATE TABLE t(a);");
    f.run_script("BEGIN;\nINSERT INTO t VALUES (1);");
    f.assert_sees("Transaction open");
    f.assert_sees("The transaction is not committed.");
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 0);

    f.prefix('c');
    f.assert_sees("[Commit and close]");
    f.assert_sees("[Rollback and close]");
    f.press(key(KeyCode::Enter));
    assert!(!f.app.should_quit(), "the default option cancels the close");

    f.prefix('c');
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Enter));
    f.pump_until(|f| f.app.should_quit());
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 1);
}

#[test]
fn rollback_and_close_discards_the_change() {
    let mut f = Fixture::with("tx-rollback", KeyboardMode::Conventional, Settings::default(), "CREATE TABLE t(a);");
    f.run_script("BEGIN;\nINSERT INTO t VALUES (1);");
    f.prefix('c');
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Enter));
    f.pump_until(|f| f.app.should_quit());
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 0);
}

#[test]
fn a_failed_commit_at_the_close_keeps_the_console_open_and_offers_a_retry() {
    let setup = "CREATE TABLE p(id PRIMARY KEY); CREATE TABLE c(p REFERENCES p(id) DEFERRABLE INITIALLY DEFERRED);";
    let mut f = Fixture::with("tx-fail", KeyboardMode::Conventional, Settings::default(), setup);
    f.run_script("PRAGMA foreign_keys = ON;\nBEGIN;\nINSERT INTO c VALUES (1);");
    f.prefix('c');
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Enter));
    f.pump_until(|f| f.sees("[Try again]"));
    assert!(!f.app.should_quit());
    f.assert_sees("FOREIGN KEY");
    f.assert_sees("Transaction open");

    f.press(key(KeyCode::Esc));
    assert!(!f.app.should_quit());
    f.prefix('R');
    f.pump_until(|f| f.sees("No transaction"));
    f.prefix('c');
    assert!(f.app.should_quit());
}

#[test]
fn the_close_during_an_execution_asks_and_stops_first() {
    let mut f = Fixture::new("close-busy");
    f.write(LONG);
    f.press(key(KeyCode::F(5)));
    f.pump_until(|f| f.sees("Running"));

    f.prefix('c');
    f.assert_sees("[Stop and close]");
    f.press(key(KeyCode::Esc));
    assert!(f.sees("Running"), "a canceled close requests no stop");

    f.prefix('c');
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Enter));
    f.pump_until(|f| f.app.should_quit());
}

#[test]
fn manual_mode_needs_a_commit() {
    let mut f = Fixture::with("manual", KeyboardMode::Conventional, Settings::default(), "CREATE TABLE t(a);");
    f.prefix('m');
    f.assert_sees("Manual");
    f.run_script("INSERT INTO t VALUES (1);");
    f.assert_sees("Transaction open");
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 0);

    f.prefix('C');
    f.pump_until(|f| f.sees("No transaction"));
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 1);

    f.run_script("INSERT INTO t VALUES (2);");
    f.prefix('R');
    f.pump_until(|f| f.sees("No transaction"));
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 1);
}

#[test]
fn the_change_to_autocommit_asks_with_an_open_transaction() {
    let mut f = Fixture::with("mode-change", KeyboardMode::Conventional, Settings::default(), "CREATE TABLE t(a);");
    f.prefix('m');
    f.run_script("INSERT INTO t VALUES (1);");

    f.prefix('m');
    f.assert_sees("[Commit and use autocommit]");
    f.press(key(KeyCode::Esc));
    f.assert_sees("Manual");
    f.assert_sees("Transaction open");

    f.prefix('m');
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Tab));
    f.press(key(KeyCode::Enter));
    f.pump_until(|f| f.sees("Auto ·"));
    f.assert_sees("No transaction");
    assert_eq!(f.file_query("SELECT count(*) FROM t"), 1);
}

#[test]
fn commit_and_mode_are_blocked_during_an_execution() {
    let mut f = Fixture::new("blocked");
    f.write(LONG);
    f.press(key(KeyCode::F(5)));
    f.pump_until(|f| f.sees("Running"));
    f.prefix('C');
    f.assert_sees("in progress");
    f.prefix('m');
    f.assert_sees("in progress");
    f.assert_sees("Auto");
    f.prefix('x');
    f.idle();
}

#[test]
fn commit_without_a_transaction_reports_it() {
    let mut f = Fixture::new("no-commit");
    f.prefix('C');
    f.assert_sees("No transaction is open.");
}

#[test]
fn suggestions_use_the_real_catalog_and_load_columns_on_demand() {
    let setup = "CREATE TABLE users(id, full_name); CREATE VIEW names AS SELECT full_name FROM users;";
    let mut f = Fixture::with("catalog", KeyboardMode::Conventional, Settings::default(), setup);
    f.type_text("SELECT * FROM us");
    f.pump_until(|f| f.sees("tbl users"));

    f.press(key(KeyCode::Tab));
    assert_eq!(f.app.console_mut().editor.text(), "SELECT * FROM users");

    f.type_text(" u WHERE u.");
    f.pump_until(|f| f.sees("col full_name"));
    f.assert_sees("col id");
}

#[test]
fn ddl_in_the_application_updates_the_catalog() {
    let mut f = Fixture::with("catalog-ddl", KeyboardMode::Conventional, Settings::default(), "CREATE TABLE first_table(a);");
    f.type_text("SELECT * FROM fir");
    f.pump_until(|f| f.sees("tbl first_table"));
    f.run_script("CREATE TABLE second_table(a);");
    f.select_all_and_write("SELECT * FROM ");
    f.type_text("sec");
    f.pump_until(|f| f.sees("tbl second_table"));
}

#[test]
fn the_keys_screen_opens_and_closes() {
    let mut f = Fixture::new("keys");
    f.press(key(KeyCode::F(1)));
    f.assert_sees("Run the selection");
    f.press(key(KeyCode::Esc));
    assert!(!f.sees("Run the selection"));
    f.prefix('?');
    f.assert_sees("Run the selection");
}

#[test]
fn the_plain_theme_has_no_color_on_the_full_screen() {
    use ratatui::style::Color;
    let mut f = Fixture::new("plain");
    f.run_script("SELECT 1, 'a', NULL; SELECT * FROM missing;");
    for cell in f.buffer().content() {
        assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{cell:?}");
    }
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE })
}

fn grid_of(f: &mut Fixture, sql: &str) {
    f.run_script(sql);
    f.prefix('j');
}

const WORDS: &str = "SELECT * FROM (VALUES ('alpha', 'one'), ('Beta', 'two'), ('gamma', 'beta'));";

#[test]
fn ctrl_f_finds_text_in_the_loaded_rows_and_moves_the_cursor() {
    let mut f = Fixture::new("grid-find");
    grid_of(&mut f, WORDS);
    f.press(ctrl('f'));
    f.assert_sees("Enter next");
    f.type_text("BETA");
    f.pump_until(|f| f.sees("1 of 2"));
    let result = &f.app.console_mut().results[0];
    assert_eq!((result.view.row, result.view.col), (1, 0));

    f.press(key(KeyCode::Enter));
    f.assert_sees("2 of 2");
    let result = &f.app.console_mut().results[0];
    assert_eq!((result.view.row, result.view.col), (2, 1));
    f.press(key(KeyCode::Up));
    f.assert_sees("1 of 2");
    f.press(key(KeyCode::Down));
    f.assert_sees("2 of 2");
}

#[test]
fn the_grid_search_reports_a_miss_and_escape_closes_the_bar() {
    let mut f = Fixture::new("grid-find-miss");
    grid_of(&mut f, WORDS);
    f.press(ctrl('f'));
    f.type_text("zz");
    f.pump_until(|f| f.sees("no matches"));
    f.press(key(KeyCode::Backspace));
    f.press(key(KeyCode::Backspace));
    assert!(!f.sees("no matches"));
    f.press(key(KeyCode::Esc));
    assert!(!f.app.console_mut().search_is_open());
    f.assert_sees("3 rows");
}

#[test]
fn slash_opens_the_grid_search_in_vim() {
    let mut f = Fixture::with("grid-find-vim", KeyboardMode::Vim, Settings::default(), "");
    f.select_all_and_write(WORDS);
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    f.idle();
    f.prefix('j');
    f.press(key(KeyCode::Char('/')));
    f.type_text("two");
    f.pump_until(|f| f.sees("1 of 1"));
}

#[test]
fn the_grid_search_bar_takes_each_character_as_text() {
    let mut f = Fixture::new("grid-find-text");
    grid_of(&mut f, "SELECT 'a]x[', 'b'; SELECT 2;");
    f.app.console_mut().active = 0;
    f.press(ctrl('f'));
    f.type_text("]x[");
    f.pump_until(|f| f.sees("1 of 1"));
    assert_eq!(f.app.console_mut().active, 0);
}

#[test]
fn a_paste_enters_the_grid_search_bar_and_keeps_the_editor() {
    let mut f = Fixture::new("grid-find-paste");
    grid_of(&mut f, WORDS);
    let before = f.app.console_mut().editor.text();
    f.press(ctrl('f'));
    f.app.on_event(Event::Paste("gam".to_owned()));
    f.pump_until(|f| f.sees("1 of 1"));
    assert_eq!(f.app.console_mut().editor.text(), before);
    assert_eq!(f.app.console_mut().focus, Focus::Results);
}

#[test]
fn x_closes_a_result_and_frees_its_budget() {
    let mut f = Fixture::new("grid-close");
    grid_of(&mut f, "SELECT 'first'; SELECT 'second'; SELECT 'third';");
    f.app.console_mut().active = 0;
    let before = f.app.console_mut().results[0].table.bytes;
    assert!(before > 0);
    f.press(key(KeyCode::Char('x')));
    assert_eq!(f.app.console_mut().results.len(), 2);
    f.assert_sees("Result 2");
    assert!(!f.sees("Result 1"));
    f.assert_sees("'second'");

    f.press(key(KeyCode::Delete));
    f.assert_sees("Result 3");
    f.assert_sees("'third'");
    f.press(key(KeyCode::Char('x')));
    assert!(f.app.console_mut().results.is_empty());
    assert!(f.app.console_mut().showing_messages());
    f.press(key(KeyCode::Char('x')));
    f.assert_sees("Messages");
    assert!(!f.app.should_quit());
}

#[test]
fn a_new_execution_starts_the_result_numbers_again() {
    let mut f = Fixture::new("grid-numbers");
    grid_of(&mut f, "SELECT 1; SELECT 2;");
    f.app.console_mut().active = 0;
    f.press(key(KeyCode::Char('x')));
    f.run_script("SELECT 3;");
    f.assert_sees("Result 1");
}

#[test]
fn a_result_that_receives_rows_stays_open() {
    let settings = Settings { row_cap: usize::MAX, ..Settings::default() };
    let mut f = Fixture::with("grid-close-reading", KeyboardMode::Conventional, settings, "");
    f.write("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT x FROM c;");
    f.press(key(KeyCode::F(5)));
    f.pump_until(|f| f.app.console_mut().results.first().is_some_and(|r| !r.table.rows.is_empty()));
    f.prefix('j');
    f.press(key(KeyCode::Char('x')));
    assert_eq!(f.app.console_mut().results.len(), 1);
    f.assert_sees("Ctrl+W x stops the script");
    f.prefix('x');
    f.idle();
    f.app.console_mut().active = 0;
    f.press(key(KeyCode::Char('x')));
    assert!(f.app.console_mut().results.is_empty());
}

#[test]
fn angle_keys_change_the_width_and_equals_resets_it() {
    let mut f = Fixture::new("grid-width");
    grid_of(&mut f, "SELECT 'a value' AS one, 'b' AS two;");
    assert_eq!(f.app.console_mut().results[0].table.width(0), 7);
    f.press(key(KeyCode::Char('<')));
    f.press(key(KeyCode::Char('<')));
    assert_eq!(f.app.console_mut().results[0].table.width(0), 5);
    f.assert_sees("a val b");
    f.press(key(KeyCode::Char('>')));
    assert_eq!(f.app.console_mut().results[0].table.width(0), 6);
    f.press(key(KeyCode::Char('=')));
    assert_eq!(f.app.console_mut().results[0].table.width(0), 7);
    f.assert_sees("a value b");
}

#[test]
fn a_drag_on_the_edge_of_a_header_changes_the_width() {
    let mut f = Fixture::new("grid-drag");
    f.prefix('b');
    grid_of(&mut f, "SELECT 'a value' AS one, 'b' AS two;");
    f.screen();
    let buffer = f.buffer();
    let header = (0..buffer.area.height)
        .find(|y| (0..3).map(|x| buffer[(x, *y)].symbol()).collect::<String>() == "one")
        .unwrap();
    f.app.on_event(mouse(MouseEventKind::Down(MouseButton::Left), 7, header));
    f.app.on_event(mouse(MouseEventKind::Drag(MouseButton::Left), 12, header));
    f.app.on_event(mouse(MouseEventKind::Up(MouseButton::Left), 12, header));
    assert_eq!(f.app.console_mut().results[0].table.width(0), 12);
    assert_eq!(f.app.console_mut().results[0].view.anchor, None);
    f.screen();
    f.app.on_event(mouse(MouseEventKind::Drag(MouseButton::Left), 20, header));
    assert_eq!(f.app.console_mut().results[0].table.width(0), 12);
}

#[test]
fn a_frame_with_a_large_value_stays_fast() {
    let mut f = Fixture::new("grid-large");
    f.run_script("SELECT replace(hex(zeroblob(20000000)), '0', 'x') AS big;");
    f.prefix('j');
    assert!(f.app.console_mut().results[0].table.bytes >= 40_000_000);
    let started = Instant::now();
    for _ in 0..10 {
        f.screen();
    }
    let frame = started.elapsed() / 10;
    assert!(frame < Duration::from_millis(50), "{frame:?}");
}

fn underlined(f: &mut Fixture, text: &str) -> bool {
    use ratatui::style::Modifier;
    let buffer = f.buffer();
    let width = text.chars().count() as u16;
    (0..buffer.area.height).any(|y| {
        (0..buffer.area.width.saturating_sub(width)).any(|x| {
            text.chars().enumerate().all(|(index, c)| {
                let cell = &buffer[(x + index as u16, y)];
                cell.symbol() == c.to_string() && cell.modifier.contains(Modifier::UNDERLINED)
            })
        })
    })
}

#[test]
fn the_editor_marks_the_token_of_a_database_error() {
    let mut f = Fixture::new("error-spot");
    f.run_script("SELECT 1;\nSELECT * FROMM t;\nSELECT 3;");
    f.assert_sees("FROMM");
    assert!(underlined(&mut f, "FROMM"), "{}", f.screen());
    assert!(!underlined(&mut f, "SELECT"));
}

#[test]
fn an_edit_removes_the_mark_of_a_database_error() {
    let mut f = Fixture::new("error-spot-edit");
    f.run_script("SELECT * FROMM t;");
    assert!(underlined(&mut f, "FROMM"));
    f.app.console_mut().focus = Focus::Editor;
    f.press(key(KeyCode::Char('x')));
    assert!(!underlined(&mut f, "FROMM"));
}

#[test]
fn an_edit_during_the_execution_leaves_no_mark() {
    let mut f = Fixture::new("error-spot-running");
    f.write("SELECT * FROMM t;");
    f.press(key(KeyCode::F(5)));
    f.press(key(KeyCode::Char(' ')));
    f.idle();
    assert!(!underlined(&mut f, "FROMM"));
}

#[test]
fn a_new_execution_removes_the_mark_of_a_database_error() {
    let mut f = Fixture::new("error-spot-new");
    f.run_script("SELECT * FROMM t;");
    assert!(underlined(&mut f, "FROMM"));
    f.press(key(KeyCode::F(5)));
    f.idle();
    assert!(underlined(&mut f, "FROMM"), "the same error marks the token again");
    f.run_script("SELECT 2;");
    assert!(!underlined(&mut f, "SELECT"));
}

#[test]
fn an_error_during_the_read_keeps_the_budget_exact() {
    let settings = Settings { row_cap: usize::MAX, ..Settings::default() };
    let mut f = Fixture::with("read-error-budget", KeyboardMode::Conventional, settings, "");
    f.run_script(
        "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000) \
         SELECT CASE WHEN x > 300 THEN abs(-9223372036854775807 - 1) ELSE x END FROM c;",
    );
    assert_eq!(f.app.console_mut().results[0].table.rows.len(), 300);
    f.prefix('j');
    f.app.console_mut().active = 0;
    f.assert_sees("300 rows");
    f.press(key(KeyCode::Char('x')));
    f.assert_sees("Data 0/256 MiB");
}

#[test]
fn the_detail_window_of_a_large_value_stays_fast() {
    let mut f = Fixture::new("detail-large");
    f.run_script("SELECT replace(hex(zeroblob(20000000)), '0', 'x') AS big;");
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    f.assert_sees("40,000,000 characters");
    let started = Instant::now();
    for _ in 0..5 {
        f.press(key(KeyCode::Down));
        f.screen();
    }
    let frame = started.elapsed() / 5;
    println!("detail frame {frame:?}");
    assert!(frame < Duration::from_millis(50), "{frame:?}");
}

#[test]
fn the_detail_window_scrolls_to_the_end_and_keeps_the_text_complete() {
    let mut f = Fixture::new("detail-scroll");
    f.run_script("SELECT 'first row' || char(10) || 'second row' AS value;");
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    // The editor behind the window also shows this text, so the test counts it.
    let shown = |f: &mut Fixture| f.screen().matches("first row").count();
    let at_the_top = shown(&mut f);
    f.assert_sees("second row");
    for _ in 0..5 {
        f.press(key(KeyCode::Down));
    }
    assert_eq!(shown(&mut f), at_the_top - 1);
    f.assert_sees("second row");
    f.press(key(KeyCode::Up));
    assert_eq!(shown(&mut f), at_the_top, "the view starts at the last row");
    f.screen();
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "first row\nsecond row");
}

fn finish_with_stop(f: &mut Fixture, statuses: Vec<sqln::exec::StatementStatus>) {
    use sqln::editor::RunScope;
    use sqln::exec::{ExecEvent, ExecutionEnd, StatementKey};
    use sqln::grid::Budget;

    let budget = Budget::new(1024);
    let plan = f.app.console_mut().editor.plan_run(RunScope::Script).unwrap();
    let console = f.app.console_mut().id;
    f.app.console_mut().begin_execution(7, &plan, &budget);
    for (index, status) in statuses.into_iter().enumerate() {
        let key = StatementKey { console, execution: 7, index };
        let end = Some(Default::default());
        let event = ExecEvent::StatementFinished { key, status, end, error_offset: None };
        f.app.console_mut().on_event(event, &budget);
    }
    let event = ExecEvent::ExecutionFinished {
        console,
        execution: 7,
        end: ExecutionEnd::Stopped,
        message: None,
    };
    f.app.console_mut().on_event(event, &budget);
}

#[test]
fn a_stop_between_statements_explains_the_end() {
    use sqln::exec::StatementStatus;
    let mut f = Fixture::new("stop-between");
    f.write("SELECT 1; SELECT 2; SELECT 3;");
    let statuses =
        vec![StatementStatus::Success, StatementStatus::NotExecuted, StatementStatus::NotExecuted];
    finish_with_stop(&mut f, statuses);
    f.assert_sees("Stopped before statement 2. The database did not receive it.");
}

#[test]
fn a_statement_that_ends_after_the_stop_request_explains_the_end() {
    let mut f = Fixture::new("stop-success");
    f.write("SELECT 1;");
    finish_with_stop(&mut f, vec![sqln::exec::StatementStatus::Success]);
    f.assert_sees("The last statement ended before the stop request.");
}

#[test]
fn a_stopped_statement_needs_no_second_message() {
    let mut f = Fixture::new("stop-once");
    f.write("SELECT 1; SELECT 2;");
    use sqln::exec::StatementStatus;
    finish_with_stop(&mut f, vec![StatementStatus::Stopped, StatementStatus::NotExecuted]);
    f.assert_sees("Statement 1: stopped.");
    assert!(!f.sees("Stopped before"));
}
