// The home screen: the screen of the application without a tab.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, prefix_page, temp_db, temp_dir};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::navigator::RowKind;
use sqln::store::{Root, Store};
use sqln::ui::text;
use sqln::ui::theme::Theme;
use sqln::ui::welcome::{Welcome, draw_welcome};

struct Fixture {
    app: App,
}

impl Fixture {
    fn with(options: Options) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
        }
    }

    /// Starts as the program does, without a connection.
    fn start() -> Self {
        Self::with(Options {
            start_home: true,
            ..Options::default()
        })
    }

    /// Starts with a SQLite file that has one table.
    fn sqlite(name: &str) -> Self {
        Self::with(Options {
            database: Some(database(name)),
            start_home: true,
            ..Options::default()
        })
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

    /// Returns the tab row, the first row of the screen.
    fn tab_row(&mut self) -> String {
        self.screen().lines().next().unwrap_or_default().to_owned()
    }

    fn pump_until(&mut self, condition: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !condition(self) {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the condition did not happen");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn database(name: &str) -> PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY);")
        .unwrap();
    path
}

#[test]
fn a_start_without_a_connection_shows_the_home_screen() {
    let mut f = Fixture::start();
    assert!(f.app.home());
    assert!(f.app.navigator_focused(), "the explorer has the focus");
    let screen = f.screen();
    for part in [
        text::HOME_LOGO[4].trim(),
        text::HOME_VERSION,
        text::WELCOME_NO_CONNECTION,
        "Ctrl+W h a  Add a connection",
        "Ctrl+W h s  Scripts without a connection",
        "Ctrl+W ,  Open the settings",
        "Ctrl+W q  Quit SQLN",
    ] {
        assert!(screen.contains(part), "{part}:\n{screen}");
    }
    assert!(!screen.contains(text::HOME_CONSOLE), "{screen}");
    assert!(!f.tab_row().contains("console-"), "{screen}");
}

#[test]
fn the_home_screen_explains_the_connection_requirement_for_sql_files() {
    let mut f = Fixture::start();
    let screen = f.screen();
    assert!(screen.contains("Connect to a database"), "{screen}");
    assert!(
        screen.contains("Add a connection before opening SQL files"),
        "{screen}"
    );
    assert!(screen.contains("Open saved work"), "{screen}");
    assert!(f.app.console().results.is_empty());
}

#[test]
fn the_home_screen_offers_sql_files_and_saved_consoles_with_a_connection() {
    let mut f = Fixture::sqlite("home-tasks");
    f.prefix('c');
    let screen = f.screen();
    for part in [
        "Connect to a database",
        "Open a SQL file",
        "Open saved work",
        "Saved consoles of selected connection",
    ] {
        assert!(screen.contains(part), "{part}:\n{screen}");
    }
    assert!(screen.contains("Ctrl+P"), "{screen}");
    assert!(f.app.console().results.is_empty());
}

#[test]
fn home_tasks_fit_the_minimum_size_and_keep_custom_keys_in_both_modes() {
    use sqln::editor::KeyboardMode;
    use sqln::input::Keymap;
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let overrides = [
            ("openSqlFile".to_owned(), vec!["f8".to_owned()]),
            ("newConsole".to_owned(), vec!["f9".to_owned()]),
        ]
        .into_iter()
        .collect();
        let mut f = Fixture::with(Options {
            database: Some(database("home-custom-tasks")),
            keyboard: mode,
            keymap: Some(Keymap::with_overrides(mode, &overrides).unwrap()),
            ..Options::default()
        });
        f.prefix('c');
        for (width, height) in [(80, 24), (120, 35), (80, 24)] {
            f.app.on_event(Event::Resize(width, height));
            let screen = f.screen_at(width, height);
            for part in [
                "F8  Open a SQL file",
                "F9  Saved consoles of selected connection",
                "Scripts without a connection",
                "Open saved work",
            ] {
                assert!(screen.contains(part), "{part}:\n{screen}");
            }
            assert!(f.app.home());
        }
        f.code(KeyCode::F(8));
        assert!(f.screen().contains(text::TITLE_BROWSER));
        f.code(KeyCode::Esc);
        assert!(f.app.home());
        assert!(f.app.console().results.is_empty());
    }
}

#[test]
fn a_removed_home_task_shortcut_uses_the_palette_command_label() {
    use sqln::editor::KeyboardMode;
    use sqln::input::Keymap;
    let overrides = [("newConsole".to_owned(), Vec::new())]
        .into_iter()
        .collect();
    let mut f = Fixture::with(Options {
        database: Some(database("home-removed-task-key")),
        keymap: Some(Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap()),
        ..Options::default()
    });
    f.prefix('c');
    let screen = f.screen_at(80, 24);
    assert!(
        screen.contains("Ctrl+W h Ctrl+P →  New console"),
        "{screen}"
    );
    assert!(!screen.contains("Ctrl+W h n"), "{screen}");
}

