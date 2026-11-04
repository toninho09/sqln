// Key navigation from a row of a table tab: the keys that leave and that reach a table.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, open_table, prefix_page, shift, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::catalog::{Columns, ForeignKey, Relation, RelationRef};
use sqln::clipboard::MemoryClipboard;
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::editor::KeyboardMode;
use sqln::grid::{Row, Value};
use sqln::sql::Dialect;
use sqln::table::follow_choices;
use sqln::ui::text;
use sqln::ui::theme::Theme;

fn database(name: &str) -> std::path::PathBuf {
    let path = temp_db(name);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INT REFERENCES customers,
               total INT);
             CREATE TABLE items (order_id INT REFERENCES orders (id), line INT, sku TEXT,
               PRIMARY KEY (order_id, line));
             CREATE TABLE notes (order_id INT, line INT, body TEXT,
               FOREIGN KEY (order_id, line) REFERENCES items (order_id, line));
             CREATE TABLE payments (id INTEGER PRIMARY KEY, payer_id INT REFERENCES customers (id));
             CREATE VIEW v_orders AS SELECT * FROM orders;
             INSERT INTO customers VALUES (1, 'Ana'), (2, 'Bo');
             INSERT INTO orders VALUES (10, 1, 5), (11, NULL, 7);
             INSERT INTO items VALUES (10, 1, 'a'), (10, 2, 'b');
             INSERT INTO notes VALUES (10, 2, 'n');
             INSERT INTO payments VALUES (100, 2);",
        )
        .unwrap();
    path
}

struct Fixture {
    app: App,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let options = Options {
            keyboard: KeyboardMode::Conventional,
            connections: vec![Definition::sqlite(SqliteTarget::new(database(name)))],
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        Self {
            app: App::new(options, clipboard, Theme::plain()),
        }
    }

    fn press(&mut self, code: KeyCode) {
        let event = match code {
            KeyCode::Char(letter) if letter.is_ascii_uppercase() => shift(code),
            _ => key(code),
        };
        self.app.on_event(Event::Key(event));
    }

    fn write(&mut self, text: &str) {
        for letter in text.chars() {
            self.press(KeyCode::Char(letter));
        }
    }

    /// Waits until the tab on screen read its page.
    fn idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self
            .app
            .table()
            .is_none_or(|tab| tab.is_busy() || tab.pending)
        {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the page did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
    }

    fn open(&mut self, name: &str) {
        open_table(&mut self.app, 0, name);
        self.idle();
    }

    /// Presses `F` and waits for the keys of the table and for the page of the target.
    fn follow(&mut self) {
        let name = self.tab().0;
        self.press(KeyCode::Char('F'));
        self.wait_for_keys(&name);
    }

    /// Waits for the keys of a table, which arrive with its columns.
    fn wait_for_keys(&mut self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.table_columns(name) == Columns::NotLoaded {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the keys did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
        self.idle();
    }

    /// Returns the hint row: the last row of the screen.
    fn hints(&mut self) -> String {
        self.screen().lines().last().unwrap_or_default().to_owned()
    }

    fn tab(&self) -> (String, String, String, usize) {
        let tab = self.app.table().expect("a table tab");
        (
            tab.relation.relation.clone(),
            tab.applied_filter().to_owned(),
            tab.applied_order().to_owned(),
            tab.loaded(),
        )
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal.backend().buffer().content().chunks(120);
        rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn table_columns(&self, name: &str) -> Columns {
        let catalog = self.app.connections()[0].catalog.as_ref().unwrap();
        catalog.relation(None, name).unwrap().1.columns.clone()
    }
}

fn key_to(columns: &[&str], schema: &str, table: &str, targets: &[&str]) -> ForeignKey {
    ForeignKey {
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        target: RelationRef {
            schema: schema.into(),
            relation: table.into(),
        },
        target_columns: targets.iter().map(|column| (*column).to_owned()).collect(),
    }
}

#[test]
fn the_keys_of_the_cursor_column_come_first_and_a_null_key_stays_out() {
    let relation = Relation {
        name: "orders".into(),
        columns: Columns::Loaded(vec!["id".into(), "customer_id".into(), "shop".into()]),
        foreign_keys: vec![
            key_to(&["shop"], "sales", "shops", &["id"]),
            key_to(&["customer_id"], "public", "customers", &["id"]),
        ],
        referencing: vec![key_to(&["id"], "public", "items", &["order id"])],
        ..Relation::default()
    };
    let columns = ["id", "customer_id", "shop"].map(str::to_owned);
    let row: Row = [Value::Number("10"), Value::Number("1"), Value::Text("it's")]
        .into_iter()
        .collect();
    let choices = follow_choices("public", &relation, &columns, &row, 1, Dialect::Postgres);
    let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "→ customers (id)",
            "→ sales.shops (id)",
            "← items (order id)"
        ]
    );
    assert_eq!(choices[0].filter, "\"id\" = 1");
    assert_eq!(choices[1].filter, "\"id\" = E'it''s'");
    assert_eq!(choices[2].filter, "\"order id\" = 10");
    assert_eq!(choices[2].target.relation, "items");

