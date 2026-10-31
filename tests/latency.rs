// The target is under 50 ms per key. Run it with `cargo test --release --test latency`.

mod common;

use std::time::{Duration, Instant};

use common::{Fixture, key};
use crossterm::event::KeyCode;
use edtui::Index2;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::ui::draw_editor;
use sqln::ui::theme::Theme;

fn worst_key_and_frame(lines: usize) -> Duration {
    let row = "INSERT INTO orders (id, name, total) VALUES (1, 'a text value', 10.50);\n";
    let mut f = Fixture::conventional(&row.repeat(lines));
    f.editor.set_cursor(Index2::new(lines / 2, 0));
    let theme = Theme::ansi();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut worst = Duration::ZERO;
    for index in 0..60 {
        let started = Instant::now();
        let code = if index % 6 == 5 { KeyCode::Backspace } else { KeyCode::Char('s') };
        f.press(key(code));
        terminal.draw(|frame| draw_editor(frame, frame.area(), &mut f.editor, true, &theme)).unwrap();
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
        assert!(worst < Duration::from_millis(50), "{lines} lines: {worst:?}");
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the target applies to an optimized build")]
fn a_key_stays_under_the_target_with_the_full_budget_loaded() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crossterm::event::{Event, KeyEvent, KeyModifiers};
    use sqln::app::{App, Options};
    use sqln::clipboard::MemoryClipboard;
    use sqln::exec::Settings;

    let path = common::temp_db("latency-budget");
    rusqlite::Connection::open(&path).unwrap();
    let settings = Settings { row_cap: usize::MAX, sql_timeout: None, ..Settings::default() };
    let options = Options { database: Some(path), settings, ..Options::default() };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::ansi());
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

    app.on_event(Event::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)));
    app.on_event(Event::Key(key(KeyCode::Char('j'))));
    let mut worst = Duration::ZERO;
    for index in 0..200 {
        let started = Instant::now();
        let code = if index % 2 == 0 { KeyCode::Down } else { KeyCode::Right };
        app.on_event(Event::Key(key(code)));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        worst = worst.max(started.elapsed());
    }
    let started_copy = Instant::now();
    app.on_event(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::ALT)));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    println!("worst key plus frame with the full budget {worst:?}; one more {:?}", started_copy.elapsed());
    assert!(worst < Duration::from_millis(50), "{worst:?}");
    assert!(worst_frame < Duration::from_millis(50), "{worst_frame:?}");

    // The search reads each row, so it must leave the keys free during that work.
    app.on_event(Event::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)));
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
fn a_key_in_a_large_navigator_stays_under_the_target() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crossterm::event::{Event, KeyEvent, KeyModifiers};
    use sqln::app::{App, Options};
    use sqln::clipboard::MemoryClipboard;

    let path = common::temp_db("latency-tree");
    let mut setup = String::from("BEGIN;");
    for index in 0..20_000 {
        setup.push_str(&format!("CREATE TABLE table_{index}(a, b, c);"));
    }
    setup.push_str("COMMIT;");
    rusqlite::Connection::open(&path).unwrap().execute_batch(&setup).unwrap();
    let options = Options { database: Some(path), ..Options::default() };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::ansi());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let press = |app: &mut App, code| app.on_event(Event::Key(key(code)));

    app.on_event(Event::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)));
    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Right);
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.connections()[0].catalog.is_none() {
        app.drain_events();
        assert!(Instant::now() < deadline);
    }
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Right);

    let mut worst = Duration::ZERO;
    for index in 0..200 {
        let started = Instant::now();
        press(&mut app, if index < 100 { KeyCode::Down } else { KeyCode::Up });
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

    use crossterm::event::Event;
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
    let store = Store { global, project: None };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let options = Options { store: Some(store), ..Options::default() };
    let mut app = App::new(options, clipboard, Theme::ansi());
    let row = "INSERT INTO orders (id, name, total) VALUES (1, 'a text value', 10.50);\n";
    app.on_event(Event::Paste(row.repeat(50_000)));
    // The first steps see the edit. The last one writes the 3.6 MiB text.
    let start = Instant::now();
    app.tick_at(start);
    let write = Instant::now();
    app.tick_at(start + Duration::from_millis(600));
    let spent = write.elapsed();
    println!("the autosave of 50,000 lines took {spent:?}");
    assert!(spent < Duration::from_millis(50), "{spent:?}");
}
