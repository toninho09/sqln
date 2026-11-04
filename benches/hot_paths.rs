//! Measures the work that can hold the interface or slow a read, and prints one table.
//!
//! Run all of it with `make bench`. Run a part with
//! `cargo bench --bench hot_paths -- <part of a name>`.
//!
//! This report never fails: `tests/latency.rs` holds the targets that can fail.
//! A step runs on the interface thread, so the report compares it with 50 ms per key.
//! A rate runs out of the interface thread, so the report gives only its speed.
//!
//! Set the variables of `scripts/test-env.sh` to read from the Docker servers too.

#[path = "../tests/common/mod.rs"]
mod common;

use std::cell::{OnceCell, RefCell};
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{Fixture, ctrl, key, shift, temp_db, temp_dir};
use crossterm::event::{Event, KeyCode};
use edtui::Index2;
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use sqln::app::{App, Options};
use sqln::browser::FileBrowser;
use sqln::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use sqln::clipboard::MemoryClipboard;
use sqln::connector::network_connector;
use sqln::connector::target::parse_uri;
use sqln::editor::RunScope;
use sqln::exec::Settings;
use sqln::export::{Encoder, ExportFormat, ExportOptions, ExportWriter};
use sqln::grid::{Binary, GridView, Row, RowSearch, RowWriter, Value};
use sqln::history::{HistoryRow, history_list};
use sqln::input::Action;
use sqln::result::ResultTab;
use sqln::sql::{Dialect, split, tokenize};
use sqln::store::history::{self, Entry, LOAD_LIMIT, STATEMENT_LIMIT};
use sqln::store::scripts::{self, ScriptMeta};
use sqln::store::{Root, Scope};
use sqln::ui::console::draw_grid;
use sqln::ui::draw_editor;
use sqln::ui::theme::Theme;

/// The time that one key can take, with its frame.
const TARGET: Duration = Duration::from_millis(50);
const MIN_RUNS: usize = 3;
const MAX_RUNS: usize = 30;
/// A measure adds runs until this time ends.
const MEASURE_TIME: Duration = Duration::from_secs(2);
const MIB: usize = 1024 * 1024;

const SCRIPT_LINE: &str =
    "INSERT INTO orders (id, name, total) VALUES (1, 'a text value', 10.50);\n";
const SCRIPT_LINES: usize = 50_000;
const LARGE_VALUE: usize = 64 * MIB;
const LARGE_TABLE: usize = 1_000_000;
const WIDE_TABLE: usize = 2_000;

/// A read with the same text in each SQL dialect. It returns 100,000 rows.
const READ_ROWS: usize = 100_000;
const READ_SQL: &str = "WITH d(n) AS (SELECT 0 UNION ALL SELECT 1 UNION ALL SELECT 2 \
    UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 \
    UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) \
    SELECT a.n + 10 * b.n + 100 * c.n + 1000 * e.n + 10000 * f.n, \
    'a text value of a usual size', a.n * 2 FROM d a, d b, d c, d e, d f;";

#[derive(Default)]
struct Bench {
    filter: Option<String>,
    /// The steps above the target, with their median.
    over: Vec<(String, Duration)>,
}

/// Returns the sorted times of a work, with a new state before each run.
fn times<S, T>(mut prepare: impl FnMut() -> S, mut work: impl FnMut(&mut S) -> T) -> Vec<Duration> {
    let started = Instant::now();
    let mut times = Vec::new();
    while times.len() < MIN_RUNS || (times.len() < MAX_RUNS && started.elapsed() < MEASURE_TIME) {
        let mut state = prepare();
        let run = Instant::now();
        let output = work(&mut state);
        times.push(run.elapsed());
        black_box(output);
    }
    times.sort();
    times
}

fn median(times: &[Duration]) -> Duration {
    times[times.len() / 2]
}

fn print_row(name: &str, times: &[Duration], note: &str) {
    let (middle, worst) = (median(times), times[times.len() - 1]);
    println!(
        "{name:<58} {:>10} {:>10}  {note}",
        format!("{middle:.1?}"),
        format!("{worst:.1?}")
    );
}

impl Bench {
    fn skips(&self, name: &str) -> bool {
        let filter = self.filter.as_deref();
        filter.is_some_and(|part| !name.contains(part))
    }

