// The table view: tabs, pages, the filter bar, sorting, the budget, and the pool.
// The queries come from a recording connector. The data tests use real SQLite files.

mod common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use common::{ctrl, key, temp_db};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use common::sample_catalog;
use sqln::catalog::{Catalog, RelationRef};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::connector::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use sqln::editor::KeyboardMode;
use sqln::exec::{Flow, ResultSink, Settings};
use sqln::grid::{Row, Value};
use sqln::sql::Dialect;
use sqln::table::{TableFocus, clicked_order, page_sql};
use sqln::ui::theme::Theme;
use tokio::sync::Notify;

// ---------------------------------------------------------------------------
// The text of the queries.

fn relation() -> RelationRef {
    RelationRef { schema: "public".to_owned(), relation: "users".to_owned() }
}

#[test]
fn a_page_asks_one_row_more_without_a_count() {
    let sql = page_sql(Dialect::Postgres, &relation(), "", "", 100, 0).unwrap();
    assert_eq!(sql, "SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0");
    let sql = page_sql(Dialect::Postgres, &relation(), "", "", 100, 200).unwrap();
    assert!(sql.ends_with("\nLIMIT 101 OFFSET 200"));
    assert!(!sql.to_uppercase().contains("COUNT"));
}

#[test]
fn the_clauses_start_their_own_lines() {
    let sql = page_sql(Dialect::Postgres, &relation(), "active = true", "created_at DESC, id ASC", 10, 0)
        .unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM \"public\".\"users\"\nWHERE active = true\nORDER BY created_at DESC, id ASC\nLIMIT 11 OFFSET 0"
    );
}

#[test]
fn a_line_comment_in_the_filter_cannot_remove_the_limit() {
    let sql = page_sql(Dialect::Postgres, &relation(), "a = 1 -- note", "", 10, 0).unwrap();
    assert!(sql.ends_with("\nLIMIT 11 OFFSET 0"), "{sql}");
    let sql = page_sql(Dialect::MySql, &relation(), "a = 1 # note", "a -- note", 10, 0).unwrap();
    assert!(sql.ends_with("\nLIMIT 11 OFFSET 0"), "{sql}");
}

#[test]
fn each_dialect_quotes_the_names_of_the_table() {
    let odd = RelationRef { schema: "my\"schema".to_owned(), relation: "Order Items".to_owned() };
    let postgres = page_sql(Dialect::Postgres, &odd, "", "", 1, 0).unwrap();
    assert!(postgres.starts_with("SELECT * FROM \"my\"\"schema\".\"Order Items\"\n"), "{postgres}");
    let sqlite = page_sql(Dialect::Sqlite, &odd, "", "", 1, 0).unwrap();
    assert!(sqlite.starts_with("SELECT * FROM \"my\"\"schema\".\"Order Items\"\n"), "{sqlite}");
    let mysql_name = RelationRef { schema: "shop".to_owned(), relation: "or`ders".to_owned() };
    let mysql = page_sql(Dialect::MySql, &mysql_name, "", "", 1, 0).unwrap();
    assert!(mysql.starts_with("SELECT * FROM `shop`.`or``ders`\n"), "{mysql}");
}

#[test]
fn a_text_that_is_not_one_statement_is_refused() {
    for filter in ["1 = 1; DROP TABLE users", "1 = 1;", "name = 'open", "name = \"open", "a = 1 /* open"] {
        let result = page_sql(Dialect::Postgres, &relation(), filter, "", 10, 0);
        assert!(result.is_err(), "{filter:?} must be refused");
        let result = page_sql(Dialect::MySql, &relation(), "", filter, 10, 0);
        assert!(result.is_err(), "{filter:?} must be refused in the order");
    }
    // A backtick opens a name in MySQL and in SQLite, and it is a plain mark in PostgreSQL.
    for dialect in [Dialect::MySql, Dialect::Sqlite] {
        assert!(page_sql(dialect, &relation(), "a = `open", "", 10, 0).is_err());
    }
    assert!(page_sql(Dialect::Postgres, &relation(), "a = `open", "", 10, 0).is_ok());
}

#[test]
fn a_semicolon_inside_a_string_or_a_comment_is_not_a_second_statement() {
    assert!(page_sql(Dialect::Postgres, &relation(), "name = 'a;b'", "", 10, 0).is_ok());
    assert!(page_sql(Dialect::Postgres, &relation(), "a = 1 -- x; y", "", 10, 0).is_ok());
    assert!(page_sql(Dialect::Postgres, &relation(), "a = 1 /* x; y */", "", 10, 0).is_ok());
    assert!(page_sql(Dialect::Postgres, &relation(), "a = $$x;y$$", "", 10, 0).is_ok());
}

#[test]
fn a_click_on_a_header_goes_up_then_down_then_off() {
    let dialect = Dialect::Postgres;
    assert_eq!(clicked_order("", "name", dialect), "name ASC");
    assert_eq!(clicked_order("name ASC", "name", dialect), "name DESC");
    assert_eq!(clicked_order("NAME  desc", "name", dialect), "");
    // Another order, such as two columns, starts again from ascending.
    assert_eq!(clicked_order("created_at DESC, id ASC", "name", dialect), "name ASC");
    assert_eq!(clicked_order("id ASC", "name", dialect), "name ASC");
    // A name that needs quotes keeps them in the field.
    assert_eq!(clicked_order("", "Order Items", dialect), "\"Order Items\" ASC");
    assert_eq!(clicked_order("", "Order Items", Dialect::MySql), "`Order Items` ASC");
}

// ---------------------------------------------------------------------------
// A connection that records each statement and builds the rows of a table.

const COLUMNS: [&str; 2] = ["id", "name"];

#[derive(Default)]
struct Recorder {
    /// The rows of the table.
    rows: usize,
    /// The text of each value in the second column.
    value_size: usize,
    statements: Mutex<Vec<String>>,
    opened: AtomicUsize,
    /// True makes `run` wait for `release`.
    hold: AtomicBool,
    release: Notify,
    cancelled: AtomicBool,
    /// A statement with this text fails.
    fail_when: Mutex<Option<String>>,
    /// True makes `connect` fail.
    refuse_connect: AtomicBool,
    /// True makes `run` wait for `release` after `mid_rows` rows.
    mid_hold: AtomicBool,
    mid_rows: usize,
}

impl Recorder {
    fn statements(&self) -> Vec<String> {
        self.statements.lock().unwrap().clone()
    }

    fn count(&self) -> usize {
        self.statements.lock().unwrap().len()
    }
}

struct RecordingConnector(Arc<Recorder>);

struct RecordingSession(Arc<Recorder>);

struct StopHold(Arc<Recorder>);

#[async_trait]
impl Canceller for StopHold {
    async fn cancel(&self) -> Result<(), String> {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.release.notify_one();
        Ok(())
    }
}

#[async_trait]
impl Connector for RecordingConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { namespace: Namespace::Schema, transactions: true }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        if self.0.refuse_connect.load(Ordering::SeqCst) {
            return Err(ConnectError::new("connection refused by the test".to_owned()));
        }
        self.0.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(RecordingSession(self.0.clone())))
    }
}

