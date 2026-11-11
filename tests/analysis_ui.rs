//! Result analysis commands use input events and the shared terminal widgets.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, open_table, temp_db};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::KeyboardMode;
use sqln::exec::Settings;
use sqln::input::Keymap;
use sqln::ui::theme::Theme;

struct Screen {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
    path: std::path::PathBuf,
}

impl Screen {
    fn new(
        name: &str,
        mode: KeyboardMode,
        settings: Settings,
        overrides: &[(&str, &[&str])],
    ) -> Self {
        let path = temp_db(&format!("analysis-{name}"));
        rusqlite::Connection::open(&path).unwrap();
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let overrides = overrides
            .iter()
            .map(|(name, keys)| {
                (
                    (*name).to_owned(),
                    keys.iter().map(|key| (*key).to_owned()).collect(),
                )
            })
            .collect::<HashMap<_, _>>();
        let app = App::new(
            Options {
                database: Some(path.clone()),
                keyboard: mode,
                settings,
                keymap: Some(Keymap::with_overrides(mode, &overrides).unwrap()),
                ..Options::default()
            },
            clipboard.clone(),
            Theme::plain(),
        );
        Self {
            app,
            clipboard,
            path,
        }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }
    fn control(&mut self, c: char) {
        self.app.on_event(Event::Key(ctrl(c)));
    }
    fn prefix(&mut self, c: char) {
        self.control('w');
        self.press(KeyCode::Char(c));
    }
    fn paste(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    fn run(&mut self, sql: &str) {
        self.prefix('k');
        self.control('a');
        self.paste(sql);
        self.app
            .on_event(Event::Key(crate::common::shift(KeyCode::F(5))));
        self.wait();
        assert!(
            !self.app.console().editor.is_blank(),
            "{}",
            self.draw(120, 30)
        );
        assert!(
            !self.app.console().results.is_empty() || sql.contains("no_such_table"),
            "{}",
            self.draw(120, 30)
        );
        self.prefix('j');
    }

    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.app.drain_events();
            if !self.app.console().is_busy()
                && self.app.table().is_none_or(|tab| !tab.is_busy())
                && !self.app.has_work()
            {
                break;
            }
            assert!(Instant::now() < deadline, "{}", self.draw(120, 30));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn draw(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn copy_report(&mut self) -> String {
        self.wait();
        self.control('c');
        self.clipboard.borrow().text.clone()
    }

    fn chart(&mut self, line: bool) {
        self.press(KeyCode::Char('b'));
        if line {
            self.press(KeyCode::Down);
        }
        self.press(KeyCode::Enter);
        self.press(KeyCode::Enter);
        self.press(KeyCode::Down);
        self.press(KeyCode::Enter);
    }

    fn compare(&mut self) {
        self.press(KeyCode::Char('D'));
        self.press(KeyCode::Down);
        self.press(KeyCode::Enter);
        self.press(KeyCode::Up);
        self.press(KeyCode::Enter);
    }
}

#[test]
fn profile_and_charts_work_in_both_modes_without_changing_sql_or_grid() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut s = Screen::new("modes", mode, Settings::default(), &[]);
        let sql = "SELECT 3 AS x, -2 AS y UNION ALL SELECT 1, 4 UNION ALL SELECT 3, 7;";
        s.run(sql);
        let generation = s.app.console().results[0].table.generation;
        s.press(KeyCode::Char('p'));
        let profile = s.copy_report();
        assert!(profile.contains("Distinct (without NULL): 2"), "{profile}");
        assert!(s.draw(80, 24).contains("Column profile"));
        s.press(KeyCode::Esc);
        for line in [false, true] {
            s.chart(line);
            let chart = s.copy_report();
            assert!(chart.contains("X=3 Y=-2"), "{chart}");
            assert!(chart.contains("X=1 Y=4"));
            assert!(chart.contains("X=3 Y=7"));
            let small = s.draw(80, 24);
            assert!(small.contains("Text chart"), "{small}");
            assert!(s.draw(110, 35).contains("Text chart"));
            s.press(KeyCode::Esc);
        }
        assert_eq!(s.app.console().editor.text(), sql);
        assert_eq!(s.app.console().results[0].table.generation, generation);
    }
}