    fn report_step(&mut self, name: &str, times: &[Duration]) {
        let over = median(times) > TARGET;
        print_row(name, times, if over { "OVER" } else { "" });
        if over {
            self.over.push((name.to_owned(), median(times)));
        }
    }

    /// Measures interface work that needs a new state before each run.
    fn step<S, T>(
        &mut self,
        name: &str,
        prepare: impl FnMut() -> S,
        work: impl FnMut(&mut S) -> T,
    ) {
        if !self.skips(name) {
            self.report_step(name, &times(prepare, work));
        }
    }

    /// Measures interface work that can repeat on one state.
    fn key<S, T>(
        &mut self,
        name: &str,
        prepare: impl FnOnce() -> S,
        mut work: impl FnMut(&mut S) -> T,
    ) {
        if !self.skips(name) {
            let mut state = prepare();
            self.report_step(name, &times(|| (), |()| work(&mut state)));
        }
    }

    /// Measures work out of the interface thread. `amount` counts `unit` in one run.
    fn rate<S, T>(
        &mut self,
        name: &str,
        (amount, unit): (usize, &str),
        prepare: impl FnMut() -> S,
        work: impl FnMut(&mut S) -> T,
    ) {
        if !self.skips(name) {
            let times = times(prepare, work);
            let rate = scaled(amount as f64 / median(&times).as_secs_f64());
            print_row(name, &times, &format!("{rate} {unit}/s"));
        }
    }
}

fn scaled(rate: f64) -> String {
    match rate {
        rate if rate >= 1e9 => format!("{:.1} G", rate / 1e9),
        rate if rate >= 1e6 => format!("{:.1} M", rate / 1e6),
        rate if rate >= 1e3 => format!("{:.1} k", rate / 1e3),
        rate => format!("{rate:.0}"),
    }
}

/// Types a letter and erases it, in turns, and draws a frame after each key.
struct Screen {
    fixture: Fixture,
    terminal: Terminal<TestBackend>,
    theme: Theme,
    letter: char,
    typed: bool,
}

impl Screen {
    fn new(fixture: Fixture, letter: char) -> Self {
        Self::sized(fixture, letter, 120, 40)
    }

    fn sized(fixture: Fixture, letter: char, width: u16, height: u16) -> Self {
        Self {
            fixture,
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
            theme: Theme::ansi(),
            letter,
            typed: false,
        }
    }

    fn key_and_frame(&mut self) {
        self.typed = !self.typed;
        let code = if self.typed {
            KeyCode::Char(self.letter)
        } else {
            KeyCode::Backspace
        };
        self.fixture.press(key(code));
        let Self {
            fixture,
            terminal,
            theme,
            ..
        } = self;
        terminal
            .draw(|frame| draw_editor(frame, frame.area(), &mut fixture.editor, true, theme))
            .unwrap();
    }
}

fn large_catalog(tables: usize, columns: usize) -> Catalog {
    let names: Vec<String> = (0..columns)
        .map(|index| format!("column_{index}"))
        .collect();
    let relations = (0..tables)
        .map(|index| Relation {
            name: format!("table_{index}"),
            kind: RelationKind::Table,
            columns: Columns::Loaded(names.clone()),
            foreign_keys: Vec::new(),
        })
        .collect();
    Catalog {
        schemas: vec![Schema {
            name: "public".to_owned(),
            relations,
            partial: false,
        }],
        current_schema: Some("public".to_owned()),
    }
}

fn sql(bench: &mut Bench) {
    let script = SCRIPT_LINE.repeat(SCRIPT_LINES);
    let size = (script.len(), "bytes");
    bench.rate(
        "sql/tokenize a script",
        size,
        || (),
        |()| tokenize(&script, Dialect::Postgres),
    );
    bench.rate(
        "sql/split a script",
        size,
        || (),
        |()| split(&script, Dialect::Postgres),
    );
}

fn long_text(bytes: usize, lines: usize) -> String {
    let line = "SELECT a, b FROM t WHERE c = 1; ".repeat(bytes / 32);
    vec![line; lines].join("\n")
}

