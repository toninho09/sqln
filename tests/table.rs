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
use common::sample_catalog;
use common::{ctrl, key, shift, temp_db};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::{Catalog, RelationRef};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::connector::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use sqln::console::Phase;
use sqln::editor::KeyboardMode;
use sqln::exec::{Flow, ReadLimit, ResultSink, Settings};
use sqln::grid::{Row, Value};
use sqln::input::SuggestionKeys;
use sqln::navigator::RowKind;
use sqln::sql::Dialect;
use sqln::structure::{Scope, Section};
use sqln::table::{TableFocus, clicked_order, page_sql};
use sqln::ui::theme::Theme;
use tokio::sync::Notify;

// ---------------------------------------------------------------------------
// The text of the queries.

fn relation() -> RelationRef {
    RelationRef {
        schema: "public".to_owned(),
        relation: "users".to_owned(),
    }
}

#[test]
fn a_page_asks_one_row_more_without_a_count() {
    let sql = page_sql(Dialect::Postgres, &relation(), "", "", 100, 0).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0"
    );
    let sql = page_sql(Dialect::Postgres, &relation(), "", "", 100, 200).unwrap();
    assert!(sql.ends_with("\nLIMIT 101 OFFSET 200"));
    assert!(!sql.to_uppercase().contains("COUNT"));
}

#[test]
fn the_clauses_start_their_own_lines() {
    let sql = page_sql(
        Dialect::Postgres,
        &relation(),
        "active = true",
        "created_at DESC, id ASC",
        10,
        0,
    )
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
    let sql = page_sql(
        Dialect::MySql,
        &relation(),
        "a = 1 # note",
        "a -- note",
        10,
        0,
    )
    .unwrap();
    assert!(sql.ends_with("\nLIMIT 11 OFFSET 0"), "{sql}");
}

#[test]
fn each_dialect_quotes_the_names_of_the_table() {
    let odd = RelationRef {
        schema: "my\"schema".to_owned(),
        relation: "Order Items".to_owned(),
    };
    let postgres = page_sql(Dialect::Postgres, &odd, "", "", 1, 0).unwrap();
    assert!(
        postgres.starts_with("SELECT * FROM \"my\"\"schema\".\"Order Items\"\n"),
        "{postgres}"
    );
    let sqlite = page_sql(Dialect::Sqlite, &odd, "", "", 1, 0).unwrap();
    assert!(
        sqlite.starts_with("SELECT * FROM \"my\"\"schema\".\"Order Items\"\n"),
        "{sqlite}"
    );
    let mysql_name = RelationRef {
        schema: "shop".to_owned(),
        relation: "or`ders".to_owned(),
    };
    let mysql = page_sql(Dialect::MySql, &mysql_name, "", "", 1, 0).unwrap();
    assert!(
        mysql.starts_with("SELECT * FROM `shop`.`or``ders`\n"),
        "{mysql}"
    );
}

#[test]
fn a_text_that_is_not_one_statement_is_refused() {
    for filter in [
        "1 = 1; DROP TABLE users",
        "1 = 1;",
        "name = 'open",
        "name = \"open",
        "a = 1 /* open",
    ] {
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
    assert!(
        page_sql(
            Dialect::Postgres,
            &relation(),
            "a = 1 /* x; y */",
            "",
            10,
            0
        )
        .is_ok()
    );
    assert!(page_sql(Dialect::Postgres, &relation(), "a = $$x;y$$", "", 10, 0).is_ok());
}

#[test]
fn a_click_on_a_header_goes_up_then_down_then_off() {
    let dialect = Dialect::Postgres;
    assert_eq!(clicked_order("", "name", dialect), "name ASC");
    assert_eq!(clicked_order("name ASC", "name", dialect), "name DESC");
    assert_eq!(clicked_order("NAME  desc", "name", dialect), "");
    // Another order, such as two columns, starts again from ascending.
    assert_eq!(
        clicked_order("created_at DESC, id ASC", "name", dialect),
        "name ASC"
    );
    assert_eq!(clicked_order("id ASC", "name", dialect), "name ASC");
    // A name that needs quotes keeps them in the field.
    assert_eq!(
        clicked_order("", "Order Items", dialect),
        "\"Order Items\" ASC"
    );
    assert_eq!(
        clicked_order("", "Order Items", Dialect::MySql),
        "`Order Items` ASC"
    );
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
    /// True makes the source a Redis server.
    redis: bool,
    /// The answer of the database to a signal. `f` means that the session is gone.
    signal_answer: Option<&'static str>,
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
        if self.0.redis {
            Dialect::Redis
        } else {
            Dialect::Postgres
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Schema,
            transactions: true,
            table_filters: !self.0.redis,
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        if self.0.refuse_connect.load(Ordering::SeqCst) {
            return Err(ConnectError::new(
                "connection refused by the test".to_owned(),
            ));
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
        // A page of a key is not SQL, and these tests read no value.
        if self.0.redis {
            return Ok(StatementEnd::default());
        }
        if sql.starts_with("SELECT COUNT(*)") {
            sink.set_columns(vec!["count".to_owned()]).await;
            let total = Value::Number(&self.0.rows.to_string());
            sink.push(vec![total].into_iter().collect()).await;
            let (rows, limit) = sink.finish().await;
            return Ok(StatementEnd {
                rows,
                has_columns: true,
                changed: None,
                limit,
            });
        }
        if sql.contains("_backend(") && !sql.contains("pg_stat_activity") {
            sink.set_columns(vec!["signal".to_owned()]).await;
            let answer = self.0.signal_answer.unwrap_or("t");
            sink.push(vec![Value::Text(answer)].into_iter().collect())
                .await;
            let (rows, limit) = sink.finish().await;
            return Ok(StatementEnd {
                rows,
                has_columns: true,
                changed: None,
                limit,
            });
        }
        let (limit, offset) = page_of(sql);
        sink.set_columns(COLUMNS.iter().map(|name| (*name).to_owned()).collect())
            .await;
        let mut sent = 0;
        for index in offset..self.0.rows.min(offset + limit) {
            let text = if self.0.value_size > 0 {
                "x".repeat(self.0.value_size)
            } else {
                format!("name{index}")
            };
            let row: Row = vec![Value::Number(&index.to_string()), Value::Text(&text)]
                .into_iter()
                .collect();
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
        Ok(StatementEnd {
            rows,
            has_columns: true,
            changed: None,
            limit,
        })
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
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

impl Fixture {
    fn start(options: Options) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard.clone(), Theme::plain()),
            clipboard,
        }
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
        Self::start(Options {
            database: Some(path),
            settings,
            ..Options::default()
        })
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
            assert!(
                Instant::now() < deadline,
                "the condition did not happen:\n{}",
                self.screen()
            );
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
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row.trim_end().to_owned() + "\n"
            })
            .collect()
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

    /// Returns the modifiers of the first cell of a text on the screen.
    fn modifier_of(&mut self, text: &str) -> ratatui::style::Modifier {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
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
        let start = lines
            .iter()
            .position(|line| line.contains(anchor))
            .unwrap_or_else(|| {
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
        self.app
            .on_event(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
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
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(setup)
        .unwrap();
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
    table.rows[0].get(0).unwrap().display()
}

// ---------------------------------------------------------------------------
// The structure tab.

impl Fixture {
    /// Presses `i` on a row of the navigator.
    fn structure_of(&mut self, kinds: &[RowKind], name: &str) {
        common::select_row(&mut self.app, 0, kinds, name);
        self.code(KeyCode::Char('i'));
        self.loaded();
    }

    fn structure_of_table(&mut self, name: &str) {
        self.structure_of(&[RowKind::Table], name);
    }

    fn section(&self) -> Option<Section> {
        self.app.table().unwrap().section()
    }

    /// Returns the cells of the first column, for a tab of a real database.
    fn first_column(&self) -> Vec<String> {
        let table = &self.app.table().unwrap().result.table;
        table
            .rows
            .iter()
            .map(|row| row.get(0).unwrap().display())
            .collect()
    }
}

const STRUCTURE_SETUP: &str = "\
    CREATE TABLE owners(id INTEGER PRIMARY KEY);\
    CREATE TABLE items(\
      id INTEGER PRIMARY KEY,\
      owner INTEGER REFERENCES owners(id),\
      label TEXT NOT NULL DEFAULT 'none');\
    CREATE INDEX items_label ON items(label);";

fn structure_files(name: &str) -> Fixture {
    Fixture::sqlite(name, STRUCTURE_SETUP, Settings::default())
}

#[test]
fn i_on_a_table_shows_its_columns_in_a_new_tab() {
    let mut f = structure_files("structure-table");
    f.structure_of_table("items");
    assert!(!f.app.navigator_focused());
    f.assert_sees("items (structure)");
    f.assert_sees("nullable");
    f.assert_sees("INTEGER");
    f.assert_sees("none");
    assert_eq!(f.first_column(), ["id", "owner", "label"]);
}

#[test]
fn a_structure_tab_has_no_filter_fields_and_a_reload_button() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.structure_of_table("users");
    f.assert_not_sees("WHERE");
    f.assert_not_sees("ORDER BY");
    f.assert_sees("Reload");
}

#[test]
fn a_second_i_selects_the_open_structure_tab() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    f.structure_of_table("users");
    let reads = recorder.count();
    f.structure_of_table("users");
    assert_eq!(f.app.tables().len(), 1);
    assert_eq!(recorder.count(), reads);
}

#[test]
fn the_rows_and_the_structure_of_a_table_are_two_tabs() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.open_table("users");
    f.structure_of_table("users");
    assert_eq!(f.app.tables().len(), 2);
    f.assert_sees("users (table)");
    f.assert_sees("users (structure)");
    f.open_table("users");
    assert_eq!(f.app.tables().len(), 2);
    assert!(f.app.table().unwrap().structure.is_none());
}

#[test]
fn i_on_the_grid_of_a_table_shows_its_structure() {
    let mut f = structure_files("structure-grid");
    f.open_table("items");
    f.code(KeyCode::Char('i'));
    f.loaded();
    assert_eq!(f.app.tables().len(), 2);
    assert_eq!(f.first_column(), ["id", "owner", "label"]);
}

#[test]
fn i_in_a_structure_tab_opens_no_other_tab() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.structure_of_table("users");
    f.code(KeyCode::Char('i'));
    assert_eq!(f.app.tables().len(), 1);
}

#[test]
fn the_keys_of_the_results_change_the_section_and_read_it() {
    let mut f = structure_files("structure-sections");
    f.structure_of_table("items");
    assert_eq!(f.section(), Some(Section::Columns));
    f.assert_sees("Foreign keys");

    f.code(KeyCode::Char(']'));
    f.loaded();
    assert_eq!(f.section(), Some(Section::Indexes));
    assert_eq!(f.first_column(), ["items_label"]);

    f.code(KeyCode::Char(']'));
    f.loaded();
    assert_eq!(f.section(), Some(Section::ForeignKeys));
    f.assert_sees("owners (id)");

    f.code(KeyCode::Char('['));
    f.loaded();
    assert_eq!(f.section(), Some(Section::Indexes));
}

#[test]
fn a_click_on_a_section_reads_it() {
    let mut f = structure_files("structure-click");
    f.structure_of_table("items");
    f.click_text("Foreign keys");
    f.loaded();
    assert_eq!(f.section(), Some(Section::ForeignKeys));
    f.assert_sees("owners (id)");
}

#[test]
fn a_section_cannot_change_during_a_read() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    recorder.hold.store(true, Ordering::SeqCst);
    common::select_row(&mut f.app, 0, &[RowKind::Table], "users");
    f.code(KeyCode::Char('i'));
    f.pump_until(|f| f.app.table().is_some_and(|tab| tab.is_busy()));
    f.code(KeyCode::Char(']'));
    assert_eq!(f.section(), Some(Section::Columns));
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    f.loaded();
}