#[test]
fn palette_and_panel_menu_show_analysis_with_effective_shortcuts() {
    let mut s = Screen::new(
        "keys",
        KeyboardMode::Conventional,
        Settings::default(),
        &[("gridProfile", &["z"])],
    );
    s.run("SELECT 1 AS x, 2 AS y;");
    let commands = s.app.context_menu();
    assert!(
        commands
            .iter()
            .flat_map(|group| &group.keys)
            .any(|(key, label)| key == "z" && *label == "Column profile")
    );
    s.prefix(' ');
    let screen = s.draw(80, 24);
    assert!(screen.contains("Column profile"), "{screen}");
    assert!(screen.contains("Text chart"), "{screen}");
    s.press(KeyCode::Char('z'));
    assert!(s.copy_report().contains("Column: x"));
    s.press(KeyCode::Esc);
    s.control('p');
    s.paste("histogram");
    assert!(s.draw(80, 24).contains("column profile"));
    s.press(KeyCode::Enter);
    assert!(s.copy_report().contains("Column: x"));
}

#[test]
fn retention_is_optional_and_comparison_matches_the_selected_key() {
    let mut s = Screen::new(
        "compare",
        KeyboardMode::Conventional,
        Settings::default(),
        &[],
    );
    s.run("SELECT 1 AS id, 'before' AS v UNION ALL SELECT 2, 'removed';");
    assert!(!s.app.console().retain_result);
    s.press(KeyCode::Char('B'));
    assert!(s.app.console().retain_result);
    s.run("SELECT 3 AS id, 'added' AS v UNION ALL SELECT 1, 'after';");
    assert!(s.app.console().previous_result.is_some());
    s.compare();
    let report = s.copy_report();
    assert!(report.contains("Previous -> Current"), "{report}");
    assert!(report.contains("v: before -> after"));
    assert!(report.contains("1 added, 1 removed, 1 changed"));
    s.press(KeyCode::Esc);
    s.press(KeyCode::Char('B'));
    assert!(!s.app.console().retain_result);
    assert!(s.app.console().previous_result.is_none());
}

#[test]
fn a_failed_multi_result_or_limited_execution_cancels_comparison() {
    for sql in [
        "SELECT * FROM no_such_table;",
        "SELECT 1 AS id; SELECT 2 AS id;",
        "SELECT 1 AS id UNION ALL SELECT 2;",
    ] {
        let mut s = Screen::new(
            "refusal",
            KeyboardMode::Conventional,
            Settings {
                row_cap: 1,
                cap_in_sql: false,
                ..Settings::default()
            },
            &[],
        );
        s.run("SELECT 1 AS id;");
        s.press(KeyCode::Char('B'));
        s.run(sql);
        assert!(s.app.console().previous_result.is_none(), "{sql}");
        assert!(!s.app.console().comparison_ready, "{sql}");
        assert!(s.app.console().warnings() > 0);
    }
}

#[test]
fn a_retained_result_that_fills_the_budget_cancels_comparison_and_allows_a_new_run() {
    let mut s = Screen::new(
        "budget",
        KeyboardMode::Conventional,
        Settings {
            budget_bytes: 600,
            ..Settings::default()
        },
        &[],
    );
    let sql = format!("SELECT '{}' AS v;", "a".repeat(400));
    s.run(&sql);
    assert!(s.app.console().comparison_ready);
    s.press(KeyCode::Char('B'));
    s.run(&sql);
    assert!(s.app.console().previous_result.is_none());
    assert!(!s.app.console().comparison_ready);
    assert!(s.app.console().warnings() > 0);
    s.run("SELECT 1;");
    assert!(s.app.console().comparison_ready);
}

#[test]
fn dialogs_block_sql_execution_and_cancel_keeps_text_cursor_and_grid() {
    let mut s = Screen::new(
        "cancel",
        KeyboardMode::Conventional,
        Settings::default(),
        &[],
    );
    s.run("SELECT 1 AS x, 2 AS y;");
    let generation = s.app.console().results[0].table.generation;
    let cursor = s.app.console().results[0].view;
    s.press(KeyCode::Char('b'));
    s.press(KeyCode::F(5));
    s.paste("DELETE FROM anything;");
    assert!(!s.app.console().is_busy());
    s.press(KeyCode::Esc);
    s.press(KeyCode::Char('p'));
    assert!(s.app.has_work());
    s.press(KeyCode::Esc);
    assert!(!s.app.has_work());
    assert_eq!(s.app.console().editor.text(), "SELECT 1 AS x, 2 AS y;");
    assert_eq!(s.app.console().results[0].table.generation, generation);
    assert_eq!(s.app.console().results[0].view.row, cursor.row);
    assert_eq!(s.app.console().results[0].view.col, cursor.col);
}

