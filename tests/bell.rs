// The bell at the end of a long run.

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, temp_db, temp_db_path, temp_dir};
use crossterm::event::{Event, KeyCode};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::CatalogState;
use sqln::console::{RunClock, RunEnd};
use sqln::settings::{Field, SECTIONS, SettingsForm};
use sqln::store::config::{self, Preferences};
use sqln::store::{Root, Store};
use sqln::ui::theme::Theme;

const LIMIT: Duration = Duration::from_secs(10);

fn clock(end: Option<RunEnd>, last: Option<Duration>) -> RunClock {
    let mut clock = RunClock::default();
    clock.last = last;
    clock.end = end;
    clock
}

#[test]
fn a_long_run_rings_at_a_normal_end_and_after_a_failure() {
    assert!(clock(Some(RunEnd::Done), Some(LIMIT)).rings_after(LIMIT));
    assert!(clock(Some(RunEnd::Failed), Some(LIMIT * 2)).rings_after(LIMIT));
}

#[test]
fn a_stop_a_short_run_and_a_run_without_an_end_do_not_ring() {
    assert!(!clock(Some(RunEnd::Stopped), Some(LIMIT)).rings_after(LIMIT));
    assert!(!clock(Some(RunEnd::Done), Some(LIMIT / 2)).rings_after(LIMIT));
    assert!(!clock(None, Some(LIMIT)).rings_after(LIMIT));
    assert!(!clock(Some(RunEnd::Done), None).rings_after(LIMIT));
}

/// Opens a database file with a global file that holds these preferences.
fn app_with(name: &str, preferences: &str) -> App {
    let base = temp_dir(name);
    let global = Root::global(base.join("global"));
    fs::create_dir_all(&global.dir).unwrap();
    let file = temp_db(&format!("{name}.db"));
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch("CREATE TABLE t (a INT)")
        .unwrap();
    fs::write(
        global.config_file(),
        format!(r#"{{"version":1,"preferences":{preferences},"connections":[]}}"#),
    )
    .unwrap();
    let options = Options {
        store: Some(Store {
            global,
            project: None,
        }),
        database: Some(file),
        session_target: true,
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::plain())
}

fn run(app: &mut App, sql: &str) {
    app.on_event(Event::Key(ctrl('a')));
    app.on_event(Event::Paste(sql.to_owned()));
    app.on_event(Event::Key(key(KeyCode::F(5))));
    let deadline = Instant::now() + Duration::from_secs(120);
    while app.console().is_busy() {
        app.drain_events();
        assert!(Instant::now() < deadline, "the run did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    app.drain_events();
}

/// Holds the write lock of the database for `hold`, so a read of the application waits.
/// The run then takes a known time, whatever the speed of the machine.
fn hold_database(path: std::path::PathBuf, hold: Duration) -> std::thread::JoinHandle<()> {
    let (locked, ready) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let connection = rusqlite::Connection::open(path).unwrap();
        connection.busy_timeout(Duration::from_secs(5)).unwrap();
        connection.execute_batch("BEGIN EXCLUSIVE").unwrap();
        locked.send(()).unwrap();
        std::thread::sleep(hold);
        connection.execute_batch("COMMIT").unwrap();
    });
    ready
        .recv_timeout(Duration::from_secs(5))
        .expect("the lock did not start");
    handle
}

#[test]
fn the_bell_follows_the_time_of_the_option() {
    let mut app = app_with("bell-limit", r#"{"bellAfterSeconds":1}"#);
    // The first run loads the catalog, so no read of it waits behind the lock below.
    run(&mut app, "SELECT 1");
    assert!(!app.take_bell());
    let deadline = Instant::now() + LIMIT;
    while app.connections()[0].catalog_state == CatalogState::Loading {
        app.drain_events();
        assert!(Instant::now() < deadline, "the catalog did not load");
        std::thread::sleep(Duration::from_millis(1));
    }
    let holder = hold_database(temp_db_path("bell-limit.db"), Duration::from_millis(1200));
    run(&mut app, "SELECT count(*) FROM t");
    holder.join().unwrap();
    let last = app.console().run.last.expect("the run has a time");
    assert!(last >= Duration::from_secs(1), "{last:?}");
    assert!(app.take_bell(), "{last:?}");
    assert!(!app.take_bell(), "the request lasts for one bell");
}

#[test]
fn a_short_run_with_the_default_does_not_ring() {
    let mut app = app_with("bell-short", "{}");
    run(&mut app, "SELECT 1");
    assert!(!app.take_bell());
}

#[test]
fn the_value_zero_stops_the_bell() {
    let mut app = app_with("bell-off", r#"{"bellAfterSeconds":0}"#);
    run(&mut app, "SELECT 1");
    assert!(!app.take_bell());
}

#[test]
fn the_default_is_ten_seconds_and_a_project_has_priority() {
    assert_eq!(config::DEFAULT_BELL_AFTER_SECONDS, 10);
    let base = temp_dir("bell-scope");
    let project = Root::project(&base.join("project"));
    let global = Root::global(base.join("global"));
    let write = |root: &Root, value: u64| {
        fs::create_dir_all(&root.dir).unwrap();
        fs::write(
            root.config_file(),
            format!(
                r#"{{"version":1,"preferences":{{"bellAfterSeconds":{value}}},"connections":[]}}"#
            ),
        )
        .unwrap();
    };
    write(&global, 30);
    write(&project, 0);
    let store = Store {
        global,
        project: Some(project),
    };
    let loaded = config::load(&store).config.unwrap();
    assert_eq!(loaded.bell_after_seconds, 0);
}

#[test]
fn settings_shows_the_field_in_the_first_section_and_accepts_zero() {
    assert!(SECTIONS[0].fields.contains(&Field::BellAfter));
    let mut form = SettingsForm::new(Preferences::default(), None);
    assert!(form.inherited(Field::BellAfter).contains("10"));
    form.focus_on(Field::BellAfter);
    form.paste("0");
    let preferences = form.preferences().unwrap();
    assert_eq!(preferences.bell_after_seconds, Some(0));
}

#[test]
fn settings_keeps_an_empty_field_out_of_the_file() {
    let form = SettingsForm::new(Preferences::default(), None);
    let preferences = form.preferences().unwrap();
    assert_eq!(preferences.bell_after_seconds, None);
}
