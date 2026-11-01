// The question before a run: a write without WHERE, and a change on production.

mod common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{key, shift, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::form::{ConnectionForm, Field};
use sqln::sql::{Dialect, changes_state, lacks_where};
use sqln::store::config::{self, ConnectionEntry};
use sqln::ui::theme::Theme;

const TITLE: &str = "Confirm the run";

struct Run {
    app: App,
    path: PathBuf,
}

impl Run {
    /// Opens a file with three rows, through a definition that `change` can edit.
    fn new(name: &str, change: impl FnOnce(&mut Definition)) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t (a INT); INSERT INTO t VALUES (1), (2), (3);")
            .unwrap();
        let mut definition = Definition::sqlite(SqliteTarget::new(&path));
        change(&mut definition);
        let options = Options {
            connections: vec![definition],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(options, clipboard, Theme::plain());
        Self { app, path }
    }

    fn write(&mut self, sql: &str) {
        self.app.on_event(Event::Paste(sql.to_owned()));
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
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

    fn run(&mut self, sql: &str) {
        self.write(sql);
        self.press(KeyCode::F(5));
        self.idle();
    }

    fn run_script(&mut self, sql: &str) {
        self.write(sql);
        self.app.on_event(Event::Key(shift(KeyCode::F(5))));
        self.idle();
    }

    fn rows(&self) -> i64 {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT count(*) FROM t", [], |row| row.get(0))
            .unwrap()
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal.backend().buffer().content().chunks(100);
        rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn a_statement_without_where_in_the_main_level_is_found() {
    for sql in [
        "UPDATE t SET a = 1",
        "delete from t",
        "DELETE FROM t -- WHERE a = 1",
        "DELETE FROM t /* WHERE a = 1 */",
        "UPDATE t SET a = 'WHERE'",
        "UPDATE t SET a = (SELECT max(b) FROM u WHERE c = 1)",
        "WITH x AS (SELECT 1 WHERE true) DELETE FROM t",
    ] {
        assert!(lacks_where(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn a_statement_with_where_or_another_verb_is_not_found() {
    for sql in [
        "UPDATE t SET a = 1 WHERE b = 2",
        "delete from t where a in (select b from u)",
        "UPDATE t SET a = 1 FROM u WHERE t.b = u.b",
        "SELECT * FROM t FOR UPDATE",
        "WITH x AS (SELECT 1) SELECT * FROM x FOR UPDATE",
        "INSERT INTO t VALUES (1) ON CONFLICT (a) DO UPDATE SET a = 2",
        "TRUNCATE t",
        "DROP TABLE t",
    ] {
        assert!(!lacks_where(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn redis_has_no_rule_for_these_questions() {
    assert!(!lacks_where("DEL key", Dialect::Redis));
    assert!(!changes_state("FLUSHALL", Dialect::Redis));
}

#[test]
fn a_read_and_a_session_statement_change_no_state() {
    for sql in [
        "SELECT 1",
        "SHOW TABLES",
        "EXPLAIN SELECT 1",
        "BEGIN",
        "START TRANSACTION",
        "COMMIT",
        "END",
        "ROLLBACK",
        "SAVEPOINT a",
        "RELEASE SAVEPOINT a",
        "USE app",
    ] {
        assert!(!changes_state(sql, Dialect::MySql), "{sql}");
    }
}

#[test]
fn a_write_a_setting_and_an_unknown_function_can_change_state() {
    for sql in [
        "INSERT INTO t VALUES (1)",
        "DROP TABLE t",
        "TRUNCATE t",
        "SET GLOBAL a = 1",
        "SELECT my_function()",
        "EXPLAIN ANALYZE DELETE FROM t",
    ] {
        assert!(changes_state(sql, Dialect::Postgres), "{sql}");
    }
}

#[test]
fn cancel_has_the_focus_and_the_rows_stay() {
    let mut run = Run::new("guard-cancel.db", |_| {});
    run.run("DELETE FROM t");
    assert!(run.screen().contains(TITLE));
    run.press(KeyCode::Enter);
    run.idle();
    assert!(!run.screen().contains(TITLE));
    assert_eq!(run.rows(), 3);
}

#[test]
fn the_second_option_runs_the_statement_with_no_new_question() {
    let mut run = Run::new("guard-run.db", |_| {});
    run.run("DELETE FROM t");
    run.press(KeyCode::Right);
    run.press(KeyCode::Enter);
    run.idle();
    assert!(!run.screen().contains(TITLE));
    assert_eq!(run.rows(), 0);
}

#[test]
fn a_statement_with_where_asks_nothing() {
    let mut run = Run::new("guard-where.db", |_| {});
    run.run("DELETE FROM t WHERE a = 1");
    assert!(!run.screen().contains(TITLE));
    assert_eq!(run.rows(), 2);
}

#[test]
fn a_script_asks_once_with_the_count_and_the_first_statement() {
    let mut run = Run::new("guard-script.db", |_| {});
    run.run_script("SELECT 1; UPDATE t SET a = 9; DELETE FROM t; SELECT 2;");
    let screen = run.screen();
    assert!(
        screen.contains("Statements without WHERE: 2 of 4."),
        "{screen}"
    );
    assert!(screen.contains("UPDATE t SET a = 9"), "{screen}");
    run.press(KeyCode::Right);
    run.press(KeyCode::Enter);
    run.idle();
    assert!(!run.screen().contains(TITLE));
    assert_eq!(run.rows(), 0);
}

#[test]
fn a_read_only_connection_asks_nothing() {
    let mut run = Run::new("guard-read-only.db", |definition| {
        definition.read_only = true;
        definition.production = true;
    });
    run.run("DELETE FROM t");
    assert!(!run.screen().contains(TITLE));
    assert_eq!(run.rows(), 3);
}

#[test]
fn production_asks_before_a_write_with_where() {
    let mut run = Run::new("guard-production.db", |definition| {
        definition.production = true;
    });
    run.run("DELETE FROM t WHERE a = 1");
    let screen = run.screen();
    assert!(screen.contains("is a production connection."), "{screen}");
    assert!(
        screen.contains("Statements that can change data: 1 of 1."),
        "{screen}"
    );
    assert!(!screen.contains("without WHERE"), "{screen}");
    assert_eq!(run.rows(), 3);
}

#[test]
fn production_shows_one_question_with_the_two_risks() {
    let mut run = Run::new("guard-both.db", |definition| {
        definition.production = true;
    });
    run.run("DELETE FROM t");
    let screen = run.screen();
    assert!(
        screen.contains("Statements that can change data: 1 of 1."),
        "{screen}"
    );
    assert!(
        screen.contains("Statements without WHERE: 1 of 1."),
        "{screen}"
    );
    run.press(KeyCode::Right);
    run.press(KeyCode::Enter);
    run.idle();
    assert!(!run.screen().contains(TITLE));
    assert_eq!(run.rows(), 0);
}

#[test]
fn production_lets_a_read_and_a_session_statement_run() {
    let mut run = Run::new("guard-read.db", |definition| {
        definition.production = true;
    });
    run.run("SELECT * FROM t;");
    assert!(!run.screen().contains(TITLE));
    run.run("\nROLLBACK;");
    assert!(!run.screen().contains(TITLE));
}

#[test]
fn the_mark_of_production_has_text_for_a_screen_without_color() {
    let mut marked = Run::new("guard-mark.db", |definition| {
        definition.production = true;
    });
    assert!(marked.screen().contains("PROD"));
    let mut other = Run::new("guard-no-mark.db", |_| {});
    assert!(!other.screen().contains("PROD"));
}

#[test]
fn each_flag_of_the_form_keeps_its_own_value() {
    let mut definition = Definition::sqlite(SqliteTarget::new("a.db"));
    definition.production = true;
    let form = ConnectionForm::editing(1, &definition, false);
    assert!(form.flag(Field::Production));
    assert!(!form.flag(Field::ReadOnly));
}

#[test]
fn the_mark_of_production_can_change_on_a_connected_form() {
    let definition = Definition::sqlite(SqliteTarget::new("a.db"));
    let form = ConnectionForm::editing(1, &definition, true);
    assert!(form.fields().contains(&Field::Production));
}

#[test]
fn the_file_holds_the_mark_of_production_only_when_it_is_on() {
    let mut definition = Definition::sqlite(SqliteTarget::new("a.db"));
    let off = serde_json::to_string(&config::to_entry("a", &definition)).unwrap();
    assert!(!off.contains("production"), "{off}");

    definition.production = true;
    let on = serde_json::to_string(&config::to_entry("a", &definition)).unwrap();
    let entry: ConnectionEntry = serde_json::from_str(&on).unwrap();
    assert!(config::to_definition(&entry, PathBuf::new()).production);
}