#[test]
fn i_on_a_column_shows_the_structure_of_its_table() {
    let mut f = structure_files("structure-column");
    common::select_row(&mut f.app, 0, &[RowKind::Table], "items");
    f.code(KeyCode::Right);
    f.pump_until(|f| {
        let connections = f.app.connections();
        let rows = f.app.navigator().rows(connections);
        rows.iter().any(|row| row.kind == RowKind::Column)
    });
    f.code(KeyCode::Down);
    f.code(KeyCode::Char('i'));
    f.loaded();
    f.assert_sees("items (structure)");
}

#[test]
fn i_on_a_schema_lists_the_columns_of_each_table_without_sections() {
    let mut f = structure_files("structure-schema");
    f.structure_of(&[RowKind::Schema], "main");
    f.assert_sees("main (structure)");
    f.assert_not_sees("Foreign keys");
    assert_eq!(f.section(), None);
    let tab = f.app.table().unwrap();
    assert_eq!(tab.structure.as_ref().unwrap().scope, Scope::Schema);
    assert_eq!(tab.loaded(), 4);
    f.assert_sees("owners");
    f.assert_sees("items");
}

#[test]
fn i_on_a_connection_lists_the_columns_of_each_schema() {
    let mut f = structure_files("structure-connection");
    let name = f.app.connections()[0].name.clone();
    f.structure_of(&[RowKind::Connection], &name);
    let tab = f.app.table().unwrap();
    assert_eq!(tab.structure.as_ref().unwrap().scope, Scope::Connection);
    assert_eq!(tab.loaded(), 4);
    assert_eq!(first_cell(&f), "main");
}

#[test]
fn the_keys_of_the_results_change_the_page_of_a_schema() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.structure_of(&[RowKind::Schema], "public");
    assert_eq!(f.rows_loaded(), 100);
    f.code(KeyCode::Char(']'));
    f.loaded();
    assert_eq!(f.rows_loaded(), 200);
    assert_eq!(page_of(&recorder.statements()[1]), (101, 100));
}

#[test]
fn a_structure_tab_reads_again_with_the_reload_button() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    f.structure_of_table("users");
    let reads = recorder.count();
    f.click_text("Reload");
    f.loaded();
    assert_eq!(recorder.count(), reads + 1);
}

#[test]
fn the_hints_of_a_structure_tab_name_the_section() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.structure_of_table("users");
    f.assert_sees("] section");
    f.assert_not_sees("i struct");
    f.assert_not_sees("where");
}

#[test]
fn the_hints_of_a_table_show_the_structure_key_at_the_minimum_width() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.open_table("users");
    let hints = f.hints_at(80);
    assert!(hints.contains("s sort  i struct  Ctrl+C copy"), "{hints}");
    let hints = f.hints_at(120);
    assert!(hints.ends_with("e export  I import  F1 keys"), "{hints}");
}

#[test]
fn a_table_of_many_rows_keeps_the_structure_key_at_the_minimum_width() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.open_table("users");
    let hints = f.hints_at(80);
    assert!(hints.contains("] page  i struct"), "{hints}");
}

impl Fixture {
    fn hints_at(&mut self, width: u16) -> String {
        self.screen_at(width, 24)
            .lines()
            .last()
            .unwrap()
            .trim()
            .to_owned()
    }
}

#[test]
fn a_busy_table_puts_the_stop_key_first() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    recorder.hold.store(true, Ordering::SeqCst);
    common::open_table(&mut f.app, 0, "users");
    f.pump_until(|f| f.app.table().is_some_and(|tab| tab.is_busy()));
    let row = f.hints_at(80);
    assert!(row.starts_with("Ctrl+W x stop"), "{row}");
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    f.loaded();
    let row = f.hints_at(100);
    assert!(!row.contains("stop"), "{row}");
}

#[test]
fn a_busy_filter_field_shows_the_stop_key() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    f.open_table("users");
    f.focus_filter();
    recorder.hold.store(true, Ordering::SeqCst);
    f.code(KeyCode::Enter);
    f.pump_until(|f| f.app.table().is_some_and(|tab| tab.is_busy()));
    let row = f.hints_at(80);
    assert!(row.starts_with("Ctrl+W x stop"), "{row}");
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    f.loaded();
}

#[test]
fn a_busy_result_of_a_console_shows_the_stop_key() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    f.app
        .on_event(Event::Paste("SELECT 1 LIMIT 1 OFFSET 0;".to_owned()));
    recorder.hold.store(true, Ordering::SeqCst);
    f.code(KeyCode::F(5));
    f.pump_until(|f| f.app.console_mut().is_busy());
    f.prefix('j');
    let row = f.hints_at(80);
    assert!(row.starts_with("Ctrl+W x stop"), "{row}");
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    f.pump_until(|f| !f.app.console_mut().is_busy());
}

#[test]
fn a_table_without_more_rows_hides_the_commands_of_more_rows() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.open_table("users");
    let row = f.hints_at(100);
    assert!(!row.contains("m more"), "{row}");
    assert!(!row.contains("] page"), "{row}");
    assert!(row.contains("i struct"), "{row}");
}

#[test]
fn a_table_with_more_rows_shows_the_commands_of_more_rows() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.open_table("users");
    let row = f.hints_at(100);
    assert!(row.contains("m more  ] page  i struct"), "{row}");
}

#[test]
fn a_table_without_rows_hides_the_commands_of_a_row() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.open_table("users");
    let row = f.hints_at(100);
    assert!(row.contains("f where"), "{row}");
    for command in ["sort", "copy", "value", "find"] {
        assert!(!row.contains(command), "{command}: {row}");
    }
}

#[test]
fn a_table_shows_the_find_key_with_rows() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.open_table("users");
    let row = f.hints_at(120);
    assert!(row.contains("Ctrl+F find"), "{row}");
}

#[test]
fn a_structure_tab_hides_the_load_key_without_more_rows() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.structure_of_table("users");
    let row = f.hints_at(100);
    assert!(!row.contains("m more"), "{row}");
    assert!(row.starts_with("] section"), "{row}");
}

#[test]
fn a_connection_shows_the_disconnect_key_only_with_sessions() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    let disconnect = "D Disconnect".to_owned();
    assert!(!common::explorer_page(&f.app).contains(&disconnect));
    f.open_table("users");
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    assert!(common::explorer_page(&f.app).contains(&disconnect));
}

#[test]
fn a_redis_row_hides_the_structure_key() {
    let (mut f, _) = Fixture::recorded(Recorder {
        redis: true,
        ..Recorder::default()
    });
    common::select_row(&mut f.app, 0, &[RowKind::Schema], "public");
    let page = common::explorer_page(&f.app);
    assert!(
        !page.contains(&"i Show the structure".to_owned()),
        "{page:?}"
    );
    assert!(!page.contains(&"S Sessions".to_owned()), "{page:?}");
}

#[test]
fn the_row_of_sections_fits_the_smallest_screen() {
    let mut f = structure_files("structure-small");
    f.structure_of_table("items");
    let screen = f.screen_at(80, 24);
    assert!(screen.contains("Foreign keys"), "{screen}");
    assert!(screen.contains("nullable"), "{screen}");
}

#[test]
fn the_title_of_a_connection_does_not_repeat_its_name() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.structure_of(&[RowKind::Connection], "server");
    f.assert_sees("server · all schemas · structure");
}

#[test]
fn the_help_of_a_structure_tab_lists_only_the_keys_that_act() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.structure_of_table("users");
    f.code(KeyCode::F(1));
    let screen = f.screen_at(100, 70);
    assert!(screen.contains("Next section"), "{screen}");
    assert!(!screen.contains("Next page of rows"), "{screen}");
    assert!(!screen.contains("Edit the WHERE filter"), "{screen}");
    assert!(!screen.contains("Show the structure"), "{screen}");
}

#[test]
fn the_help_of_a_table_tab_lists_the_structure_key() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.open_table("users");
    f.code(KeyCode::F(1));
    let screen = f.screen_at(100, 70);
    assert!(screen.contains("Next page of rows"), "{screen}");
    assert!(
        screen.contains("Show the structure of the table"),
        "{screen}"
    );
}

#[test]
fn the_palette_opens_the_structure_of_the_table_on_screen() {
    let (mut f, _) = Fixture::recorded(Recorder::default());
    f.open_table("users");
    f.press(ctrl('p'));
    f.type_text("structure of the table");
    f.code(KeyCode::Enter);
    f.loaded();
    f.assert_sees("users (structure)");

    f.press(ctrl('p'));
    f.assert_not_sees("Show the structure of the table");
}

#[test]
fn a_redis_source_refuses_the_structure_with_a_reason() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        redis: true,
        ..Recorder::default()
    });
    common::select_row(&mut f.app, 0, &[RowKind::Schema], "public");
    f.code(KeyCode::Char('i'));
    assert!(f.app.tables().is_empty());
    assert_eq!(recorder.count(), 0);
    f.assert_sees(sqln::ui::text::STRUCTURE_REFUSED);
}

#[test]
fn a_key_of_redis_refuses_the_structure_in_its_grid() {
    let (mut f, _) = Fixture::recorded(Recorder {
        redis: true,
        ..Recorder::default()
    });
    f.open_table("users");
    f.assert_not_sees("i struct");
    f.code(KeyCode::Char('i'));
    assert_eq!(f.app.tables().len(), 1);
    f.assert_sees(sqln::ui::text::STRUCTURE_REFUSED);
}

// ---------------------------------------------------------------------------
// The sessions of a server.

impl Fixture {
    fn open_sessions(&mut self) {
        common::select_row(&mut self.app, 0, &[RowKind::Schema], "public");
        self.press(shift(KeyCode::Char('S')));
        self.loaded();
    }

