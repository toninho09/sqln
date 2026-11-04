//! Measures the memory that the application keeps and the peak of its large steps.
//!
//! Run all of it with `make bench`. Run a part with
//! `cargo bench --bench memory -- <part of a name>`.
//!
//! Each measure counts the real size of each block of the allocator, so a small value
//! costs its full block. The columns do not include the pages that the allocator keeps
//! free. This report never fails.

#[path = "../tests/common/mod.rs"]
mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::ffi::c_void;
use std::hint::black_box;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{Fixture, ctrl, key, temp_db, temp_dir};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use sqln::clipboard::MemoryClipboard;
use sqln::exec::Settings;
use sqln::export::{ExportFormat, ExportOptions, ExportWriter};
use sqln::grid::{GridView, Row, RowWriter, Value};
use sqln::history::{HistoryRow, history_list};
use sqln::result::ResultTab;
use sqln::sql::Dialect;
use sqln::store::history::{LOAD_LIMIT, STATEMENT_LIMIT};
use sqln::ui::draw_editor;
use sqln::ui::theme::Theme;

const MIB: usize = 1024 * 1024;
const SCRIPT_LINE: &str =
    "INSERT INTO orders (id, name, total) VALUES (1, 'a text value', 10.50);\n";
const SCRIPT_LINES: usize = 50_000;
const UNDO_ENTRIES: usize = 100;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn malloc_size(block: *const c_void) -> usize;
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn malloc_usable_size(block: *mut c_void) -> usize;
}

#[cfg(target_os = "macos")]
unsafe fn block_size(block: *mut u8) -> usize {
    unsafe { malloc_size(block.cast()) }
}

#[cfg(target_os = "linux")]
unsafe fn block_size(block: *mut u8) -> usize {
    unsafe { malloc_usable_size(block.cast()) }
}

