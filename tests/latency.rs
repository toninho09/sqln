// The target is under 50 ms per key. Run it with `make bench`: the tests must run one at a
// time, because tests that run together take time from each other and fail.

mod common;

use std::time::{Duration, Instant};

use common::{Fixture, ctrl, key, shift};
use crossterm::event::{Event, KeyCode};
use edtui::Index2;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::exec::Settings;
use sqln::ui::draw_editor;
use sqln::ui::theme::Theme;

const TARGET: Duration = Duration::from_millis(50);
const ROW: &str = "INSERT INTO orders (id, name, total) VALUES (1, 'a text value', 10.50);";

fn sqlite_app(name: &str, settings: Settings) -> App {
    use std::cell::RefCell;
    use std::rc::Rc;

    use sqln::clipboard::MemoryClipboard;

    let path = common::temp_db(name);
    rusqlite::Connection::open(&path).unwrap();
    let options = Options {
        database: Some(path),
        settings,
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::ansi())
}

fn worst_key_and_frame(lines: usize) -> Duration {
    let mut f = Fixture::conventional(&format!("{ROW}\n").repeat(lines));
    f.editor.set_cursor(Index2::new(lines / 2, 0));
    worst_key_on(&mut f)
}

fn worst_key_on(f: &mut Fixture) -> Duration {
    worst_key_in(f, 120, 40)
}

