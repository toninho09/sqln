// The complete interface: several consoles, the navigator, the form, and the panels.
// Keys enter the application, and SQL runs in real SQLite files.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::sample_catalog;
use crate::common::{ctrl, key, temp_db};
use async_trait::async_trait;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use sqln::app::{App, Options};
use sqln::catalog::Catalog;
use sqln::clipboard::{Clipboard, CopyOutcome, MemoryClipboard};
use sqln::connection::{CatalogState, Definition, Source};
use sqln::connector::sqlite::SqliteTarget;
use sqln::connector::target::{
    Address, ClientCertificate, NetworkTarget, Tls, TlsFiles, parse_uri,
};
use sqln::connector::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use sqln::console::Focus;
use sqln::editor::KeyboardMode;
use sqln::exec::{ResultSink, Settings};
use sqln::form::{ConnectionForm, Field, Kind};
use sqln::sql::Dialect;
use sqln::ui::theme::Theme;
use tokio::sync::Notify;

const LONG: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000000000) SELECT count(*) FROM c;";
const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

struct Fixture {
    app: App,
    paths: Vec<PathBuf>,
}

fn database(name: &str, setup: &str) -> PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(setup)
        .unwrap();
    path
}

impl Fixture {
    fn open(paths: Vec<PathBuf>, keyboard: KeyboardMode) -> Self {
        let definitions = paths[1..]
            .iter()
            .map(|path| Definition::sqlite(SqliteTarget::new(path)))
            .collect();
        let options = Options {
            keyboard,
            database: Some(paths[0].clone()),
            connections: definitions,
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
            paths,
        }
    }

    fn new(name: &str, setup: &str) -> Self {
        Self::open(vec![database(name, setup)], KeyboardMode::Conventional)
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn prefix(&mut self, c: char) {
        self.press(ctrl('w'));
        self.code(KeyCode::Char(c));
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.code(KeyCode::Char(c));
        }
    }