    /// Answers the question with its second option.
    fn confirm(&mut self) {
        self.code(KeyCode::Right);
        self.code(KeyCode::Enter);
    }
}

fn signals(recorder: &Recorder) -> Vec<String> {
    let statements = recorder.statements();
    statements
        .into_iter()
        .filter(|sql| sql.contains("_backend(") && !sql.contains("pg_stat_activity"))
        .collect()
}

fn session_reads(recorder: &Recorder) -> usize {
    let statements = recorder.statements();
    statements
        .iter()
        .filter(|sql| sql.contains("pg_stat_activity"))
        .count()
}

fn two_sessions() -> Recorder {
    Recorder {
        rows: 2,
        ..Recorder::default()
    }
}

#[test]
fn the_key_opens_a_tab_with_the_sessions_of_the_server() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    assert_eq!(session_reads(&recorder), 1);
    let screen = f.screen();
    assert!(screen.contains("server (sessions)"), "{screen}");
    assert!(screen.contains("name1"), "{screen}");
}

#[test]
fn a_second_key_gives_the_focus_to_the_open_tab() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    f.open_sessions();
    assert_eq!(f.app.tables().len(), 1);
    assert_eq!(session_reads(&recorder), 1);
}

#[test]
fn a_source_without_sessions_refuses_with_a_reason() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        redis: true,
        ..Recorder::default()
    });
    common::select_row(&mut f.app, 0, &[RowKind::Schema], "public");
    f.press(shift(KeyCode::Char('S')));
    assert!(f.app.tables().is_empty());
    assert_eq!(recorder.count(), 0);
    f.assert_sees(sqln::ui::text::SESSIONS_REFUSED);
}

#[test]
fn sqlite_refuses_the_sessions_with_a_reason() {
    let mut f = Fixture::sqlite(
        "sessions-sqlite.db",
        "CREATE TABLE users (id INT);",
        Settings::default(),
    );
    f.press(ctrl('w'));
    f.code(KeyCode::Char('h'));
    f.press(shift(KeyCode::Char('S')));
    assert!(f.app.tables().is_empty());
    f.assert_sees(sqln::ui::text::SESSIONS_REFUSED);
}

#[test]
fn a_cancel_asks_first_and_then_reads_the_sessions_again() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    f.code(KeyCode::Char('q'));
    let screen = f.screen();
    assert!(screen.contains("Session 0 of name0."), "{screen}");
    assert!(screen.contains("The query stops"), "{screen}");
    assert!(signals(&recorder).is_empty(), "the question comes first");

    f.confirm();
    f.loaded();
    assert_eq!(
        signals(&recorder),
        ["SELECT pg_catalog.pg_cancel_backend(0)"]
    );
    assert_eq!(session_reads(&recorder), 2);
    f.assert_sees(sqln::ui::text::SESSION_SIGNAL_SENT);
}

#[test]
fn the_end_of_a_session_uses_the_row_of_the_cursor() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    f.code(KeyCode::Down);
    f.press(shift(KeyCode::Char('K')));
    let screen = f.screen();
    assert!(screen.contains("Session 1 of name1."), "{screen}");
    assert!(screen.contains("The session closes"), "{screen}");
    f.confirm();
    f.loaded();
    assert_eq!(
        signals(&recorder),
        ["SELECT pg_catalog.pg_terminate_backend(1)"]
    );
}

#[test]
fn the_cancel_option_of_the_question_sends_nothing() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    f.press(shift(KeyCode::Char('K')));
    f.code(KeyCode::Enter);
    f.loaded();
    assert!(signals(&recorder).is_empty());
    assert_eq!(session_reads(&recorder), 1);
}

#[test]
fn a_session_that_ended_first_gives_a_note() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 1,
        signal_answer: Some("f"),
        ..Recorder::default()
    });
    f.open_sessions();
    f.press(shift(KeyCode::Char('K')));
    f.confirm();
    f.loaded();
    assert_eq!(signals(&recorder).len(), 1);
    assert_eq!(session_reads(&recorder), 2);
    f.assert_sees(sqln::ui::text::SESSION_GONE);
}

#[test]
fn a_refused_signal_shows_the_error_and_keeps_the_rows() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    *recorder.fail_when.lock().unwrap() = Some("pg_terminate_backend(".to_owned());
    f.press(shift(KeyCode::Char('K')));
    f.confirm();
    f.loaded();
    assert_eq!(session_reads(&recorder), 1, "an error reads nothing");
    let screen = f.screen();
    assert!(screen.contains("syntax error near the filter"), "{screen}");
    assert!(screen.contains("name1"), "{screen}");
}

#[test]
fn a_busy_tab_refuses_a_signal_before_the_question() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    recorder.hold.store(true, Ordering::SeqCst);
    f.press(shift(KeyCode::Char('R')));
    f.press(shift(KeyCode::Char('K')));
    f.assert_not_sees("Session 0");
    f.assert_sees(sqln::ui::text::TABLE_BUSY);
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    f.loaded();
    assert!(signals(&recorder).is_empty());
}

#[test]
fn a_tab_out_of_sight_reads_the_sessions_when_it_comes_back() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    recorder.hold.store(true, Ordering::SeqCst);
    f.press(shift(KeyCode::Char('K')));
    f.confirm();
    f.prefix('n');
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.app.tables()[0].pending {
        f.app.drain_events();
        assert!(Instant::now() < deadline, "the signal did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(session_reads(&recorder), 1);
    f.prefix('n');
    f.loaded();
    assert_eq!(session_reads(&recorder), 2);
}

#[test]
fn a_read_only_connection_refuses_the_signals() {
    let settings = Settings {
        read_only: true,
        ..Settings::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(two_sessions(), settings);
    f.open_sessions();
    let screen = f.screen();
    assert!(!screen.contains("end session"), "{screen}");
    f.press(shift(KeyCode::Char('K')));
    f.code(KeyCode::Char('q'));
    assert!(signals(&recorder).is_empty());
    f.assert_sees(sqln::ui::text::SESSIONS_READ_ONLY);
}

#[test]
fn the_keys_of_a_session_do_nothing_in_a_table() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_table("users");
    f.press(shift(KeyCode::Char('K')));
    f.code(KeyCode::Char('q'));
    f.assert_not_sees("Session 0");
    assert!(signals(&recorder).is_empty());
}

#[test]
fn the_hints_of_a_sessions_tab_name_its_keys() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.open_sessions();
    let hint = f.app.hint();
    assert!(hint.contains("cancel query"), "{hint}");
    assert!(hint.contains("end session"), "{hint}");
    assert!(hint.contains("reload"), "{hint}");
    assert!(!hint.contains("struct"), "{hint}");
}

#[test]
fn the_reload_key_reads_a_tab_without_a_filter_again() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_sessions();
    f.press(shift(KeyCode::Char('R')));
    f.loaded();
    assert_eq!(session_reads(&recorder), 2);
}

#[test]
fn the_reload_key_does_nothing_in_a_table_with_a_filter() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_table("users");
    let before = recorder.count();
    f.press(shift(KeyCode::Char('R')));
    f.loaded();
    assert_eq!(recorder.count(), before);
    assert!(!f.app.hint().contains("reload"));
}

// ---------------------------------------------------------------------------
// The `Ctrl+W` menu and its explorer page.

impl Fixture {
    /// Presses `Ctrl+W Tab`, which opens the explorer page.
    fn open_explorer_page(&mut self) {
        self.press(ctrl('w'));
        self.code(KeyCode::Tab);
    }

    fn session_tabs(&self) -> usize {
        let tabs = self.app.tables().iter();
        tabs.filter(|tab| tab.shows_sessions()).count()
    }

    fn menu_has(&self, row: &str) -> bool {
        let rows = common::prefix_page(&self.app);
        rows.iter().any(|item| item == row)
    }
}

/// The rows of the first page that act only on a console.
const CONSOLE_ROWS: &[&str] = &[
    "e Run the statement",
    "s Run the script",
    "E Show the plan",
    "A Plan with a run",
    "f Format the SQL",
    "/ Comment the rows",
    "N Rename the tab",
    "m Auto or manual",
    "d Schema or database",
    "a Change connection",
];

#[test]
fn the_menu_of_a_console_of_a_server_lists_its_commands() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.app.on_event(Event::Paste("SELECT 1;".to_owned()));
    for row in CONSOLE_ROWS.iter().chain(&["S Sessions"]) {
        assert!(f.menu_has(row), "{row}");
    }
}

#[test]
fn the_menu_of_a_table_leaves_out_the_commands_of_a_console() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.open_table("users");
    for row in CONSOLE_ROWS {
        assert!(!f.menu_has(row), "{row}");
    }
    for row in [
        "hk Move the focus",
        "n Next tab",
        "c Close the tab",
        "S Sessions",
    ] {
        assert!(f.menu_has(row), "{row}");
    }
    f.press(ctrl('w'));
    f.assert_sees("Application");
    f.assert_not_sees("Transaction");
    f.code(KeyCode::Esc);
    f.focus_filter();
    for row in CONSOLE_ROWS {
        assert!(!f.menu_has(row), "{row}");
    }
    assert!(f.menu_has("hj Move the focus"));
}

#[test]
fn a_tab_without_filters_has_no_key_up_in_the_menu() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.press(ctrl('w'));
    f.press(shift(KeyCode::Char('S')));
    f.loaded();
    assert!(f.menu_has("h Move the focus"));
}

#[test]
fn the_menu_of_the_explorer_keeps_the_sessions_of_the_row() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.prefix('h');
    assert!(f.menu_has("S Sessions"));
    assert!(!f.menu_has("d Schema or database"));
}

#[test]
fn a_source_without_sessions_has_no_sessions_row() {
    let f = Fixture::sqlite(
        "menu-no-sessions.db",
        "CREATE TABLE users (id INT);",
        Settings::default(),
    );
    assert!(!f.menu_has("S Sessions"));
}

#[test]
fn the_stop_row_needs_a_run_and_replaces_the_run_rows() {
    let (mut f, recorder) = Fixture::recorded(Recorder::default());
    f.app
        .on_event(Event::Paste("SELECT 1 LIMIT 1 OFFSET 0;".to_owned()));
    assert!(!f.menu_has("x Stop"));
    assert!(f.menu_has("e Run the statement"));
    recorder.hold.store(true, Ordering::SeqCst);
    f.code(KeyCode::F(5));
    f.pump_until(|f| f.app.console_mut().is_busy());
    assert!(f.menu_has("x Stop"));
    assert!(!f.menu_has("e Run the statement"));
    f.prefix('h');
    assert!(f.menu_has("x Stop"), "the session stays occupied");
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
    f.pump_until(|f| !f.app.console_mut().is_busy());
}