fn worst_key_in(f: &mut Fixture, width: u16, height: u16) -> Duration {
    let theme = Theme::ansi();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut worst = Duration::ZERO;
    for index in 0..60 {
        let started = Instant::now();
        let code = if index % 6 == 5 {
            KeyCode::Backspace
        } else {
            KeyCode::Char('s')
        };
        f.press(key(code));
        terminal
            .draw(|frame| draw_editor(frame, frame.area(), &mut f.editor, true, &theme))
            .unwrap();
        worst = worst.max(started.elapsed());
    }
    worst
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn key_with_suggestions_and_frame_stay_under_the_target() {
    for lines in [100, 1_000, 10_000, 50_000] {
        let worst = worst_key_and_frame(lines);
        println!("{lines} lines: worst key plus frame {worst:?}");
        assert!(worst < TARGET, "{lines} lines: {worst:?}");
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_on_a_long_row_stays_under_the_target() {
    for kib in [100, 1024] {
        let text = format!("{ROW} ").repeat(kib * 1024 / (ROW.len() + 1));
        let mut f = Fixture::conventional(&text);
        f.editor.set_cursor(Index2::new(0, text.len() / 2));
        let worst = worst_key_on(&mut f);
        println!("one row of {kib} KiB: worst key plus frame {worst:?}");
        assert!(worst < TARGET, "{kib} KiB: {worst:?}");
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_on_a_screen_of_long_rows_stays_under_the_target() {
    let row = format!("{ROW} ").repeat(30 * 1024 / (ROW.len() + 1));
    let mut f = Fixture::conventional(&vec![row; 40].join("\n"));
    let worst = worst_key_on(&mut f);
    println!("40 rows of 30 KiB: worst key plus frame {worst:?}");
    assert!(worst < TARGET, "{worst:?}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_on_a_full_large_terminal_stays_under_the_target() {
    let row = format!("{ROW} ").repeat(300 / (ROW.len() + 1) + 1);
    let mut f = Fixture::conventional(&vec![row; 200].join("\n"));
    let worst = worst_key_in(&mut f, 300, 100);
    println!("300 by 100 cells of SQL: worst key plus frame {worst:?}");
    assert!(worst < TARGET, "{worst:?}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn replace_all_in_one_row_stays_under_the_target() {
    let mut f = Fixture::conventional(&format!("{ROW} ").repeat(50_000));
    f.press(ctrl('f'));
    f.write("order");
    f.press(key(KeyCode::Tab));
    f.write("sale");
    let started = Instant::now();
    f.press(ctrl('a'));
    let time = started.elapsed();
    println!("replace all of 50,000 matches in one row: {time:?}");
    assert!(f.editor.search().unwrap().matches.is_empty());
    assert!(time < TARGET, "{time:?}");
}

/// The time of a run grows with its statements. A frame for each few events gave a time
/// that grew with the square of the statements.
#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_large_script_ends_soon_and_the_next_key_stays_under_the_target() {
    const STATEMENTS: usize = 50_000;
    let mut app = sqlite_app("latency-script", Settings::default());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let script = "CREATE TEMP TABLE IF NOT EXISTS t(a);\n".repeat(STATEMENTS);
    app.on_event(Event::Paste(script));
    app.on_event(Event::Key(shift(KeyCode::F(5))));
    let started = Instant::now();
    while app.console_mut().is_busy() {
        std::thread::sleep(app.key_wait());
        if app.drain_events() {
            terminal.draw(|frame| app.draw(frame)).unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    println!("{STATEMENTS} statements: {:?}", started.elapsed());
    assert!(app.console_mut().messages.len() >= STATEMENTS);

    let mut worst = Duration::ZERO;
    for _ in 0..20 {
        let started = Instant::now();
        app.on_event(Event::Key(key(KeyCode::Char('x'))));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst = worst.max(started.elapsed());
    }
    println!("{STATEMENTS} marks: worst key plus frame {worst:?}");
    assert!(worst < TARGET, "{worst:?}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_stays_under_the_target_with_the_full_budget_loaded() {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let settings = Settings {
        row_cap: usize::MAX,
        sql_timeout: None,
        ..Settings::default()
    };
    let mut app = sqlite_app("latency-budget", settings);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    app.on_event(Event::Paste(
        "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) \
         SELECT x, 'a text value of a usual size', x * 2 FROM c;"
            .to_owned(),
    ));
    app.on_event(Event::Key(key(KeyCode::F(5))));

    // The frames during the load count too, because the interface must stay available.
    let started = Instant::now();
    let mut worst_frame = Duration::ZERO;
    let mut frames = 0;
    loop {
        let frame = Instant::now();
        app.drain_events();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst_frame = worst_frame.max(frame.elapsed());
        frames += 1;
        if !app.console_mut().is_busy() {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(120));
    }
    let rows = app.console_mut().results[0].table.rows.len();
    let bytes = app.console_mut().results[0].table.bytes;
    println!(
        "loaded {rows} rows, {} MiB, in {:?}; worst frame during the load {worst_frame:?} of {frames}",
        bytes / (1024 * 1024),
        started.elapsed(),
    );
    assert!(bytes > 250 * 1024 * 1024, "{bytes}");

    app.on_event(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    app.on_event(Event::Key(key(KeyCode::Char('j'))));
    let mut worst = Duration::ZERO;
    for index in 0..200 {
        let started = Instant::now();
        let code = if index % 2 == 0 {
            KeyCode::Down
        } else {
            KeyCode::Right
        };
        app.on_event(Event::Key(key(code)));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst = worst.max(started.elapsed());
    }
    let started_copy = Instant::now();
    app.on_event(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::ALT)));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    println!(
        "worst key plus frame with the full budget {worst:?}; one more {:?}",
        started_copy.elapsed()
    );
    assert!(worst < Duration::from_millis(50), "{worst:?}");
    assert!(worst_frame < Duration::from_millis(50), "{worst_frame:?}");

    // The search reads each row, so it must leave the keys free during that work.
    app.on_event(Event::Key(KeyEvent::new(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    )));
    let mut worst_search = Duration::ZERO;
    let started_search = Instant::now();
    let mut slices = 0;
    for c in "absent text".chars() {
        let started = Instant::now();
        app.on_event(Event::Key(key(KeyCode::Char(c))));
        app.drain_events();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst_search = worst_search.max(started.elapsed());
    }
    while app.has_work() {
        let started = Instant::now();
        app.drain_events();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst_search = worst_search.max(started.elapsed());
        slices += 1;
    }
    println!(
        "search of {rows} rows in {:?}, {slices} slices; worst slice plus frame {worst_search:?}",
        started_search.elapsed(),
    );
    assert!(worst_search < Duration::from_millis(50), "{worst_search:?}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn table_page_keys_and_frames_stay_under_the_target() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use sqln::app::{App, Options};
    use sqln::clipboard::MemoryClipboard;
    use sqln::exec::Settings;

    let path = common::temp_db("latency-table-pages");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE pages(id INTEGER PRIMARY KEY, label TEXT); \
         WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 50000) \
         INSERT INTO pages SELECT x, 'row' || x FROM n;",
        )
        .unwrap();
    let settings = Settings {
        page_size: 10_000,
        ..Settings::default()
    };
    let options = Options {
        database: Some(path),
        settings,
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::ansi());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    common::open_table(&mut app, 0, "pages");
    for page in 0..5 {
        if page > 0 {
            app.on_event(Event::Key(key(KeyCode::Char('m'))));
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.table().unwrap().is_busy() {
            app.drain_events();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(app.table().unwrap().loaded(), 50_000);
    let mut worst = Duration::ZERO;
    for index in 0..200 {
        let started = Instant::now();
        let direction = if (index / 4) % 2 == 0 { ']' } else { '[' };
        app.on_event(Event::Key(key(KeyCode::Char(direction))));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst = worst.max(started.elapsed());
        assert!(!app.table().unwrap().is_busy());
    }
    println!("50,000 table rows: worst page key plus frame {worst:?}");
    assert!(worst < Duration::from_millis(50), "{worst:?}");
}

/// Opens a navigator with 20,000 tables, and returns after its catalog arrives.
fn large_navigator(name: &str) -> sqln::app::App {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crossterm::event::{KeyEvent, KeyModifiers};
    use sqln::app::{App, Options};
    use sqln::clipboard::MemoryClipboard;

    let path = common::temp_db(name);
    let mut setup = String::from("BEGIN;");
    for index in 0..20_000 {
        setup.push_str(&format!("CREATE TABLE table_{index}(a, b, c);"));
    }
    setup.push_str("COMMIT;");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(&setup)
        .unwrap();
    let options = Options {
        database: Some(path),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::ansi());
    let press = |app: &mut App, code| app.on_event(Event::Key(key(code)));

    app.on_event(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Right);
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.connections()[0].catalog.is_none() {
        app.drain_events();
        assert!(Instant::now() < deadline);
    }
    app
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_in_a_filtered_navigator_stays_under_the_target() {
    let mut app = large_navigator("latency-filter");
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.on_event(Event::Key(key(KeyCode::Char('/'))));

    let mut worst = Duration::ZERO;
    for code in "table_1".chars().map(KeyCode::Char).chain([
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Backspace,
    ]) {
        let started = Instant::now();
        app.on_event(Event::Key(key(code)));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst = worst.max(started.elapsed());
    }
    println!("20,000 tables with a filter: worst key plus frame {worst:?}");
    assert!(worst < Duration::from_millis(50), "{worst:?}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_in_a_large_navigator_stays_under_the_target() {
    let mut app = large_navigator("latency-tree");
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let press = |app: &mut sqln::app::App, code| app.on_event(Event::Key(key(code)));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Right);

    let mut worst = Duration::ZERO;
    for index in 0..200 {
        let started = Instant::now();
        press(
            &mut app,
            if index < 100 {
                KeyCode::Down
            } else {
                KeyCode::Up
            },
        );
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst = worst.max(started.elapsed());
    }
    println!("20,000 tables: worst key plus frame {worst:?}");
    assert!(worst < Duration::from_millis(50), "{worst:?}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn the_autosave_of_a_large_script_stays_under_the_target() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use sqln::app::{App, Options};
    use sqln::clipboard::MemoryClipboard;
    use sqln::store::{Root, Store};

    let base = common::temp_dir("latency-autosave");
    let db = common::temp_db("latency-autosave");
    rusqlite::Connection::open(&db).unwrap();
    let global = Root::global(base.join("global"));
    std::fs::create_dir_all(&global.dir).unwrap();
    std::fs::write(
        global.config_file(),
        format!(
            r#"{{"version":1,"connections":[{{"id":"g","name":"big","kind":"sqlite","file":"{}"}}]}}"#,
            db.display()
        ),
    )
    .unwrap();
    let store = Store {
        global,
        project: None,
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let options = Options {
        store: Some(store),
        ..Options::default()
    };
    let mut app = App::new(options, clipboard, Theme::ansi());
    let row = "INSERT INTO orders (id, name, total) VALUES (1, 'a text value', 10.50);\n";
    app.on_event(Event::Paste(row.repeat(50_000)));
    // The last step requests the write without waiting for the disk.
    let start = Instant::now();
    app.tick_at(start);
    let write = Instant::now();
    app.tick_at(start + Duration::from_millis(600));
    let spent = write.elapsed();
    println!("the autosave request for 50,000 lines took {spent:?}");
    assert!(spent < Duration::from_millis(50), "{spent:?}");
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.has_pending_saves() {
        app.drain_events();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn binary_previews_stay_under_the_target_after_classification() {
    use sqln::grid::Value;

    for ending in [b'x', 0, 255] {
        let mut bytes = vec![b'x'; 16 * 1024 * 1024];
        bytes.push(ending);
        let value = Value::binary(&bytes);
        let started = Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(value.preview(40));
        }
        let spent = started.elapsed();
        println!("1,000 binary previews: {spent:?}");
        assert!(spent < Duration::from_millis(50), "{spent:?}");
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_search_slice_in_one_large_cell_stays_under_the_target() {
    use sqln::grid::{ResultTable, RowSearch, Value};

    let mut table = ResultTable::new(vec!["value".into()]);
    table.append(
        vec![[Value::Text(&"a".repeat(50_000_000))].into_iter().collect()],
        0,
    );
    let mut search = RowSearch::new((0, 0));
    search.push_str("aaaaab");
    let started = Instant::now();
    search.scan(&table, started + Duration::from_millis(8));
    let spent = started.elapsed();
    assert!(search.has_work(&table));
    assert!(spent < Duration::from_millis(50), "{spent:?}");
}

/// A schema of `count` tables. Each table after the first has one or two keys to tables
/// before it, from a fixed sequence.
fn large_diagram(count: usize) -> sqln::diagram::Model {
    use sqln::catalog::RelationRef;
    use sqln::diagram::{ColumnRow, Focus, KeyRow, Model};

    let relation = |index: usize| RelationRef {
        schema: "app".to_owned(),
        relation: format!("table_{index:04}"),
    };
    let column = |index: usize, name: &str, primary: Option<u32>| ColumnRow {
        relation: relation(index),
        name: name.to_owned(),
        ty: "integer".to_owned(),
        nullable: primary.is_none(),
        primary,
    };
    let mut columns = Vec::new();
    let mut keys = Vec::new();
    for index in 0..count {
        columns.push(column(index, "id", Some(1)));
        for name in ["name", "created_at", "updated_at", "status"] {
            columns.push(column(index, name, None));
        }
        let seed = (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let links = if index == 0 {
            0
        } else {
            1 + usize::from(seed.is_multiple_of(5))
        };
        for place in 0..links {
            let target = (seed >> (8 * place)) as usize % index;
            let name = format!("ref_{place}");
            columns.push(column(index, &name, None));
            keys.push(KeyRow {
                relation: relation(index),
                key: name.clone(),
                column: name,
                target: relation(target),
                target_column: Some("id".to_owned()),
            });
        }
    }
    Model::build(columns, keys, &Focus::Schema("app".to_owned()))
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn the_layout_and_the_frame_of_a_large_diagram_stay_under_the_target() {
    use sqln::diagram::Focus;
    use sqln::diagram::tab::DiagramTab;
    use sqln::ui::diagram::{DiagramBar, draw_diagram};

    let mut tab = DiagramTab::new(1, 1, Focus::Schema("app".to_owned()), 1);
    tab.model = large_diagram(500);
    let theme = Theme::ansi();
    let bar = DiagramBar {
        connection: "server",
        title: true,
    };
    let mut terminal = Terminal::new(TestBackend::new(200, 60)).unwrap();
    let mut worst = Duration::ZERO;
    for _ in 0..6 {
        let started = Instant::now();
        // The key of "keys only" places the boxes again, as the end of a read does.
        tab.toggle_keys_only();
        terminal
            .draw(|frame| {
                draw_diagram(frame, frame.area(), &mut tab, &bar, true, &theme);
            })
            .unwrap();
        worst = worst.max(started.elapsed());
    }
    let links = tab.model.links.len();
    println!("500 tables, {links} keys: worst layout plus frame {worst:?}");
    assert!((550..=650).contains(&links), "{links}");
    assert_eq!(tab.layout.conflicts, 0);
    assert!(worst < TARGET, "{worst:?}");
}