    fn write(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    fn navigator(&mut self) {
        self.prefix('h');
        assert!(self.app.navigator_focused());
    }

    fn pump_until(&mut self, condition: impl Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if condition(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the condition did not happen:\n{}",
                self.screen()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn tree_screen(&mut self) -> String {
        self.screen()
            .lines()
            .skip(2)
            .take(usize::from(HEIGHT - 4))
            .map(|line| line.split('│').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn pump_tree(&mut self, text: &str) {
        self.pump_until(|f| f.tree_screen().contains(text));
    }

    #[track_caller]
    fn assert_not_tree(&mut self, text: &str) {
        assert!(
            !self.tree_screen().contains(text),
            "{text:?} remains in the explorer:\n{}",
            self.screen()
        );
    }

    fn pump_sees(&mut self, text: &str) {
        let text = text.to_owned();
        self.pump_until(move |f| f.sees(&text));
    }

    /// Waits for the active console.
    fn idle(&mut self) {
        self.pump_until(|f| !f.app.console().is_busy());
        self.app.drain_events();
    }

    /// Runs the whole text of the active console.
    fn run(&mut self, sql: &str) {
        self.app.console_mut().focus = Focus::Editor;
        self.press(ctrl('a'));
        self.code(KeyCode::Delete);
        self.write(sql);
        self.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
        self.idle();
    }

    fn buffer_at(&mut self, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer(&mut self) -> Buffer {
        self.buffer_at(WIDTH, HEIGHT)
    }

    fn screen(&mut self) -> String {
        let buffer = self.buffer();
        rows(&buffer).join("\n")
    }

    fn sees(&mut self, text: &str) -> bool {
        self.screen().contains(text)
    }

    #[track_caller]
    fn assert_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(
            screen.contains(text),
            "{text:?} is not on the screen:\n{screen}"
        );
    }

    #[track_caller]
    fn assert_not_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(
            !screen.contains(text),
            "{text:?} is on the screen:\n{screen}"
        );
    }

    fn query(&self, index: usize, sql: &str) -> i64 {
        let conn = rusqlite::Connection::open(&self.paths[index]).unwrap();
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn click(&mut self, column: u16, row: u16) {
        self.buffer();
        let event = mouse(MouseEventKind::Down(MouseButton::Left), column, row);
        self.app.on_event(event);
    }

    /// Returns the column and the row of a text on the screen.
    fn locate(&mut self, text: &str) -> (u16, u16) {
        let buffer = self.buffer();
        for (y, row) in rows(&buffer).iter().enumerate() {
            if let Some(byte) = row.find(text) {
                return (row[..byte].chars().count() as u16, y as u16);
            }
        }
        panic!("{text:?} is not on the screen:\n{}", self.screen());
    }

    fn click_text(&mut self, text: &str) {
        let (column, row) = self.locate(text);
        self.click(column, row);
    }

    /// Opens the form and writes a SQLite connection.
    fn fill_sqlite(&mut self, name: &str, path: &Path) {
        self.navigator();
        self.code(KeyCode::Char('a'));
        self.code(KeyCode::Left);
        self.code(KeyCode::Left);
        assert_eq!(self.app.form().unwrap().kind, Kind::Sqlite);
        self.focus_form(Field::Name);
        self.type_text(name);
        self.focus_form(Field::File);
        self.write(path.to_str().unwrap());
    }

    fn focus_form(&mut self, field: Field) {
        crate::common::focus_form_field(&mut self.app, field);
    }

    fn save(&mut self) {
        self.focus_form(Field::Save);
        self.code(KeyCode::Enter);
    }
}

fn rows(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn consoles_keep_their_own_text_and_results() {
    let mut f = Fixture::new("two-consoles", "");
    f.run("SELECT 'first console' AS a;");
    f.assert_sees("first console");

    f.navigator();
    f.code(KeyCode::Char('n'));
    assert_eq!(f.app.consoles().len(), 2);
    assert_eq!(f.app.console().editor.text(), "");
    f.assert_sees("console-2");
    f.assert_not_sees("first console");

    f.run("SELECT 'second console' AS a;");
    f.assert_sees("second console");
    f.prefix('p');
    f.assert_sees("first console");
    f.assert_not_sees("second console");
    assert_eq!(f.app.console().name, "console-1");
    f.prefix('n');
    f.assert_sees("second console");
    f.prefix('n');
    assert_eq!(f.app.console().name, "console-1", "the tabs wrap");
    let histories: Vec<String> = f
        .app
        .consoles()
        .iter()
        .map(|console| {
            console
                .messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    assert!(histories[0].contains("first console"));
    assert!(!histories[0].contains("second console"));
    assert!(histories[1].contains("second console"));
    assert!(!histories[1].contains("first console"));
}

#[test]
fn a_console_works_while_another_one_runs() {
    let mut f = Fixture::new("parallel", "");
    f.app.console_mut().editor.on_paste(LONG);
    f.code(KeyCode::F(5));
    f.pump_until(|f| f.app.console().phase == sqln::console::Phase::Running);

    f.navigator();
    f.code(KeyCode::Char('n'));
    f.run("SELECT 'free' AS a;");
    f.assert_sees("free");
    assert!(f.screen().contains("s  console-1"), "{}", f.screen());
    assert!(
        f.app.consoles()[0].is_busy(),
        "the first console still runs"
    );

    f.prefix('p');
    f.prefix('x');
    f.idle();
    f.assert_sees("console-2");
}

#[test]
fn a_second_execution_stays_refused_in_its_own_console() {
    let mut f = Fixture::new("one-per-console", "");
    f.app.console_mut().editor.on_paste(LONG);
    f.code(KeyCode::F(5));
    f.code(KeyCode::F(5));
    f.assert_sees("The console has an execution in progress.");
    f.prefix('x');
    f.idle();
}

#[test]
fn closing_a_console_frees_its_rows_and_the_last_tab_shows_the_home_screen() {
    let mut f = Fixture::new("close-tab", "");
    f.run("SELECT 'value in the first console' AS a;");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('p');
    f.prefix('c');
    assert_eq!(f.app.consoles().len(), 1);
    assert_eq!(f.app.console().name, "console-2");
    f.assert_sees("Data 0/256 MiB");
    assert!(!f.app.home());
    f.prefix('c');
    assert!(f.app.home());
    assert!(f.app.console().messages.is_empty());
    assert!(!f.app.should_quit());
}

#[test]
fn the_tab_after_a_closed_tab_keeps_the_selection() {
    let mut f = Fixture::new("close-left", "");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('p');
    assert_eq!(f.app.console().name, "console-2");
    f.prefix('p');
    f.prefix('c');
    assert_eq!(f.app.console().name, "console-2");
}

#[test]
fn a_close_in_the_background_keeps_the_selected_tab() {
    let mut f = Fixture::new("close-background", "");
    for _ in 0..2 {
        f.navigator();
        f.code(KeyCode::Char('n'));
    }
    f.prefix('n');
    assert_eq!(f.app.console().name, "console-1");
    f.app.console_mut().editor.on_paste(LONG);
    f.code(KeyCode::F(5));
    f.pump_until(|f| f.app.console().phase == sqln::console::Phase::Running);
    f.prefix('c');
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.prefix('n');
    assert_eq!(f.app.console().name, "console-2");
    f.pump_until(|f| f.app.consoles().len() == 2);
    assert_eq!(f.app.console().name, "console-2");
}

#[test]
fn a_canceled_exit_leaves_the_close_of_a_tab_as_it_was() {
    let mut f = Fixture::new("exit-canceled", "CREATE TABLE t(a);");
    f.run("BEGIN; INSERT INTO t VALUES (1);");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('q');
    f.assert_sees("console-1: This console has an open transaction");
    f.code(KeyCode::Esc);
    f.prefix('n');
    f.prefix('c');
    assert_eq!(f.app.consoles().len(), 1, "{}", f.screen());
    assert!(!f.app.should_quit());
}

#[test]
fn a_click_on_a_main_tab_selects_it() {
    let mut f = Fixture::new("click-tab", "");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.click_text("console-1");
    assert_eq!(f.app.console().name, "console-1");
}

#[test]
fn the_active_tab_stays_visible_with_many_consoles() {
    let mut f = Fixture::new("many-tabs", "");
    for _ in 0..30 {
        f.navigator();
        f.code(KeyCode::Char('n'));
    }
    f.assert_sees("console-31");
    f.click_text("console-31");
    assert_eq!(f.app.console().name, "console-31");
}

#[test]
fn the_exit_asks_each_console_and_applies_each_answer_at_once() {
    let mut f = Fixture::new("exit", "CREATE TABLE t(a);");
    f.run("BEGIN; INSERT INTO t VALUES (1);");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.run("SELECT 1;");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.run("BEGIN;");
    f.pump_sees("Transaction open");

    f.prefix('q');
    f.assert_sees("console-1: This console has an open transaction");
    assert_eq!(f.app.console().name, "console-1");
    // The third option is the commit.
    f.code(KeyCode::Tab);
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.pump_sees("console-3: This console has an open transaction");
    assert_eq!(
        f.query(0, "SELECT count(*) FROM t"),
        1,
        "the commit runs before the next question"
    );
    assert!(!f.app.should_quit());

    f.code(KeyCode::Esc);
    assert!(
        !f.app.should_quit(),
        "the cancel keeps the application open"
    );
    assert_eq!(f.app.consoles().len(), 3);
    f.assert_not_sees("This console has an open transaction");
    assert_eq!(
        f.query(0, "SELECT count(*) FROM t"),
        1,
        "the cancel does not undo the commit"
    );

    f.prefix('q');
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.pump_until(|f| f.app.should_quit());
}

#[test]
fn the_exit_stops_a_running_console_first() {
    let mut f = Fixture::new("exit-busy", "");
    f.app.console_mut().editor.on_paste(LONG);
    f.code(KeyCode::F(5));
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('q');
    f.assert_sees("console-1: Stop the execution and close?");
    assert_eq!(
        f.app.console().name,
        "console-1",
        "the question shows its console"
    );
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    f.pump_until(|f| f.app.should_quit());
}

#[test]
fn the_exit_without_open_work_quits_at_once() {
    let mut f = Fixture::new("exit-clean", "");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('q');
    assert!(f.app.should_quit());
}

#[test]
fn closing_one_console_leaves_the_transaction_of_another_one() {
    let mut f = Fixture::new("close-other", "CREATE TABLE t(a);");
    f.run("BEGIN; INSERT INTO t VALUES (1);");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('c');
    assert_eq!(f.app.consoles().len(), 1);
    assert_eq!(f.app.console().transaction, TransactionState::Open);
}

#[test]
fn the_navigator_opens_each_level_of_the_catalog() {
    let setup = "CREATE TABLE users(id, name); CREATE VIEW names AS SELECT name FROM users;";
    let mut f = Fixture::new("tree", setup);
    f.assert_sees("tree.db");
    f.assert_sees("Pool 0/5");
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_tree("main");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.assert_sees("▸ users");
    f.assert_sees("▸ names (view)");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.assert_sees("users");
    f.code(KeyCode::Right);
    f.pump_tree("id");
    f.assert_sees("name");

    // Left closes an open row, and then it moves to the parent.
    f.code(KeyCode::Left);
    f.assert_sees("▸ users");
    f.code(KeyCode::Left);
    f.code(KeyCode::Left);
    f.assert_not_tree("users");
}

#[test]
fn vim_keys_move_in_the_navigator_and_stay_text_in_the_editor() {
    let path = database("tree-vim", "CREATE TABLE t(a);");
    let mut f = Fixture::open(vec![path], KeyboardMode::Vim);
    f.navigator();
    f.code(KeyCode::Char('l'));
    f.pump_tree("main");
    f.code(KeyCode::Char('j'));
    f.code(KeyCode::Char('j'));
    f.code(KeyCode::Char('l'));
    f.pump_sees("▸ t");
    f.code(KeyCode::Char('k'));
    f.code(KeyCode::Char('h'));
    f.code(KeyCode::Char('h'));
    f.assert_not_tree("main");

    f.prefix('l');
    f.code(KeyCode::Char('i'));
    f.type_text("jk");
    assert_eq!(f.app.console().editor.text(), "jk");
}

#[test]
fn a_click_in_the_navigator_selects_and_its_mark_opens() {
    let mut f = Fixture::new("tree-click", "CREATE TABLE t(a);");
    let (column, row) = f.locate("▸ tree-click");
    f.click(column + 4, row);
    assert!(f.app.navigator_focused());
    f.assert_not_tree("main");
    f.click(column, row);
    f.pump_tree("main");
    f.click(column, row);
    f.assert_not_tree("main");
}

#[test]
fn the_wheel_moves_the_navigator() {
    let setup: String = (0..60)
        .map(|index| format!("CREATE TABLE t{index}(a);"))
        .collect();
    let mut f = Fixture::new("tree-wheel", &setup);
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_tree("main");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.pump_sees("▸ t0");
    for _ in 0..30 {
        f.buffer();
        f.app.on_event(mouse(MouseEventKind::ScrollDown, 2, 3));
    }
    f.assert_not_sees("▸ t0 ");
    let selected = sqln::navigator::Row::name;
    let row = f.app.navigator().current(f.app.connections()).unwrap();
    assert_ne!(selected(&row, f.app.connections()), "t0");
}

#[test]
fn a_catalog_read_keeps_the_cursor_and_the_open_columns() {
    let mut f = Fixture::new(
        "tree-reload",
        "CREATE TABLE a_first(x); CREATE TABLE b_second(y);",
    );
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_tree("main");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.pump_sees("  y");

    f.prefix('l');
    f.run("DROP TABLE a_first;");
    f.pump_until(|f| !f.sees("▸ a_first"));
    f.pump_sees("  y");
    f.navigator();
    let row = f.app.navigator().current(f.app.connections()).unwrap();
    assert_eq!(row.name(f.app.connections()), "b_second");
}

#[test]
fn a_removed_item_moves_the_cursor_to_its_parent() {
    let mut f = Fixture::new("tree-removed", "CREATE TABLE a(x); CREATE TABLE gone(y);");
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_tree("main");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.prefix('l');
    f.run("DROP TABLE gone;");
    f.pump_until(|f| !f.sees("▸ gone"));
    let row = f.app.navigator().current(f.app.connections()).unwrap();
    assert_eq!(row.name(f.app.connections()), "main");
}

#[test]
fn r_reads_the_catalog_again() {
    let mut f = Fixture::new("tree-refresh", "CREATE TABLE before_change(a);");
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_tree("main");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.pump_sees("before_change");
    rusqlite::Connection::open(&f.paths[0])
        .unwrap()
        .execute_batch("CREATE TABLE outside_change(a);")
        .unwrap();
    f.code(KeyCode::Char('r'));
    f.pump_sees("outside_change");
}

#[test]
fn a_catalog_failure_shows_in_the_tree_and_a_refresh_can_repair_it() {
    let path = temp_db("bad");
    let mut f = Fixture::open(
        vec![database("first-file", ""), path.clone()],
        KeyboardMode::Conventional,
    );
    f.navigator();
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.pump_sees("(failed)");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE fixed(a);")
        .unwrap();
    f.code(KeyCode::Char('r'));
    f.pump_until(|f| !f.sees("(failed)"));
}

#[test]
fn each_console_uses_the_database_of_its_connection() {
    let first = database(
        "connection-a",
        "CREATE TABLE only_a(x); INSERT INTO only_a VALUES (1);",
    );
    let second = database(
        "connection-b",
        "CREATE TABLE only_b(x); INSERT INTO only_b VALUES (2);",
    );
    let mut f = Fixture::open(vec![first, second], KeyboardMode::Conventional);
    f.assert_sees("connection-a");
    f.assert_sees("connection-b");
    f.run("SELECT x + 100 AS shown FROM only_b;");
    f.prefix('j');
    f.prefix('k');
    assert!(
        f.sees("no such table") || f.sees("Messages"),
        "{}",
        f.screen()
    );

    f.navigator();
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    assert_eq!(f.app.consoles().len(), 2);
    assert!(!f.app.navigator_focused());
    f.run("SELECT x + 100 AS shown FROM only_b;");
    f.assert_sees("102");
    f.assert_sees("connection-b");

    f.navigator();
    f.code(KeyCode::Enter);
    assert_eq!(f.app.consoles().len(), 2, "Enter selects the open console");
}

#[test]
fn the_tree_keeps_the_order_of_the_command_line() {
    let paths = ["order-a", "order-b", "order-c"].map(|name| database(name, ""));
    let f = Fixture::open(paths.to_vec(), KeyboardMode::Conventional);
    let names: Vec<&str> = f
        .app
        .connections()
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["order-a.db", "order-b.db", "order-c.db"]);
}

#[test]
fn the_catalog_of_a_console_comes_from_its_connection() {
    let first = database("complete-a", "CREATE TABLE alpha_table(x);");
    let second = database("complete-b", "CREATE TABLE beta_table(x);");
    let mut f = Fixture::open(vec![first, second], KeyboardMode::Conventional);
    f.navigator();
    f.code(KeyCode::Down);
    f.code(KeyCode::Char('n'));
    f.type_text("SELECT * FROM bet");
    f.pump_sees("beta_table");
    f.code(KeyCode::Esc);
    f.prefix('p');
    f.type_text("SELECT * FROM alp");
    f.pump_sees("alpha_table");
    f.assert_not_sees("beta_table");
}

#[test]
fn disconnect_refuses_a_console_with_a_transaction() {
    let mut f = Fixture::new("disconnect-tx", "CREATE TABLE t(a);");
    f.run("BEGIN; INSERT INTO t VALUES (1);");
    f.navigator();
    f.code(KeyCode::Char('D'));
    f.assert_sees("console-1 has an execution or a transaction");
    assert_eq!(f.app.console().transaction, TransactionState::Open);
}

#[test]
fn removal_asks_first_and_the_consoles_stay_without_a_connection() {
    let mut f = Fixture::new("remove", "");
    f.write("SELECT 1;");
    f.run("SELECT 1;");
    f.navigator();
    f.code(KeyCode::Delete);
    f.assert_sees("Remove the definition of remove.db?");
    f.code(KeyCode::Enter);
    assert_eq!(f.app.connections().len(), 1, "the first option cancels");

    f.code(KeyCode::Delete);
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    assert!(f.app.connections().is_empty());
    assert_eq!(f.app.console().editor.text(), "SELECT 1;", "the text stays");
    assert!(f.paths[0].exists(), "the removal keeps the database file");
    f.assert_sees("No connections");

    f.prefix('l');
    f.code(KeyCode::F(5));
    f.assert_sees("No database.");
}

#[test]
fn removal_uses_x_in_vim_mode() {
    let path = database("remove-vim", "");
    let mut f = Fixture::open(vec![path], KeyboardMode::Vim);
    f.navigator();
    f.code(KeyCode::Delete);
    f.assert_not_sees("Remove the definition");
    f.code(KeyCode::Char('x'));
    f.assert_sees("Remove the definition");
}

#[test]
fn removal_refuses_a_connection_with_open_work() {
    let mut f = Fixture::new("remove-tx", "CREATE TABLE t(a);");
    f.run("BEGIN; INSERT INTO t VALUES (1);");
    f.navigator();
    f.code(KeyCode::Delete);
    f.assert_not_sees("Remove the definition");
    f.assert_sees("console-1 has an execution or a transaction");
}

#[test]
fn the_form_adds_a_connection_and_its_console_reads_the_file() {
    let mut f = Fixture::new("form-first", "");
    let path = database(
        "form-added",
        "CREATE TABLE t(a); INSERT INTO t VALUES (41);",
    );
    f.fill_sqlite("added", &path);
    f.save();
    assert!(f.app.form().is_none());
    f.assert_sees("▸ added");
    f.assert_sees("Pool 0/5");
    f.code(KeyCode::Char('n'));
    f.run("SELECT a + 1 AS b FROM t;");
    f.assert_sees("42");
}

#[test]
fn the_first_connection_of_an_empty_start_uses_the_empty_console() {
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let app = App::new(Options::default(), clipboard, Theme::plain());
    let mut f = Fixture {
        app,
        paths: Vec::new(),
    };
    f.assert_sees("No connections");
    f.code(KeyCode::F(5));
    f.assert_sees("No database.");

    let path = database("form-empty", "");
    f.fill_sqlite("first", &path);
    f.save();
    f.code(KeyCode::Enter);
    assert_eq!(f.app.consoles().len(), 1);
    f.run("SELECT 7 AS seven;");
    f.assert_sees("seven");
}

#[test]
fn the_form_refuses_a_missing_name_and_a_repeated_name() {
    let mut f = Fixture::new("Form-Names", "");
    let path = database("form-names-2", "");
    f.fill_sqlite("", &path);
    f.save();
    f.assert_sees("The connection needs a name.");
    f.focus_form(Field::Name);
    f.type_text("FORM-NAMES.DB");
    f.save();
    f.assert_sees("Another connection has this name.");
    assert_eq!(f.app.connections().len(), 1);
    f.code(KeyCode::Esc);
    assert!(f.app.form().is_none());
    assert_eq!(f.app.connections().len(), 1);
}

#[test]
fn a_new_file_needs_an_explicit_answer() {
    let mut f = Fixture::new("form-create", "");
    let path = temp_db("form-created-file");
    f.fill_sqlite("created", &path);
    f.save();
    f.assert_sees("Create it?");
    f.code(KeyCode::Enter);
    assert!(f.app.form().is_some(), "the cancel keeps the form");
    assert_eq!(f.app.connections().len(), 1);
    assert!(!path.exists());

    f.save();
    f.code(KeyCode::Tab);
    f.code(KeyCode::Enter);
    assert_eq!(f.app.connections().len(), 2);
    f.code(KeyCode::Char('n'));
    f.run("CREATE TABLE t(a);");
    assert!(path.exists());
}

#[test]
fn the_memory_form_saves_one_in_memory_connection() {
    let mut f = Fixture::new("form-memory", "");
    f.fill_sqlite("memory", Path::new(":memory:"));
    f.save();
    f.assert_sees("memory");
    assert_eq!(f.app.connections().len(), 2);
}

#[test]
fn the_test_button_reports_the_answer_of_the_connection() {
    let mut f = Fixture::new("form-test", "");
    let path = database("form-tested", "");
    f.fill_sqlite("tested", &path);
    f.focus_form(Field::Test);
    f.code(KeyCode::Enter);
    f.pump_sees("The connection works.");

    let missing = temp_db("form-missing-file");
    f.focus_form(Field::File);
    for _ in 0..path.to_str().unwrap().chars().count() {
        f.code(KeyCode::Backspace);
    }
    f.write(missing.to_str().unwrap());
    f.focus_form(Field::Test);
    f.code(KeyCode::Enter);
    f.assert_sees("The file does not exist.");
    assert!(!missing.exists(), "the test does not create the file");
}

#[test]
fn a_failed_network_test_keeps_the_password_out_of_the_screen() {
    let mut f = Fixture::new("form-network", "");
    f.navigator();
    f.code(KeyCode::Char('a'));
    f.code(KeyCode::Tab);
    f.type_text("remote");
    f.focus_form(Field::Host);
    f.type_text("127.0.0.1");
    f.focus_form(Field::Port);
    f.type_text("1");
    f.focus_form(Field::User);
    f.type_text("reader");
    f.focus_form(Field::Password);
    f.type_text("hunter2-secret");
    f.assert_sees("••••••••••••••");
    f.focus_form(Field::Test);
    f.code(KeyCode::Enter);
    f.pump_until(|f| f.app.form().unwrap().test.is_none());
    f.assert_not_sees("hunter2-secret");
    f.assert_not_sees("The connection works.");
}

#[test]
fn the_form_suggests_tls_from_the_address_until_the_user_chooses() {
    let mut f = Fixture::new("form-tls", "");
    f.navigator();
    f.code(KeyCode::Char('a'));
    assert!(f.app.form().unwrap().verify_tls);
    f.focus_form(Field::Host);
    f.type_text("localhost");
    assert!(
        !f.app.form().unwrap().verify_tls,
        "a local address needs no TLS"
    );
    for _ in 0.."localhost".len() {
        f.code(KeyCode::Backspace);
    }
    f.type_text("db.example.test");
    assert!(
        f.app.form().unwrap().verify_tls,
        "a remote address verifies"
    );

    f.focus_form(Field::Tls);
    f.code(KeyCode::Right);
    assert!(!f.app.form().unwrap().verify_tls);
    f.focus_form(Field::Host);
    f.code(KeyCode::Char('x'));
    assert!(
        !f.app.form().unwrap().verify_tls,
        "the choice of the user stays"
    );
}

#[test]
fn a_connection_text_with_a_password_never_shows_it() {
    let mut f = Fixture::new("form-uri", "");
    f.navigator();
    f.code(KeyCode::Char('a'));
    f.code(KeyCode::Tab);
    f.type_text("by-uri");
    f.focus_form(Field::Entry);
    f.code(KeyCode::Right);
    f.focus_form(Field::Uri);
    f.write("postgres://reader:hunter2-secret@db.example.test:5439/sales?sslmode=nonsense");
    f.assert_not_sees("hunter2");
    f.code(KeyCode::Home);
    f.assert_sees("reader:••••••••••••••@db.ex");
    f.code(KeyCode::End);
    f.save();
    f.assert_sees("can fall back");
    f.assert_not_sees("hunter2");
    assert!(f.app.form().is_some());

    f.focus_form(Field::Uri);
    for _ in 0.."nonsense".len() {
        f.code(KeyCode::Backspace);
    }
    f.type_text("verify-full");
    f.save();
    assert!(f.app.form().is_none(), "{}", f.screen());
    f.assert_sees("by-uri");
    f.assert_not_sees("hunter2");
}

#[test]
fn a_password_stays_hidden_while_the_user_types_the_connection_text() {
    let mut f = Fixture::new("form-uri-typing", "");
    f.navigator();
    f.code(KeyCode::Char('a'));
    f.focus_form(Field::Entry);
    f.code(KeyCode::Right);
    f.focus_form(Field::Uri);
    f.type_text("postgres://reader:hunter2");
    f.assert_not_sees("hunter2");
    f.type_text("@db.example.test/sales");
    f.assert_not_sees("hunter2");
    f.assert_sees("reader:•••••••@db.example");
}

#[test]
fn edit_changes_a_closed_connection_and_the_next_run_uses_it() {
    let first = database(
        "edit-before",
        "CREATE TABLE t(a); INSERT INTO t VALUES (1);",
    );
    let second = database("edit-after", "CREATE TABLE t(a); INSERT INTO t VALUES (2);");
    let mut f = Fixture::open(vec![first.clone()], KeyboardMode::Conventional);
    f.navigator();
    f.code(KeyCode::Char('e'));
    assert_eq!(f.app.form().unwrap().focus, Field::Name);
    f.focus_form(Field::File);
    for _ in 0..first.to_str().unwrap().chars().count() {
        f.code(KeyCode::Backspace);
    }
    f.write(second.to_str().unwrap());
    f.save();
    assert!(f.app.form().is_none(), "{}", f.screen());
    f.prefix('l');
    f.run("SELECT a + 100 AS b FROM t;");
    f.assert_sees("102");
}

#[test]
fn the_form_works_with_the_mouse() {
    let mut f = Fixture::new("form-mouse", "");
    f.navigator();
    f.code(KeyCode::Char('a'));
    f.click_text("Name");
    assert_eq!(f.app.form().unwrap().focus, Field::Name);
    f.click_text("< PostgreSQL >");
    assert_eq!(f.app.form().unwrap().kind, Kind::MySql);
    f.click_text("[Cancel]");
    assert!(f.app.form().is_none());
}

#[test]
fn vim_keys_write_text_in_a_form_field_and_move_on_a_choice() {
    let path = database("form-vim", "");
    let mut f = Fixture::open(vec![path], KeyboardMode::Vim);
    f.navigator();
    f.code(KeyCode::Char('a'));
    f.code(KeyCode::Char('l'));
    assert_eq!(
        f.app.form().unwrap().kind,
        Kind::MySql,
        "l changes a choice"
    );
    f.code(KeyCode::Char('j'));
    assert_eq!(f.app.form().unwrap().focus, Field::Name);
    f.type_text("jkhl");
    assert_eq!(f.app.form().unwrap().text(Field::Name), "jkhl");
}

#[test]
fn a_collapsed_navigator_keeps_the_editor_and_the_results() {
    let mut f = Fixture::new("collapse", "");
    f.run("SELECT 'a value' AS shown;");
    f.navigator();
    f.prefix('b');
    assert!(
        !f.app.navigator_focused(),
        "the focus leaves the hidden panel"
    );
    f.assert_not_sees("● collapse.db");
    f.assert_sees("a value");
    f.assert_sees("SELECT 'a value'");
    f.prefix('h');
    assert!(!f.app.navigator_focused(), "a hidden panel takes no focus");
    f.prefix('b');
    f.assert_sees("▸ collapse.db");
}

#[test]
fn a_drag_on_the_dividers_changes_the_panels() {
    let mut f = Fixture::new("dividers", "");
    f.run("SELECT 'a value' AS shown;");
    f.buffer();
    let left = MouseEventKind::Down(MouseButton::Left);
    let drag = MouseEventKind::Drag(MouseButton::Left);
    let up = MouseEventKind::Up(MouseButton::Left);

    f.app.on_event(mouse(left, 24, 5));
    f.app.on_event(mouse(drag, 40, 5));
    f.app.on_event(mouse(up, 40, 5));
    assert_eq!(f.app.navigator().width, 40);
    let buffer = f.buffer();
    assert_eq!(buffer[(40, 5)].symbol(), "│");

    f.app.on_event(mouse(left, 24, 5));
    f.app.on_event(mouse(drag, 99, 5));
    f.app.on_event(mouse(up, 99, 5));
    assert_eq!(
        f.app.navigator().width,
        40,
        "a press away from the divider moves nothing"
    );

    f.app.on_event(mouse(left, 40, 5));
    f.app.on_event(mouse(drag, 99, 5));
    f.app.on_event(mouse(up, 99, 5));
    assert_eq!(
        f.app.navigator().width,
        60,
        "the console keeps its minimum width"
    );

    let (column, row) = f.locate("Messages");
    let free = column + 20;
    f.click(free, row);
    f.app.on_event(mouse(drag, free, 8));
    f.app.on_event(mouse(up, free, 8));
    let (_, moved) = f.locate("Messages");
    assert!(moved < row, "{moved} {row}");
    f.assert_sees("a value");
}

#[test]
fn the_buttons_run_and_follow_the_state_of_the_console() {
    let mut f = Fixture::new("buttons", "CREATE TABLE t(a);");
    f.write("INSERT INTO t VALUES (1);");
    f.click_text("[Run]");
    f.idle();
    assert_eq!(f.query(0, "SELECT count(*) FROM t"), 1);

    f.click_text("[Auto]");
    assert!(f.app.console().manual);
    f.assert_sees("[Manual]");
    f.prefix('k');
    f.click_text("[Run]");
    f.idle();
    f.pump_sees("Transaction open");
    assert_eq!(f.query(0, "SELECT count(*) FROM t"), 1);
    f.click_text("[Commit]");
    f.pump_sees("No transaction");
    assert_eq!(f.query(0, "SELECT count(*) FROM t"), 2);
    f.click_text("[Rollback]");
    f.assert_sees("No transaction is open.");
}

#[test]
fn the_stop_button_ends_a_script() {
    let mut f = Fixture::new("button-stop", "");
    f.app.console_mut().editor.on_paste(LONG);
    f.click_text("[Script]");
    f.pump_sees(" 1 ●");
    f.click_text("[Stop]");
    f.idle();
    f.assert_sees("■");
}

#[test]
fn sqlite_has_no_schema_choice() {
    let mut f = Fixture::new("no-schema", "");
    f.assert_not_sees("Schema:");
    f.prefix('d');
    f.assert_sees("no schema or database choice");
}

#[test]
fn the_help_screen_follows_the_panel_and_the_mode() {
    let path = database("help", "");
    let mut f = Fixture::open(vec![path], KeyboardMode::Vim);
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Vim · Editor");
    f.assert_sees("Open the suggestions");
    f.code(KeyCode::Esc);
    f.navigator();
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Vim · Navigator");
    f.assert_sees("Add a connection");
    f.assert_not_sees("Open the suggestions");
    f.code(KeyCode::Esc);
    f.prefix('j');
    f.prefix('?');
    f.assert_sees("Keys · Vim · Results");
    f.assert_sees("Copy as TSV");
    f.assert_sees("ctrl+c, y");
}

#[test]
fn a_small_terminal_keeps_the_consoles_and_the_navigator() {
    let mut f = Fixture::new("small", "");
    f.run("SELECT 'kept' AS a;");
    f.navigator();
    f.code(KeyCode::Char('n'));
    f.prefix('p');
    let small = f.buffer_at(70, 20);
    assert!(rows(&small).join("\n").contains("Small terminal"));
    assert_eq!(f.app.consoles().len(), 2);
    f.assert_sees("kept");
}

#[test]
fn every_screen_fits_the_minimum_terminal() {
    let mut f = Fixture::new("minimum", "");
    f.run("SELECT 1 AS a;");
    let screen = rows(&f.buffer_at(80, 24)).join("\n");
    assert!(screen.contains("[Rollback]"), "{screen}");
    f.navigator();
    f.code(KeyCode::Char('a'));
    let screen = rows(&f.buffer_at(80, 24)).join("\n");
    for text in ["Type", "[Test connection]", "[Save]", "[Cancel]"] {
        assert!(screen.contains(text), "{text}\n{screen}");
    }
    // The largest section also fits.
    f.focus_form(Field::Icon);
    let screen = rows(&f.buffer_at(80, 24)).join("\n");
    for text in ["Connect limit", "Idle limit", "Icon", "[Save]"] {
        assert!(screen.contains(text), "{text}\n{screen}");
    }
}

#[test]
fn a_name_from_the_database_cannot_send_terminal_sequences() {
    let mut f = Fixture::new("tree-escape", "CREATE TABLE \"bad\x1b[31mname\"(a);");
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_tree("main");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.pump_sees("bad");
    assert!(!f.screen().contains('\x1b'), "{}", f.screen());
}

const NEW_TABLE: &str = "new_table";

/// A connection that keeps its sessions, as a network database does.
#[derive(Default)]
struct Sessions {
    opened: AtomicUsize,
    closed: Arc<AtomicUsize>,
    contexts: Mutex<Vec<SessionContext>>,
    statements: Mutex<Vec<String>>,
    transaction: Mutex<TransactionState>,
    /// True keeps a catalog read open until `catalog_release`.
    hold_catalog: AtomicBool,
    catalog_release: Notify,
    /// True adds `NEW_TABLE` to a read that starts after this change.
    newer_catalog: AtomicBool,
    catalog_reads: AtomicUsize,
}

struct KeepConnector(Arc<Sessions>);

struct KeepSession(Arc<Sessions>);

struct NoCancel;

impl Drop for KeepSession {
    fn drop(&mut self) {
        self.0.closed.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl Canceller for NoCancel {
    async fn cancel(&self) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl Connector for KeepConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Schema,
            transactions: true,
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        self.0.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(KeepSession(self.0.clone())))
    }
}

#[async_trait]
impl Session for KeepSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(NoCancel)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.0.contexts.lock().unwrap().push(context.clone());
        Ok(())
    }

    async fn run(&mut self, sql: &str, sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.0.statements.lock().unwrap().push(sql.to_owned());
        if sql.starts_with("BEGIN") {
            *self.0.transaction.lock().unwrap() = TransactionState::Open;
        }
        sink.finish().await;
        Ok(StatementEnd::default())
    }

    async fn transaction_state(&mut self) -> TransactionState {
        *self.0.transaction.lock().unwrap()
    }

    async fn control(&mut self, _: TransactionCommand) -> Result<(), ExecError> {
        *self.0.transaction.lock().unwrap() = TransactionState::None;
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let newer = self.0.newer_catalog.load(Ordering::SeqCst);
        self.0.catalog_reads.fetch_add(1, Ordering::SeqCst);
        if self.0.hold_catalog.load(Ordering::SeqCst) {
            self.0.catalog_release.notified().await;
        }
        let mut catalog = sample_catalog();
        if newer {
            let mut relation = catalog.schemas[0].relations[0].clone();
            relation.name = NEW_TABLE.to_owned();
            catalog.schemas[0].relations.push(relation);
        }
        Ok(catalog)
    }

    async fn read_columns(&mut self, _: &str, _: &str) -> Result<Vec<String>, ExecError> {
        Ok(vec!["first".to_owned()])
    }
}

fn kept_sessions(keyboard: KeyboardMode) -> (Fixture, Arc<Sessions>) {
    kept_sessions_with(keyboard, Settings::default())
}

fn kept_sessions_with(keyboard: KeyboardMode, settings: Settings) -> (Fixture, Arc<Sessions>) {
    let sessions = Arc::new(Sessions::default());
    let options = Options {
        keyboard,
        connector: Some(Arc::new(KeepConnector(sessions.clone()))),
        label: Some("server".to_owned()),
        settings,
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let app = App::new(options, clipboard, Theme::plain());
    (
        Fixture {
            app,
            paths: Vec::new(),
        },
        sessions,
    )
}

#[test]
fn disconnect_closes_the_sessions_and_the_next_run_opens_one() {
    let (mut f, sessions) = kept_sessions(KeyboardMode::Conventional);
    f.navigator();
    f.code(KeyCode::Char('D'));
    f.assert_sees("no open session");
    f.prefix('l');
    f.run("SELECT 1;");
    f.pump_sees("● server");
    // The editor also reads the catalog, so this start can open two sessions.
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    let opened = sessions.opened.load(Ordering::SeqCst);

    f.navigator();
    f.code(KeyCode::Char('D'));
    f.pump_sees("Disconnected.");
    f.assert_not_sees("● server");
    f.pump_until(|_| sessions.closed.load(Ordering::SeqCst) == opened);

    f.prefix('l');
    f.run("SELECT 2;");
    f.pump_sees("● server");
    assert!(sessions.opened.load(Ordering::SeqCst) > opened);
}

/// Runs a statement, and then a catalog read holds the session of the pool.
fn hold_a_session_in_a_catalog_read(f: &mut Fixture, sessions: &Sessions) {
    f.prefix('l');
    sessions.hold_catalog.store(true, Ordering::SeqCst);
    f.run("SELECT 1;");
    f.pump_sees("● server");
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_sees("Pool 1/5");
}

fn has_new_table(f: &Fixture) -> bool {
    let catalog = f.app.connections()[0].catalog.as_ref();
    catalog.is_some_and(|item| {
        item.schemas[0]
            .relations
            .iter()
            .any(|table| table.name == NEW_TABLE)
    })
}

#[test]
fn an_old_catalog_answer_does_not_end_a_newer_read() {
    let (mut f, sessions) = kept_sessions(KeyboardMode::Conventional);
    hold_a_session_in_a_catalog_read(&mut f, &sessions);
    sessions.newer_catalog.store(true, Ordering::SeqCst);
    f.code(KeyCode::Char('r'));
    f.pump_until(|_| sessions.catalog_reads.load(Ordering::SeqCst) == 2);

    sessions.catalog_release.notify_one();
    std::thread::sleep(Duration::from_millis(100));
    f.app.drain_events();
    assert_eq!(f.app.connections()[0].catalog_state, CatalogState::Loading);

    sessions.catalog_release.notify_one();
    f.pump_until(|f| f.app.connections()[0].catalog_state == CatalogState::Loaded);
    assert!(has_new_table(&f));
}

#[test]
fn an_old_catalog_answer_does_not_replace_a_newer_read() {
    let (mut f, sessions) = kept_sessions(KeyboardMode::Conventional);
    hold_a_session_in_a_catalog_read(&mut f, &sessions);
    sessions.newer_catalog.store(true, Ordering::SeqCst);
    sessions.hold_catalog.store(false, Ordering::SeqCst);
    f.code(KeyCode::Char('r'));
    f.pump_until(|f| has_new_table(f));

    sessions.catalog_release.notify_one();
    std::thread::sleep(Duration::from_millis(100));
    f.app.drain_events();
    assert!(has_new_table(&f));
}

#[test]
fn disconnect_waits_for_a_session_in_use() {
    let (mut f, sessions) = kept_sessions(KeyboardMode::Conventional);
    hold_a_session_in_a_catalog_read(&mut f, &sessions);
    f.code(KeyCode::Char('D'));
    std::thread::sleep(Duration::from_millis(100));
    f.app.drain_events();
    f.assert_not_sees("Disconnected.");

    sessions.catalog_release.notify_one();
    f.pump_sees("Disconnected.");
    f.assert_not_sees("● server");
}

#[test]
fn a_session_that_stays_in_use_keeps_the_message_true() {
    let settings = Settings {
        stop_grace: Duration::from_millis(20),
        ..Settings::default()
    };
    let (mut f, sessions) = kept_sessions_with(KeyboardMode::Conventional, settings);
    hold_a_session_in_a_catalog_read(&mut f, &sessions);
    f.code(KeyCode::Char('D'));
    f.pump_sees("A session is still in use.");
    f.assert_not_sees("Disconnected.");

    sessions.catalog_release.notify_one();
    f.pump_until(|f| !f.sees("● server"));
}

#[test]
fn the_schema_choice_applies_to_each_new_session() {
    let (mut f, sessions) = kept_sessions(KeyboardMode::Conventional);
    f.assert_sees("Schema: [-]");
    f.prefix('d');
    f.pump_sees("sales");
    f.assert_sees("(default of the connection)");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.assert_sees("Schema: [sales]");
    assert_eq!(f.app.console().schema.as_deref(), Some("sales"));

    f.run("SELECT 1;");
    f.navigator();
    f.code(KeyCode::Char('D'));
    f.pump_sees("Disconnected.");
    f.prefix('l');
    f.run("SELECT 2;");
    let contexts = sessions.contexts.lock().unwrap().clone();
    assert!(contexts.len() >= 2, "each reservation applies the context");
    assert!(
        contexts
            .iter()
            .all(|context| context.schema.as_deref() == Some("sales"))
    );
    assert_eq!(
        f.app.console().editor.text(),
        "SELECT 2;",
        "the choice does not change the text"
    );

    f.prefix('d');
    f.code(KeyCode::Up);
    f.code(KeyCode::Up);
    f.code(KeyCode::Enter);
    assert_eq!(f.app.console().schema, None);
    f.assert_sees("Schema: [public]");
}

#[test]
fn the_bar_shows_the_default_schema_after_the_catalog_read() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.assert_sees("Schema: [-]");
    f.prefix('h');
    f.code(KeyCode::Right);
    f.pump_sees("Schema: [public]");
    assert_eq!(
        f.app.console().schema,
        None,
        "the default is not a choice of the console"
    );
}

#[test]
fn a_paste_in_the_navigator_changes_no_text() {
    let mut f = Fixture::new("paste-tree", "");
    f.navigator();
    f.write("SELECT 1;");
    assert_eq!(f.app.console().editor.text(), "");
    assert!(f.app.navigator_focused());
}

#[test]
fn only_the_prefix_shows_the_prefix_hint() {
    let path = database("hint", "");
    let mut f = Fixture::open(vec![path], KeyboardMode::Vim);
    f.run("SELECT 1 AS a;");
    f.prefix('j');
    f.code(KeyCode::Char('g'));
    f.assert_not_sees("Ctrl+W …");
    f.code(KeyCode::Esc);
    f.press(ctrl('w'));
    f.assert_sees("Ctrl+W …");
}

#[test]
fn each_console_has_its_own_schema_choice() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.prefix('d');
    f.pump_sees("sales");
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.navigator();
    f.code(KeyCode::Char('n'));
    assert_eq!(f.app.console().schema, None);
    f.prefix('p');
    assert_eq!(f.app.console().schema.as_deref(), Some("public"));
}

#[test]
fn the_schema_choice_gives_priority_to_its_relations_in_the_list() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.prefix('d');
    f.pump_sees("sales");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.type_text("SELECT * FROM inv");
    f.pump_sees("invoices");
    let screen = f.screen();
    assert!(
        !screen.contains("sales.invoices"),
        "the selected schema needs no qualifier:\n{screen}"
    );
}

#[test]
fn the_schema_choice_waits_for_the_end_of_a_transaction() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.run("BEGIN;");
    f.pump_sees("Transaction open");
    f.prefix('d');
    f.assert_sees("End the execution and the transaction");
    f.assert_not_sees("default of the connection");
}

#[test]
fn escape_keeps_the_schema_and_vim_keys_move_in_the_list() {
    let (mut f, _) = kept_sessions(KeyboardMode::Vim);
    f.prefix('d');
    f.pump_sees("sales");
    f.code(KeyCode::Char('j'));
    f.code(KeyCode::Esc);
    assert_eq!(f.app.console().schema, None);
    f.prefix('d');
    f.code(KeyCode::Char('j'));
    f.code(KeyCode::Char('j'));
    f.code(KeyCode::Char('k'));
    f.code(KeyCode::Enter);
    assert_eq!(f.app.console().schema.as_deref(), Some("public"));
}

#[test]
fn a_click_on_the_schema_control_opens_the_list() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.click_text("Schema:");
    f.pump_sees("sales");
}

#[test]
fn a_click_chooses_an_item_of_the_list() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.prefix('d');
    f.pump_sees("sales");
    f.click_text("sales");
    assert_eq!(f.app.console().schema.as_deref(), Some("sales"));
}

