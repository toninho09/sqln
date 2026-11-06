// The filter of the navigator tree.

use std::cell::RefCell;
use std::rc::Rc;

use crate::common::{ctrl, key, select_row, temp_db};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::{Clipboard, MemoryClipboard};
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::editor::KeyboardMode;
use sqln::navigator::RowKind;
use sqln::ui::theme::Theme;

const WIDTH: u16 = 100;

struct Tree {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
}

fn database(name: &str) -> std::path::PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE customers (id INT);
             CREATE TABLE orders (id INT);
             CREATE TABLE order_items (id INT);
             CREATE VIEW v_orders AS SELECT id FROM orders;",
        )
        .unwrap();
    path
}

impl Tree {
    /// Opens two connections. The tests read only the catalog of the first one.
    fn new(name: &str, keyboard: KeyboardMode) -> Self {
        let options = Options {
            keyboard,
            connections: vec![
                Definition::sqlite(SqliteTarget::new(database(name))),
                Definition::sqlite(SqliteTarget::new(database(&format!("{name}-other")))),
            ],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let mut app = App::new(options, clipboard.clone(), Theme::plain());
        select_row(&mut app, 0, &[RowKind::Table], "customers");
        Self { app, clipboard }
    }

    fn conventional(name: &str) -> Self {
        Self::new(name, KeyboardMode::Conventional)
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    fn write(&mut self, text: &str) {
        for letter in text.chars() {
            self.press(KeyCode::Char(letter));
        }
    }

    fn filter(&mut self, text: &str) {
        self.press(KeyCode::Char('/'));
        self.write(text);
    }

    fn names(&self) -> Vec<String> {
        let connections = self.app.connections();
        let rows = self.app.navigator().rows(connections);
        rows.iter()
            .filter(|row| matches!(row.kind, RowKind::Table | RowKind::View))
            .map(|row| row.name(connections).to_owned())
            .collect()
    }

    fn current(&self) -> String {
        let connections = self.app.connections();
        let row = self.app.navigator().current(connections).unwrap();
        row.name(connections).to_owned()
    }

    fn screen_rows(&mut self) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(WIDTH));
        rows.map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    fn screen(&mut self) -> String {
        self.screen_rows().join("\n")
    }

    fn click(&mut self, column: u16, row: u16) {
        self.app.on_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    fn row_of(&mut self, text: &str) -> u16 {
        let rows = self.screen_rows();
        let found = rows.iter().position(|row| row.contains(text));
        u16::try_from(found.unwrap_or_else(|| panic!("{text} is not on the screen"))).unwrap()
    }
}

#[test]
fn the_tree_keeps_the_matches_without_case_and_the_cursor_goes_to_the_first_one() {
    let mut tree = Tree::conventional("filter-case");
    tree.filter("ORDER");
    assert_eq!(tree.names(), ["order_items", "orders", "v_orders"]);
    assert_eq!(tree.current(), "order_items");
    let screen = tree.screen();
    assert!(screen.contains("> ORDER"), "{screen}");
    assert!(!screen.contains("customers"), "{screen}");
}

#[test]
fn each_word_must_match_and_the_schema_counts() {
    let mut tree = Tree::conventional("filter-words");
    tree.filter("ord items");
    assert_eq!(tree.names(), ["order_items"]);
    tree.press(KeyCode::Esc);
    tree.filter("main cust");
    assert_eq!(tree.names(), ["customers"]);
}

#[test]
fn a_connection_without_a_catalog_stays_closed_and_out_of_the_list() {
    let mut tree = Tree::conventional("filter-unread");
    tree.filter("orders");
    assert_eq!(tree.names(), ["orders", "v_orders"]);
    assert!(tree.app.connections()[1].catalog.is_none());
    let connections = tree.app.connections();
    let rows = tree.app.navigator().rows(connections);
    assert!(rows.iter().all(|row| row.connection == 0));
}

#[test]
fn no_match_says_that_the_list_reads_only_loaded_catalogs() {
    let mut tree = Tree::conventional("filter-none");
    tree.filter("zzz");
    let screen = tree.screen();
    assert!(screen.contains("No match"), "{screen}");
    assert!(screen.contains("Loaded catalogs only"), "{screen}");
    tree.press(KeyCode::Enter);
    assert!(tree.app.navigator().filter_is_editing());
}

#[test]
fn a_letter_in_the_field_is_text_and_not_a_command_of_the_tree() {
    let mut tree = Tree::conventional("filter-text");
    let open_rows = tree.app.navigator().rows(tree.app.connections()).len();
    tree.filter("nrasi");
    let screen = tree.screen();
    assert!(screen.contains("> nrasi"), "{screen}");
    assert!(!screen.contains("New connection"), "{screen}");
    tree.press(KeyCode::Esc);
    assert_eq!(
        tree.app.navigator().rows(tree.app.connections()).len(),
        open_rows
    );
}

#[test]
fn up_and_down_move_between_the_relations() {
    let mut tree = Tree::conventional("filter-move");
    tree.filter("order");
    tree.press(KeyCode::Down);
    assert_eq!(tree.current(), "orders");
    tree.press(KeyCode::Down);
    tree.press(KeyCode::Down);
    assert_eq!(tree.current(), "v_orders");
    tree.press(KeyCode::Up);
    assert_eq!(tree.current(), "orders");
    assert!(tree.app.navigator().filter_is_editing());
}

#[test]
fn enter_opens_the_table_and_the_filter_stays_for_the_next_table() {
    let mut tree = Tree::conventional("filter-open");
    tree.filter("orders");
    tree.press(KeyCode::Enter);
    assert!(!tree.app.navigator_focused());
    assert!(tree.screen().contains("orders (table)"));

    tree.app.on_event(Event::Key(ctrl('w')));
    tree.press(KeyCode::Char('h'));
    assert!(tree.app.navigator().is_filtered());
    assert!(!tree.app.navigator().filter_is_editing());
    tree.press(KeyCode::Down);
    tree.press(KeyCode::Enter);
    assert!(tree.screen().contains("v_orders (view)"));
}

#[test]
fn tab_sends_the_keys_to_the_filtered_tree() {
    let mut tree = Tree::conventional("filter-tab");
    tree.filter("items");
    tree.press(KeyCode::Tab);
    assert!(!tree.app.navigator().filter_is_editing());
    tree.press(KeyCode::Char('y'));
    assert_eq!(
        tree.clipboard.borrow_mut().paste().as_deref(),
        Some("order_items")
    );
    tree.press(KeyCode::Char('g'));
    assert!(tree.screen().contains("Generate SQL"));
    tree.press(KeyCode::Esc);
    tree.press(KeyCode::Char('i'));
    assert!(tree.screen().contains("order_items (structure)"));
}

#[test]
fn the_keys_that_open_or_close_a_row_do_nothing_in_the_filtered_tree() {
    let mut tree = Tree::conventional("filter-arrows");
    tree.filter("items");
    tree.press(KeyCode::Tab);
    let before = tree.app.navigator().rows(tree.app.connections());
    tree.press(KeyCode::Right);
    tree.press(KeyCode::Left);
    assert_eq!(tree.app.navigator().rows(tree.app.connections()), before);
    assert_eq!(tree.current(), "order_items");
}

#[test]
fn esc_in_the_field_cancels_and_returns_the_cursor() {
    let mut tree = Tree::conventional("filter-cancel");
    tree.filter("items");
    assert_eq!(tree.current(), "order_items");
    tree.press(KeyCode::Esc);
    assert!(tree.app.navigator().filter_input().is_none());
    assert_eq!(tree.current(), "customers");
    assert!(!tree.screen().contains("> items"));
}

#[test]
fn esc_in_the_tree_clears_the_filter_and_keeps_the_row_in_sight() {
    let mut tree = Tree::new("filter-clear", KeyboardMode::Conventional);
    tree.press(KeyCode::Up);
    tree.press(KeyCode::Up);
    tree.press(KeyCode::Left);
    tree.press(KeyCode::Up);
    tree.press(KeyCode::Left);
    assert_eq!(tree.names(), Vec::<String>::new());

    tree.filter("items");
    tree.press(KeyCode::Tab);
    tree.press(KeyCode::Esc);
    assert!(tree.app.navigator().filter_input().is_none());
    assert_eq!(tree.current(), "order_items");
    assert!(tree.names().contains(&"customers".to_owned()));
}

#[test]
fn an_empty_field_closes_with_enter_and_with_tab() {
    let mut tree = Tree::conventional("filter-empty");
    tree.press(KeyCode::Char('/'));
    tree.press(KeyCode::Enter);
    assert!(tree.app.navigator().filter_input().is_none());
    tree.press(KeyCode::Char('/'));
    tree.press(KeyCode::Tab);
    assert!(tree.app.navigator().filter_input().is_none());
    assert_eq!(tree.current(), "customers");
}

#[test]
fn backspace_to_an_empty_field_returns_the_tree_and_the_cursor() {
    let mut tree = Tree::conventional("filter-backspace");
    tree.filter("i");
    tree.press(KeyCode::Backspace);
    assert!(!tree.app.navigator().is_filtered());
    assert_eq!(tree.current(), "customers");
}

#[test]
fn control_f_opens_the_field_and_a_paste_writes_one_line() {
    let mut tree = Tree::conventional("filter-paste");
    tree.app.on_event(Event::Key(ctrl('f')));
    assert!(tree.app.navigator().filter_is_editing());
    tree.app.on_event(Event::Paste("items\nother".to_owned()));
    assert_eq!(tree.names(), ["order_items"]);
}

#[test]
fn a_paste_in_the_tree_changes_nothing() {
    let mut tree = Tree::conventional("filter-paste-tree");
    tree.filter("items");
    tree.press(KeyCode::Tab);
    tree.app.on_event(Event::Paste("orders".to_owned()));
    assert_eq!(tree.names(), ["order_items"]);
}

#[test]
fn the_control_w_prefix_works_in_the_field() {
    let mut tree = Tree::conventional("filter-prefix");
    tree.filter("items");
    tree.app.on_event(Event::Key(ctrl('w')));
    tree.press(KeyCode::Char('l'));
    assert!(!tree.app.navigator_focused());
    assert!(tree.app.navigator().is_filtered());
}

#[test]
fn the_vim_keys_of_the_tree_are_text_in_the_field() {
    let mut tree = Tree::new("filter-vim", KeyboardMode::Vim);
    tree.filter("kj");
    assert_eq!(tree.names(), Vec::<String>::new());
    tree.press(KeyCode::Esc);
    tree.filter("items");
    tree.press(KeyCode::Tab);
    tree.press(KeyCode::Char('k'));
    assert_eq!(tree.names(), ["order_items"]);
}

#[test]
fn the_mouse_opens_the_field_and_selects_a_row_below_it() {
    let mut tree = Tree::conventional("filter-mouse");
    tree.filter("order");
    tree.press(KeyCode::Tab);
    let row = tree.row_of("> order");
    tree.click(4, row);
    assert!(tree.app.navigator().filter_is_editing());

    let row = tree.row_of("orders");
    tree.press(KeyCode::Tab);
    tree.click(12, row);
    assert_eq!(tree.current(), "orders");
}

#[test]
fn a_click_on_a_mark_does_not_change_the_filtered_tree() {
    let mut tree = Tree::conventional("filter-mark");
    tree.filter("items");
    tree.press(KeyCode::Tab);
    let before = tree.app.navigator().rows(tree.app.connections());
    let row = tree.row_of("main");
    tree.click(3, row);
    assert_eq!(tree.app.navigator().rows(tree.app.connections()), before);
}

#[test]
fn the_hints_follow_the_place_of_the_keys() {
    let mut tree = Tree::conventional("filter-hints");
    tree.filter("items");
    let screen = tree.screen();
    assert!(screen.contains("Enter open table"), "{screen}");
    assert!(screen.contains("Tab tree"), "{screen}");
    assert!(screen.contains("FILTER"), "{screen}");
    tree.press(KeyCode::Tab);
    let screen = tree.screen();
    assert!(screen.contains("Esc clear filter"), "{screen}");
    assert!(!screen.contains("columns"), "{screen}");
    assert!(!screen.contains("collapse"), "{screen}");
}