fn long_line(bytes: usize) -> Screen {
    let text = long_text(bytes, 1);
    let mut fixture = Fixture::conventional(&text);
    fixture.editor.set_cursor(Index2::new(0, text.len() / 2));
    Screen::new(fixture, 's')
}

fn suggestions(bench: &mut Bench) {
    for tables in [1_000, 20_000] {
        bench.key(
            &format!("editor/key and frame, table names of {tables} tables"),
            || {
                let mut fixture = Fixture::conventional("SELECT * FROM ");
                fixture.catalog = large_catalog(tables, 50);
                fixture.editor.set_cursor(Index2::new(0, 14));
                Screen::new(fixture, 't')
            },
            Screen::key_and_frame,
        );
        bench.key(
            &format!("editor/key and frame, columns in {tables} tables"),
            || {
                let text = format!("SELECT  FROM table_{}", tables - 1);
                let mut fixture = Fixture::conventional(&text);
                fixture.catalog = large_catalog(tables, 50);
                fixture.editor.set_cursor(Index2::new(0, 7));
                Screen::new(fixture, 'c')
            },
            Screen::key_and_frame,
        );
    }
}

fn find_fixture(text: &str) -> Fixture {
    let mut fixture = Fixture::conventional(text);
    fixture.press(ctrl('f'));
    fixture.write("order");
    let matches = fixture.editor.search().unwrap().matches.len();
    assert_eq!(matches, text.matches("order").count());
    fixture
}

fn replace_fixture(text: &str) -> Fixture {
    let mut fixture = find_fixture(text);
    fixture.press(key(KeyCode::Tab));
    fixture.write("sale");
    fixture
}

fn editor(bench: &mut Bench) {
    for (width, height) in [(120, 40), (200, 60), (300, 100)] {
        bench.key(
            &format!("editor/key and frame, full SQL screen of {width} by {height}"),
            || {
                let fixture = Fixture::conventional(&long_text(usize::from(width), 200));
                Screen::sized(fixture, 's', width, height)
            },
            Screen::key_and_frame,
        );
    }
    for kib in [10, 30, 100, 1024] {
        bench.key(
            &format!("editor/key and frame, one line of {kib} KiB"),
            || long_line(kib * 1024),
            Screen::key_and_frame,
        );
    }
    for kib in [10, 30] {
        bench.key(
            &format!("editor/key and frame, a screen of lines of {kib} KiB"),
            || Screen::new(Fixture::conventional(&long_text(kib * 1024, 40)), 's'),
            Screen::key_and_frame,
        );
    }
    suggestions(bench);

    let script = SCRIPT_LINE.repeat(SCRIPT_LINES);
    bench.key(
        "editor/key and frame in the find bar, 50,000 matches",
        || Screen::new(find_fixture(&script), 's'),
        Screen::key_and_frame,
    );
    let one_line = script.replace('\n', " ");
    bench.key(
        "editor/key and frame in the find bar, 50,000 matches in one line",
        || Screen::new(find_fixture(&one_line), 's'),
        Screen::key_and_frame,
    );
    for (shape, text) in [
        ("50,000 matches in 50,000 lines", &script),
        ("50,000 matches in one line", &one_line),
    ] {
        bench.step(
            &format!("editor/replace all, {shape}"),
            || replace_fixture(text),
            |fixture| fixture.press(ctrl('a')),
        );
    }
    bench.step(
        "editor/paste 50,000 lines",
        || Fixture::conventional(""),
        |fixture| fixture.editor.on_paste(&script),
    );
    bench.step(
        "editor/undo a paste of 50,000 lines",
        || {
            let mut fixture = Fixture::conventional("");
            fixture.editor.on_paste(&script);
            fixture
        },
        |fixture| fixture.press(ctrl('z')),
    );
    bench.key(
        "editor/comment 50,000 selected lines",
        || {
            let mut fixture = Fixture::conventional(&script);
            fixture.press(ctrl('a'));
            fixture
        },
        |fixture| fixture.editor.toggle_comment(),
    );
    bench.key(
        "editor/plan a run of 50,000 statements",
        || Fixture::conventional(&script),
        |fixture| fixture.editor.plan_run(RunScope::Script).map(|_| ()),
    );
    for kib in [64, 1024] {
        let sql = format!(
            "SELECT {}1 FROM t WHERE a = 1;",
            "column_name_01, ".repeat(kib * 1024 / 16)
        );
        bench.step(
            &format!("editor/format one statement of {kib} KiB"),
            || Fixture::conventional(&sql),
            |fixture| fixture.editor.format_sql().map(|_| ()),
        );
    }
}