#[test]
fn a_click_answers_a_dialog_and_misses_do_nothing() {
    let mut f = Fixture::new("dialog-click", "");
    f.navigator();
    f.code(KeyCode::Delete);
    f.click(1, 1);
    f.assert_sees("Remove the definition");
    f.click_text("[Cancel]");
    assert_eq!(f.app.connections().len(), 1);
    f.code(KeyCode::Delete);
    f.click_text("[Remove]");
    assert!(f.app.connections().is_empty());
}

#[test]
fn the_navigator_opens_a_view_and_its_columns() {
    let (mut f, _) = kept_sessions(KeyboardMode::Conventional);
    f.navigator();
    f.code(KeyCode::Right);
    f.pump_sees("public");
    f.code(KeyCode::Down);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.assert_sees("▸ active_users");
    f.assert_sees("Order Items");
}

fn local_target() -> NetworkTarget {
    parse_uri("postgres://reader:hunter2-secret@localhost/app").unwrap()
}

#[test]
fn a_form_for_a_network_definition_keeps_each_value() {
    let mut target = local_target();
    target.tls = Tls::Verify(TlsFiles {
        ca: Some("ca.pem".into()),
        client: Some(ClientCertificate {
            certificate: "c.pem".into(),
            key: "k.pem".into(),
        }),
    });
    let mut definition = Definition::new("kept", Source::Network(target));
    definition.read_only = true;
    definition.max_connections = 2;
    definition.connect_timeout = Duration::from_secs(9);
    let form = ConnectionForm::editing(1, &definition, false);
    assert_eq!(form.definition().unwrap(), definition);
}

