//! Command discovery, panel pages, and width-dependent hints.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key};
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::connector::TransactionState;
use sqln::editor::KeyboardMode;
use sqln::input::{Action, Chord, Keymap, Region};
use sqln::navigator::RowKind;
use sqln::ui::text;
use sqln::ui::theme::Theme;

struct Screen {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
}

impl Screen {
    fn new(name: &str, mode: KeyboardMode, overrides: &[(&str, &[&str])]) -> Self {
        let path = common::temp_db(&format!("commands-{name}"));
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE items(id INTEGER PRIMARY KEY, label TEXT);
             INSERT INTO items VALUES (1, 'one'), (2, 'two');",
            )
            .unwrap();
        let overrides = overrides
            .iter()
            .map(|(name, keys)| {
                (
                    (*name).to_owned(),
                    keys.iter().map(|key| (*key).to_owned()).collect(),
                )
            })
            .collect::<HashMap<_, _>>();
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(
            Options {
                database: Some(path),
                keyboard: mode,
                keymap: Some(Keymap::with_overrides(mode, &overrides).unwrap()),
                ..Options::default()
            },
            clipboard.clone(),
            Theme::plain(),
        );
        Self { app, clipboard }
    }

    fn code(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }
    fn control(&mut self, c: char) {
        self.app.on_event(Event::Key(ctrl(c)));
    }
    fn prefix(&mut self, c: char) {
        self.control('w');
        self.code(KeyCode::Char(c));
    }
    fn paste(&mut self, value: &str) {
        self.app.on_event(Event::Paste(value.to_owned()));
    }

    fn draw(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait(&mut self, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if done(&self.app) {
                break;
            }
            assert!(Instant::now() < deadline, "{}", self.draw(120, 30));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn results(&mut self, sql: &str) {
        self.paste(sql);
        self.code(KeyCode::F(5));
        self.wait(|app| !app.console().is_busy());
        self.prefix('j');
    }

    fn palette(&mut self, query: &str) -> String {
        self.control('p');
        self.paste(query);
        self.draw(120, 30)
    }

    fn choose(&mut self, query: &str) {
        let screen = self.palette(query);
        assert!(!screen.contains(text::NO_COMMANDS), "{screen}");
        self.code(KeyCode::Enter);
    }
}

#[test]
fn space_after_ctrl_w_opens_the_panel_page_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let map = Keymap::new(mode);
        assert_eq!(
            map.after(
                Region::Global,
                Chord::parse("ctrl+w").unwrap(),
                Chord::parse("space").unwrap()
            ),
            Some(Action::ContextCommands)
        );
        let mut s = Screen::new("editor-page", mode, &[]);
        s.paste("SELECT 1;");
        s.prefix(' ');
        let screen = s.draw(80, 24);
        assert!(screen.contains("Ctrl+W · Editor"), "{screen}");
        assert!(screen.contains("Save now"), "{screen}");
        s.control('f');
        assert!(s.app.console().editor.search().is_some());
        assert_eq!(s.app.console().editor.text(), "SELECT 1;");
    }
}

#[test]
fn escape_and_an_unknown_key_close_the_page_without_editing() {
    let mut s = Screen::new("page-cancel", KeyboardMode::Conventional, &[]);
    s.paste("SELECT 1;");
    for code in [KeyCode::Esc, KeyCode::Char('!')] {
        s.prefix(' ');
        s.code(code);
        assert!(!s.draw(80, 24).contains("Ctrl+W · Editor"));
        assert_eq!(s.app.console().editor.text(), "SELECT 1;");
    }
}

#[test]
fn the_grid_page_shows_each_bound_command_at_the_minimum_size() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut s = Screen::new("grid-page", mode, &[]);
        s.results("SELECT id, label FROM items;");
        let groups = s.app.context_menu();
        s.prefix(' ');
        let screen = s.draw(80, 24);
        for (_, label) in groups.iter().flat_map(|group| &group.keys) {
            assert!(screen.contains(label), "{label}: {screen}");
        }
        if mode == KeyboardMode::Vim {
            s.code(KeyCode::Char('y'));
        } else {
            s.control('c');
        }
        assert_eq!(s.clipboard.borrow().text, "1\n");
        assert!(!s.draw(80, 24).contains("Ctrl+W · Results"));
    }
}