fn write_cell(cells: &mut RowWriter, row: usize, column: usize) {
    match column % 3 {
        0 => cells.push(Value::Number(&row.to_string())),
        1 => cells.push(Value::Text(&format!("a text value of row {row}"))),
        _ if row.is_multiple_of(10) => cells.push(Value::Null),
        _ => cells.push(Value::Text("a \"quoted\" text, with a comma")),
    }
}

fn rows(rows: usize, columns: usize) -> Vec<Row> {
    let mut cells = RowWriter::default();
    let mut row = |row| {
        (0..columns).for_each(|column| write_cell(&mut cells, row, column));
        cells.finish()
    };
    (0..rows).map(&mut row).collect()
}

fn names(columns: usize) -> Vec<String> {
    (0..columns)
        .map(|column| format!("column_{column}"))
        .collect()
}

fn result(row_count: usize, columns: usize) -> ResultTab {
    let mut tab = ResultTab::new(1, names(columns));
    tab.table.append(rows(row_count, columns), 0);
    tab
}

fn value_result(text: String) -> ResultTab {
    let mut tab = ResultTab::new(1, vec!["value".to_owned()]);
    tab.table
        .append(vec![[Value::Text(&text)].into_iter().collect()], 0);
    tab
}

fn json_document(bytes: usize) -> String {
    let member = r#"{"id":1,"name":"a text value","tags":["a","b"]},"#;
    format!("[{}0]", member.repeat(bytes / member.len()))
}

fn grid(bench: &mut Bench) {
    let area = Rect::new(0, 0, 120, 30);
    let theme = Theme::ansi();
    let mut at_end = false;
    bench.key(
        "grid/jump over 2,000 columns and draw",
        || (result(100, WIDE_TABLE), Buffer::empty(area)),
        |(tab, buffer)| {
            at_end = !at_end;
            let column = if at_end { WIDE_TABLE - 1 } else { 0 };
            tab.view.move_to(0, column, &tab.table, false);
            draw_grid(buffer, area, tab, true, &theme);
        },
    );
    bench.key(
        "grid/row window of 2,000 columns",
        || result(1, WIDE_TABLE),
        |tab| tab.row_window(0).map(|window| window.text.len()),
    );

    let large = OnceCell::new();
    let large = || large.get_or_init(|| result(LARGE_TABLE, 3));
    bench.key(
        "grid/copy 1,000,000 rows of 3 columns",
        || {
            let mut view = GridView::default();
            view.select_all(&large().table);
            view
        },
        |view| view.copy(&large().table).map(|text| text.len()),
    );
    bench.rate(
        "grid/search a text that no row holds",
        (LARGE_TABLE, "rows"),
        || {
            let mut search = RowSearch::new((0, 0));
            search.push_str("absent text");
            search
        },
        |search| {
            let deadline = Instant::now() + Duration::from_secs(3600);
            search.scan(&large().table, deadline)
        },
    );
    bench.rate(
        "grid/append rows in batches of 200",
        (LARGE_TABLE, "rows"),
        || {
            let tab = ResultTab::new(1, names(3));
            (tab, rows(LARGE_TABLE, 3))
        },
        |(tab, rows)| {
            while !rows.is_empty() {
                let batch = rows.split_off(rows.len().saturating_sub(200));
                tab.table.append(batch, 0);
            }
        },
    );
    bench.rate(
        "grid/classify a binary value",
        (LARGE_VALUE, "bytes"),
        || vec![b'x'; LARGE_VALUE],
        |bytes| Binary::new(bytes).len(),
    );

    let text = "a text value of a usual size ".repeat(LARGE_VALUE / 29);
    bench.key(
        "grid/open a text value of 64 MiB",
        || value_result(text.clone()),
        |tab| tab.on_action(Action::GridOpenValue, false, false),
    );
    bench.key(
        "grid/open a binary value of 64 MiB",
        || {
            let mut tab = ResultTab::new(1, vec!["value".to_owned()]);
            let bytes = vec![0xFF; LARGE_VALUE];
            let value = Value::binary(&bytes);
            tab.table.append(vec![vec![value].into_iter().collect()], 0);
            tab
        },
        |tab| tab.on_action(Action::GridOpenValue, false, false),
    );
    bench.key(
        "grid/open a JSON value of 1 MiB",
        || value_result(json_document(MIB - 100)),
        |tab| tab.on_action(Action::GridOpenValue, false, false),
    );
}