#[test]
fn a_form_for_a_socket_keeps_the_address() {
    let mut target = local_target();
    target.address = Address::Socket("/tmp/db.sock".into());
    let definition = Definition::new("socket", Source::Network(target));
    let form = ConnectionForm::editing(1, &definition, false);
    assert_eq!(form.definition().unwrap(), definition);
}

#[test]
fn a_form_for_a_sqlite_definition_keeps_each_value() {
    let mut target = SqliteTarget::new("data.db");
    target.foreign_keys = Some(true);
    target.busy_timeout = Duration::from_secs(9);
    let mut definition = Definition::new("file", Source::Sqlite(target));
    definition.read_only = true;
    definition.max_connections = 1;
    let form = ConnectionForm::editing(1, &definition, false);
    assert_eq!(form.definition().unwrap(), definition);
}

#[test]
fn the_summary_of_a_definition_holds_no_password() {
    let definition = Definition::new("kept", Source::Network(local_target()));
    assert!(!definition.summary().contains("hunter2-secret"));
    assert!(!format!("{definition:?}").contains("hunter2-secret"));
}

const REFUSAL: &str = "The cursor is outside a statement.";
const NO_CLIPBOARD: &str = "Clipboard unavailable.";
const MESSAGE_TIME: Duration = Duration::from_secs(8);