/// Reads the numbers after `LIMIT` and `OFFSET`.
fn page_of(sql: &str) -> (usize, usize) {
    let words: Vec<&str> = sql.split_whitespace().collect();
    let after = |name: &str| {
        let at = words.iter().position(|word| *word == name).unwrap();
        words[at + 1].parse().unwrap()
    };
    (after("LIMIT"), after("OFFSET"))
}

#[async_trait]
impl Session for RecordingSession {
    fn canceller(&self) -> Arc<dyn Canceller> {
        Arc::new(StopHold(self.0.clone()))
    }

    async fn apply_context(&mut self, _: &SessionContext) -> Result<(), ExecError> {
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        self.0.statements.lock().unwrap().push(sql.to_owned());
        if self.0.hold.load(Ordering::SeqCst) {
            self.0.release.notified().await;
        }
        if self.0.cancelled.swap(false, Ordering::SeqCst) {
            return Err(ExecError::new("canceled"));
        }
        let failing = self.0.fail_when.lock().unwrap().clone();
        if failing.is_some_and(|fragment| sql.contains(&fragment)) {
            return Err(ExecError::new("syntax error near the filter"));
        }
        let (limit, offset) = page_of(sql);
        sink.set_columns(COLUMNS.iter().map(|name| (*name).to_owned()).collect()).await;
        let mut sent = 0;
        for index in offset..self.0.rows.min(offset + limit) {
            let text = if self.0.value_size > 0 {
                "x".repeat(self.0.value_size)
            } else {
                format!("name{index}")
            };
            let row: Row = vec![
                Value::Number(index.to_string().into_boxed_str()),
                Value::Text(text.into_boxed_str()),
            ]
            .into_boxed_slice();
            if sink.push(row).await != Flow::Continue {
                break;
            }
            sent += 1;
            if sent == self.0.mid_rows && self.0.mid_hold.load(Ordering::SeqCst) {
                self.0.release.notified().await;
                if self.0.cancelled.swap(false, Ordering::SeqCst) {
                    return Err(ExecError::new("canceled"));
                }
            }
        }
        let (rows, limit) = sink.finish().await;
        Ok(StatementEnd { rows, has_columns: true, changed: None, limit })
    }

    async fn transaction_state(&mut self) -> TransactionState {
        TransactionState::None
    }

    async fn control(&mut self, _: TransactionCommand) -> Result<(), ExecError> {
        Ok(())
    }

    async fn reset(&mut self) -> ResetOutcome {
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        Ok(sample_catalog())
    }

    async fn read_columns(&mut self, _: &str, _: &str) -> Result<Vec<String>, ExecError> {
        Ok(COLUMNS.iter().map(|name| (*name).to_owned()).collect())
    }
}

// ---------------------------------------------------------------------------
// The application, with keys, the mouse, and the screen.

struct Fixture {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE })
}

impl Fixture {
    fn start(options: Options) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self { app: App::new(options, clipboard.clone(), Theme::plain()), clipboard }
    }

    fn recorded(recorder: Recorder) -> (Self, Arc<Recorder>) {
        Self::recorded_with(recorder, Settings::default())
    }

    fn recorded_with(recorder: Recorder, settings: Settings) -> (Self, Arc<Recorder>) {
        Self::recorded_in(recorder, settings, KeyboardMode::Conventional)
    }

    fn recorded_in(
        recorder: Recorder,
        settings: Settings,
        keyboard: KeyboardMode,
    ) -> (Self, Arc<Recorder>) {
        let recorder = Arc::new(recorder);
        let options = Options {
            keyboard,
            connector: Some(Arc::new(RecordingConnector(recorder.clone()))),
            label: Some("server".to_owned()),
            settings,
            ..Options::default()
        };
        (Self::start(options), recorder)
    }

    fn sqlite(name: &str, setup: &str, settings: Settings) -> Self {
        let path = database(name, setup);
        Self::start(Options { database: Some(path), settings, ..Options::default() })
    }

    /// Two databases, as two connections.
    fn two_files(first: &str, second: &str, setup: &str) -> Self {
        let (one, two) = (database(first, setup), database(second, setup));
        let options = Options {
            database: Some(one),
            connections: vec![Definition::sqlite(SqliteTarget::new(two))],
            ..Options::default()
        };
        Self::start(options)
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

    /// Waits until the table on screen has no load in progress.
    fn loaded(&mut self) {
        self.pump_until(|f| f.app.table().is_some_and(|tab| !tab.is_busy()));
        self.app.drain_events();
    }

    fn screen(&mut self) -> String {
        self.screen_at(100, 30)
    }

    fn screen_at(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect();
                row.trim_end().to_owned() + "\n"
            })
            .collect()
    }

    #[track_caller]
    fn assert_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(screen.contains(text), "{text:?} is not on the screen:\n{screen}");
    }

    #[track_caller]
    fn assert_not_sees(&mut self, text: &str) {
        let screen = self.screen();
        assert!(!screen.contains(text), "{text:?} is on the screen:\n{screen}");
    }

    /// Returns the modifiers of the first cell of a text on the screen.
    fn modifier_of(&mut self, text: &str) -> ratatui::style::Modifier {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect();
            if let Some(byte) = row.find(text) {
                return buffer[(row[..byte].chars().count() as u16, y)].modifier;
            }
        }
        panic!("{text:?} is not on the screen");
    }

    /// Returns the column and the row of the first text below the row of `anchor`.
    fn locate_below(&mut self, anchor: &str, text: &str) -> (u16, u16) {
        let screen = self.screen();
        let lines: Vec<&str> = screen.lines().collect();
        let start = lines.iter().position(|line| line.contains(anchor)).unwrap_or_else(|| {
            panic!("{anchor:?} is not on the screen:\n{screen}");
        });
        for (offset, line) in lines.iter().enumerate().skip(start + 1) {
            if let Some(byte) = line.find(text) {
                return (line[..byte].chars().count() as u16, offset as u16);
            }
        }
        panic!("{text:?} is not below {anchor:?}:\n{screen}");
    }

    fn click(&mut self, column: u16, row: u16) {
        self.screen();
        self.app.on_event(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
    }

    fn click_text(&mut self, text: &str) {
        let screen = self.screen();
        for (y, line) in screen.lines().enumerate() {
            if let Some(byte) = line.find(text) {
                self.click(line[..byte].chars().count() as u16, y as u16);
                return;
            }
        }
        panic!("{text:?} is not on the screen:\n{screen}");
    }

    /// Selects a row of the navigator and opens it, with `Enter`.
    fn open_table_of(&mut self, connection: usize, name: &str) {
        common::open_table(&mut self.app, connection, name);
    }

    fn open_table(&mut self, name: &str) {
        self.open_table_of(0, name);
        self.loaded();
    }

    fn rows_loaded(&self) -> usize {
        self.app.table().unwrap().loaded()
    }

    fn focus_filter(&mut self) {
        self.prefix('k');
        assert_eq!(self.app.table().unwrap().focus, TableFocus::Filter);
    }

    fn type_filter(&mut self, text: &str) {
        self.focus_filter();
        self.type_text(text);
    }

    fn type_order(&mut self, text: &str) {
        self.focus_filter();
        self.code(KeyCode::Tab);
        assert_eq!(self.app.table().unwrap().focus, TableFocus::Order);
        self.type_text(text);
    }

    /// Applies the fields with `Enter` and waits for the first page.
    fn apply(&mut self) {
        self.code(KeyCode::Enter);
        self.loaded();
    }

    fn grid(&mut self) {
        self.prefix('j');
        assert_eq!(self.app.table().unwrap().focus, TableFocus::Grid);
    }

    fn load_more(&mut self) {
        self.grid();
        self.code(KeyCode::Char('m'));
        self.loaded();
    }
}