#[test]
fn ctrl_w_s_opens_the_sessions_of_the_connection_of_the_console() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.press(ctrl('w'));
    f.press(shift(KeyCode::Char('S')));
    f.loaded();
    assert_eq!(f.session_tabs(), 1);
    assert_eq!(session_reads(&recorder), 1);
}

#[test]
fn ctrl_w_s_uses_the_connection_of_a_table_tab() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_table("users");
    f.press(ctrl('w'));
    f.press(shift(KeyCode::Char('S')));
    f.loaded();
    assert_eq!(f.session_tabs(), 1);
    assert_eq!(session_reads(&recorder), 1);
}

#[test]
fn ctrl_w_s_refuses_a_source_without_sessions() {
    let mut f = Fixture::sqlite(
        "menu-sessions-sqlite.db",
        "CREATE TABLE users (id INT);",
        Settings::default(),
    );
    f.press(ctrl('w'));
    f.press(shift(KeyCode::Char('S')));
    assert!(f.app.tables().is_empty());
    f.assert_sees(sqln::ui::text::SESSIONS_REFUSED);
}

#[test]
fn the_page_runs_a_command_from_any_focus_on_the_row_of_the_explorer() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    f.prefix('l');
    assert!(!f.app.hint().contains("Ctrl+W commands"), "the focus left");
    f.open_explorer_page();
    f.assert_sees("Ctrl+W · Explorer · server");
    f.press(shift(KeyCode::Char('S')));
    f.loaded();
    assert_eq!(f.session_tabs(), 1);
    assert_eq!(session_reads(&recorder), 1);
}

#[test]
fn the_page_keeps_the_keys_of_the_explorer() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    let consoles = f.app.consoles().len();
    f.open_explorer_page();
    f.code(KeyCode::Char('n'));
    f.assert_not_sees("Ctrl+W · Explorer");
    assert_eq!(f.app.consoles().len(), consoles + 1);
}

#[test]
fn the_delete_key_of_the_page_asks_first() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    f.open_explorer_page();
    f.code(KeyCode::Delete);
    f.assert_sees("Remove");
    assert_eq!(f.app.connections().len(), 1);
}

#[test]
fn esc_closes_the_page_and_runs_nothing() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    f.open_explorer_page();
    f.assert_sees("Ctrl+W · Explorer");
    f.code(KeyCode::Esc);
    f.assert_not_sees("Ctrl+W · Explorer");
    assert!(f.app.tables().is_empty());
    assert_eq!(recorder.count(), 0);
}

#[test]
fn a_key_without_a_command_on_the_row_closes_the_page() {
    let (mut f, recorder) = Fixture::recorded(two_sessions());
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    let before = recorder.count();
    f.open_explorer_page();
    // `g` acts only on a table or a view.
    f.code(KeyCode::Char('g'));
    f.assert_not_sees("Ctrl+W · Explorer");
    assert_eq!(recorder.count(), before);
}

#[test]
fn the_next_key_after_the_page_is_a_normal_key() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.open_explorer_page();
    f.code(KeyCode::Esc);
    f.press(ctrl('w'));
    f.assert_sees("Tab explorer");
}

#[test]
fn the_keys_of_ctrl_w_keep_their_meaning_with_the_focus_in_the_explorer() {
    let (mut f, _) = Fixture::recorded(two_sessions());
    f.open_table("users");
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    let consoles = f.app.consoles().len();
    let before = f.app.tables()[0].opened;
    // `Ctrl+W n` is the next tab, and not the new console of the key `n`.
    f.prefix('n');
    f.app.drain_events();
    assert_eq!(f.app.consoles().len(), consoles);
    assert_eq!(f.app.tables()[0].opened, before);
    assert!(f.app.table().is_none(), "the next tab is the console");
}

#[test]
fn the_vim_mode_uses_its_own_key_in_the_page() {
    let (mut f, _) = Fixture::recorded_in(two_sessions(), Settings::default(), KeyboardMode::Vim);
    common::select_row(&mut f.app, 0, &[RowKind::Connection], "server");
    assert!(common::explorer_page(&f.app).contains(&"x Remove connection".to_owned()));
    f.open_explorer_page();
    f.code(KeyCode::Char('x'));
    f.assert_sees("Remove");
}

// ---------------------------------------------------------------------------
// Opening, tabs, and pages.

#[test]
fn enter_on_a_table_opens_a_tab_and_loads_the_first_page() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.open_table("users");
    assert_eq!(f.app.tables().len(), 1);
    assert!(f.app.table().is_some());
    assert_eq!(f.rows_loaded(), 100);
    assert_eq!(
        recorder.statements(),
        ["SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0"]
    );
    f.assert_sees("users (table)");
    f.assert_sees("server · public.users · table");
    f.assert_sees("100 rows loaded · more rows available");
    f.assert_sees("name0");
    f.assert_not_sees("name100");
}

#[test]
fn the_tabs_of_consoles_and_tables_share_one_row() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    let screen = f.screen();
    let tabs = screen.lines().next().unwrap();
    assert!(
        tabs.contains("console-1") && tabs.contains("users (table)"),
        "{tabs}"
    );
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
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.click_text("console-1");
    assert!(f.app.table().is_none());
    f.click_text("users (table)");
    assert!(f.app.table().is_some());
}

#[test]
fn a_view_opens_with_a_label_that_names_it() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
    f.open_table("active_users");
    f.assert_sees("active_users (view)");
    f.assert_sees("server · public.active_users · view");
    assert!(recorder.statements()[0].contains("\"active_users\""));
}

#[test]
fn opening_an_open_table_selects_its_tab() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    f.assert_sees("▸ t");
    // The mark of a row opens its columns, so the clicks hit the name.
    let screen = f.screen();
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("▸ t"))
        .unwrap();
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.open_table("users");
    f.load_more();
    assert_eq!(f.rows_loaded(), 200);
    assert_eq!(first_cell(&f), "0", "the first rows stay");
    f.assert_sees("200 rows loaded · more rows available");
    f.load_more();
    assert_eq!(f.rows_loaded(), 250);
    f.assert_sees("250 rows loaded · no more rows");
    let statements = recorder.statements();
    assert!(
        statements[1].ends_with("LIMIT 101 OFFSET 100"),
        "{statements:?}"
    );
    assert!(
        statements[2].ends_with("LIMIT 101 OFFSET 200"),
        "{statements:?}"
    );
    // Without more rows, the key asks nothing.
    f.code(KeyCode::Char('m'));
    f.assert_sees("no more rows to load");
    assert_eq!(recorder.count(), 3);
}

#[test]
fn a_page_with_zero_fewer_exact_and_more_rows_than_the_page_size() {
    for (rows, more) in [(0, false), (99, false), (100, false), (101, true)] {
        let (mut f, recorder) = Fixture::recorded(Recorder {
            rows,
            ..Recorder::default()
        });
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
    let settings = Settings {
        page_size: 7,
        ..Settings::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(
        Recorder {
            rows: 30,
            ..Recorder::default()
        },
        settings,
    );
    f.open_table("users");
    assert_eq!(f.rows_loaded(), 7);
    assert!(recorder.statements()[0].ends_with("LIMIT 8 OFFSET 0"));
    f.load_more();
    assert_eq!(f.rows_loaded(), 14);
}

#[test]
fn the_load_more_button_and_its_key_do_the_same() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 150,
        ..Recorder::default()
    });
    f.open_table("users");
    f.click_text("[Load more]");
    f.loaded();
    assert_eq!(f.rows_loaded(), 150);
    assert_eq!(recorder.count(), 2);
}

#[test]
fn each_page_uses_one_session_and_gives_it_back() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.open_table("users");
    f.assert_sees("Pool 0/5");
    f.load_more();
    f.load_more();
    f.assert_sees("Pool 0/5");
    assert_eq!(
        recorder.opened.load(Ordering::SeqCst),
        1,
        "no extra session"
    );
}

#[test]
fn the_grid_of_a_table_copies_tsv_without_a_header() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 3,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.open_table("users");
    f.load_more();
    assert_eq!(f.rows_loaded(), 200);
    f.type_filter("x = 1");
    f.apply();
    assert_eq!(f.rows_loaded(), 100, "the old rows are gone");
    assert!(
        recorder
            .statements()
            .last()
            .unwrap()
            .ends_with("LIMIT 101 OFFSET 0")
    );
}

#[test]
fn a_button_that_has_no_effect_looks_disabled() {
    use ratatui::style::Modifier;
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 150,
        ..Recorder::default()
    });
    f.open_table("users");
    assert!(
        !f.modifier_of("[Load more]").contains(Modifier::DIM),
        "a next page exists"
    );
    assert!(!f.modifier_of("[Apply]").contains(Modifier::DIM));
    assert!(
        f.modifier_of("[Stop]").contains(Modifier::DIM),
        "nothing runs"
    );
    f.load_more();
    assert!(
        f.modifier_of("[Load more]").contains(Modifier::DIM),
        "no more rows"
    );
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
    f.code(KeyCode::Down);
    f.code(KeyCode::Right);
    let screen = f.screen();
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("▸ t"))
        .unwrap();
    let column = line.chars().position(|c| c == '▸').unwrap() as u16 + 3;
    f.click(column, row as u16);
    std::thread::sleep(Duration::from_millis(500));
    f.click(column, row as u16);
    assert!(f.app.tables().is_empty());
}

#[test]
fn the_apply_and_clear_buttons_work_with_the_mouse() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.type_filter("a = 1");
    f.click_text("[Apply]");
    f.loaded();
    assert!(recorder.statements()[1].contains("WHERE a = 1"));
    f.click_text("[Clear]");
    f.loaded();
    assert_eq!(
        recorder.statements()[2],
        "SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0"
    );
    let tab = f.app.table().unwrap();
    assert_eq!(tab.filter.text(), "");
    assert_eq!(tab.order.text(), "");
}

#[test]
fn a_click_on_a_field_gives_it_the_focus() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.focus_filter();
    f.code(KeyCode::Tab);
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Order);
    f.press(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Filter);
}

#[test]
fn a_paste_writes_one_line_in_the_focused_field() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.focus_filter();
    f.app.on_event(Event::Paste("a = 1\nAND b = 2".to_owned()));
    assert_eq!(f.app.table().unwrap().filter.text(), "a = 1 AND b = 2");
    assert_eq!(recorder.count(), 1);
}

#[test]
fn a_text_with_two_statements_leaves_an_empty_grid_and_the_reason() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.type_filter("cre");
    f.assert_sees("created_at");
    f.code(KeyCode::Tab);
    assert_eq!(f.app.table().unwrap().filter.text(), "created_at");
    assert_eq!(
        f.app.table().unwrap().focus,
        TableFocus::Filter,
        "Tab accepted the suggestion"
    );
    assert_eq!(recorder.count(), 1, "accepting asks nothing");
}

