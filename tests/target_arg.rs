// The target of the command line: a file or a URI that lasts for one session.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{key, temp_db, temp_dir};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::{Definition, Source};
use sqln::connector::target::{Engine, OpenTarget, open_target, parse_uri};
use sqln::store::workspace::{self, OpenTab, OpenTabs};
use sqln::store::{Root, Scope, Store};
use sqln::ui::theme::Theme;

fn sqln(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sqln"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn app_with(options: Options) -> App {
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    App::new(options, clipboard, Theme::plain())
}

fn press(app: &mut App, code: KeyCode) {
    app.on_event(Event::Key(key(code)));
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let rows = terminal.backend().buffer().content().chunks(100);
    rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A global folder whose configuration names one saved SQLite connection.
fn store_with_saved_connection(name: &str, saved: &Path) -> (Store, Root) {
    let base = temp_dir(name);
    let global = Root::global(base.join("global"));
    fs::create_dir_all(&global.dir).unwrap();
    let text = format!(
        r#"{{"version":1,"preferences":{{}},"connections":[{{"id":"saved","name":"saved","kind":"sqlite","file":"{}"}}]}}"#,
        saved.display()
    );
    fs::write(global.config_file(), text).unwrap();
    let root = global.clone();
    (
        Store {
            global,
            project: None,
        },
        root,
    )
}

fn run_until_idle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.console().is_busy() {
        app.drain_events();
        assert!(Instant::now() < deadline, "the run did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    app.drain_events();
}

#[test]
fn a_text_without_a_scheme_is_a_file() {
    assert_eq!(
        open_target("data/app.db"),
        Ok(OpenTarget::File(PathBuf::from("data/app.db")))
    );
}

#[test]
fn a_text_with_a_scheme_is_a_uri() {
    let Ok(OpenTarget::Network(target)) = open_target("postgres://ana@db.example/app") else {
        panic!("expected a network target");
    };
    assert_eq!(target.engine, Engine::Postgres);
    assert_eq!(target.user, "ana");
    assert_eq!(target.password, None);
}

#[test]
fn a_uri_with_an_unknown_scheme_stops_the_start() {
    let output = sqln(&["oracle://host/name"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_of(&output).contains("The scheme must be"));
}

#[test]
fn a_second_target_is_refused() {
    let output = sqln(&["a.db", "b.db"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("Give one file or one URI."));
}

#[test]
fn an_unknown_option_is_still_an_error() {
    let output = sqln(&["--nothing"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("Unknown option: --nothing"));
}

#[test]
fn the_help_names_the_target() {
    let output = sqln(&["--help"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Usage: sqln [options] [target]"));
}

#[test]
fn the_first_console_uses_the_target_and_not_the_saved_connection() {
    let saved = temp_db("target-saved.db");
    let target = temp_db("target-file.db");
    fs::write(&saved, "").unwrap();
    fs::write(&target, "").unwrap();
    let (store, _) = store_with_saved_connection("target-first", &saved);
    let app = app_with(Options {
        database: Some(target),
        session_target: true,
        store: Some(store),
        ..Options::default()
    });
    let id = app.console().connection.unwrap();
    assert_eq!(app.connections()[0].id, id);
    assert_eq!(app.connections()[0].key, None);
    assert_eq!(app.connections().len(), 2);
}

#[test]
fn a_network_target_needs_no_saved_definition() {
    let target = parse_uri("postgres://ana@127.0.0.1:1/app").unwrap();
    let definition = Definition::new(target.label(), Source::Network(target));
    let app = app_with(Options {
        connections: vec![definition],
        session_target: true,
        ..Options::default()
    });
    assert_eq!(app.connections()[0].name, "127.0.0.1:1/app");
    assert!(app.console().connection.is_some());
}

#[test]
fn a_session_target_writes_no_file_of_the_configuration() {
    let saved = temp_db("target-keep-saved.db");
    let target = temp_db("target-keep-file.db");
    fs::write(&saved, "").unwrap();
    fs::write(&target, "").unwrap();
    let (store, global) = store_with_saved_connection("target-keep", &saved);
    let config = fs::read(global.config_file()).unwrap();
    let tabs = OpenTabs {
        tabs: vec![OpenTab::Console {
            scope: Scope::Global,
            id: "old".to_owned(),
        }],
        active: Some(0),
    };
    let saved_tabs = tabs.clone();
    workspace::update(&global, |workspace| workspace.open_tabs = Some(saved_tabs)).unwrap();

    let mut app = app_with(Options {
        database: Some(target),
        session_target: true,
        store: Some(store),
        ..Options::default()
    });
    app.on_event(Event::Paste("SELECT 1;".to_owned()));
    press(&mut app, KeyCode::F(5));
    run_until_idle(&mut app);
    app.shutdown();

    assert_eq!(fs::read(global.config_file()).unwrap(), config);
    assert_eq!(workspace::load(&global).open_tabs, Some(tabs));
    assert!(!global.consoles_dir().exists());
}

#[test]
fn a_session_target_offers_no_project() {
    let target = temp_db("target-offer.db");
    fs::write(&target, "").unwrap();
    let base = temp_dir("target-offer");
    let mut app = app_with(Options {
        database: Some(target),
        session_target: true,
        offer_project: Some(base.clone()),
        store: Some(Store {
            global: Root::global(base.join("global")),
            project: None,
        }),
        ..Options::default()
    });
    assert!(!screen(&mut app).contains("Create project"));
}

#[test]
fn a_missing_file_asks_first_and_then_binds_the_console() {
    let saved = temp_db("target-missing-saved.db");
    fs::write(&saved, "").unwrap();
    let target = temp_dir("target-missing").join("new.db");
    let (store, _) = store_with_saved_connection("target-missing-store", &saved);
    let mut app = app_with(Options {
        database: Some(target.clone()),
        session_target: true,
        store: Some(store),
        ..Options::default()
    });
    assert_eq!(app.console().connection, None);
    assert!(screen(&mut app).contains("Create database"));

    // The first option is Cancel, and the second one creates the file.
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Enter);

    let id = app.console().connection.unwrap();
    let connection = app.connections().iter().find(|c| c.id == id).unwrap();
    assert_eq!(connection.key, None);
    assert!(connection.name.contains("new.db"));
}

#[test]
fn a_memory_target_keeps_one_database_across_statements() {
    let mut app = app_with(Options {
        database: Some(PathBuf::from(":memory:")),
        session_target: true,
        ..Options::default()
    });
    assert!(app.console().connection.is_some());
    let connection = &app.connections()[0];
    assert_eq!(connection.name, ":memory:");
    assert!(connection.capabilities.single_session);
    app.on_event(Event::Paste(
        "CREATE TABLE demo (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO demo VALUES (1, 'kept');"
            .to_owned(),
    ));
    press(&mut app, KeyCode::F(5));
    run_until_idle(&mut app);
    // The second execution reads what the first wrote: one session holds the database.
    app.on_event(Event::Paste("SELECT value FROM demo".to_owned()));
    press(&mut app, KeyCode::F(5));
    run_until_idle(&mut app);
    assert!(screen(&mut app).contains("kept"));
}