fn database(name: &str, setup: &str) -> PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path).unwrap().execute_batch(setup).unwrap();
    path
}

/// Returns SQL that fills `t` with rows `1..=count`.
fn numbered(count: usize) -> String {
    format!(
        "CREATE TABLE t(id INTEGER, name TEXT); \
         WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < {count}) \
         INSERT INTO t SELECT x, 'row' || x FROM n;"
    )
}

fn first_cell(f: &Fixture) -> String {
    let table = &f.app.table().unwrap().result.table;
    table.rows[0][0].display()
}

// ---------------------------------------------------------------------------
// Opening, tabs, and pages.

#[test]
fn enter_on_a_table_opens_a_tab_and_loads_the_first_page() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 250, ..Recorder::default() });
    f.open_table("users");
    assert_eq!(f.app.tables().len(), 1);
    assert!(f.app.table().is_some());
    assert_eq!(f.rows_loaded(), 100);
    assert_eq!(recorder.statements(), ["SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0"]);
    f.assert_sees("users (table)");
    f.assert_sees("server · public.users · table");
    f.assert_sees("100 rows loaded · more rows available");
    f.assert_sees("name0");
    f.assert_not_sees("name100");
}

#[test]
fn the_tabs_of_consoles_and_tables_share_one_row() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    let screen = f.screen();
    let tabs = screen.lines().next().unwrap();
    assert!(tabs.contains("console-1") && tabs.contains("users (table)"), "{tabs}");
    // `Ctrl+W p` and `Ctrl+W n` cycle through both kinds.
    f.prefix('p');
    assert!(f.app.table().is_none());
    f.prefix('n');
    assert!(f.app.table().is_some());
    f.prefix('n');
    assert!(f.app.table().is_none());
}

#[test]
fn a_click_on_a_tab_switches_between_a_console_and_a_table() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.click_text("console-1");
    assert!(f.app.table().is_none());
    f.click_text("users (table)");
    assert!(f.app.table().is_some());
}

#[test]
fn a_view_opens_with_a_label_that_names_it() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 3, ..Recorder::default() });
    f.open_table("active_users");
    f.assert_sees("active_users (view)");
    f.assert_sees("server · public.active_users · view");
    assert!(recorder.statements()[0].contains("\"active_users\""));
}

#[test]
fn opening_an_open_table_selects_its_tab() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.prefix('p');
    assert!(f.app.table().is_none());
    f.open_table_of(0, "users");
    assert!(f.app.table().is_some());
    assert_eq!(f.app.tables().len(), 1, "no second tab");
    assert_eq!(recorder.count(), 1, "no second query");
    assert!(!f.app.navigator_focused());
}

#[test]
fn tables_with_the_same_name_in_two_connections_are_two_tabs() {
    let mut f = Fixture::two_files("same-one", "same-two", &numbered(3));
    f.open_table_of(0, "t");
    f.loaded();
    f.open_table_of(1, "t");
    f.loaded();
    assert_eq!(f.app.tables().len(), 2);
    assert_ne!(f.app.tables()[0].connection, f.app.tables()[1].connection);
}

#[test]
fn a_double_click_opens_a_table_and_one_click_does_not() {
    let mut f = Fixture::sqlite("double-click", &numbered(3), Settings::default());
    f.prefix('h');
    f.code(KeyCode::Right);
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.assert_sees("▸ t");
    // The mark of a row opens its columns, so the clicks hit the name.
    let screen = f.screen();
    let (row, line) = screen.lines().enumerate().find(|(_, line)| line.contains("▸ t")).unwrap();
    let column = line.chars().position(|c| c == '▸').unwrap() as u16 + 3;
    f.click(column, row as u16);
    assert!(f.app.tables().is_empty(), "one click only selects");
    f.click(column, row as u16);
    assert_eq!(f.app.tables().len(), 1, "the second click opens the table");
}

#[test]
fn right_still_opens_the_columns_of_a_table_in_the_navigator() {
    let mut f = Fixture::sqlite("columns", &numbered(3), Settings::default());
    f.prefix('h');
    f.code(KeyCode::Right);
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.pump_until(|f| f.sees_text("name"));
    assert!(f.app.tables().is_empty());
}

impl Fixture {
    fn sees_text(&mut self, text: &str) -> bool {
        self.screen().contains(text)
    }
}

#[test]
fn load_more_adds_the_next_page_to_the_rows() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 250, ..Recorder::default() });
    f.open_table("users");
    f.load_more();
    assert_eq!(f.rows_loaded(), 200);
    assert_eq!(first_cell(&f), "0", "the first rows stay");
    f.assert_sees("200 rows loaded · more rows available");
    f.load_more();
    assert_eq!(f.rows_loaded(), 250);
    f.assert_sees("250 rows loaded · no more rows");
    let statements = recorder.statements();
    assert!(statements[1].ends_with("LIMIT 101 OFFSET 100"), "{statements:?}");
    assert!(statements[2].ends_with("LIMIT 101 OFFSET 200"), "{statements:?}");
    // Without more rows, the key asks nothing.
    f.code(KeyCode::Char('m'));
    f.assert_sees("no more rows to load");
    assert_eq!(recorder.count(), 3);
}

#[test]
fn a_page_with_zero_fewer_exact_and_more_rows_than_the_page_size() {
    for (rows, more) in [(0, false), (99, false), (100, false), (101, true)] {
        let (mut f, recorder) = Fixture::recorded(Recorder { rows, ..Recorder::default() });
        f.open_table("users");
        assert_eq!(f.rows_loaded(), rows.min(100), "{rows} rows");
        assert_eq!(f.app.table().unwrap().has_more, more, "{rows} rows");
        assert_eq!(recorder.count(), 1);
        if rows == 0 {
            // A result without rows keeps its columns and says zero rows.
            f.assert_sees("id");
            f.assert_sees("0 rows loaded · no more rows");
        }
    }
}

#[test]
fn the_page_size_comes_from_the_settings() {
    let settings = Settings { page_size: 7, ..Settings::default() };
    let (mut f, recorder) = Fixture::recorded_with(Recorder { rows: 30, ..Recorder::default() }, settings);
    f.open_table("users");
    assert_eq!(f.rows_loaded(), 7);
    assert!(recorder.statements()[0].ends_with("LIMIT 8 OFFSET 0"));
    f.load_more();
    assert_eq!(f.rows_loaded(), 14);
}