#[test]
fn a_large_column_profile_yields_to_cancel_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut s = Screen::new("large-profile-cancel", mode, Settings::default(), &[]);
        s.run("SELECT hex(zeroblob(1024 * 1024)) AS value;");
        let generation = s.app.console().results[0].table.generation;
        s.press(KeyCode::Char('p'));
        s.app.drain_events();
        assert!(s.app.has_work());
        assert!(s.draw(80, 24).contains("Calculating loaded rows"));
        s.press(KeyCode::Esc);
        assert!(!s.app.has_work());
        assert_eq!(s.app.console().results[0].table.generation, generation);
        assert!(!s.app.console().is_busy());
    }
}

#[test]
fn mouse_can_choose_chart_type_and_columns() {
    let mut s = Screen::new(
        "mouse",
        KeyboardMode::Conventional,
        Settings::default(),
        &[],
    );
    s.run("SELECT 1 AS x, 2 AS y;");
    s.press(KeyCode::Char('b'));
    let screen = s.draw(80, 24);
    let rows: Vec<_> = screen.lines().collect();
    let row = rows.iter().position(|line| line.contains("Bar")).unwrap();
    let column = rows[row].find("Bar").unwrap();
    s.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: column as u16,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(s.draw(80, 24).contains("numeric X"));
    s.press(KeyCode::Enter);
    s.press(KeyCode::Down);
    s.press(KeyCode::Enter);
    assert!(s.copy_report().contains("X=1 Y=2"));
}

#[test]
fn empty_results_have_a_profile_but_no_chart() {
    let mut s = Screen::new(
        "empty",
        KeyboardMode::Conventional,
        Settings::default(),
        &[],
    );
    s.run("SELECT 1 AS x, 2 AS y WHERE 0;");
    s.press(KeyCode::Char('p'));
    assert!(s.copy_report().contains("Rows: 0"));
    s.press(KeyCode::Esc);
    assert!(
        !s.app
            .context_menu()
            .iter()
            .flat_map(|group| &group.keys)
            .any(|(_, label)| *label == "Text chart")
    );
}

#[test]
fn table_profiles_label_partial_pages_and_refuse_pending_edits() {
    let mut s = Screen::new(
        "table",
        KeyboardMode::Conventional,
        Settings {
            page_size: 1,
            ..Settings::default()
        },
        &[],
    );
    s.run("CREATE TABLE demo(id INTEGER PRIMARY KEY, value INT); INSERT INTO demo VALUES(1, 3), (2, 4); SELECT id, value FROM demo;");
    open_table(&mut s.app, 0, "demo");
    s.wait();
    s.press(KeyCode::Char('p'));
    let profile = s.copy_report();
    assert!(profile.contains("incomplete result"), "{profile}");
    assert!(profile.contains("Rows: 1"));
    s.press(KeyCode::Esc);
    s.press(KeyCode::Char('n'));
    s.wait();
    assert!(!s.app.table().unwrap().result.edits.is_empty());
    assert!(
        !s.app
            .context_menu()
            .iter()
            .flat_map(|group| &group.keys)
            .any(|(_, label)| *label == "Column profile" || *label == "Text chart")
    );
}

#[test]
fn analysis_does_not_reread_the_database() {
    let mut s = Screen::new(
        "loaded-only",
        KeyboardMode::Conventional,
        Settings::default(),
        &[],
    );
    s.run("CREATE TABLE demo(x INT, y INT); INSERT INTO demo VALUES(1, 2); SELECT x, y FROM demo;");
    rusqlite::Connection::open(&s.path)
        .unwrap()
        .execute_batch("DROP TABLE demo;")
        .unwrap();
    s.press(KeyCode::Char('p'));
    assert!(s.copy_report().contains("Rows: 1"));
    s.press(KeyCode::Esc);
    s.chart(false);
    assert!(s.copy_report().contains("X=1 Y=2"));
}

#[test]
fn long_reports_scroll_with_the_mouse_and_keep_the_grid_cursor() {
    let mut s = Screen::new(
        "scroll",
        KeyboardMode::Conventional,
        Settings::default(),
        &[],
    );
    s.run("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<100) SELECT x, x AS y FROM n;");
    s.chart(false);
    s.wait();
    let first = s.draw(80, 24);
    s.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 40,
        row: 15,
        modifiers: KeyModifiers::NONE,
    }));
    let next = s.draw(80, 24);
    assert_ne!(first, next);
    assert_eq!(s.app.console().results[0].view.row, 0);
    s.press(KeyCode::Esc);
}