    let row: Row = [Value::Number("11"), Value::Null, Value::Null]
        .into_iter()
        .collect();
    let choices = follow_choices("public", &relation, &columns, &row, 0, Dialect::MySql);
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].filter, "`order id` = 11");
}

#[test]
fn one_key_goes_to_the_row_that_it_reads_at_once() {
    let mut f = Fixture::new("follow-one");
    f.open("notes");
    assert_eq!(f.table_columns("notes"), Columns::NotLoaded);
    f.follow();
    assert_eq!(
        f.tab(),
        (
            "items".into(),
            "\"order_id\" = 10 AND \"line\" = 2".into(),
            String::new(),
            1
        )
    );
}

#[test]
fn several_keys_open_a_list_with_the_keys_of_the_column_first() {
    let mut f = Fixture::new("follow-list");
    f.open("orders");
    f.press(KeyCode::Right);
    f.follow();
    let screen = f.screen();
    assert!(screen.contains(text::TITLE_FOLLOW), "{screen}");
    let first = screen
        .find("→ customers (id)")
        .expect("the key of the column");
    let other = screen
        .find("← items (order_id)")
        .expect("a key that reaches the table");
    assert!(first < other, "{screen}");
    f.press(KeyCode::Enter);
    f.idle();
    assert_eq!(
        f.tab(),
        ("customers".into(), "\"id\" = 1".into(), String::new(), 1)
    );
}

#[test]
fn a_key_that_reaches_the_table_opens_the_rows_of_the_other_table() {
    let mut f = Fixture::new("follow-reach");
    f.open("customers");
    f.follow();
    let screen = f.screen();
    assert!(screen.contains("← orders (customer_id)"), "{screen}");
    assert!(screen.contains("← payments (payer_id)"), "{screen}");
    f.press(KeyCode::Enter);
    f.idle();
    assert_eq!(
        f.tab(),
        (
            "orders".into(),
            "\"customer_id\" = 1".into(),
            String::new(),
            1
        )
    );
}

#[test]
fn a_key_with_null_in_the_row_stays_out() {
    let mut f = Fixture::new("follow-null");
    f.open("orders");
    f.press(KeyCode::Down);
    // Without the key to `customers`, one key stays, so SQLN goes there at once.
    f.follow();
    assert_eq!(
        f.tab(),
        ("items".into(), "\"order_id\" = 11".into(), String::new(), 0)
    );
}

#[test]
fn an_open_tab_takes_the_new_filter_and_keeps_its_order() {
    let mut f = Fixture::new("follow-open");
    f.open("customers");
    f.press(KeyCode::Char('o'));
    f.write("name DESC");
    f.press(KeyCode::Enter);
    f.idle();
    assert_eq!(f.tab().2, "name DESC");
    f.open("orders");
    f.press(KeyCode::Right);
    f.follow();
    f.press(KeyCode::Enter);
    f.idle();
    assert_eq!(
        f.tab(),
        (
            "customers".into(),
            "\"id\" = 1".into(),
            "name DESC".into(),
            1
        )
    );
    assert_eq!(f.app.tables().len(), 2);
}

#[test]
fn a_view_has_no_key_to_follow() {
    let mut f = Fixture::new("follow-view");
    f.open("v_orders");
    f.press(KeyCode::Char('F'));
    let screen = f.screen();
    assert!(screen.contains(text::FOLLOW_REFUSED), "{screen}");
    f.press(KeyCode::Down);
    let hints = f.hints();
    assert!(!hints.contains("follow"), "{hints}");
}

#[test]
fn the_menu_and_the_hint_row_show_the_command() {
    let mut f = Fixture::new("follow-menu");
    f.open("orders");
    let hints = f.hints();
    assert!(hints.contains("F follow"), "{hints}");
    let page = prefix_page(&f.app).join("\n");
    assert!(page.contains(&format!("F {}", text::MENU_FOLLOW)), "{page}");
    f.app.on_event(Event::Key(ctrl('w')));
    f.press(KeyCode::Char('F'));
    f.wait_for_keys("orders");
    let screen = f.screen();
    assert!(screen.contains(text::TITLE_FOLLOW), "{screen}");
}