#[test]
fn the_load_more_button_and_its_key_do_the_same() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 150, ..Recorder::default() });
    f.open_table("users");
    f.click_text("[Load more]");
    f.loaded();
    assert_eq!(f.rows_loaded(), 150);
    assert_eq!(recorder.count(), 2);
}

#[test]
fn each_page_uses_one_session_and_gives_it_back() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 250, ..Recorder::default() });
    f.open_table("users");
    f.assert_sees("Pool 0/5");
    f.load_more();
    f.load_more();
    f.assert_sees("Pool 0/5");
    assert_eq!(recorder.opened.load(Ordering::SeqCst), 1, "no extra session");
}

#[test]
fn the_grid_of_a_table_copies_tsv_without_a_header() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 3, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "0\n");
    f.press(ctrl('a'));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "0\tname0\n1\tname1\n2\tname2\n");
}

// ---------------------------------------------------------------------------
// The filter bar.

#[test]
fn typing_in_the_bar_sends_no_query_and_enter_applies_both_fields() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("active = true");
    f.code(KeyCode::Tab);
    f.type_text("created_at DESC, id ASC");
    assert_eq!(recorder.count(), 1, "typing asks nothing");
    f.apply();
    let statements = recorder.statements();
    assert_eq!(
        statements[1],
        "SELECT * FROM \"public\".\"users\"\nWHERE active = true\nORDER BY created_at DESC, id ASC\nLIMIT 101 OFFSET 0"
    );
    f.assert_sees("WHERE    [active = true");
    f.assert_sees("ORDER BY [created_at DESC, id ASC");
}

#[test]
fn applying_starts_again_from_the_first_page() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 250, ..Recorder::default() });
    f.open_table("users");
    f.load_more();
    assert_eq!(f.rows_loaded(), 200);
    f.type_filter("x = 1");
    f.apply();
    assert_eq!(f.rows_loaded(), 100, "the old rows are gone");
    assert!(recorder.statements().last().unwrap().ends_with("LIMIT 101 OFFSET 0"));
}

#[test]
fn a_button_that_has_no_effect_looks_disabled() {
    use ratatui::style::Modifier;
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 150, ..Recorder::default() });
    f.open_table("users");
    assert!(!f.modifier_of("[Load more]").contains(Modifier::DIM), "a next page exists");
    assert!(!f.modifier_of("[Apply]").contains(Modifier::DIM));
    assert!(f.modifier_of("[Stop]").contains(Modifier::DIM), "nothing runs");
    f.load_more();
    assert!(f.modifier_of("[Load more]").contains(Modifier::DIM), "no more rows");
    // While a load runs, only Stop works.
    recorder.hold.store(true, Ordering::SeqCst);
    f.focus_filter();
    f.code(KeyCode::Enter);
    f.pump_until(|_| recorder.count() == 3);
    assert!(f.modifier_of("[Apply]").contains(Modifier::DIM));
    assert!(!f.modifier_of("[Stop]").contains(Modifier::DIM));
    recorder.release.notify_one();
    f.loaded();
}

#[test]
fn two_clicks_far_apart_in_time_do_not_open_a_table() {
    let mut f = Fixture::sqlite("slow-click", &numbered(3), Settings::default());
    f.prefix('h');
    f.code(KeyCode::Right);
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    let screen = f.screen();
    let (row, line) = screen.lines().enumerate().find(|(_, line)| line.contains("▸ t")).unwrap();
    let column = line.chars().position(|c| c == '▸').unwrap() as u16 + 3;
    f.click(column, row as u16);
    std::thread::sleep(Duration::from_millis(500));
    f.click(column, row as u16);
    assert!(f.app.tables().is_empty());
}

#[test]
fn the_apply_and_clear_buttons_work_with_the_mouse() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("a = 1");
    f.click_text("[Apply]");
    f.loaded();
    assert!(recorder.statements()[1].contains("WHERE a = 1"));
    f.click_text("[Clear]");
    f.loaded();
    assert_eq!(recorder.statements()[2], "SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0");
    let tab = f.app.table().unwrap();
    assert_eq!(tab.filter.text(), "");
    assert_eq!(tab.order.text(), "");
}

#[test]
fn a_click_on_a_field_gives_it_the_focus() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.click_text("WHERE");
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
    f.click_text("ORDER BY");
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Order);
    let (column, row) = f.locate_below("[Apply]", "id");
    f.click(column, row);
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Grid);
}

#[test]
fn f_and_o_in_the_grid_focus_the_fields() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.code(KeyCode::Char('f'));
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
    f.code(KeyCode::Esc);
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Grid);
    f.code(KeyCode::Char('o'));
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Order);
    // The keys are text in a field.
    f.type_text("fosm");
    assert_eq!(f.app.table().unwrap().order.text(), "fosm");
}

#[test]
fn tab_moves_between_the_fields_when_the_list_is_closed() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.focus_filter();
    f.code(KeyCode::Tab);
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Order);
    f.press(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
}

#[test]
fn a_paste_writes_one_line_in_the_focused_field() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.focus_filter();
    f.app.on_event(Event::Paste("a = 1\nAND b = 2".to_owned()));
    assert_eq!(f.app.table().unwrap().filter.text(), "a = 1 AND b = 2");
    assert_eq!(recorder.count(), 1);
}

#[test]
fn a_text_with_two_statements_leaves_an_empty_grid_and_the_reason() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("1 = 1; DROP TABLE users");
    f.code(KeyCode::Enter);
    assert_eq!(recorder.count(), 1, "nothing reaches the database");
    assert_eq!(f.rows_loaded(), 0);
    f.assert_sees("must stay in one statement");
    f.assert_not_sees("name0");
    // The fields stay open for a correction.
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
    for _ in 0.."; DROP TABLE users".len() {
        f.code(KeyCode::Backspace);
    }
    f.apply();
    assert_eq!(f.rows_loaded(), 5);
    f.assert_not_sees("must stay in one statement");
}

#[test]
fn a_failed_filter_clears_the_grid_and_shows_the_error() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    assert_eq!(f.rows_loaded(), 5);
    *recorder.fail_when.lock().unwrap() = Some("bad".to_owned());
    f.type_filter("bad");
    f.apply();
    assert_eq!(f.rows_loaded(), 0, "no data from the old query");
    f.assert_sees("syntax error near the filter");
    f.assert_not_sees("name0");
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
    // A correction loads the table again.
    *recorder.fail_when.lock().unwrap() = None;
    f.apply();
    assert_eq!(f.rows_loaded(), 5);
    f.assert_not_sees("syntax error");
}

#[test]
fn an_apply_waits_for_the_load_in_progress() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|_| recorder.count() == 1);
    assert!(f.app.table().unwrap().is_busy());
    f.type_filter("a = 1");
    f.code(KeyCode::Enter);
    f.assert_sees("load in progress");
    assert_eq!(recorder.count(), 1);
    recorder.release.notify_one();
    f.loaded();
    assert_eq!(f.rows_loaded(), 5);
}