#[test]
fn home_tasks_and_the_sql_file_browser_open_no_database_session() {
    use crate::common::mock::MockConnector;
    use sqln::sql::Dialect;
    use std::sync::Arc;
    let connector = MockConnector::new(
        Dialect::Postgres,
        Arc::new(|_, _| Box::pin(async { panic!("the task screen must not execute SQL") })),
    );
    let overrides = [("openSqlFile".to_owned(), vec!["f8".to_owned()])]
        .into_iter()
        .collect();
    let mut f = Fixture::with(Options {
        connector: Some(Arc::new(connector.clone())),
        keymap: Some(
            sqln::input::Keymap::with_overrides(
                sqln::editor::KeyboardMode::Conventional,
                &overrides,
            )
            .unwrap(),
        ),
        ..Options::default()
    });
    f.prefix('c');
    f.screen_at(80, 24);
    f.code(KeyCode::F(8));
    assert!(f.screen().contains(text::TITLE_BROWSER));
    f.code(KeyCode::Esc);
    assert!(f.app.home());
    assert_eq!(connector.opened(), 0);
    assert!(connector.statements().is_empty());
}

#[test]
fn home_tasks_keep_multiple_connections_and_cancel_the_file_connection_choice() {
    use sqln::connection::Definition;
    use sqln::connector::sqlite::SqliteTarget;
    let other = database("home-other-connection");
    let overrides = [("openSqlFile".to_owned(), vec!["f8".to_owned()])]
        .into_iter()
        .collect();
    let mut f = Fixture::with(Options {
        database: Some(database("home-first-connection")),
        connections: vec![Definition::sqlite(SqliteTarget::new(&other))],
        keymap: Some(
            sqln::input::Keymap::with_overrides(
                sqln::editor::KeyboardMode::Conventional,
                &overrides,
            )
            .unwrap(),
        ),
        ..Options::default()
    });
    f.prefix('c');
    let screen = f.screen_at(80, 24);
    assert!(screen.contains("Open saved work"), "{screen}");
    assert!(screen.contains("F8  Open a SQL file"), "{screen}");
    f.code(KeyCode::F(8));
    let screen = f.screen();
    assert!(screen.contains(text::TITLE_SQL_FILE), "{screen}");
    assert!(screen.contains("home-other-connection"), "{screen}");
    f.code(KeyCode::Esc);
    assert!(f.app.home());
    assert_eq!(f.app.connections().len(), 2);
    assert!(f.app.console().results.is_empty());
}

#[test]
fn the_home_screen_takes_no_text() {
    let mut f = Fixture::start();
    f.prefix('l');
    f.code(KeyCode::Char('x'));
    f.app.on_event(Event::Paste("SELECT 1".to_owned()));
    // A click on the middle of the screen finds no editor.
    f.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 70,
        row: 15,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(f.app.home());
    assert!(f.app.navigator_focused());
    assert_eq!(f.app.console().editor.text(), "");
}

#[test]
fn the_explorer_adds_a_connection_from_the_home_screen() {
    let mut f = Fixture::start();
    f.code(KeyCode::Char('a'));
    assert!(f.app.form().is_some());
}

#[test]
fn a_start_with_a_connection_keeps_its_console() {
    let mut f = Fixture::sqlite("home-start-connection");
    assert!(!f.app.home());
    assert!(f.app.console().connection.is_some());
    assert!(f.tab_row().contains("console-1"));
}

#[test]
fn a_start_with_a_file_of_the_command_line_keeps_its_console() {
    let mut f = Fixture::with(Options {
        database: Some(temp_db("home-start-missing")),
        session_target: true,
        start_home: true,
        ..Options::default()
    });
    assert!(
        !f.app.home(),
        "the console waits for the answer of the dialog"
    );
    assert!(f.screen().contains("console-1"));
}

#[test]
fn the_quit_button_stays_on_the_home_screen() {
    let mut f = Fixture::start();
    assert!(f.app.home());
    let row = f.tab_row();
    let byte = row
        .find("[Quit]")
        .unwrap_or_else(|| panic!("no button: {row}"));
    let column = u16::try_from(row[..byte].chars().count()).unwrap();
    f.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(f.app.should_quit());
}

#[test]
fn a_start_without_the_option_keeps_the_empty_console() {
    let f = Fixture::with(Options::default());
    assert!(!f.app.home());
}

#[test]
fn closing_the_last_console_shows_the_home_screen() {
    let mut f = Fixture::with(Options::default());
    f.prefix('c');
    assert!(!f.app.should_quit());
    assert!(f.app.home());
    assert!(f.app.navigator_focused());
    assert!(!f.tab_row().contains("console-"));
    f.prefix('q');
    assert!(f.app.should_quit());
}

#[test]
fn closing_the_last_table_shows_the_home_screen() {
    let mut f = Fixture::sqlite("home-last-table");
    crate::common::open_table(&mut f.app, 0, "t");
    f.pump_until(|f| f.app.table().is_some_and(|tab| !tab.is_busy()));
    f.prefix('p');
    assert!(f.app.table().is_none());
    f.prefix('c');
    assert!(f.app.table().is_some(), "the table takes the screen");
    f.prefix('c');
    assert!(!f.app.should_quit());
    assert!(f.app.home());
    assert!(f.app.tables().is_empty());
}