fn history_root(name: &str) -> Root {
    let root = Root::global(temp_dir(name));
    let mut lines = Vec::new();
    while lines.len() < 8 * MIB {
        let entry = Entry {
            at: 1_700_000_000,
            connection: "other".to_owned(),
            ok: true,
            sql: SCRIPT_LINE.repeat(2),
        };
        history::push_line(&mut lines, &entry);
    }
    fs::write(root.history_file(), lines).unwrap();
    root
}

fn history_rows() -> Vec<HistoryRow> {
    let sql = SCRIPT_LINE.repeat(STATEMENT_LIMIT / SCRIPT_LINE.len());
    let row = |_| HistoryRow::new(true, "1 hour".to_owned(), sql.clone());
    (0..LOAD_LIMIT).map(row).collect()
}

fn folder_of_files(name: &str, files: usize) -> PathBuf {
    let folder = temp_dir(name);
    for index in 0..files {
        File::create(folder.join(format!("file_{index}.db"))).unwrap();
    }
    folder
}

fn script_root(name: &str, count: usize) -> Root {
    let root = Root::global(temp_dir(name));
    fs::create_dir_all(root.consoles_dir()).unwrap();
    for index in 0..count {
        let id = format!("script-{index}");
        let meta = ScriptMeta::new(
            id.clone(),
            format!("Script {index}"),
            "g1".to_owned(),
            Scope::Global,
        );
        let folder = root.consoles_dir();
        fs::write(folder.join(format!("{id}.sql")), SCRIPT_LINE).unwrap();
        let path = folder.join(format!("{id}.json"));
        fs::write(path, serde_json::to_vec(&meta).unwrap()).unwrap();
    }
    root
}

fn lists(bench: &mut Bench) {
    bench.key(
        "lists/read a history file of 8 MiB",
        || history_root("bench-history"),
        |root| history::load(root, "g1", LOAD_LIMIT),
    );
    bench.step(
        "lists/open the history with 1,000 statements of 64 KiB",
        history_rows,
        |rows| history_list(std::mem::take(rows)),
    );
    let mut typed = false;
    bench.key(
        "lists/filter key, 1,000 statements of 64 KiB",
        || history_list(history_rows()),
        |list| {
            typed = !typed;
            let code = if typed {
                KeyCode::Char('z')
            } else {
                KeyCode::Backspace
            };
            list.on_key(key(code))
        },
    );
    bench.key(
        "lists/read a folder of 20,000 files",
        || {
            let folder = folder_of_files("bench-browser", 20_000);
            format!("{}/", folder.display())
        },
        |path| FileBrowser::new(path, false, None),
    );
    bench.key(
        "lists/read 5,000 saved scripts",
        || script_root("bench-scripts", 5_000),
        |root| scripts::list(root, Scope::Global),
    );
}

fn open_app(options: Options, theme: Theme) -> App {
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, theme)
}

/// Returns options without the limits of a usual read, because a limit ends the measure.
fn database_options(path: &Path) -> Options {
    let settings = Settings {
        row_cap: usize::MAX,
        sql_timeout: None,
        ..Settings::default()
    };
    Options {
        database: Some(path.to_owned()),
        settings,
        ..Options::default()
    }
}

fn sqlite_database(name: &str) -> PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path).unwrap();
    path
}

/// Waits for the end of a run. The application draws a frame after each change.
fn wait_for_run(app: &mut App, mut frame: impl FnMut(&mut App)) {
    let deadline = Instant::now() + Duration::from_secs(300);
    while app.console_mut().is_busy() {
        // The main loop waits for a key at this point.
        std::thread::sleep(app.key_wait());
        if app.drain_events() {
            frame(app);
        }
        assert!(Instant::now() < deadline, "the run did not end");
    }
}