/// A clipboard that always fails, to get a message from the editor.
struct BrokenClipboard;

impl Clipboard for BrokenClipboard {
    fn copy(&mut self, _text: &str) -> CopyOutcome {
        CopyOutcome::Failed
    }

    fn paste(&mut self) -> Option<String> {
        None
    }
}

fn fixture_without_clipboard(name: &str) -> Fixture {
    let path = database(name, "");
    let options = Options {
        database: Some(path.clone()),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(BrokenClipboard));
    Fixture {
        app: App::new(options, clipboard, Theme::plain()),
        paths: vec![path],
    }
}

#[test]
fn a_message_leaves_the_hint_bar_after_its_time() {
    let mut f = Fixture::new("message-time", "");
    f.code(KeyCode::F(5));
    f.assert_sees(REFUSAL);
    let start = Instant::now();
    f.app.tick_at(start);
    assert!(
        !f.app
            .tick_at(start + MESSAGE_TIME - Duration::from_millis(100))
    );
    f.assert_sees(REFUSAL);
    assert!(
        f.app.tick_at(start + MESSAGE_TIME),
        "the screen must draw again"
    );
    f.assert_not_sees(REFUSAL);
    f.assert_sees("F5 run");
}

#[test]
fn a_repeated_message_gets_a_full_time() {
    let mut f = Fixture::new("message-again", "");
    let start = Instant::now();
    f.code(KeyCode::F(5));
    f.app.tick_at(start);
    let later = start + Duration::from_secs(7);
    f.app.tick_at(later);
    f.code(KeyCode::F(5));
    f.app.tick_at(later);
    f.app.tick_at(later + Duration::from_secs(7));
    f.assert_sees(REFUSAL);
    f.app.tick_at(later + MESSAGE_TIME);
    f.assert_not_sees(REFUSAL);
}

