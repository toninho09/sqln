// The age of an open transaction in the status line.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connector::TransactionState;
use sqln::console::Phase;
use sqln::ui::statusline::{StatusData, StatusPanel, draw_statusline};
use sqln::ui::text;
use sqln::ui::theme::Theme;

const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn the_age_has_whole_minutes_and_no_text_below_one_minute() {
    let age = |seconds| text::transaction_age(Duration::from_secs(seconds));
    assert_eq!(age(0), None);
    assert_eq!(age(59), None);
    assert_eq!(age(60).as_deref(), Some("1m"));
    assert_eq!(age(3599).as_deref(), Some("59m"));
    assert_eq!(age(3600).as_deref(), Some("1h 00m"));
    assert_eq!(age(3900).as_deref(), Some("1h 05m"));
}

fn screen(app: &mut App, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let rows = terminal
        .backend()
        .buffer()
        .content()
        .chunks(usize::from(width));
    rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn app_in_transaction(name: &str) -> App {
    let path = temp_db(name);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE t (a INT)")
        .unwrap();
    let options = Options {
        database: Some(path),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::plain());
    run(&mut app, "BEGIN");
    app
}

fn run(app: &mut App, sql: &str) {
    app.on_event(Event::Key(ctrl('a')));
    app.on_event(Event::Paste(sql.to_owned()));
    app.on_event(Event::Key(key(KeyCode::F(5))));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.console().is_busy() {
        app.drain_events();
        assert!(Instant::now() < deadline, "the run did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    app.drain_events();
}

#[test]
fn an_open_transaction_shows_its_age_and_asks_for_a_frame_when_the_minute_changes() {
    let mut app = app_in_transaction("age-open.db");
    assert!(screen(&mut app, 100).contains("Transaction open"));
    assert!(!screen(&mut app, 100).contains("Transaction open 1m"));

    let now = Instant::now();
    // Finish syntax analysis before checking which clock changes require a frame.
    let deadline = now + Duration::from_secs(10);
    app.tick_at(now);
    while app.console().editor.syntax_report().is_none() {
        app.tick_at(now + Duration::from_secs(1));
        assert!(Instant::now() < deadline, "syntax analysis did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.tick_at(now + 3 * MINUTE));
    assert!(screen(&mut app, 100).contains("Transaction open 3m"));
    assert!(!app.tick_at(now + 3 * MINUTE));
}

#[test]
fn the_age_fits_the_smallest_screen() {
    let mut app = app_in_transaction("age-small.db");
    app.tick_at(Instant::now() + 65 * MINUTE);
    assert!(screen(&mut app, 80).contains("Transaction open 1h 05m"));
}

#[test]
fn the_end_of_the_transaction_ends_its_age() {
    let mut app = app_in_transaction("age-end.db");
    assert!(app.console().transaction_age(Instant::now()).is_some());
    run(&mut app, "COMMIT");
    assert_eq!(app.console().transaction_age(Instant::now()), None);
    assert!(screen(&mut app, 100).contains("No transaction"));
}

#[test]
fn a_failed_transaction_keeps_the_start_of_its_clock() {
    let mut app = app_in_transaction("age-failed.db");
    let console = app.console_mut();
    let start = console.transaction_age(Instant::now());
    std::thread::sleep(Duration::from_millis(20));
    console.set_transaction(TransactionState::Failed);
    let age = console.transaction_age(Instant::now());
    assert!(age >= start.map(|start| start + Duration::from_millis(20)));
}

fn style_of_state(age: Option<Duration>, theme: &Theme) -> Style {
    let data = StatusData {
        panel: StatusPanel::Editor,
        connection: "c",
        production: false,
        schema: None,
        session: Some((false, TransactionState::Open)),
        transaction_age: age,
        phase: Phase::Idle,
        run_time: None,
        pool: None,
        data: (0, 1),
        position: None,
        agent: None,
        auto: None,
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
    terminal
        .draw(|frame| draw_statusline(frame, frame.area(), &data, theme))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    let start = row.find(text::TRANSACTION_OPEN).unwrap();
    buffer.content()[start].style()
}

#[test]
fn a_long_transaction_changes_to_the_error_style() {
    let theme = Theme::ansi();
    let young = style_of_state(Some(4 * MINUTE), &theme);
    let old = style_of_state(Some(5 * MINUTE), &theme);
    assert_eq!(young.fg, theme.status_warning.fg);
    assert_eq!(old.fg, theme.status_error.fg);
    assert_ne!(young.fg, old.fg);
}