#[test]
fn enter_applies_the_typed_text_and_does_not_accept_the_highlighted_suggestion() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.type_filter("cre");
    assert!(f.app.table().unwrap().list.is_some());
    f.apply();
    assert!(
        recorder.statements()[1].contains("\nWHERE cre\n"),
        "{:?}",
        recorder.statements()
    );
    assert_eq!(f.app.table().unwrap().filter.text(), "cre");
}

#[test]
fn the_order_field_has_its_own_list_and_escape_closes_the_list_first() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.type_order("em");
    assert!(f.app.table().unwrap().list.is_some());
    f.assert_sees("email");
    f.code(KeyCode::Esc);
    assert!(f.app.table().unwrap().list.is_none());
    assert_eq!(
        f.app.table().unwrap().focus,
        TableFocus::Order,
        "the first Esc closed the list"
    );
    f.code(KeyCode::Esc);
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Grid);
}

#[test]
fn control_space_opens_the_list_without_a_prefix() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    assert_eq!(
        tab.filter.text(),
        "a = 1 AND draft = 2",
        "the draft stays a draft"
    );
    assert_eq!(tab.applied_filter(), "a = 1");
}

#[test]
fn clicking_the_same_header_again_reverses_and_then_removes_the_order() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    for expected in ["ORDER BY name ASC", "ORDER BY name DESC"] {
        let (column, row) = f.locate_below("[Apply]", "name");
        f.click(column, row);
        f.loaded();
        assert!(
            recorder.statements().last().unwrap().contains(expected),
            "{:?}",
            recorder.statements()
        );
    }
    let (column, row) = f.locate_below("[Apply]", "name");
    f.click(column, row);
    f.loaded();
    assert!(!recorder.statements().last().unwrap().contains("ORDER BY"));
    assert_eq!(f.app.table().unwrap().order.text(), "");
}

#[test]
fn the_s_key_sorts_by_the_current_column() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.grid();
    f.code(KeyCode::Right);
    f.code(KeyCode::Char('s'));
    f.loaded();
    assert!(
        recorder
            .statements()
            .last()
            .unwrap()
            .contains("ORDER BY name ASC")
    );
}

#[test]
fn the_warning_about_the_order_shows_until_an_order_is_applied() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
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
    assert!(
        recorder
            .statements()
            .last()
            .unwrap()
            .ends_with("OFFSET 100")
    );
}

#[test]
fn a_stop_in_the_middle_of_the_first_page_keeps_the_rows_and_the_next_page_continues() {
    // The sink sends a batch at 200 rows, so the page must be larger than that.
    let settings = Settings {
        page_size: 300,
        ..Settings::default()
    };
    let recorder = Recorder {
        rows: 1_000,
        mid_rows: 250,
        ..Recorder::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(recorder, settings);
    recorder.mid_hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.pump_until(|f| f.app.table().is_some_and(|tab| tab.loaded() == 200));
    f.prefix('x');
    f.loaded();
    assert_eq!(f.rows_loaded(), 200, "the sent batch stays");
    assert!(
        f.app.table().unwrap().has_more,
        "the page ended early, so more rows can exist"
    );
    recorder.mid_hold.store(false, Ordering::SeqCst);
    f.grid();
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert!(
        recorder
            .statements()
            .last()
            .unwrap()
            .ends_with("LIMIT 301 OFFSET 200")
    );
    assert_eq!(f.rows_loaded(), 500);
}

#[test]
fn a_page_can_be_larger_than_the_row_cap_of_the_consoles() {
    let settings = Settings {
        page_size: 1_500,
        row_cap: 1_000,
        ..Settings::default()
    };
    let (mut f, _) = Fixture::recorded_with(
        Recorder {
            rows: 2_000,
            ..Recorder::default()
        },
        settings,
    );
    f.open_table("users");
    assert_eq!(f.rows_loaded(), 1_500);
    assert!(f.app.table().unwrap().has_more);
}

#[test]
fn the_stop_button_works_too() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 50,
        ..Recorder::default()
    });
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
    assert_eq!(
        recorder.statements().last().unwrap(),
        "SELECT * FROM \"public\".\"users\"\nLIMIT 101 OFFSET 0"
    );
}

#[test]
fn the_fields_stay_while_the_tab_stays_open() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    assert!(
        !f.app.console().manual,
        "the mode of the console did not change"
    );
}

#[test]
fn the_help_of_a_table_lists_its_own_keys() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.grid();
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Conventional · Table");
    f.code(KeyCode::Down);
    f.assert_sees("Sort by the column");
    f.assert_not_sees("Close the result");
    for _ in 0..4 {
        f.code(KeyCode::Down);
    }
    f.assert_sees("Load more rows");
    f.code(KeyCode::Esc);
    f.focus_filter();
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Conventional · Filter");
    f.assert_sees("Apply the fields");
}

#[test]
fn the_help_of_a_console_hides_the_keys_of_a_table() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.prefix('j');
    f.code(KeyCode::F(1));
    f.assert_sees("Keys · Conventional · Results");
    f.assert_sees("Close the result");
    f.assert_not_sees("Load more rows");
}

#[test]
fn the_hint_follows_the_part_of_the_table_in_focus() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.grid();
    f.assert_sees("f where  o order by");
    f.focus_filter();
    f.assert_sees("Enter apply");
}

#[test]
fn the_table_view_fits_the_minimum_terminal() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 300,
        ..Recorder::default()
    });
    f.open_table("users");
    f.type_filter("a very long filter that needs more room than the field has in a small terminal");
    let screen = f.screen_at(80, 24);
    for text in [
        "WHERE",
        "ORDER BY",
        "[Apply]",
        "[Clear]",
        "[Load more]",
        "[Stop]",
        "rows loaded",
        "No ORDER BY",
    ] {
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
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
    let settings = Settings {
        sql_timeout: Some(Duration::from_millis(50)),
        ..Settings::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(
        Recorder {
            rows: 5,
            ..Recorder::default()
        },
        settings,
    );
    recorder.hold.store(true, Ordering::SeqCst);
    f.open_table_of(0, "users");
    f.loaded();
    assert_eq!(f.rows_loaded(), 0);
    f.assert_sees("The time limit ended");
}

#[test]
fn a_refused_connection_shows_its_error_in_the_table() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 50,
        ..Recorder::default()
    });
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
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
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
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 100,
        ..Recorder::default()
    });
    f.open_table("users");
    // The wheel moves the cursor by rows.
    let (column, row) = f.locate_below("[Apply]", "name0");
    f.app
        .on_event(mouse(MouseEventKind::ScrollDown, column, row));
    assert_eq!(f.app.table().unwrap().result.view.row, 3);
    // A drag selects a range of cells.
    f.click(column, row);
    f.app.on_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        column + 2,
        row + 2,
    ));
    let view = f.app.table().unwrap().result.view;
    assert_eq!(
        view.selection().map(|range| (range.top, range.bottom)),
        Some((0, 2))
    );
    // A drag on the edge of a header changes the width of the column.
    let (edge, header) = f.locate_below("[Apply]", "id");
    let before = f.app.table().unwrap().result.table.width(0);
    let edge = edge + before as u16;
    f.click(edge, header);
    f.app.on_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        edge + 4,
        header,
    ));
    f.app.on_event(mouse(
        MouseEventKind::Up(MouseButton::Left),
        edge + 4,
        header,
    ));
    assert_eq!(f.app.table().unwrap().result.table.width(0), before + 4);
    assert_eq!(f.app.tables().len(), 1);
    assert_eq!(
        f.app.table().unwrap().applied_order(),
        "",
        "a drag on an edge does not sort"
    );
}

#[test]
fn the_search_of_a_table_grid_finds_loaded_rows() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 30,
        ..Recorder::default()
    });
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
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    f.grid();
    f.press(ctrl('f'));
    f.type_text("a");
    f.click_text("WHERE");
    f.app.on_event(Event::Paste("b = 1".to_owned()));
    assert_eq!(f.app.table().unwrap().filter.text(), "b = 1");
    assert_eq!(
        f.app
            .table()
            .unwrap()
            .result
            .search
            .as_ref()
            .unwrap()
            .query(),
        "a"
    );
}