#[test]
fn a_click_removes_a_message_and_the_pointer_and_the_wheel_do_not() {
    let mut f = Fixture::new("message-click", "");
    f.code(KeyCode::F(5));
    f.buffer();
    f.app.on_event(mouse(MouseEventKind::Moved, 60, 10));
    f.app.on_event(mouse(MouseEventKind::ScrollDown, 60, 10));
    f.assert_sees(REFUSAL);
    f.click(60, 10);
    f.assert_not_sees(REFUSAL);
}

#[test]
fn a_click_keeps_the_message_of_its_own_action() {
    let mut f = Fixture::new("message-own", "");
    f.click_text("[Run]");
    f.assert_sees(REFUSAL);
}

#[test]
fn a_message_of_the_editor_leaves_with_a_key_of_another_panel() {
    let mut f = fixture_without_clipboard("message-panel");
    f.press(ctrl('v'));
    f.assert_sees(NO_CLIPBOARD);
    f.navigator();
    f.assert_not_sees(NO_CLIPBOARD);
}

#[test]
fn a_message_of_the_editor_leaves_after_its_time() {
    let mut f = fixture_without_clipboard("message-editor");
    f.press(ctrl('v'));
    let start = Instant::now();
    f.app.tick_at(start);
    f.assert_sees(NO_CLIPBOARD);
    f.app.tick_at(start + MESSAGE_TIME);
    f.assert_not_sees(NO_CLIPBOARD);
}