#[test]
fn the_list_offers_the_columns_of_the_table_and_tab_accepts_without_a_query() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("cre");
    f.assert_sees("created_at");
    f.code(KeyCode::Tab);
    assert_eq!(f.app.table().unwrap().filter.text(), "created_at");
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter, "Tab accepted the suggestion");
    assert_eq!(recorder.count(), 1, "accepting asks nothing");
}

#[test]
fn enter_applies_the_typed_text_and_does_not_accept_the_highlighted_suggestion() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("cre");
    assert!(f.app.table().unwrap().list.is_some());
    f.apply();
    assert!(recorder.statements()[1].contains("\nWHERE cre\n"), "{:?}", recorder.statements());
    assert_eq!(f.app.table().unwrap().filter.text(), "cre");
}

#[test]
fn the_order_field_has_its_own_list_and_escape_closes_the_list_first() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_order("em");
    assert!(f.app.table().unwrap().list.is_some());
    f.assert_sees("email");
    f.code(KeyCode::Esc);
    assert!(f.app.table().unwrap().list.is_none());
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Order, "the first Esc closed the list");
    f.code(KeyCode::Esc);
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Grid);
}

#[test]
fn control_space_opens_the_list_without_a_prefix() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.focus_filter();
    f.press(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL));
    f.assert_sees("name");
    assert!(f.app.table().unwrap().list.is_some());
}

// ---------------------------------------------------------------------------
// Sorting.

#[test]
fn a_header_click_sorts_at_once_and_keeps_the_applied_filter_not_the_draft() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("a = 1");
    f.apply();
    // A draft that nobody applied.
    f.focus_filter();
    f.type_text(" AND draft = 2");
    let (column, row) = f.locate_below("[Apply]", "name");
    f.click(column, row);
    f.loaded();
    let statements = recorder.statements();
    assert_eq!(
        statements.last().unwrap(),
        "SELECT * FROM \"public\".\"users\"\nWHERE a = 1\nORDER BY name ASC\nLIMIT 101 OFFSET 0",
        "{statements:?}"
    );
    let tab = f.app.table().unwrap();
    assert_eq!(tab.order.text(), "name ASC", "the field shows the order");
    assert_eq!(tab.filter.text(), "a = 1 AND draft = 2", "the draft stays a draft");
    assert_eq!(tab.applied_filter(), "a = 1");
}

#[test]
fn clicking_the_same_header_again_reverses_and_then_removes_the_order() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    for expected in ["ORDER BY name ASC", "ORDER BY name DESC"] {
        let (column, row) = f.locate_below("[Apply]", "name");
        f.click(column, row);
        f.loaded();
        assert!(recorder.statements().last().unwrap().contains(expected), "{:?}", recorder.statements());
    }
    let (column, row) = f.locate_below("[Apply]", "name");
    f.click(column, row);
    f.loaded();
    assert!(!recorder.statements().last().unwrap().contains("ORDER BY"));
    assert_eq!(f.app.table().unwrap().order.text(), "");
}

#[test]
fn the_s_key_sorts_by_the_current_column() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.code(KeyCode::Right);
    f.code(KeyCode::Char('s'));
    f.loaded();
    assert!(recorder.statements().last().unwrap().contains("ORDER BY name ASC"));
}

#[test]
fn the_warning_about_the_order_shows_until_an_order_is_applied() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.assert_sees("No ORDER BY: pages can repeat or skip rows.");
    f.type_order("id");
    f.apply();
    f.assert_not_sees("No ORDER BY");
    f.type_order("");
    f.focus_filter();
    f.code(KeyCode::Tab);
    f.code(KeyCode::Backspace);
    f.code(KeyCode::Backspace);
    f.apply();
    f.assert_sees("No ORDER BY");
}

// ---------------------------------------------------------------------------
// Stop, close, and the other keys.

#[test]
fn stop_ends_a_page_and_keeps_the_rows_that_were_loaded() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 250, ..Recorder::default() });
    f.open_table("users");
    recorder.hold.store(true, Ordering::SeqCst);
    f.grid();
    f.code(KeyCode::Char('m'));
    f.pump_until(|_| recorder.count() == 2);
    assert!(f.app.table().unwrap().is_busy());
    f.assert_sees("loading");
    f.prefix('x');
    f.loaded();
    assert_eq!(f.rows_loaded(), 100, "the first page stays");
    f.assert_sees("The load stopped");
    // The next page can continue after the rows that stayed.
    recorder.hold.store(false, Ordering::SeqCst);
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert_eq!(f.rows_loaded(), 200);
    assert!(recorder.statements().last().unwrap().ends_with("OFFSET 100"));
}

#[test]
fn a_stop_in_the_middle_of_the_first_page_keeps_the_rows_and_the_next_page_continues() {
    // The sink sends a batch at 200 rows, so the page must be larger than that.
    let settings = Settings { page_size: 300, ..Settings::default() };
    let recorder = Recorder { rows: 1_000, mid_rows: 250, ..Recorder::default() };
    let (mut f, recorder) = Fixture::recorded_with(recorder, settings);
    recorder.mid_hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|f| f.app.table().is_some_and(|tab| tab.loaded() == 200));
    f.prefix('x');
    f.loaded();
    assert_eq!(f.rows_loaded(), 200, "the sent batch stays");
    assert!(f.app.table().unwrap().has_more, "the page ended early, so more rows can exist");
    recorder.mid_hold.store(false, Ordering::SeqCst);
    f.grid();
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert!(recorder.statements().last().unwrap().ends_with("LIMIT 301 OFFSET 200"));
    assert_eq!(f.rows_loaded(), 500);
}

#[test]
fn a_page_can_be_larger_than_the_row_cap_of_the_consoles() {
    let settings = Settings { page_size: 1_500, row_cap: 1_000, ..Settings::default() };
    let (mut f, _) = Fixture::recorded_with(Recorder { rows: 2_000, ..Recorder::default() }, settings);
    f.open_table("users");
    assert_eq!(f.rows_loaded(), 1_500);
    assert!(f.app.table().unwrap().has_more);
}

#[test]
fn the_stop_button_works_too() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|_| recorder.count() == 1);
    f.click_text("[Stop]");
    f.loaded();
    assert_eq!(f.rows_loaded(), 0);
    f.assert_sees("The load stopped");
}

#[test]
fn a_stopped_first_page_leaves_the_table_ready_for_a_new_apply() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|_| recorder.count() == 1);
    f.prefix('x');
    f.loaded();
    recorder.hold.store(false, Ordering::SeqCst);
    f.focus_filter();
    f.apply();
    assert_eq!(f.rows_loaded(), 5);
}

#[test]
fn closing_a_table_tab_frees_its_rows_and_drops_its_filters() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 50, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("a = 1");
    f.type_order("id");
    f.apply();
    f.assert_sees("Data 1/256 MiB");
    f.prefix('c');
    assert!(f.app.tables().is_empty());
    assert!(f.app.table().is_none());
    f.assert_sees("Data 0/256 MiB");
    // Reopening starts without filters or an order.
    f.open_table("users");
    let tab = f.app.table().unwrap();
    assert_eq!((tab.filter.text(), tab.order.text()), ("", ""));
    assert_eq!(recorder.statements().last().unwrap(), "SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0");
}