struct Counter;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn add(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn remove(bytes: usize) {
    LIVE.fetch_sub(bytes, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let block = unsafe { System.alloc(layout) };
        if !block.is_null() {
            add(unsafe { block_size(block) });
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        remove(unsafe { block_size(block) });
        unsafe { System.dealloc(block, layout) };
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let before = unsafe { block_size(block) };
        let moved = unsafe { System.realloc(block, layout, size) };
        if !moved.is_null() {
            remove(before);
            add(unsafe { block_size(moved) });
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counter = Counter;

struct Report {
    filter: Option<String>,
}

fn sized(bytes: usize) -> String {
    match bytes {
        bytes if bytes >= MIB => format!("{:.1} MiB", bytes as f64 / MIB as f64),
        bytes if bytes >= 1024 => format!("{:.1} KiB", bytes as f64 / 1024.0),
        bytes => format!("{bytes} B"),
    }
}

impl Report {
    /// Measures what `build` leaves in memory, and its peak. `amount` counts the units
    /// of the note.
    fn measure<T>(&self, name: &str, amount: Option<(usize, &str)>, build: impl FnOnce() -> T) {
        if self
            .filter
            .as_ref()
            .is_some_and(|part| !name.contains(part.as_str()))
        {
            return;
        }
        let before = LIVE.load(Ordering::Relaxed);
        PEAK.store(before, Ordering::Relaxed);
        let state = build();
        let kept = LIVE.load(Ordering::Relaxed).saturating_sub(before);
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
        black_box(&state);
        let note = amount.map_or_else(String::new, |(count, unit)| {
            format!("{} B/{unit}", kept / count.max(1))
        });
        println!("{name:<58} {:>10} {:>10}  {note}", sized(kept), sized(peak));
        drop(state);
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

fn row(cells: &mut RowWriter, index: usize, columns: usize) -> Row {
    (0..columns).for_each(|column| write_cell(cells, index, column));
    cells.finish()
}

fn names(columns: usize) -> Vec<String> {
    (0..columns)
        .map(|column| format!("column_{column}"))
        .collect()
}

/// Adds rows in batches, as a read does.
fn table(rows: usize, columns: usize) -> ResultTab {
    let mut tab = ResultTab::new(1, names(columns));
    let mut cells = RowWriter::default();
    for start in (0..rows).step_by(200) {
        let batch = start..rows.min(start + 200);
        let batch = batch.map(|index| row(&mut cells, index, columns));
        tab.table.append(batch.collect(), 0);
    }
    tab
}

fn grid(report: &Report) {
    for (rows, columns) in [(1_000_000, 3), (100_000, 20), (10_000, 200)] {
        report.measure(
            &format!("grid/{rows} rows of {columns} columns"),
            Some((rows * columns, "cell")),
            || table(rows, columns),
        );
    }
    let large = table(1_000_000, 3);
    report.measure("grid/copy 1,000,000 rows of 3 columns", None, || {
        let mut view = GridView::default();
        view.select_all(&large.table);
        view.copy(&large.table).map(|text| text.len())
    });
    let columns = names(3);
    let path = temp_dir("memory-export").join("rows.csv");
    report.measure("grid/export 1,000,000 rows to a CSV file", None, || {
        let options = ExportOptions {
            format: ExportFormat::Csv,
            header: true,
            delimiter: b',',
            null: "",
            bom: false,
            batch: 100,
            table: "orders".to_owned(),
            dialect: Dialect::Postgres,
        };
        let writer = ExportWriter::create(&path, options).unwrap();
        writer.start(&columns);
        for row in &large.table.rows {
            assert!(writer.write_row(row.cells()));
        }
        writer.promote(&path).unwrap();
    });
}

fn frame(fixture: &mut Fixture, terminal: &mut Terminal<TestBackend>) {
    let theme = Theme::ansi();
    terminal
        .draw(|frame| draw_editor(frame, frame.area(), &mut fixture.editor, true, &theme))
        .unwrap();
}

fn terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(120, 40)).unwrap()
}

fn editor(report: &Report) {
    let script = SCRIPT_LINE.repeat(SCRIPT_LINES);
    let lines = Some((SCRIPT_LINES, "line"));
    report.measure("editor/script of 50,000 lines, no frame", lines, || {
        Fixture::conventional(&script)
    });
    report.measure("editor/script of 50,000 lines, one frame", lines, || {
        let mut fixture = Fixture::conventional(&script);
        frame(&mut fixture, &mut terminal());
        fixture
    });
    report.measure("editor/script of 50,000 lines, Vim mode", lines, || {
        let mut fixture = Fixture::vim(&script);
        frame(&mut fixture, &mut terminal());
        fixture
    });
    report.measure(
        "editor/script of 50,000 lines, 100 undo entries",
        lines,
        || {
            let mut fixture = Fixture::conventional(&script);
            for _ in 0..UNDO_ENTRIES {
                fixture.press(key(KeyCode::Char('a')));
                fixture.press(key(KeyCode::Char(' ')));
            }
            fixture
        },
    );
    report.measure("editor/key and frame on 50,000 lines", None, || {
        let mut fixture = Fixture::conventional(&script);
        let mut terminal = terminal();
        frame(&mut fixture, &mut terminal);
        fixture.press(key(KeyCode::Char('a')));
        frame(&mut fixture, &mut terminal);
    });
    report.measure("editor/find bar, 50,000 matches", None, || {
        let mut fixture = Fixture::conventional(&script);
        fixture.press(ctrl('f'));
        fixture.write("order");
        fixture
    });
    let one_line = SCRIPT_LINE.repeat(15_000).replace('\n', " ");
    report.measure("editor/one line of 1 MiB, key and frame", None, || {
        let mut fixture = Fixture::conventional(&one_line);
        let mut terminal = terminal();
        frame(&mut fixture, &mut terminal);
        fixture.press(key(KeyCode::Char('a')));
        frame(&mut fixture, &mut terminal);
    });
}

fn catalog(tables: usize, columns: usize) -> Catalog {
    let names: Vec<String> = (0..columns)
        .map(|index| format!("column_{index}"))
        .collect();
    let relations = (0..tables)
        .map(|index| Relation {
            name: format!("table_{index}"),
            kind: RelationKind::Table,
            columns: Columns::Loaded(names.clone()),
            foreign_keys: Vec::new(),
            referencing: Vec::new(),
        })
        .collect();
    Catalog {
        schemas: vec![Schema {
            name: "public".to_owned(),
            relations,
            partial: false,
            objects: Vec::new(),
        }],
        current_schema: Some("public".to_owned()),
    }
}

fn lists(report: &Report) {
    report.measure(
        "lists/catalog of 20,000 tables of 50 columns",
        Some((20_000, "table")),
        || catalog(20_000, 50),
    );
    let sql = SCRIPT_LINE.repeat(STATEMENT_LIMIT / SCRIPT_LINE.len());
    report.measure("lists/history of 1,000 statements of 64 KiB", None, || {
        let rows = (0..LOAD_LIMIT)
            .map(|_| HistoryRow::new(true, "1 hour".to_owned(), sql.clone()))
            .collect();
        history_list(rows)
    });
}

fn database(name: &str) -> PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path).unwrap();
    path
}

fn open(sql: &str) -> (App, Terminal<TestBackend>) {
    let settings = Settings {
        row_cap: usize::MAX,
        sql_timeout: None,
        ..Settings::default()
    };
    let options = Options {
        database: Some(database("memory-results")),
        settings,
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::astrodark());
    app.on_event(Event::Paste(sql.to_owned()));
    (app, terminal())
}

fn run(app: &mut App, terminal: &mut Terminal<TestBackend>, code: crossterm::event::KeyEvent) {
    app.on_event(Event::Key(code));
    let deadline = Instant::now() + Duration::from_secs(300);
    while app.console_mut().is_busy() {
        std::thread::sleep(app.key_wait());
        if app.drain_events() {
            terminal.draw(|frame| app.draw(frame)).unwrap();
        }
        assert!(Instant::now() < deadline, "the run did not end");
    }
}

fn application(report: &Report) {
    report.measure("app/open, first frame", None, || {
        let (mut app, mut terminal) = open("");
        terminal.draw(|frame| app.draw(frame)).unwrap();
        (app, terminal)
    });
    const READ: &str = "WITH d(n) AS (SELECT 0 UNION ALL SELECT 1 UNION ALL SELECT 2 \
        UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 \
        UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) \
        SELECT a.n + 10 * b.n + 100 * c.n + 1000 * e.n + 10000 * f.n, \
        'a text value of a usual size', a.n * 2 FROM d a, d b, d c, d e, d f;";
    report.measure(
        "app/read 100,000 rows from SQLite",
        Some((100_000, "row")),
        || {
            let (mut app, mut terminal) = open(READ);
            run(&mut app, &mut terminal, key(KeyCode::F(5)));
            assert_eq!(app.console_mut().results[0].table.rows.len(), 100_000);
            (app, terminal)
        },
    );
    let shift_f5 = common::shift(KeyCode::F(5));
    let reads = "SELECT 1;\n".repeat(10_000);
    report.measure("app/10,000 result tabs", Some((10_000, "tab")), || {
        let (mut app, mut terminal) = open(&reads);
        run(&mut app, &mut terminal, shift_f5);
        (app, terminal)
    });
    let writes = "CREATE TEMP TABLE IF NOT EXISTS t(a);\n".repeat(10_000);
    report.measure("app/10,000 messages", Some((10_000, "message")), || {
        let (mut app, mut terminal) = open(&writes);
        run(&mut app, &mut terminal, shift_f5);
        (app, terminal)
    });
}

fn main() {
    // `cargo bench` adds `--bench`, so the filter is the first text without a dash.
    let filter = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    let report = Report { filter };
    println!("{:<58} {:>10} {:>10}  note", "measure", "kept", "peak");
    grid(&report);
    editor(&report);
    lists(&report);
    application(&report);
}