impl Fixture {
    /// Opens consoles until the row has `count` tabs. The last one stays on screen.
    fn open_consoles(&mut self, count: usize) {
        while self.app.consoles().len() < count {
            self.navigator();
            self.code(KeyCode::Char('n'));
        }
        self.prefix('l');
    }

    /// Clicks the close mark of a tab.
    fn close_tab(&mut self, name: &str) {
        let (start, row) = self.locate(name);
        let column = start + name.chars().count() as u16 + 1;
        let tabs = self.screen();
        let mark = tabs
            .lines()
            .next()
            .unwrap()
            .chars()
            .nth(usize::from(column));
        assert_eq!(mark, Some('X'), "{tabs}");
        self.click(column, row);
    }

    fn console_names(&self) -> Vec<&str> {
        self.app
            .consoles()
            .iter()
            .map(|console| console.name.as_str())
            .collect()
    }
}

#[test]
fn each_main_tab_has_a_close_mark() {
    let mut f = Fixture::new("close-marks", "");
    f.open_consoles(2);
    let screen = f.screen();
    let tabs = screen.lines().next().unwrap();
    assert!(tabs.starts_with(" console-1 X  console-2 X "), "{tabs}");
}

#[test]
fn the_close_mark_closes_a_tab_that_is_not_on_screen() {
    let mut f = Fixture::new("close-other", "");
    f.open_consoles(3);
    assert_eq!(f.app.console().name, "console-3");
    f.close_tab("console-1");
    assert_eq!(f.console_names(), ["console-2", "console-3"]);
    assert_eq!(f.app.console().name, "console-3");
}