#[test]
fn the_fields_stay_while_the_tab_stays_open() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("a = 1");
    f.apply();
    f.prefix('p');
    f.prefix('n');
    let tab = f.app.table().unwrap();
    assert_eq!(tab.filter.text(), "a = 1");
    assert_eq!(tab.applied_filter(), "a = 1");
}

#[test]
fn the_keys_of_the_script_do_nothing_in_a_table() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.code(KeyCode::F(5));
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    f.prefix('e');
    f.prefix('m');
    f.prefix('C');
    f.prefix('R');
    f.prefix('d');
    f.grid();
    f.code(KeyCode::Char('x'));
    f.code(KeyCode::Char(']'));
    assert_eq!(recorder.count(), 1);
    assert_eq!(f.app.tables().len(), 1, "x does not close the tab");
    assert!(!f.app.console().manual, "the mode of the console did not change");
}

#[test]
fn the_help_of_a_table_lists_its_own_keys() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Conventional · Table");
    f.assert_sees("Load more rows");
    f.assert_sees("Sort by the column");
    f.assert_not_sees("Close the result");
    f.code(KeyCode::Esc);
    f.focus_filter();
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Conventional · Filter");
    f.assert_sees("Apply the fields");
}

#[test]
fn the_help_of_a_console_hides_the_keys_of_a_table() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.prefix('j');
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Conventional · Results");
    f.assert_sees("Close the result");
    f.assert_not_sees("Load more rows");
}

#[test]
fn the_hint_follows_the_part_of_the_table_in_focus() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.assert_sees("f where · o order by");
    f.focus_filter();
    f.assert_sees("Enter apply");
}

#[test]
fn the_table_view_fits_the_minimum_terminal() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 300, ..Recorder::default() });
    f.open_table("users");
    f.type_filter("a very long filter that needs more room than the field has in a small terminal");
    let screen = f.screen_at(80, 24);
    for text in ["WHERE", "ORDER BY", "[Apply]", "[Clear]", "[Load more]", "[Stop]", "rows loaded", "No ORDER BY"] {
        assert!(screen.contains(text), "{text}\n{screen}");
    }
    assert_eq!(screen.lines().count(), 24);
}

#[test]
fn a_database_name_cannot_send_terminal_sequences_through_a_table_tab() {
    let setup = "CREATE TABLE \"bad\x1b[31mname\"(\"co\x1b[31ml\", v); INSERT INTO \"bad\x1b[31mname\" VALUES ('x\x1b[2Jy', 1);";
    let mut f = Fixture::sqlite("table-escape", setup, Settings::default());
    f.prefix('h');
    f.code(KeyCode::Right);
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.loaded();
    f.type_filter("1 = 1 \x1b[0m");
    assert!(!f.screen().contains('\x1b'));
    assert!(f.screen().contains("bad"));
}

#[test]
fn a_failure_on_a_later_page_keeps_the_rows_and_lets_the_user_try_again() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 250, ..Recorder::default() });
    f.open_table("users");
    *recorder.fail_when.lock().unwrap() = Some("OFFSET 100".to_owned());
    f.grid();
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert_eq!(f.rows_loaded(), 100, "the first page stays");
    f.assert_sees("syntax error near the filter");
    f.assert_sees("name0");
    assert!(f.app.table().unwrap().has_more);
    *recorder.fail_when.lock().unwrap() = None;
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert_eq!(f.rows_loaded(), 200);
    f.assert_not_sees("syntax error");
}

#[test]
fn the_time_limit_ends_a_page_and_says_so() {
    let settings = Settings { sql_timeout: Some(Duration::from_millis(50)), ..Settings::default() };
    let (mut f, recorder) = Fixture::recorded_with(Recorder { rows: 5, ..Recorder::default() }, settings);
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.loaded();
    assert_eq!(f.rows_loaded(), 0);
    f.assert_sees("The time limit ended");
}

#[test]
fn a_refused_connection_shows_its_error_in_the_table() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.prefix('h');
    f.code(KeyCode::Right);
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    // The catalog read left an idle session, so the test closes it first.
    f.code(KeyCode::Char('D'));
    f.pump_until(|f| f.sees_text("Disconnected."));
    recorder.refuse_connect.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.loaded();
    f.assert_sees("connection refused by the test");
    assert_eq!(f.rows_loaded(), 0);
    // The fields stay available, and a new try works when the connection works.
    recorder.refuse_connect.store(false, Ordering::SeqCst);
    f.focus_filter();
    f.apply();
    assert_eq!(f.rows_loaded(), 5);
}

#[test]
fn closing_a_table_during_a_load_frees_the_rows_that_arrive_later() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 50, ..Recorder::default() });
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|_| recorder.count() == 1);
    f.prefix('c');
    assert!(f.app.tables().is_empty());
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    for _ in 0..50 {
        f.app.drain_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    f.assert_sees("Data 0/256 MiB");
}

#[test]
fn a_load_in_progress_blocks_disconnect_and_removal() {
    let (mut f, recorder) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|_| recorder.count() == 1);
    f.prefix('h');
    f.code(KeyCode::Char('D'));
    f.assert_sees("users (table) has an execution or a transaction");
    f.code(KeyCode::Delete);
    f.assert_not_sees("[Remove]");
    assert_eq!(f.app.connections().len(), 1);
    recorder.release.notify_one();
    f.loaded();
    f.code(KeyCode::Delete);
    f.assert_sees("[Remove]");
}

#[test]
fn the_mouse_scrolls_selects_and_resizes_in_the_grid_of_a_table() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 100, ..Recorder::default() });
    f.open_table("users");
    // The wheel moves the cursor by rows.
    let (column, row) = f.locate_below("[Apply]", "name0");
    f.app.on_event(mouse(MouseEventKind::ScrollDown, column, row));
    assert_eq!(f.app.table().unwrap().result.view.row, 3);
    // A drag selects a range of cells.
    f.click(column, row);
    f.app.on_event(mouse(MouseEventKind::Drag(MouseButton::Left), column + 2, row + 2));
    let view = f.app.table().unwrap().result.view;
    assert_eq!(view.selection().map(|range| (range.top, range.bottom)), Some((0, 2)));
    // A drag on the edge of a header changes the width of the column.
    let (edge, header) = f.locate_below("[Apply]", "id");
    let before = f.app.table().unwrap().result.table.width(0);
    let edge = edge + before as u16;
    f.click(edge, header);
    f.app.on_event(mouse(MouseEventKind::Drag(MouseButton::Left), edge + 4, header));
    f.app.on_event(mouse(MouseEventKind::Up(MouseButton::Left), edge + 4, header));
    assert_eq!(f.app.table().unwrap().result.table.width(0), before + 4);
    assert_eq!(f.app.tables().len(), 1);
    assert_eq!(f.app.table().unwrap().applied_order(), "", "a drag on an edge does not sort");
}

#[test]
fn the_search_of_a_table_grid_finds_loaded_rows() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 30, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.press(ctrl('f'));
    f.type_text("name2");
    f.pump_until(|f| f.sees_text("of 11"));
    f.assert_sees("Find:");
    f.code(KeyCode::Esc);
    f.assert_sees("30 rows loaded");
}