#[test]
fn vim_keys_move_in_the_grid_of_a_table_and_stay_text_in_a_field() {
    let (mut f, _) = Fixture::recorded_in(
        Recorder {
            rows: 5,
            ..Recorder::default()
        },
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
    use sqln::exec::{ExecEvent, ExecutionEnd, StatementKey};
    use sqln::grid::Budget;
    use sqln::table::TableTab;

    let budget = Budget::new(1 << 20);
    let mut tab = TableTab::new(7, 1, relation(), false, 2);
    tab.started(5, true, None);
    let key = StatementKey {
        console: 7,
        execution: 5,
        index: 0,
    };
    let row = |n: &str| -> Row { vec![Value::Text(n)].into_iter().collect() };
    let rows = vec![row("a"), row("b"), row("c")];
    let cost: usize = rows
        .iter()
        .map(|row| budget.reserve(row))
        .map(|r| match r {
            sqln::grid::Reservation::Accepted(bytes) => bytes,
            other => panic!("{other:?}"),
        })
        .sum();
    tab.on_event(
        ExecEvent::Columns {
            key,
            columns: vec!["v".to_owned()],
        },
        &budget,
    );
    tab.on_event(
        ExecEvent::Rows {
            key,
            rows,
            bytes: cost,
        },
        &budget,
    );
    tab.on_event(
        ExecEvent::ResultDone {
            key,
            rows: 3,
            limit: ReadLimit::None,
        },
        &budget,
    );
    assert_eq!(tab.loaded(), 2, "the page keeps two rows");
    assert!(tab.has_more, "the third row proves a next page");
    assert_eq!(
        budget.used(),
        tab.result.table.bytes,
        "the extra row left the budget"
    );
    let one = sqln::grid::row_cost(&row("a"));
    assert_eq!(budget.used(), 2 * one);
    tab.on_event(
        ExecEvent::ExecutionFinished {
            console: 7,
            execution: 5,
            end: ExecutionEnd::Completed,
            message: None,
        },
        &budget,
    );
    // Rows of an old execution never reach the grid.
    let stale = StatementKey {
        execution: 4,
        ..key
    };
    let reserved = match budget.reserve(&row("z")) {
        sqln::grid::Reservation::Accepted(bytes) => bytes,
        other => panic!("{other:?}"),
    };
    tab.on_event(
        ExecEvent::Rows {
            key: stale,
            rows: vec![row("z")],
            bytes: reserved,
        },
        &budget,
    );
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
    let settings = Settings {
        // A page of 8 rows takes about 210 bytes.
        budget_bytes: 350,
        page_size: 8,
        ..Settings::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(
        Recorder {
            rows: 100,
            ..Recorder::default()
        },
        settings,
    );
    f.open_table("orders");
    let first = f.rows_loaded();
    assert_eq!(first, 8, "the first table fits");
    f.open_table("users");
    let loaded = f.rows_loaded();
    assert!(
        loaded > 0 && loaded < 8,
        "the budget stopped the page at {loaded} rows"
    );
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
    assert_eq!(
        f.app.table().unwrap().relation.relation,
        "users",
        "the tab on the right is on screen"
    );
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert!(f.rows_loaded() > loaded);
    assert!(
        recorder
            .statements()
            .last()
            .unwrap()
            .contains(&format!("OFFSET {loaded}"))
    );
}

#[test]
fn freed_space_loads_nothing_until_the_user_asks() {
    let settings = Settings {
        // A page of 8 rows takes about 210 bytes.
        budget_bytes: 350,
        page_size: 8,
        ..Settings::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(
        Recorder {
            rows: 100,
            ..Recorder::default()
        },
        settings,
    );
    f.open_table("orders");
    f.open_table("users");
    f.assert_sees("data budget reached");
    let queries = recorder.count();
    f.prefix('p');
    f.prefix('c');
    f.loaded();
    f.app.drain_events();
    assert_eq!(recorder.count(), queries);
}

#[test]
fn a_value_larger_than_the_budget_says_so_and_never_loads() {
    let settings = Settings {
        budget_bytes: 100,
        ..Settings::default()
    };
    let recorder = Recorder {
        rows: 5,
        value_size: 500,
        ..Recorder::default()
    };
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
    f.app
        .table()
        .unwrap()
        .result
        .table
        .rows
        .iter()
        .map(|row| row.get(0).unwrap().display())
        .collect()
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
    let setup = format!(
        "{} CREATE VIEW big AS SELECT id FROM t WHERE id > 5;",
        numbered(8)
    );
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
    let tabs = screen
        .lines()
        .find(|line| line.contains("console-"))
        .unwrap();
    assert!(
        tabs.find("(table)").unwrap() < tabs.find("console-").unwrap(),
        "{tabs}"
    );
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
    let tabs = screen
        .lines()
        .find(|line| line.contains("console-1"))
        .unwrap();
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
    let settings = Settings {
        max_connections: 1,
        ..Settings::default()
    };
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
    assert_eq!(
        f.rows_loaded(),
        100,
        "the first page stays while the next one waits"
    );
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

impl Fixture {
    /// Clicks the close mark of a tab.
    fn close_tab(&mut self, name: &str) {
        let screen = self.screen();
        let tabs = screen.lines().next().unwrap();
        let start = tabs[..tabs.find(name).unwrap()].chars().count();
        let column = start + name.chars().count() + 1;
        assert_eq!(tabs.chars().nth(column), Some('X'), "{tabs}");
        self.click(column as u16, 0);
    }
}

#[test]
fn the_close_mark_closes_the_table_on_screen() {
    let mut f = Fixture::sqlite("close-table", &numbered(3), Settings::default());
    f.open_table("t");
    f.close_tab("t (table)");
    assert!(f.app.tables().is_empty());
    assert!(f.app.table().is_none());
    assert_eq!(f.app.console().name, "console-1");
    assert!(!f.app.should_quit());
}

#[test]
fn the_close_mark_closes_a_table_that_is_not_on_screen() {
    let mut f = Fixture::sqlite("close-hidden-table", &numbered(3), Settings::default());
    f.open_table("t");
    f.click_text("console-1");
    assert!(f.app.table().is_none());
    f.close_tab("t (table)");
    assert!(f.app.tables().is_empty());
    assert_eq!(f.app.console().name, "console-1");
}

#[test]
fn the_close_mark_of_a_console_keeps_the_table_on_screen() {
    let mut f = Fixture::sqlite("close-console-of-table", &numbered(3), Settings::default());
    f.open_table("t");
    f.close_tab("console-1");
    assert_eq!(f.app.tables().len(), 1);
    assert!(f.app.table().is_some());
    assert!(!f.app.should_quit());
    let screen = f.screen();
    assert!(
        !screen.lines().next().unwrap().contains("console-"),
        "{screen}"
    );
}

#[test]
fn the_close_mark_of_the_last_table_ends_the_application() {
    let mut f = Fixture::sqlite("close-last-table", &numbered(3), Settings::default());
    f.open_table("t");
    f.close_tab("console-1");
    f.close_tab("t (table)");
    assert!(f.app.should_quit());
}

// ---- the state of a run in the tab row ----

/// The connector of these tests reads the page of a statement.
const PAGE: &str = "SELECT 1 LIMIT 5 OFFSET 0;";

impl Fixture {
    fn top(&mut self) -> String {
        self.screen().lines().next().unwrap().to_owned()
    }

    /// Replaces the text of the console and starts a run.
    fn run_text(&mut self, sql: &str) {
        self.press(ctrl('a'));
        self.code(KeyCode::Delete);
        self.app.on_event(Event::Paste(sql.to_owned()));
        self.code(KeyCode::F(5));
    }

    /// Runs a statement and waits until the connector holds it.
    fn run_held(&mut self, recorder: &Recorder, sql: &str) {
        recorder.hold.store(true, Ordering::SeqCst);
        self.run_text(sql);
        self.pump_until(|f| f.app.console().phase == Phase::Running);
    }

    fn console_idle(&mut self) {
        self.pump_until(|f| f.app.consoles().iter().all(|console| !console.is_busy()));
    }
}

fn release(recorder: &Recorder) {
    recorder.hold.store(false, Ordering::SeqCst);
    recorder.release.notify_one();
}

/// Returns true when a text has a time such as `1m 05.123s`.
fn has_three_decimals(text: &str) -> bool {
    text.match_indices('.').any(|(at, _)| {
        let rest = &text[at + 1..];
        rest.len() >= 4
            && rest[..3].chars().all(|c| c.is_ascii_digit())
            && rest[3..].starts_with('s')
    })
}

#[test]
fn a_running_tab_shows_its_time_in_the_tab_row_and_in_the_status_line() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.run_held(&recorder, PAGE);
    f.app.tick_at(Instant::now() + Duration::from_secs(65));
    assert!(has_three_decimals(&f.top()), "{}", f.top());
    let screen = f.screen();
    let status = screen
        .lines()
        .find(|line| line.contains("Running 1m 0"))
        .unwrap();
    assert!(has_three_decimals(status), "{status}");
    release(&recorder);
    f.console_idle();
    assert!(!f.top().contains("1m 0"), "{}", f.top());
}

#[test]
fn the_mark_of_a_run_moves_and_an_idle_screen_does_not_draw_again() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    let start = Instant::now();
    assert!(!f.app.tick_at(start + Duration::from_secs(1)));
    assert!(!f.app.tick_at(start + Duration::from_secs(2)));

    f.run_held(&recorder, PAGE);
    let now = Instant::now();
    f.app.tick_at(now);
    let first = f.top();
    assert!(
        f.app.tick_at(now + Duration::from_millis(100)),
        "the time needs a new frame"
    );
    assert_ne!(first, f.top());
    assert!(
        !f.app.tick_at(now + Duration::from_millis(100)),
        "the same frame draws once"
    );
    let mark = |row: &str| row.trim_start().chars().next();
    let mut picture = mark(&f.top());
    let mut changes = 0;
    for step in 2..=9 {
        f.app.tick_at(now + Duration::from_millis(100) * step);
        let next = mark(&f.top());
        changes += u32::from(next != picture);
        picture = next;
    }
    assert_eq!(changes, 4, "the picture of the mark changes every 200 ms");
    release(&recorder);
    f.console_idle();
}

#[test]
fn a_tab_that_ends_out_of_sight_keeps_its_mark_until_the_user_leaves_it() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.run_held(&recorder, PAGE);
    recorder.hold.store(false, Ordering::SeqCst);
    f.open_table("users");
    assert!(f.app.table().is_some());
    release(&recorder);
    f.console_idle();
    assert!(f.top().contains(" ok  console-1"), "{}", f.top());

    f.click_text("console-1");
    assert!(f.app.table().is_none());
    assert!(
        f.top().contains(" ok  console-1"),
        "the mark stays while the user looks"
    );
    f.prefix('n');
    assert!(f.app.table().is_some());
    assert!(!f.top().contains(" ok  console-1"), "{}", f.top());
}

#[test]
fn the_mark_of_a_console_stays_during_a_visit_and_goes_after_it() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.run_held(&recorder, PAGE);
    f.prefix('h');
    f.code(KeyCode::Char('n'));
    assert_eq!(f.app.console().name, "console-2");
    release(&recorder);
    f.console_idle();
    assert!(f.top().contains(" ok  console-1"), "{}", f.top());

    f.click_text("console-1");
    assert_eq!(f.app.console().name, "console-1");
    assert!(
        f.top().contains(" ok  console-1"),
        "the mark stays while the user looks"
    );
    f.prefix('n');
    assert_eq!(f.app.console().name, "console-2");
    assert!(!f.top().contains(" ok  console-1"), "{}", f.top());
}

#[test]
fn a_failed_run_shows_an_error_mark_that_the_next_run_replaces() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    *recorder.fail_when.lock().unwrap() = Some("boom".to_owned());
    f.run_text("SELECT boom;");
    f.console_idle();
    assert!(f.top().contains(" ERR  console-1"), "{}", f.top());
    f.assert_sees("Idle (last ");

    *recorder.fail_when.lock().unwrap() = None;
    f.run_text(PAGE);
    f.console_idle();
    assert!(f.top().contains(" ok  console-1"), "{}", f.top());
    assert!(!f.top().contains("ERR"), "{}", f.top());
}

#[test]
fn a_stopped_run_shows_a_stop_mark() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.run_held(&recorder, PAGE);
    f.prefix('x');
    f.console_idle();
    assert!(f.top().contains(" stop  console-1"), "{}", f.top());
}

#[test]
fn a_table_tab_shows_the_same_time_and_marks() {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.open_table("users");
    assert!(f.top().contains(" ok  users (table)"), "{}", f.top());

    recorder.hold.store(true, Ordering::SeqCst);
    f.type_filter("id > 1");
    f.code(KeyCode::Enter);
    f.pump_until(|f| f.app.table().is_some_and(|tab| tab.phase == Phase::Running));
    f.app.tick_at(Instant::now() + Duration::from_secs(65));
    assert!(f.top().contains("1m 0"), "{}", f.top());
    f.assert_sees("Running 1m 0");
    release(&recorder);
    f.loaded();

    *recorder.fail_when.lock().unwrap() = Some("bad".to_owned());
    f.type_filter(" and bad");
    f.code(KeyCode::Enter);
    f.loaded();
    assert!(f.top().contains(" ERR  users (table)"), "{}", f.top());
}