#[test]
fn the_close_mark_closes_the_tab_on_screen() {
    let mut f = Fixture::new("close-active", "");
    f.open_consoles(3);
    f.close_tab("console-3");
    assert_eq!(f.console_names(), ["console-1", "console-2"]);
    assert_eq!(f.app.console().name, "console-2");
}

#[test]
fn a_click_on_the_label_of_a_tab_does_not_close_it() {
    let mut f = Fixture::new("close-label", "");
    f.open_consoles(2);
    let (start, row) = f.locate("console-1");
    // The last column of the label, and the column before the close mark.
    for column in [start + 8, start + 9] {
        f.click(column, row);
    }
    assert_eq!(f.console_names(), ["console-1", "console-2"]);
    assert_eq!(f.app.console().name, "console-1");
}

#[test]
fn the_close_mark_asks_the_same_question_as_the_key() {
    let mut f = Fixture::new("close-question", "CREATE TABLE t(a);");
    f.run("BEGIN; INSERT INTO t VALUES (1);");
    f.open_consoles(2);
    f.close_tab("console-1");
    f.assert_sees("console-1: This console has an open transaction");
    assert_eq!(f.app.console().name, "console-2");
    // `Esc` cancels the close, and the tab stays.
    f.code(KeyCode::Esc);
    assert_eq!(f.console_names(), ["console-1", "console-2"]);
}

#[test]
fn the_close_mark_of_the_last_tab_shows_the_home_screen() {
    let mut f = Fixture::new("close-last", "");
    f.close_tab("console-1");
    assert!(f.app.home());
    assert!(!f.app.should_quit());
}

#[test]
fn a_memory_target_round_trips_through_the_form_with_one_session() {
    let definition = Definition::new("scratch", Source::Sqlite(SqliteTarget::in_memory()));
    let form = ConnectionForm::editing(1, &definition, false);
    assert_eq!(form.definition().unwrap(), definition);
    let settings = definition.settings(&sqln::exec::Settings::default());
    assert_eq!(settings.max_connections, 1);
    assert_eq!(settings.idle_timeout, None);
}