#[test]
fn a_paste_goes_to_the_field_in_focus_and_not_to_the_hidden_search() {
    let (mut f, _) = Fixture::recorded(Recorder { rows: 5, ..Recorder::default() });
    f.open_table("users");
    f.grid();
    f.press(ctrl('f'));
    f.type_text("a");
    f.click_text("WHERE");
    f.app.on_event(Event::Paste("b = 1".to_owned()));
    assert_eq!(f.app.table().unwrap().filter.text(), "b = 1");
    assert_eq!(f.app.table().unwrap().result.search.as_ref().unwrap().query(), "a");
}

#[test]
fn vim_keys_move_in_the_grid_of_a_table_and_stay_text_in_a_field() {
    let (mut f, _) = Fixture::recorded_in(
        Recorder { rows: 5, ..Recorder::default() },
        Settings::default(),
        KeyboardMode::Vim,
    );
    f.open_table("users");
    f.grid();
    f.code(KeyCode::Char('j'));
    f.code(KeyCode::Char('j'));
    assert_eq!(f.app.table().unwrap().result.view.row, 2);
    f.code(KeyCode::Char('f'));
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
    f.type_text("jjhl");
    assert_eq!(f.app.table().unwrap().filter.text(), "jjhl");
    f.code(KeyCode::Esc);
    f.code(KeyCode::Char('m'));
    f.assert_sees("no more rows to load");
}

#[test]
fn the_extra_row_of_a_page_leaves_the_budget_with_the_exact_cost() {
    use sqln::exec::{ExecEvent, ExecutionEnd, ReadLimit, StatementKey};
    use sqln::grid::Budget;
    use sqln::table::TableTab;

    let budget = Budget::new(1 << 20);
    let mut tab = TableTab::new(7, 1, relation(), false, 2);
    tab.started(5, true);
    let key = StatementKey { console: 7, execution: 5, index: 0 };
    let row = |n: &str| -> Row { vec![Value::Text(n.into())].into_boxed_slice() };
    let rows = vec![row("a"), row("b"), row("c")];
    let cost: usize = rows.iter().map(|row| budget.reserve(row)).map(|r| match r {
        sqln::grid::Reservation::Accepted(bytes) => bytes,
        other => panic!("{other:?}"),
    }).sum();
    tab.on_event(ExecEvent::Columns { key, columns: vec!["v".to_owned()] }, &budget);
    tab.on_event(ExecEvent::Rows { key, rows, bytes: cost }, &budget);
    tab.on_event(ExecEvent::ResultDone { key, rows: 3, limit: ReadLimit::None }, &budget);
    assert_eq!(tab.loaded(), 2, "the page keeps two rows");
    assert!(tab.has_more, "the third row proves a next page");
    assert_eq!(budget.used(), tab.result.table.bytes, "the extra row left the budget");
    let one = sqln::grid::row_cost(&row("a"));
    assert_eq!(budget.used(), 2 * one);
    tab.on_event(
        ExecEvent::ExecutionFinished { console: 7, execution: 5, end: ExecutionEnd::Completed, message: None },
        &budget,
    );
    // Rows of an old execution never reach the grid.
    let stale = StatementKey { execution: 4, ..key };
    let reserved = match budget.reserve(&row("z")) {
        sqln::grid::Reservation::Accepted(bytes) => bytes,
        other => panic!("{other:?}"),
    };
    tab.on_event(ExecEvent::Rows { key: stale, rows: vec![row("z")], bytes: reserved }, &budget);
    assert_eq!(tab.loaded(), 2);
    assert_eq!(budget.used(), 2 * one, "the stale rows left the budget");
}

#[test]
fn the_list_of_a_sqlite_table_waits_for_its_columns_and_then_shows_them() {
    let mut f = Fixture::sqlite("real-list", &numbered(3), Settings::default());
    f.open_table("t");
    f.type_filter("na");
    f.pump_until(|f| f.app.table().unwrap().list.is_some());
    f.assert_sees("name");
    f.code(KeyCode::Tab);
    assert_eq!(f.app.table().unwrap().filter.text(), "name");
}

// ---------------------------------------------------------------------------
// The data budget.

#[test]
fn the_budget_stops_a_page_and_refuses_load_more_until_space_is_free() {
    let settings = Settings { budget_bytes: 1_000, page_size: 8, ..Settings::default() };
    let (mut f, recorder) = Fixture::recorded_with(Recorder { rows: 100, ..Recorder::default() }, settings);
    f.open_table("orders");
    let first = f.rows_loaded();
    assert_eq!(first, 8, "the first table fits");
    f.open_table("users");
    let loaded = f.rows_loaded();
    assert!(loaded > 0 && loaded < 8, "the budget stopped the page at {loaded} rows");
    f.assert_sees("data budget reached");
    let queries = recorder.count();
    f.grid();
    f.code(KeyCode::Char('m'));
    f.assert_sees("data budget is full");
    assert_eq!(recorder.count(), queries, "a refused load asks nothing");
    assert_eq!(f.rows_loaded(), loaded, "the rows stay");

    // Closing the other tab frees space, and then the key loads the next page.
    f.prefix('p');
    f.prefix('c');
    assert_eq!(f.app.table().unwrap().relation.relation, "users", "the tab on the right is on screen");
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert!(f.rows_loaded() > loaded);
    assert!(recorder.statements().last().unwrap().contains(&format!("OFFSET {loaded}")));
}

#[test]
fn freed_space_loads_nothing_until_the_user_asks() {
    let settings = Settings { budget_bytes: 1_000, page_size: 8, ..Settings::default() };
    let (mut f, recorder) = Fixture::recorded_with(Recorder { rows: 100, ..Recorder::default() }, settings);
    f.open_table("orders");
    f.open_table("users");
    let queries = recorder.count();
    f.prefix('p');
    f.prefix('c');
    f.loaded();
    f.app.drain_events();
    assert_eq!(recorder.count(), queries);
}

#[test]
fn a_value_larger_than_the_budget_says_so_and_never_loads() {
    let settings = Settings { budget_bytes: 100, ..Settings::default() };
    let recorder = Recorder { rows: 5, value_size: 500, ..Recorder::default() };
    let (mut f, recorder) = Fixture::recorded_with(recorder, settings);
    f.open_table("users");
    f.assert_sees("a value is larger than the data budget");
    assert_eq!(f.rows_loaded(), 0);
    let queries = recorder.count();
    f.grid();
    f.code(KeyCode::Char('m'));
    assert_eq!(recorder.count(), queries);
}

// ---------------------------------------------------------------------------
// Real SQLite data.

fn ids(f: &Fixture) -> Vec<String> {
    f.app.table().unwrap().result.table.rows.iter().map(|row| row[0].display()).collect()
}