#[test]
fn explorer_row_commands_stay_out_of_the_editor_palette() {
    let mut s = Screen::new("palette-scope", KeyboardMode::Conventional, &[]);
    for query in [
        "edit connection",
        "read catalog",
        "copy name",
        "new console",
        "generate ddl",
    ] {
        let screen = s.palette(query);
        assert!(screen.contains(text::NO_COMMANDS), "{query}: {screen}");
        s.code(KeyCode::Esc);
    }
    for query in ["add connection", "scripts without"] {
        let screen = s.palette(query);
        assert!(!screen.contains(text::NO_COMMANDS), "{screen}");
        s.code(KeyCode::Esc);
    }
    s.prefix('h');
    let target = s
        .app
        .navigator()
        .current(s.app.connections())
        .unwrap()
        .name(s.app.connections())
        .to_owned();
    let screen = s.palette("edit connection");
    assert!(!screen.contains(text::NO_COMMANDS), "{screen}");
    assert!(screen.contains(&target), "{screen}");
}

#[test]
fn the_explicit_explorer_page_still_acts_from_the_editor() {
    let mut s = Screen::new("explorer-page", KeyboardMode::Conventional, &[]);
    s.control('w');
    s.code(KeyCode::Tab);
    let screen = s.draw(80, 24);
    assert!(screen.contains("Ctrl+W · Explorer"), "{screen}");
    s.code(KeyCode::Char('e'));
    assert!(s.app.form().is_some());
}

#[test]
fn the_palette_finds_grid_commands_and_technical_aliases() {
    let mut s = Screen::new("grid-palette", KeyboardMode::Conventional, &[]);
    s.results("SELECT id, label FROM items;");
    for query in [
        "copy cell",
        "open value",
        "find loaded",
        "automatic width",
        "close result",
        "previous result",
        "select all",
    ] {
        let screen = s.palette(query);
        assert!(!screen.contains(text::NO_COMMANDS), "{query}: {screen}");
        s.code(KeyCode::Esc);
    }
    s.choose("copy cell");
    assert_eq!(s.clipboard.borrow().text, "1\n");
    s.prefix('k');
    let screen = s.palette("EXPLAIN");
    assert!(!screen.contains(text::NO_COMMANDS), "{screen}");
    s.code(KeyCode::Esc);
    common::select_row(&mut s.app, 0, &[RowKind::Table], "items");
    let screen = s.palette("DDL");
    assert!(!screen.contains(text::NO_COMMANDS), "{screen}");
    assert!(screen.contains("items"), "{screen}");
}

#[test]
fn the_palette_hides_row_commands_for_an_empty_result() {
    let mut s = Screen::new("empty-result", KeyboardMode::Conventional, &[]);
    s.results("SELECT id FROM items WHERE 0;");
    for query in [
        "copy cell",
        "open value",
        "gridOpenRow",
        "find loaded",
        "select all",
    ] {
        let screen = s.palette(query);
        assert!(screen.contains(text::NO_COMMANDS), "{query}: {screen}");
        s.code(KeyCode::Esc);
    }
}

#[test]
fn a_failed_transaction_offers_rollback_but_not_commit_in_each_access() {
    let mut s = Screen::new("failed-transaction", KeyboardMode::Conventional, &[]);
    s.app.console_mut().transaction = TransactionState::Failed;
    let screen = s.draw(80, 24);
    let hint = screen.lines().last().unwrap();
    assert!(hint.trim().starts_with("Ctrl+W R rollback"), "{hint}");
    assert!(!hint.contains("commit"), "{hint}");
    let screen = s.palette("commit");
    assert!(!screen.contains("Ctrl+W C"), "{screen}");
    s.code(KeyCode::Esc);
    assert!(!s.palette("rollback").contains(text::NO_COMMANDS));
}