#[test]
fn closing_the_last_diagram_shows_the_home_screen() {
    let mut f = Fixture::sqlite("home-last-diagram");
    crate::common::select_row(&mut f.app, 0, &[RowKind::Schema], "main");
    f.code(KeyCode::Char('d'));
    f.pump_until(|f| f.app.diagram().is_some_and(|tab| !tab.is_busy()));
    f.prefix('p');
    f.prefix('c');
    assert!(f.app.diagram().is_some(), "the diagram takes the screen");
    f.prefix('c');
    assert!(!f.app.should_quit());
    assert!(f.app.home());
    assert!(f.app.diagrams().is_empty());
}

#[test]
fn the_keys_of_a_tab_do_nothing_on_the_home_screen() {
    let mut f = Fixture::with(Options::default());
    f.prefix('c');
    assert!(f.app.home());
    for c in ['c', 'n', 'p', 'l', 'j', 'r', 'k'] {
        f.prefix(c);
        assert!(f.app.home(), "Ctrl+W {c}");
        assert!(f.app.navigator_focused(), "Ctrl+W {c}");
    }
    assert!(!f.app.should_quit());
    assert_eq!(f.app.consoles().len(), 1);
    let page = prefix_page(&f.app);
    assert!(!page.iter().any(|row| row.contains("Close")), "{page:?}");
    assert!(page.iter().any(|row| row.contains("Quit")), "{page:?}");
}

#[test]
fn the_palette_of_the_home_screen_lists_no_command_of_a_tab() {
    let mut f = Fixture::with(Options::default());
    f.prefix('c');
    f.press(ctrl('p'));
    for c in "close".chars() {
        f.code(KeyCode::Char(c));
    }
    let screen = f.screen();
    assert!(!screen.contains("Close the tab"), "{screen}");
    f.code(KeyCode::Esc);
    f.press(ctrl('p'));
    for c in "quit".chars() {
        f.code(KeyCode::Char(c));
    }
    let screen = f.screen();
    assert!(screen.contains("Quit"), "{screen}");
}

#[test]
fn a_new_console_of_the_explorer_ends_the_home_screen() {
    let mut f = Fixture::sqlite("home-new-console");
    f.prefix('c');
    assert!(f.app.home());
    let screen = f.screen();
    assert!(screen.contains(text::HOME_NO_CONSOLE), "{screen}");
    assert!(
        screen.contains("Ctrl+W h n  Saved consoles of selected connection"),
        "{screen}"
    );
    // The connection is the first row of the explorer.
    for _ in 0..10 {
        f.code(KeyCode::Up);
    }
    f.code(KeyCode::Char('n'));
    assert!(!f.app.home());
    assert_eq!(
        f.app.consoles().len(),
        1,
        "the hidden console takes the tab"
    );
    assert!(f.app.console().connection.is_some());
    assert!(f.tab_row().contains("console-"));
}

#[test]
fn the_home_screen_names_the_explorer_when_it_is_hidden() {
    let mut f = Fixture::start();
    f.prefix('b');
    assert!(!f.app.navigator_focused());
    let screen = f.screen();
    assert!(screen.contains("Ctrl+W b  Show the explorer"), "{screen}");
    assert!(f.app.hint().contains("explorer"), "{}", f.app.hint());
    assert!(screen.contains(" HOME "), "{screen}");
    f.prefix('b');
    assert!(
        f.app.navigator_focused(),
        "the explorer takes the focus again"
    );
}

#[test]
fn a_small_area_shows_the_name_instead_of_the_logo() {
    let welcome = Welcome {
        logo: true,
        note: text::WELCOME_NO_CONNECTION,
        rows: vec![("Ctrl+W q".to_owned(), text::HOME_QUIT)],
    };
    let draw = |height| {
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        terminal
            .draw(|frame| {
                draw_welcome(
                    frame,
                    Rect::new(0, 0, 60, height),
                    &welcome,
                    &Theme::plain(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n")
            .collect::<String>()
    };
    let tall = draw(20);
    assert!(tall.contains(text::HOME_LOGO[4].trim()), "{tall}");
    let short = draw(8);
    assert!(!short.contains(text::HOME_LOGO[4].trim()), "{short}");
    assert!(short.contains(&text::welcome_title()), "{short}");
    assert!(short.contains("Quit SQLN"), "{short}");
}

#[test]
fn a_quit_on_the_home_screen_starts_there_again() {
    let base = temp_dir("home-session");
    let store = Store {
        global: Root::global(base.join("global")),
        project: None,
    };
    let options = || Options {
        store: Some(store.clone()),
        start_home: true,
        ..Options::default()
    };
    let mut f = Fixture::with(options());
    assert!(f.app.home());
    f.prefix('q');
    assert!(f.app.should_quit());
    f.app.shutdown();
    let f = Fixture::with(options());
    assert!(f.app.home());
}