/// An application with a run that waits for its key, and the frames of a usual screen.
struct Run {
    app: App,
    terminal: Terminal<TestBackend>,
}

impl Run {
    fn new(options: Options, sql: &str) -> Self {
        let mut app = open_app(options, Theme::astrodark());
        app.on_event(Event::Paste(sql.to_owned()));
        let terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        Self { app, terminal }
    }

    fn run(&mut self, code: crossterm::event::KeyEvent) {
        let Self { app, terminal } = self;
        app.on_event(Event::Key(code));
        wait_for_run(app, |app| {
            terminal.draw(|frame| app.draw(frame)).unwrap();
        });
    }

    fn frame(&mut self) {
        let Self { app, terminal } = self;
        terminal.draw(|frame| app.draw(frame)).unwrap();
    }

    fn read(&mut self) {
        self.run(key(KeyCode::F(5)));
        let rows = self.app.console_mut().results[0].table.rows.len();
        assert_eq!(rows, READ_ROWS, "the read ended early");
    }
}

fn server_options(uri: &str) -> Options {
    let settings = Settings {
        row_cap: usize::MAX,
        sql_timeout: None,
        ..Settings::default()
    };
    Options {
        connector: Some(network_connector(parse_uri(uri).unwrap())),
        label: Some("bench server".to_owned()),
        settings,
        ..Options::default()
    }
}

fn results(bench: &mut Bench) {
    let path = OnceCell::new();
    let path = || path.get_or_init(|| sqlite_database("bench-results"));
    let rows = (READ_ROWS, "rows");
    bench.rate(
        "results/read 100,000 rows from SQLite",
        rows,
        || Run::new(database_options(path()), READ_SQL),
        Run::read,
    );
    for (name, variable) in [
        ("PostgreSQL", "SQLN_TEST_POSTGRES"),
        ("MySQL", "SQLN_TEST_MYSQL"),
    ] {
        if let Ok(uri) = std::env::var(variable) {
            bench.rate(
                &format!("results/read 100,000 rows from {name}"),
                rows,
                || Run::new(server_options(&uri), READ_SQL),
                Run::read,
            );
        }
    }

    const STATEMENTS: usize = 10_000;
    // A read gives a result tab. A statement without rows gives a line in **Messages**.
    const READ: &str = "SELECT 1;\n";
    const WRITE: &str = "CREATE TEMP TABLE IF NOT EXISTS t(a);\n";
    let run_script = shift(KeyCode::F(5));
    for statements in [STATEMENTS, 5 * STATEMENTS] {
        for (name, statement) in [("reads", READ), ("writes", WRITE)] {
            let script = statement.repeat(statements);
            bench.rate(
                &format!("results/run a script of {statements} {name}"),
                (statements, "statements"),
                || Run::new(database_options(path()), &script),
                |run| run.run(run_script),
            );
        }
    }
    let finished = |statement: &str| {
        let script = statement.repeat(STATEMENTS);
        let mut run = Run::new(database_options(path()), &script);
        run.run(run_script);
        run
    };
    bench.key(
        "results/frame on a result, 10,000 result tabs",
        || {
            let mut run = finished(READ);
            assert_eq!(run.app.console_mut().results.len(), STATEMENTS);
            run
        },
        Run::frame,
    );
    bench.key(
        "results/frame at the end of 10,000 messages",
        || {
            let mut run = finished(WRITE);
            let console = run.app.console_mut();
            assert!(console.messages.len() >= STATEMENTS);
            console.active = console.results.len();
            console.messages_scroll = STATEMENTS;
            run
        },
        Run::frame,
    );
    bench.key(
        "results/key and frame after 10,000 statements",
        || {
            let mut run = finished(WRITE);
            let before = run.app.console_mut().editor.text();
            run.app.on_event(Event::Key(key(KeyCode::Char('x'))));
            assert_ne!(run.app.console_mut().editor.text(), before);
            run
        },
        |run| {
            run.app.on_event(Event::Key(key(KeyCode::Char('x'))));
            run.frame();
        },
    );
}