#[test]
fn panel_pages_and_the_reserved_palette_hint_use_custom_keys() {
    let mut s = Screen::new(
        "custom-keys",
        KeyboardMode::Conventional,
        &[
            ("contextCommands", &["f8"]),
            ("openPalette", &["f9"]),
            ("gridCopy", &["alt+c"]),
        ],
    );
    s.results("SELECT 1;");
    let screen = s.draw(80, 24);
    assert!(
        screen.lines().last().unwrap().contains("F9 commands"),
        "{screen}"
    );
    s.code(KeyCode::F(8));
    let screen = s.draw(80, 24);
    assert!(screen.contains("Alt+C"), "{screen}");
    s.app.on_event(Event::Key(common::alt(KeyCode::Char('c'))));
    assert_eq!(s.clipboard.borrow().text, "1\n");
}

#[test]
fn a_command_without_a_key_stays_in_the_palette_but_leaves_the_page() {
    let mut s = Screen::new(
        "removed-copy",
        KeyboardMode::Conventional,
        &[("gridCopy", &[])],
    );
    s.results("SELECT 1;");
    assert!(
        !s.app
            .context_menu()
            .iter()
            .flat_map(|group| &group.keys)
            .any(|(_, label)| *label == "Copy")
    );
    s.choose("copy cell");
    assert_eq!(s.clipboard.borrow().text, "1\n");
}

#[test]
fn resizing_the_same_terminal_adds_whole_commands_and_keeps_the_palette() {
    let mut s = Screen::new("resize", KeyboardMode::Conventional, &[]);
    s.prefix('h');
    let all = s.app.hint();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut counts = Vec::new();
    for width in [80, 100, 120, 160, 200, 80] {
        terminal.backend_mut().resize(width, 24);
        terminal.resize(Rect::new(0, 0, width, 24)).unwrap();
        terminal.draw(|frame| s.app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let hint = (0..width)
            .map(|x| buffer[(x, 23)].symbol())
            .collect::<String>();
        assert!(hint.contains("Ctrl+P commands"), "{hint}");
        let commands = hint.trim().split("  ").collect::<Vec<_>>();
        for command in &commands {
            assert!(
                all.split("  ").any(|item| item == *command),
                "{command}: {hint}"
            );
        }
        counts.push(commands.len());
    }
    assert!(
        counts[..5].windows(2).all(|pair| pair[0] <= pair[1]),
        "{counts:?}"
    );
    assert!(counts[4] > counts[0], "{counts:?}");
    assert_eq!(counts[0], counts[5]);
}

#[test]
fn editor_palette_operations_keep_undo_and_clipboard_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut s = Screen::new("editor-operations", mode, &[]);
        s.paste("SELECT 1;\nSELECT 2;");
        let original = s.app.console().editor.text();
        s.choose("undo last edit");
        assert!(s.app.console().editor.text().is_empty());
        s.choose("redo last edit");
        assert_eq!(s.app.console().editor.text(), original);
        s.choose("select all the text");
        assert_eq!(
            s.app.console().editor.selected_text().as_deref(),
            Some(original.as_str())
        );
        s.choose("copy selected text");
        assert_eq!(s.clipboard.borrow().text, original);
        s.choose("cut selected text");
        assert!(s.app.console().editor.text().is_empty());
        s.choose("paste into editor");
        assert_eq!(s.app.console().editor.text(), original);
    }
}

