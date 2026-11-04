// The groups of objects in the explorer tree: routines, sequences, triggers, and types.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{key, select_row, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::{Group, Object, ObjectKind, Schema, add_objects};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::{CatalogState, Definition};
use sqln::connector::sqlite::SqliteTarget;
use sqln::editor::KeyboardMode;
use sqln::navigator::RowKind;
use sqln::ui::text;
use sqln::ui::theme::Theme;

const WIDTH: u16 = 100;

struct Tree {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
}

/// A database with two triggers. The second connection has no trigger.
fn database(name: &str, triggers: bool) -> std::path::PathBuf {
    let path = temp_db(name);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INT, total INT);
             CREATE INDEX orders_customer ON orders (customer_id);
             CREATE VIEW v_orders AS SELECT id FROM orders;",
        )
        .unwrap();
    if triggers {
        connection
            .execute_batch(
                "CREATE TRIGGER b_check BEFORE INSERT ON customers
                 BEGIN SELECT 1; END;
                 CREATE TRIGGER audit_orders AFTER UPDATE ON orders
                 BEGIN SELECT 1; END;",
            )
            .unwrap();
    }
    path
}

impl Tree {
    fn new(name: &str) -> Self {
        let options = Options {
            keyboard: KeyboardMode::Conventional,
            connections: vec![
                Definition::sqlite(SqliteTarget::new(database(name, true))),
                Definition::sqlite(SqliteTarget::new(database(&format!("{name}-plain"), false))),
            ],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let mut app = App::new(options, clipboard.clone(), Theme::plain());
        select_row(&mut app, 0, &[RowKind::Table], "customers");
        Self { app, clipboard }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    fn write(&mut self, text: &str) {
        for letter in text.chars() {
            self.press(KeyCode::Char(letter));
        }
    }

    /// Lists the rows of a connection below its schema, as `kind name`.
    fn rows(&self, connection: usize) -> Vec<String> {
        let connections = self.app.connections();
        let rows = self.app.navigator().rows(connections);
        rows.iter()
            .filter(|row| row.connection == connection && row.depth() >= 2)
            .map(|row| {
                let kind = match row.kind {
                    RowKind::Group(_) => "group",
                    RowKind::Object(_) => "object",
                    RowKind::Table => "table",
                    RowKind::View => "view",
                    _ => "other",
                };
                format!("{kind} {}", row.name(connections))
            })
            .collect()
    }

    fn current(&self) -> (RowKind, String) {
        let connections = self.app.connections();
        let row = self.app.navigator().current(connections).unwrap();
        (row.kind, row.name(connections).to_owned())
    }

    /// Moves down to the first row with this name.
    fn go_to(&mut self, name: &str) {
        for _ in 0..50 {
            if self.current().1 == name {
                return;
            }
            self.press(KeyCode::Down);
        }
        panic!("{name} is not in the tree");
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(WIDTH))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>();
        rows.join("\n")
    }

    /// Waits for a new console after a read of a DDL, and returns its text.
    fn wait_for_console(&mut self, before: usize) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while self.app.consoles().len() == before {
            self.app.drain_events();
            assert!(std::time::Instant::now() < deadline, "no new console");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.app.console().editor.text()
    }

    fn wait_for_catalog(&mut self, connection: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while self.app.connections()[connection].catalog_state == CatalogState::Loading {
            self.app.drain_events();
            assert!(std::time::Instant::now() < deadline, "no catalog");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

#[test]
fn a_label_tells_two_overloads_and_two_triggers_apart() {
    let function = Object::new("f".into(), ObjectKind::Function, "integer".into());
    assert_eq!(function.label, "f(integer)");
    let procedure = Object::new("p".into(), ObjectKind::Procedure, String::new());
    assert_eq!(procedure.label, "p");
    let trigger = Object::new("t".into(), ObjectKind::Trigger, "orders".into());
    assert_eq!(trigger.label, "t (orders)");
    assert_eq!(ObjectKind::Domain.group(), Group::Types);
    assert_eq!(ObjectKind::Procedure.group(), Group::Routines);
}

#[test]
fn the_objects_go_to_their_schema_in_the_order_of_the_tree() {
    let mut schemas = vec![
        Schema {
            name: "public".into(),
            ..Schema::default()
        },
        Schema {
            name: "sales".into(),
            ..Schema::default()
        },
    ];
    let row = |schema: &str, name: &str, kind: &str, detail: &str| {
        [schema, name, kind, detail].map(str::to_owned)
    };
    add_objects(
        &mut schemas,
        [
            row("public", "status", "enum", ""),
            row("public", "Total", "function", "integer"),
            row("public", "total", "function", "text"),
            row("public", "audit", "trigger", "orders"),
            row("public", "ids", "sequence", ""),
            row("sales", "close", "procedure", ""),
            row("public", "odd", "rule", ""),
            row("missing", "lost", "function", ""),
        ],
    );
    let labels: Vec<&str> = schemas[0]
        .objects
        .iter()
        .map(|object| object.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Total(integer)",
            "total(text)",
            "ids",
            "audit (orders)",
            "status"
        ]
    );
    assert_eq!(schemas[1].objects[0].kind, ObjectKind::Procedure);
}

#[test]
fn the_groups_follow_the_relations_and_start_closed() {
    let mut tree = Tree::new("objects-groups");
    assert_eq!(
        tree.rows(0),
        [
            "table customers",
            "table orders",
            "view v_orders",
            "group Triggers"
        ]
    );
    let screen = tree.screen();
    assert!(screen.contains("Triggers (2)"), "{screen}");
}

#[test]
fn a_schema_without_objects_has_no_group() {
    let mut tree = Tree::new("objects-none");
    select_row(&mut tree.app, 1, &[RowKind::Table], "customers");
    assert_eq!(
        tree.rows(1),
        ["table customers", "table orders", "view v_orders"]
    );
}

#[test]
fn right_opens_a_group_and_left_returns_to_it() {
    let mut tree = Tree::new("objects-open");
    tree.go_to("Triggers");
    tree.press(KeyCode::Right);
    assert_eq!(
        &tree.rows(0)[3..],
        [
            "group Triggers",
            "object audit_orders (orders)",
            "object b_check (customers)"
        ]
    );
    tree.press(KeyCode::Down);
    assert_eq!(
        tree.current(),
        (
            RowKind::Object(Group::Triggers),
            "audit_orders (orders)".into()
        )
    );
    tree.press(KeyCode::Left);
    assert_eq!(
        tree.current(),
        (RowKind::Group(Group::Triggers), "Triggers".into())
    );
    tree.press(KeyCode::Left);
    assert_eq!(tree.rows(0).len(), 4);
}

#[test]
fn the_filter_finds_an_object_under_its_group() {
    let mut tree = Tree::new("objects-filter");
    tree.press(KeyCode::Char('/'));
    tree.write("audit");
    assert_eq!(
        tree.rows(0),
        ["group Triggers", "object audit_orders (orders)"]
    );
    assert_eq!(tree.current().1, "audit_orders (orders)");
    tree.press(KeyCode::Tab);
    tree.press(KeyCode::Esc);
    // The parents of the object stay open after the filter.
    assert_eq!(tree.current().1, "audit_orders (orders)");
    assert!(
        tree.rows(0)
            .contains(&"object b_check (customers)".to_owned())
    );
}

#[test]
fn y_copies_the_name_of_an_object_without_its_table() {
    let mut tree = Tree::new("objects-copy");
    tree.go_to("Triggers");
    tree.press(KeyCode::Right);
    tree.press(KeyCode::Down);
    tree.press(KeyCode::Char('y'));
    assert_eq!(tree.clipboard.borrow().text, "audit_orders");
}

#[test]
fn the_commands_of_a_table_do_not_act_on_an_object() {
    let mut tree = Tree::new("objects-refuse");
    tree.go_to("Triggers");
    tree.press(KeyCode::Right);
    tree.press(KeyCode::Down);
    let page = common::explorer_page(&tree.app).join("\n");
    for label in ["structure", "diagram", "compare", "import"] {
        assert!(!page.to_lowercase().contains(label), "{label}: {page}");
    }
    for letter in ['i', 'd', 'c'] {
        tree.press(KeyCode::Char(letter));
        let screen = tree.screen();
        assert!(screen.contains(text::NOT_FOR_OBJECT), "{letter}: {screen}");
    }
}

#[test]
fn the_cursor_stays_on_the_object_after_a_refresh() {
    let mut tree = Tree::new("objects-refresh");
    tree.go_to("Triggers");
    tree.press(KeyCode::Right);
    tree.go_to("b_check (customers)");
    tree.press(KeyCode::Char('r'));
    tree.wait_for_catalog(0);
    assert_eq!(tree.current().1, "b_check (customers)");
}

#[test]
fn enter_on_a_trigger_opens_its_ddl_in_a_new_console() {
    let mut tree = Tree::new("objects-ddl-trigger");
    let before = tree.app.consoles().len();
    tree.go_to("Triggers");
    tree.press(KeyCode::Right);
    tree.go_to("audit_orders (orders)");
    tree.press(KeyCode::Enter);
    let ddl = tree.wait_for_console(before);
    assert!(
        ddl.starts_with("-- Trigger main.audit_orders (orders) · "),
        "{ddl}"
    );
    assert!(
        ddl.contains("CREATE TRIGGER audit_orders AFTER UPDATE ON orders"),
        "{ddl}"
    );
    assert!(ddl.ends_with("END;\n"), "{ddl}");
    assert!(!ddl.contains(text::DEFINITION_WRITTEN), "{ddl}");
    assert!(!tree.app.navigator_focused());
    assert!(tree.app.console().results.is_empty());
}

#[test]
fn g_lists_the_ddl_of_a_table_last_with_its_indexes() {
    let mut tree = Tree::new("objects-ddl-table");
    let before = tree.app.consoles().len();
    select_row(&mut tree.app, 0, &[RowKind::Table], "orders");
    tree.press(KeyCode::Char('g'));
    let screen = tree.screen();
    assert!(screen.contains("DELETE"), "{screen}");
    assert!(screen.contains("DDL"), "{screen}");
    for _ in 0..5 {
        tree.press(KeyCode::Down);
    }
    tree.press(KeyCode::Enter);
    let ddl = tree.wait_for_console(before);
    assert!(ddl.starts_with("-- Table main.orders · "), "{ddl}");
    let table = ddl.find("CREATE TABLE orders").expect("the table");
    let index = ddl.find("CREATE INDEX orders_customer").expect("the index");
    assert!(table < index, "{ddl}");
}

#[test]
fn g_on_an_object_lists_only_the_ddl() {
    let mut tree = Tree::new("objects-ddl-g");
    let before = tree.app.consoles().len();
    tree.go_to("Triggers");
    tree.press(KeyCode::Right);
    tree.go_to("b_check (customers)");
    tree.press(KeyCode::Char('g'));
    let screen = tree.screen();
    assert!(screen.contains("DDL"), "{screen}");
    assert!(!screen.contains("SELECT *"), "{screen}");
    tree.press(KeyCode::Enter);
    let ddl = tree.wait_for_console(before);
    assert!(
        ddl.contains("CREATE TRIGGER b_check BEFORE INSERT ON customers"),
        "{ddl}"
    );
}

#[test]
fn the_ddl_of_a_view_comes_from_the_database() {
    let mut tree = Tree::new("objects-ddl-view");
    let before = tree.app.consoles().len();
    select_row(&mut tree.app, 0, &[RowKind::View], "v_orders");
    tree.press(KeyCode::Char('g'));
    tree.press(KeyCode::Down);
    tree.press(KeyCode::Down);
    tree.press(KeyCode::Enter);
    let ddl = tree.wait_for_console(before);
    assert!(
        ddl.ends_with("CREATE VIEW v_orders AS SELECT id FROM orders;\n"),
        "{ddl}"
    );
}
