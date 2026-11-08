// The auto-refresh of the tab on screen: a console repeats its last run, and a table
// tab reads its page again.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

mod common;

use crossterm::event::{Event, KeyCode};
use ratatui::{Terminal, backend::TestBackend};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::ui::text;
use sqln::ui::theme::Theme;

use common::{ctrl, key, open_table, temp_db};

struct Fixture {
    app: App,
    path: std::path::PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE demo (id INTEGER PRIMARY KEY, value TEXT);
                 INSERT INTO demo VALUES (1, 'first');",
            )
            .unwrap();
        let app = App::new(
            Options {
                connections: vec![Definition::sqlite(SqliteTarget::new(&path))],
                ..Options::default()
            },
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
        Self { app, path }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    /// Writes the text and runs the whole script.
    fn run_script(&mut self, sql: &str) {
        self.app.on_event(Event::Key(ctrl('a')));
        self.app.on_event(Event::Paste(sql.to_owned()));
        self.press(KeyCode::F(5));
        self.idle();
    }

    fn idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.app.drain_events();
            if !self.app.console().is_busy() && self.app.table().is_none_or(|tab| !tab.is_busy()) {
                break;
            }
            assert!(Instant::now() < deadline, "{}", self.screen(120, 30));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Arms the auto-refresh through the palette and the dialog, choosing the interval
    /// at `index` of the choices.
    fn arm(&mut self, index: usize) {
        self.app.on_event(Event::Key(ctrl('p')));
        self.app.on_event(Event::Paste("autorefresh".to_owned()));
        self.press(KeyCode::Enter);
        let screen = self.screen(120, 30);
        assert!(
            screen.contains(text::AUTO_REFRESH_TITLE),
            "the dialog did not open:\n{screen}"
        );
        for _ in 0..index {
            self.press(KeyCode::Right);
        }
        self.press(KeyCode::Enter);
    }

    /// Moves the clock past the interval and lets the tick fire.
    fn tick_past(&mut self, seconds: u64) {
        self.app
            .tick_at(Instant::now() + Duration::from_secs(seconds));
        self.idle();
    }

    fn screen(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|line| line.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn insert(&self, id: i64, value: &str) {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .execute(
                "INSERT INTO demo (id, value) VALUES (?1, ?2)",
                rusqlite::params![id, value],
            )
            .unwrap();
    }
}

#[test]
fn a_console_repeats_its_last_run_and_shows_the_interval() {
    let mut f = Fixture::new("auto-console.db");
    f.run_script("SELECT value FROM demo ORDER BY id;");
    f.arm(1);
    let screen = f.screen(120, 30);
    assert!(screen.contains("auto 1s"), "{screen}");
    f.insert(2, "second");
    f.tick_past(2);
    let screen = f.screen(120, 30);
    assert!(screen.contains("second"), "{screen}");
}

#[test]
fn a_run_that_can_change_data_refuses_the_auto_refresh() {
    let mut f = Fixture::new("auto-write.db");
    f.run_script("INSERT INTO demo (id, value) VALUES (9, 'made');");
    f.arm(1);
    let screen = f.screen(120, 30);
    assert!(screen.contains(text::AUTO_REFRESH_READ_ONLY), "{screen}");
    assert!(!screen.contains("auto 1s"));
}

#[test]
fn a_failed_repetition_turns_the_auto_refresh_off() {
    let mut f = Fixture::new("auto-fail.db");
    f.run_script("SELECT value FROM demo;");
    f.arm(1);
    assert!(f.screen(120, 30).contains("auto 1s"));
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute("DROP TABLE demo", [])
        .unwrap();
    f.tick_past(2);
    let screen = f.screen(120, 30);
    assert!(screen.contains("Auto-refresh stopped"), "{screen}");
    assert!(!screen.contains("auto 1s"), "{screen}");
}

#[test]
fn a_table_tab_reads_its_page_again() {
    let mut f = Fixture::new("auto-table.db");
    open_table(&mut f.app, 0, "demo");
    f.idle();
    f.arm(1);
    let screen = f.screen(120, 30);
    assert!(screen.contains("auto 1s"), "{screen}");
    f.insert(2, "second");
    f.tick_past(2);
    let screen = f.screen(120, 30);
    assert!(screen.contains("second"), "{screen}");
}