#[test]
fn editor_control_keys_keep_their_native_mode_rules() {
    let mut s = Screen::new("editor-native-modes", KeyboardMode::Vim, &[]);
    s.control(' ');
    assert!(s.app.console().editor.suggestions().is_none());
    s.paste("SELECT 1;");
    s.choose("undo last edit");
    s.code(KeyCode::Char('i'));
    s.control('r');
    assert!(s.app.console().editor.text().is_empty());
    s.choose("redo last edit");
    assert_eq!(s.app.console().editor.text(), "SELECT 1;");
    assert!(s.app.console().editor.is_inserting());

    let mut s = Screen::new("editor-shift-redo", KeyboardMode::Conventional, &[]);
    s.paste("SELECT 1;");
    for c in ['z', 'Z'] {
        s.control('z');
        assert!(s.app.console().editor.text().is_empty());
        let mut redo = ctrl(c);
        redo.modifiers |= KeyModifiers::SHIFT;
        s.app.on_event(Event::Key(redo));
        assert_eq!(s.app.console().editor.text(), "SELECT 1;");
    }
}

#[test]
fn removed_editor_bindings_do_not_reach_the_builtin_key_handler() {
    let mut s = Screen::new(
        "removed-editor-key",
        KeyboardMode::Conventional,
        &[("editorFind", &[])],
    );
    s.paste("SELECT 1;");
    s.control('f');
    assert!(s.app.console().editor.search().is_none());
    s.choose("find in editor");
    assert!(s.app.console().editor.search().is_some());
}

#[test]
fn clearing_a_selection_gets_priority_in_the_hint_row() {
    let mut s = Screen::new("selection-priority", KeyboardMode::Conventional, &[]);
    s.results("SELECT id FROM items;");
    s.control('a');
    let screen = s.draw(80, 24);
    let hint = screen.lines().last().unwrap().trim();
    assert!(hint.starts_with("Esc clear selection"), "{hint}");
    assert!(hint.ends_with("Ctrl+P commands"), "{hint}");
}

#[test]
fn table_and_diagram_pages_fit_and_run_their_local_commands() {
    let mut s = Screen::new("table-diagram-pages", KeyboardMode::Conventional, &[]);
    common::open_table(&mut s.app, 0, "items");
    s.wait(|app| app.table().is_some_and(|tab| !tab.is_busy()));
    let groups = s.app.context_menu();
    s.prefix(' ');
    let screen = s.draw(80, 24);
    for (_, label) in groups.iter().flat_map(|group| &group.keys) {
        assert!(screen.contains(label), "{label}: {screen}");
    }
    s.code(KeyCode::Char('f'));
    assert_eq!(
        s.app.table().unwrap().focus,
        sqln::table::TableFocus::Filter
    );
    s.code(KeyCode::Esc);
    s.prefix('D');
    s.wait(|app| app.diagram().is_some_and(|tab| !tab.is_busy()));
    let screen = s.palette("Mermaid");
    assert!(!screen.contains(text::NO_COMMANDS), "{screen}");
    s.code(KeyCode::Esc);
    let groups = s.app.context_menu();
    s.prefix(' ');
    let screen = s.draw(80, 24);
    for (_, label) in groups.iter().flat_map(|group| &group.keys) {
        assert!(screen.contains(label), "{label}: {screen}");
    }
    s.code(KeyCode::Char('y'));
    assert!(s.clipboard.borrow().text.contains("erDiagram"));
}

#[test]
fn table_filters_and_order_are_available_in_the_palette() {
    let mut s = Screen::new("table-commands", KeyboardMode::Conventional, &[]);
    common::open_table(&mut s.app, 0, "items");
    s.wait(|app| app.table().is_some_and(|tab| !tab.is_busy()));
    for query in ["WHERE filter", "ORDER BY", "sort column", "apply where"] {
        let screen = s.palette(query);
        assert!(!screen.contains(text::NO_COMMANDS), "{query}: {screen}");
        s.code(KeyCode::Esc);
    }
    s.code(KeyCode::Char('f'));
    s.paste("id = 1");
    s.choose("apply where");
    s.wait(|app| app.table().is_some_and(|tab| !tab.is_busy()));
    assert_eq!(s.app.table().unwrap().applied_filter(), "id = 1");
    s.choose("clear filters reload");
    s.wait(|app| app.table().is_some_and(|tab| !tab.is_busy()));
    assert_eq!(s.app.table().unwrap().loaded(), 2);
    assert_eq!(s.app.table().unwrap().applied_filter(), "");
}