/// Draws a screen of results and sends each cell through the real backend, as a first
/// frame does. The bytes go nowhere, so the time leaves out the terminal.
fn full_paint(bench: &mut Bench, width: u16, height: u16) {
    bench.key(
        &format!("frame/full paint of {width} by {height} cells"),
        || {
            let mut run = Run::new(database_options(&sqlite_database("bench-frame")), READ_SQL);
            run.read();
            let terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            (run.app, terminal, CrosstermBackend::new(io::sink()))
        },
        |(app, terminal, backend)| {
            let frame = terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = frame.buffer;
            let cells = buffer.content.iter().enumerate();
            let cells = cells.map(|(index, cell)| {
                let (x, y) = buffer.pos_of(index);
                (x, y, cell)
            });
            backend.draw(cells).unwrap();
        },
    );
}

fn navigator(bench: &mut Bench) {
    let tables = 20_000;
    let path = OnceCell::new();
    let path = || {
        path.get_or_init(|| {
            let path = sqlite_database("bench-navigator");
            let create = |index| format!("CREATE TABLE table_{index}(a, b, c);");
            let setup: String = (0..tables).map(create).collect();
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection
                .execute_batch(&format!("BEGIN;{setup}COMMIT;"))
                .unwrap();
            path
        })
    };
    bench.rate(
        "navigator/read the catalog of 20,000 tables",
        (tables, "tables"),
        || {
            let mut app = open_app(database_options(path()), Theme::ansi());
            app.on_event(Event::Key(ctrl('w')));
            app.on_event(Event::Key(key(KeyCode::Char('h'))));
            app
        },
        |app| {
            app.on_event(Event::Key(key(KeyCode::Right)));
            let deadline = Instant::now() + Duration::from_secs(60);
            while app.connections()[0].catalog.is_none() {
                app.drain_events();
                assert!(Instant::now() < deadline, "the catalog did not arrive");
            }
        },
    );
}

fn export_options(format: ExportFormat) -> ExportOptions {
    ExportOptions {
        format,
        header: true,
        delimiter: b',',
        null: "",
        bom: false,
        batch: 100,
        table: "orders".to_owned(),
        dialect: Dialect::Postgres,
    }
}

fn export(bench: &mut Bench) {
    const ROWS: usize = 200_000;
    const COLUMNS: usize = 6;

    let table = OnceCell::new();
    let table = || table.get_or_init(|| rows(ROWS, COLUMNS));
    let columns = names(COLUMNS);
    for (name, format) in [
        ("CSV", ExportFormat::Csv),
        ("TSV", ExportFormat::Tsv),
        ("JSON", ExportFormat::Json),
        ("JSON Lines", ExportFormat::JsonLines),
        ("SQL INSERT", ExportFormat::SqlInsert),
    ] {
        bench.rate(
            &format!("export/encode 200,000 rows as {name}"),
            (ROWS, "rows"),
            || Encoder::new(BufWriter::new(io::sink()), export_options(format)),
            |encoder| {
                encoder.start(&columns).unwrap();
                for row in table() {
                    encoder.row(row.cells()).unwrap();
                }
                encoder.finish().unwrap();
            },
        );
    }
    let path = temp_dir("bench-export").join("rows.csv");
    bench.rate(
        "export/write 200,000 rows to a CSV file",
        (ROWS, "rows"),
        || ExportWriter::create(&path, export_options(ExportFormat::Csv)).unwrap(),
        |writer| {
            writer.start(&columns);
            for row in table() {
                assert!(writer.write_row(row.cells()));
            }
            writer.promote(&path).unwrap();
        },
    );
}

fn main() {
    // `cargo bench` adds `--bench`, so the filter is the first text without a dash.
    let filter = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    let mut bench = Bench {
        filter,
        ..Bench::default()
    };
    println!("{:<58} {:>10} {:>10}  note", "measure", "median", "worst");
    sql(&mut bench);
    editor(&mut bench);
    grid(&mut bench);
    lists(&mut bench);
    results(&mut bench);
    full_paint(&mut bench, 120, 40);
    full_paint(&mut bench, 300, 100);
    navigator(&mut bench);
    export(&mut bench);

    bench.over.sort_by_key(|(_, time)| std::cmp::Reverse(*time));
    println!("\nSteps over the target of {TARGET:?}, the slowest first:");
    for (name, time) in &bench.over {
        println!("{time:>10.1?}  {name}");
    }
}