#[test]
fn the_close_mark_still_closes_a_tab_with_a_run_mark() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    f.run_text(PAGE);
    f.console_idle();
    f.open_table("users");
    assert!(f.top().contains(" ok  users (table)"), "{}", f.top());
    f.close_tab("users (table)");
    assert!(f.app.tables().is_empty());
}

// ---- the keys of the suggestion list in the filter ----

fn filter_with_list(keys: SuggestionKeys) -> (Fixture, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        rows: 5,
        ..Recorder::default()
    });
    let options = Options {
        connector: Some(Arc::new(RecordingConnector(recorder.clone()))),
        label: Some("server".to_owned()),
        suggestion_keys: keys,
        ..Options::default()
    };
    let mut f = Fixture::start(options);
    f.open_table("users");
    f.type_filter("na");
    f.pump_until(|f| f.app.table().unwrap().list.is_some());
    (f, recorder)
}

#[test]
fn by_default_enter_applies_the_filter_without_the_suggestion() {
    let (mut f, recorder) = filter_with_list(SuggestionKeys::Tab);
    f.assert_sees("Enter apply");
    assert!(!f.screen().contains("Enter new line"), "{}", f.screen());
    let before = recorder.count();
    f.code(KeyCode::Enter);
    f.pump_until(|_| recorder.count() > before);
    assert!(recorder.statements().last().unwrap().contains("WHERE na"));
}

#[test]
fn the_enter_choice_accepts_in_the_filter_and_the_next_enter_applies_it() {
    let (mut f, recorder) = filter_with_list(SuggestionKeys::Enter);
    f.assert_sees("Enter accept");
    let before = recorder.count();
    f.code(KeyCode::Enter);
    assert_eq!(f.app.table().unwrap().filter.text(), "name");
    assert_eq!(recorder.count(), before, "the acceptance runs no query");
    f.code(KeyCode::Enter);
    f.pump_until(|_| recorder.count() > before);
    assert!(recorder.statements().last().unwrap().contains("WHERE name"));
}

#[test]
fn the_enter_choice_keeps_tab_for_the_next_field_of_the_filter() {
    let (mut f, _) = filter_with_list(SuggestionKeys::Enter);
    f.assert_sees("Tab next field");
    f.code(KeyCode::Tab);
    let tab = f.app.table().unwrap();
    assert_eq!(tab.focus, TableFocus::Order);
    assert_eq!(tab.filter.text(), "na");
}

// ---- the pages of a table ----

fn paged(rows: usize) -> (Fixture, Arc<Recorder>) {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows,
        ..Recorder::default()
    });
    f.open_table("users");
    (f, recorder)
}

impl Fixture {
    fn view(&self) -> (usize, usize) {
        let view = self.app.table().unwrap().result.view;
        (view.row, view.top)
    }

    fn next_page(&mut self) {
        self.click_text("[Next]");
        self.loaded();
    }

    fn previous_page(&mut self) {
        self.click_text("[Previous]");
    }
}

#[test]
fn a_table_with_one_page_has_no_page_buttons() {
    for rows in [0, 5, 100] {
        let (mut f, _) = paged(rows);
        let screen = f.screen();
        assert!(
            !screen.contains("[Next]") && !screen.contains("[Previous]"),
            "{screen}"
        );
        assert!(!screen.contains("Page 1"), "{screen}");
    }
}

#[test]
fn a_full_page_with_more_rows_shows_the_buttons_and_the_next_button_loads_the_page() {
    let (mut f, recorder) = paged(250);
    f.assert_sees("Page 1 of 1+");
    assert_eq!(recorder.count(), 1);
    f.next_page();
    assert_eq!(recorder.count(), 2);
    assert_eq!(f.app.table().unwrap().loaded(), 200);
    assert_eq!(
        f.view(),
        (100, 100),
        "the first row of the page stands at the top"
    );
    f.assert_sees("Page 2 of 2+");
}

#[test]
fn the_buttons_move_over_loaded_pages_without_a_query() {
    let (mut f, recorder) = paged(250);
    f.next_page();
    f.previous_page();
    assert_eq!(f.view(), (0, 0));
    f.assert_sees("Page 1 of 2+");
    f.next_page();
    assert_eq!(f.view(), (100, 100));
    assert_eq!(recorder.count(), 2, "the rows of the page are on the grid");
}

#[test]
fn the_last_page_says_that_it_is_the_last_and_the_next_button_asks_nothing() {
    let (mut f, recorder) = paged(150);
    f.next_page();
    f.assert_sees("Page 2 of 2");
    assert!(!f.screen().contains("Page 2 of 2+"), "{}", f.screen());
    f.next_page();
    assert_eq!(recorder.count(), 2);
    assert_eq!(f.view(), (100, 100));
}

#[test]
fn the_previous_button_of_the_first_page_does_nothing() {
    let (mut f, _) = paged(250);
    f.previous_page();
    assert_eq!(f.view(), (0, 0));
}

#[test]
fn the_keys_of_the_brackets_move_between_pages_in_a_table() {
    let (mut f, recorder) = paged(250);
    f.grid();
    f.code(KeyCode::Char(']'));
    f.loaded();
    assert_eq!(f.view(), (100, 100));
    assert_eq!(recorder.count(), 2);
    f.code(KeyCode::Char('['));
    assert_eq!(f.view(), (0, 0));
}

#[test]
fn the_number_of_the_page_follows_the_cursor() {
    let (mut f, _) = paged(250);
    f.next_page();
    f.previous_page();
    f.grid();
    for _ in 0..100 {
        f.code(KeyCode::Down);
    }
    assert_eq!(f.view().0, 100);
    f.assert_sees("Page 2 of 2+");
}

#[test]
fn a_click_on_a_page_button_gives_the_focus_to_the_grid() {
    let (mut f, _) = paged(250);
    f.focus_filter();
    f.next_page();
    assert_eq!(f.app.table().unwrap().focus, TableFocus::Grid);
}

#[test]
fn the_load_more_button_keeps_the_cursor_and_the_page() {
    let (mut f, _) = paged(250);
    f.click_text("[Load more]");
    f.loaded();
    assert_eq!(f.view(), (0, 0));
    f.assert_sees("Page 1 of 2+");
}

#[test]
fn a_page_that_fails_keeps_the_rows_and_the_cursor() {
    let (mut f, recorder) = paged(250);
    *recorder.fail_when.lock().unwrap() = Some("OFFSET 100".to_owned());
    f.next_page();
    assert_eq!(f.app.table().unwrap().loaded(), 100);
    assert_eq!(f.view(), (0, 0));
}

#[test]
fn a_page_that_stops_with_a_part_of_its_rows_keeps_the_cursor() {
    // The rows arrive in batches of 200, so the page must hold more than one batch.
    let settings = Settings {
        page_size: 300,
        ..Settings::default()
    };
    let recorder = Recorder {
        rows: 1_000,
        mid_rows: 250,
        ..Recorder::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(recorder, settings);
    f.open_table("users");
    recorder.mid_hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().loaded() > 300);
    f.prefix('x');
    f.loaded();
    assert_eq!(f.view(), (0, 0));
}

#[test]
fn load_more_after_a_stopped_move_stays_on_the_page() {
    let (mut f, recorder) = paged(350);
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    f.prefix('x');
    recorder.hold.store(false, Ordering::SeqCst);
    f.loaded();
    f.click_text("[Load more]");
    f.loaded();
    assert_eq!(f.view(), (0, 0), "the stopped move left nothing behind");
}

#[test]
fn a_new_query_cancels_the_move_to_a_page() {
    let (mut f, recorder) = paged(250);
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    f.prefix('x');
    recorder.hold.store(false, Ordering::SeqCst);
    f.loaded();
    f.focus_filter();
    f.apply();
    f.loaded();
    assert_eq!(f.view(), (0, 0));
    f.assert_sees("Page 1 of 1+");
}

#[test]
fn the_next_button_refuses_a_second_load() {
    let (mut f, recorder) = paged(350);
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    f.click_text("[Next]");
    f.assert_sees("a load in progress");
    release(&recorder);
    f.loaded();
    assert_eq!(recorder.count(), 2);
    assert_eq!(f.view(), (100, 100));
}

#[test]
fn the_page_buttons_fit_the_minimum_terminal() {
    let (mut f, _) = paged(250);
    let screen = f.screen_at(80, 24);
    for text in ["[<]", "1/1+", "[>]", "100 rows loaded"] {
        assert!(screen.contains(text), "{text}\n{screen}");
    }
    let screen = f.screen_at(100, 30);
    for text in ["[Previous]", "Page 1 of 1+", "[Next]", "100 rows loaded"] {
        assert!(screen.contains(text), "{text}\n{screen}");
    }
}

#[test]
fn the_state_of_a_load_comes_before_the_page_buttons() {
    let (mut f, recorder) = paged(250);
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    for (width, height) in [(80, 24), (100, 30), (140, 40)] {
        let screen = f.screen_at(width, height);
        assert!(screen.contains("100 rows loaded"), "{screen}");
        assert!(screen.contains("loading"), "{screen}");
    }
    release(&recorder);
    f.loaded();
}