#[test]
fn sqlite_pages_filters_and_orders_real_rows() {
    let mut f = Fixture::sqlite("real-pages", &numbered(250), Settings::default());
    f.open_table("t");
    assert_eq!(f.rows_loaded(), 100);
    f.load_more();
    f.load_more();
    assert_eq!(f.rows_loaded(), 250);
    assert!(!f.app.table().unwrap().has_more);

    f.type_filter("id % 50 = 0");
    f.type_order("id DESC");
    f.apply();
    assert_eq!(ids(&f), ["250", "200", "150", "100", "50"]);
    f.assert_sees("5 rows loaded · no more rows");
    f.assert_not_sees("No ORDER BY");
}

#[test]
fn sqlite_orders_make_pages_continue_without_gaps_or_repeats() {
    let mut f = Fixture::sqlite("real-order", &numbered(130), Settings::default());
    f.open_table("t");
    f.type_order("id DESC");
    f.apply();
    f.load_more();
    let all = ids(&f);
    assert_eq!(all.len(), 130);
    let expected: Vec<String> = (1..=130).rev().map(|id| id.to_string()).collect();
    assert_eq!(all, expected);
}

#[test]
fn sqlite_reports_a_bad_column_in_the_filter() {
    let mut f = Fixture::sqlite("real-error", &numbered(5), Settings::default());
    f.open_table("t");
    f.type_filter("nope = 1");
    f.apply();
    assert_eq!(f.rows_loaded(), 0);
    f.assert_sees("no such column: nope");
    f.assert_not_sees("row1");
}

#[test]
fn sqlite_keeps_a_comment_in_the_filter_from_removing_the_limit() {
    let mut f = Fixture::sqlite("real-comment", &numbered(150), Settings::default());
    f.open_table("t");
    f.type_filter("id > 0 -- a note");
    f.apply();
    assert_eq!(f.rows_loaded(), 100);
    assert!(f.app.table().unwrap().has_more);
}

#[test]
fn sqlite_sorts_by_a_click_on_the_header() {
    let mut f = Fixture::sqlite("real-click", &numbered(20), Settings::default());
    f.open_table("t");
    let (column, row) = f.locate_below("[Apply]", "name");
    f.click(column, row);
    f.loaded();
    assert_eq!(ids(&f)[0], "1");
    assert_eq!(ids(&f)[1], "10", "text order");
    let (column, row) = f.locate_below("[Apply]", "name");
    f.click(column, row);
    f.loaded();
    assert_eq!(ids(&f)[0], "9");
}

#[test]
fn a_view_shows_its_rows() {
    let setup = format!("{} CREATE VIEW big AS SELECT id FROM t WHERE id > 5;", numbered(8));
    let mut f = Fixture::sqlite("real-view", &setup, Settings::default());
    f.open_table("big");
    assert_eq!(ids(&f), ["6", "7", "8"]);
    f.assert_sees("big (view)");
}

#[test]
fn removing_a_connection_closes_its_tables() {
    let mut f = Fixture::sqlite("real-remove", &numbered(3), Settings::default());
    f.open_table("t");
    f.assert_sees("Data 1/256 MiB");
    f.prefix('h');
    for _ in 0..4 {
        f.code(KeyCode::Up);
    }
    f.code(KeyCode::Delete);
    f.click_text("[Remove]");
    assert!(f.app.tables().is_empty());
    assert!(f.app.table().is_none());
    f.assert_sees("Data 0/256 MiB");
}

#[test]
fn closing_the_last_console_while_a_table_is_open_keeps_the_table() {
    let mut f = Fixture::sqlite("real-last", &numbered(3), Settings::default());
    f.open_table("t");
    f.prefix('p');
    assert!(f.app.table().is_none());
    f.prefix('c');
    assert!(!f.app.should_quit());
    assert!(f.app.table().is_some(), "the table is on screen");
    f.assert_not_sees("console-");
    // The last tab ends the application.
    f.prefix('c');
    assert!(f.app.should_quit());
}

#[test]
fn a_new_console_takes_the_place_after_the_tables_that_opened_before_it() {
    let mut f = Fixture::sqlite("real-order", &numbered(3), Settings::default());
    f.open_table("t");
    f.prefix('p');
    f.prefix('c');
    f.assert_not_sees("console-");
    f.prefix('h');
    f.code(KeyCode::Up);
    f.code(KeyCode::Up);
    f.code(KeyCode::Up);
    f.code(KeyCode::Char('n'));
    assert!(f.app.table().is_none(), "the new console is on screen");
    let screen = f.screen();
    let tabs = screen.lines().find(|line| line.contains("console-")).unwrap();
    assert!(tabs.find("(table)").unwrap() < tabs.find("console-").unwrap(), "{tabs}");
    f.prefix('c');
    assert!(!f.app.should_quit(), "the table stays open");
}

#[test]
fn the_tab_row_keeps_the_order_of_opening_with_consoles_and_tables_mixed() {
    let mut f = Fixture::sqlite("real-mix", &numbered(3), Settings::default());
    f.open_table("t");
    f.prefix('p');
    f.prefix('h');
    f.code(KeyCode::Up);
    f.code(KeyCode::Up);
    f.code(KeyCode::Up);
    f.code(KeyCode::Char('n'));
    let screen = f.screen();
    let tabs = screen.lines().find(|line| line.contains("console-1")).unwrap();
    let (first, table, second) = (
        tabs.find("console-1").unwrap(),
        tabs.find(" t").unwrap(),
        tabs.find("console-2").unwrap(),
    );
    assert!(first < table && table < second, "{tabs}");
    // The next tab follows the row, not the kind of the tab.
    f.prefix('p');
    assert!(f.app.table().is_some(), "the table is between the consoles");
    f.prefix('p');
    assert_eq!(f.app.console().name, "console-1");
}

#[test]
fn load_more_waits_for_a_free_connection_when_the_pool_is_full() {
    let settings = Settings { max_connections: 1, ..Settings::default() };
    let mut f = Fixture::sqlite("real-pool", &numbered(150), settings);
    f.open_table("t");
    f.assert_sees("Pool 0/1");
    // The console holds the only session with an open transaction.
    f.prefix('p');
    f.prefix('m');
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    f.app.on_event(Event::Paste("SELECT 1;".to_owned()));
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    f.pump_until(|f| !f.app.console().is_busy() && f.app.console().in_transaction());
    f.prefix('n');
    f.grid();
    f.code(KeyCode::Char('m'));
    f.pump_until(|f| f.sees_text("waiting for a connection"));
    assert_eq!(f.rows_loaded(), 100, "the first page stays while the next one waits");
    f.assert_sees("Pool 1/1");
    // A rollback frees the session, and the page loads.
    f.prefix('p');
    f.prefix('R');
    f.pump_until(|f| !f.app.console().in_transaction());
    f.prefix('n');
    f.loaded();
    assert_eq!(f.rows_loaded(), 150);
}

#[test]
fn a_catalog_that_changes_while_a_table_is_open_keeps_its_rows() {
    let mut f = Fixture::sqlite("real-refresh", &numbered(3), Settings::default());
    f.open_table("t");
    f.prefix('h');
    f.code(KeyCode::Char('r'));
    f.pump_until(|f| f.app.connections()[0].catalog.is_some());
    assert_eq!(f.app.tables().len(), 1);
    assert_eq!(f.rows_loaded(), 3);
}
