// The notices of the database in **Messages**. A fake connector sends them, so the
// tests need no server. `tests/servers.rs` reads the real notices.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::mock;
use crate::common::{ctrl, key};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connector::{ExecError, StatementEnd};
use sqln::console::MessageLevel;
use sqln::grid::Value;
use sqln::sql::Dialect;
use sqln::ui::theme::Theme;

type RunHandler = mock::RunHandler;

/// Answers what the statement text chooses.
fn fake() -> Arc<mock::MockConnector> {
    let run: RunHandler = Arc::new(|sql, mut sink| {
        Box::pin(async move {
            match sql.as_str() {
                "read" => {
                    sink.set_columns(vec!["a".to_owned()]).await;
                    sink.push([Value::Null].into_iter().collect()).await;
                    let texts = vec!["WARNING: one".to_owned(), "NOTICE: two".to_owned()];
                    sink.notices(texts, 3).await;
                    let (rows, limit) = sink.finish().await;
                    Ok(StatementEnd {
                        rows,
                        has_columns: true,
                        limit,
                        ..StatementEnd::default()
                    })
                }
                "fail" => {
                    sink.notices(vec!["NOTICE: before".to_owned()], 0).await;
                    sink.finish().await;
                    Err(ExecError::new("boom"))
                }
                _ => {
                    sink.finish().await;
                    Ok(StatementEnd::default())
                }
            }
        })
    });
    Arc::new(mock::MockConnector::new(Dialect::Postgres, run))
}

fn app() -> App {
    let options = Options {
        connector: Some(fake()),
        label: Some("fake".to_owned()),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::plain())
}

fn run(app: &mut App, sql: &str) {
    app.on_event(Event::Key(ctrl('a')));
    app.on_event(Event::Paste(sql.to_owned()));
    app.on_event(Event::Key(key(KeyCode::F(5))));
    let deadline = Instant::now() + Duration::from_secs(10);
    app.drain_events();
    while app.console().is_busy() {
        app.drain_events();
        assert!(Instant::now() < deadline, "the run did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    app.drain_events();
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let rows = terminal.backend().buffer().content().chunks(100);
    rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_notice_goes_to_messages_with_the_number_of_its_statement() {
    let mut app = app();
    run(&mut app, "read");
    let texts: Vec<&str> = app
        .console()
        .messages
        .iter()
        .filter(|message| message.level == MessageLevel::Warning)
        .map(|message| message.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "Statement 1: WARNING: one",
            "Statement 1: NOTICE: two",
            "Statement 1: 3 more notices.",
        ]
    );
}

#[test]
fn the_grid_stays_on_screen_and_the_tab_counts_the_warnings() {
    let mut app = app();
    run(&mut app, "read");
    assert!(!app.console().showing_messages());
    let screen = screen(&mut app);
    assert!(screen.contains("Messages (3)"), "{screen}");
}

#[test]
fn a_notice_before_an_error_stays() {
    let mut app = app();
    run(&mut app, "fail");
    let messages = &app.console().messages;
    assert_eq!(messages[0].level, MessageLevel::Warning);
    assert!(messages[0].text.contains("NOTICE: before"));
    assert_eq!(messages[1].level, MessageLevel::Error);
    assert!(messages[1].text.contains("boom"));
}

#[test]
fn an_added_limit_goes_to_messages_and_the_tab_counts_no_warning() {
    let mut app = app();
    run(&mut app, "SELECT 1");
    let messages = &app.console().messages;
    assert_eq!(messages[0].level, MessageLevel::Info);
    assert_eq!(
        messages[0].text,
        "Statement 1: SQLN added LIMIT 1001 to this statement."
    );
    assert_eq!(app.console().warnings(), 0);
}

#[test]
fn a_new_run_clears_the_number_of_the_tab() {
    let mut app = app();
    run(&mut app, "read");
    run(&mut app, "quiet");
    assert_eq!(app.console().warnings(), 0);
    let screen = screen(&mut app);
    assert!(!screen.contains("Messages ("), "{screen}");
    assert!(screen.contains("Messages"), "{screen}");
}