#[test]
fn a_narrow_row_keeps_short_page_buttons_with_a_long_state() {
    let settings = Settings {
        max_connections: 1,
        ..Settings::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(
        Recorder {
            rows: 250,
            ..Recorder::default()
        },
        settings,
    );
    f.open_table("users");
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    let screen = f.screen_at(100, 30);
    assert!(
        screen.contains("1/1+") || screen.contains("Page 1 of 1+"),
        "{screen}"
    );
    release(&recorder);
    f.loaded();
}

#[test]
fn the_short_buttons_also_move_between_pages() {
    let (mut f, recorder) = paged(350);
    f.next_page();
    assert_eq!(f.view(), (100, 100));
    // A load in progress on the row of the minimum terminal leaves room for the short form.
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Load more]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let (mut column, mut row) = (None, 0);
    for y in 0..24 {
        let text: String = (0..80).map(|x| buffer[(x, y)].symbol()).collect();
        if let Some(byte) = text.find("[<]") {
            assert!(text.contains("2/2+") && text.contains("loading"), "{text}");
            column = Some(text[..byte].chars().count() as u16);
            row = y;
        }
    }
    let column = column.expect("the short form is on the screen");
    f.app
        .on_event(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
    assert_eq!(f.view(), (0, 0));
    release(&recorder);
    f.loaded();
}

#[test]
fn the_row_search_takes_the_place_of_the_page_buttons() {
    let (mut f, _) = paged(250);
    f.grid();
    f.press(ctrl('f'));
    let screen = f.screen();
    assert!(screen.contains("Find:"), "{screen}");
    assert!(!screen.contains("[Next]"), "{screen}");
}

#[test]
fn the_results_of_a_console_have_no_page_buttons() {
    let (mut f, _) = Fixture::recorded(Recorder {
        rows: 250,
        ..Recorder::default()
    });
    f.run_text(PAGE);
    f.console_idle();
    let screen = f.screen();
    assert!(
        !screen.contains("[Next]") && !screen.contains("Page 1"),
        "{screen}"
    );
}

#[test]
fn the_page_buttons_show_which_directions_are_available() {
    use ratatui::style::Modifier;

    let (mut f, _) = paged(150);
    assert!(f.modifier_of("[Previous]").contains(Modifier::DIM));
    assert!(!f.modifier_of("[Next]").contains(Modifier::DIM));
    f.next_page();
    assert!(!f.modifier_of("[Previous]").contains(Modifier::DIM));
    assert!(f.modifier_of("[Next]").contains(Modifier::DIM));
    f.previous_page();
    assert!(!f.modifier_of("[Next]").contains(Modifier::DIM));
}

#[test]
fn a_click_on_the_page_label_does_not_move_or_load_rows() {
    let (mut f, recorder) = paged(250);
    f.click_text("Page 1 of 1+");
    assert_eq!(f.view(), (0, 0));
    assert_eq!(recorder.count(), 1);
}

#[test]
fn a_failed_next_does_not_move_a_later_load_more() {
    let (mut f, recorder) = paged(250);
    *recorder.fail_when.lock().unwrap() = Some("OFFSET 100".to_owned());
    f.next_page();
    assert_eq!(f.view(), (0, 0));
    *recorder.fail_when.lock().unwrap() = None;
    f.load_more();
    assert_eq!(f.view(), (0, 0));
    assert_eq!(f.rows_loaded(), 200);
    f.next_page();
    assert_eq!(f.view(), (100, 100));
    assert_eq!(recorder.count(), 3);
}

#[test]
fn a_next_load_moves_only_its_table_after_the_user_changes_tabs() {
    let (mut f, recorder) = paged(250);
    recorder.hold.store(true, Ordering::SeqCst);
    f.click_text("[Next]");
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    f.click_text("console-1");
    assert!(f.app.table().is_none());
    release(&recorder);
    f.pump_until(|f| !f.app.tables()[0].is_busy());
    assert!(f.app.table().is_none());
    assert_eq!(f.app.tables()[0].result.view.row, 100);
    f.click_text("users (table)");
    assert_eq!(f.view(), (100, 100));
    assert_eq!(recorder.count(), 2);
}

#[test]
fn loaded_pages_still_work_when_the_budget_refuses_a_new_page() {
    let settings = Settings {
        page_size: 4,
        // A row of this table takes about 70 bytes.
        budget_bytes: 470,
        ..Settings::default()
    };
    let recorder = Recorder {
        rows: 40,
        value_size: 50,
        ..Recorder::default()
    };
    let (mut f, recorder) = Fixture::recorded_with(recorder, settings);
    f.open_table("users");
    f.load_more();
    assert!(f.rows_loaded() > 4);
    f.assert_sees("data budget reached");
    let queries = recorder.count();
    f.grid();
    f.code(KeyCode::Char(']'));
    assert_eq!(f.view(), (4, 4));
    f.code(KeyCode::Char('['));
    assert_eq!(f.view(), (0, 0));
    assert_eq!(recorder.count(), queries);
}

#[test]
fn a_search_removes_the_mouse_targets_of_the_page_buttons() {
    let (mut f, recorder) = paged(250);
    let screen = f.screen();
    let (y, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("[Next]"))
        .unwrap();
    let x = line[..line.find("[Next]").unwrap()].chars().count() as u16;
    f.grid();
    f.press(ctrl('f'));
    f.click(x, y as u16);
    assert_eq!(recorder.count(), 1);
    assert_eq!(f.view(), (0, 0));
    assert!(f.app.table().unwrap().result.search.is_some());
}

#[test]
fn the_list_of_keys_names_the_pages_in_a_table_and_the_results_in_a_console() {
    let (mut f, _) = paged(250);
    f.grid();
    f.code(KeyCode::F(1));
    f.assert_sees("Next page of rows");
    f.code(KeyCode::Esc);
    f.click_text("console-1");
    f.prefix('j');
    f.code(KeyCode::F(1));
    f.assert_sees("Next result");
}

fn counted_table(rows: usize) -> (Fixture, Arc<Recorder>) {
    let (mut f, recorder) = Fixture::recorded(Recorder {
        rows,
        ..Recorder::default()
    });
    f.open_table("users");
    f.grid();
    (f, recorder)
}

#[test]
fn the_count_key_shows_the_total_and_keeps_the_pages() {
    let (mut f, recorder) = counted_table(1250);
    assert!(f.hints_at(200).contains("c count"));
    f.code(KeyCode::Char('c'));
    f.loaded();
    assert_eq!(
        recorder.statements().last().unwrap(),
        "SELECT COUNT(*) FROM \"public\".\"users\""
    );
    f.assert_sees("100 of 1,250 rows loaded");
    assert!(!f.hints_at(200).contains("c count"));
    let tab = f.app.table().unwrap();
    assert_eq!(tab.total, Some(1250));
    assert_eq!(tab.loaded(), 100);
    assert!(tab.has_more);
    assert_eq!(tab.limit, ReadLimit::None);
    assert!(!tab.result.cut_short);
    assert_eq!(tab.result.table.columns, COLUMNS);
    // The next page continues after the rows of the first page.
    f.code(KeyCode::Char('m'));
    f.loaded();
    assert_eq!(f.rows_loaded(), 200);
    f.assert_sees("200 of 1,250 rows loaded");
}

#[test]
fn the_count_returns_its_bytes_to_the_budget_and_keeps_a_limit_of_the_page() {
    use sqln::exec::{ExecEvent, ExecutionEnd, StatementKey};
    use sqln::grid::Budget;
    use sqln::table::TableTab;

    let budget = Budget::new(1 << 20);
    let mut tab = TableTab::new(7, 1, relation(), false, 2);
    tab.limit = ReadLimit::Budget;
    tab.started_side_request(5, sqln::table::PageKind::Count);
    assert!(tab.is_counting());
    assert_eq!(tab.limit, ReadLimit::Budget);
    let key = StatementKey {
        console: 7,
        execution: 5,
        index: 0,
    };
    let row: Row = vec![Value::Number("42")].into_iter().collect();
    let sqln::grid::Reservation::Accepted(bytes) = budget.reserve(&row) else {
        panic!("the budget refused the row");
    };
    tab.on_event(
        ExecEvent::Rows {
            key,
            rows: vec![row],
            bytes,
        },
        &budget,
    );
    assert_eq!(budget.used(), 0, "the number left the budget");
    tab.on_event(
        ExecEvent::ResultDone {
            key,
            rows: 1,
            limit: ReadLimit::None,
        },
        &budget,
    );
    tab.on_event(
        ExecEvent::ExecutionFinished {
            console: 7,
            execution: 5,
            end: ExecutionEnd::Completed,
            message: None,
        },
        &budget,
    );
    assert_eq!(tab.total, Some(42));
    assert_eq!(tab.loaded(), 0);
    assert_eq!(tab.limit, ReadLimit::Budget);
    assert!(!tab.is_counting());
}

#[test]
fn a_count_reads_the_applied_filter_and_a_new_filter_clears_it() {
    let (mut f, recorder) = counted_table(250);
    f.type_filter("id > 5");
    f.code(KeyCode::Enter);
    f.loaded();
    f.grid();
    f.code(KeyCode::Char('c'));
    f.loaded();
    assert_eq!(
        recorder.statements().last().unwrap(),
        "SELECT COUNT(*) FROM \"public\".\"users\"\nWHERE id > 5"
    );
    assert_eq!(f.app.table().unwrap().total, Some(250));

    // A sort does not change the number of rows.
    f.code(KeyCode::Char('s'));
    f.loaded();
    assert_eq!(f.app.table().unwrap().total, Some(250));

    f.focus_filter();
    f.code(KeyCode::Enter);
    f.loaded();
    assert_eq!(f.app.table().unwrap().total, None);
}

#[test]
fn a_stopped_count_leaves_the_pages_as_they_were() {
    let (mut f, recorder) = counted_table(250);
    recorder.hold.store(true, Ordering::SeqCst);
    f.code(KeyCode::Char('c'));
    f.pump_until(|f| f.app.table().unwrap().phase == Phase::Running);
    f.assert_sees("counting");
    f.prefix('x');
    f.loaded();
    recorder.hold.store(false, Ordering::SeqCst);
    let tab = f.app.table().unwrap();
    assert_eq!(tab.total, None);
    assert_eq!(tab.loaded(), 100);
    assert!(tab.has_more);
    assert!(!tab.result.cut_short);
}

#[test]
fn a_failed_count_shows_the_error_and_keeps_the_rows() {
    let (mut f, recorder) = counted_table(250);
    *recorder.fail_when.lock().unwrap() = Some("COUNT".to_owned());
    f.code(KeyCode::Char('c'));
    f.loaded();
    f.assert_sees("syntax error near the filter");
    let tab = f.app.table().unwrap();
    assert_eq!(tab.total, None);
    assert_eq!(tab.loaded(), 100);
    assert!(tab.has_more);
}

#[test]
fn the_help_lists_the_count_only_while_it_can_act() {
    let (mut f, _) = counted_table(250);
    f.code(KeyCode::F(1));
    for _ in 0..6 {
        f.code(KeyCode::Down);
    }
    f.assert_sees("Count the rows");
    f.code(KeyCode::Esc);

    let (mut f, _) = counted_table(5);
    f.code(KeyCode::F(1));
    for _ in 0..6 {
        f.code(KeyCode::Down);
    }
    f.assert_not_sees("Count the rows");
}

#[test]
fn the_count_key_does_nothing_when_each_row_is_on_the_grid() {
    let (mut f, recorder) = counted_table(5);
    let statements = recorder.count();
    f.code(KeyCode::Char('c'));
    f.loaded();
    assert_eq!(recorder.count(), statements);
    assert!(!f.hints_at(200).contains("c count"));
}

#[test]
fn the_row_window_works_in_a_table_and_stops_at_the_loaded_rows() {
    let (mut f, _) = counted_table(5);
    f.code(KeyCode::Char('r'));
    f.assert_sees("row 1 of 5 loaded");
    f.assert_sees("name : name0");
    f.code(KeyCode::Right);
    f.assert_sees("name : name1");
    f.code(KeyCode::Esc);
    assert_eq!(f.app.table().unwrap().result.view.row, 1);
}
