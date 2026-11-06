// The SQL text of the navigator: the templates, the list, and the copy of a name.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, select_row, temp_db};
use crossterm::event::{Event, KeyCode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::{Clipboard, MemoryClipboard};
use sqln::connection::{Definition, Source};
use sqln::connector::target::parse_uri;
use sqln::generate::{Template, generate, qualified_name};
use sqln::navigator::RowKind;
use sqln::sql::Dialect;
use sqln::ui::theme::Theme;

fn columns() -> Vec<String> {
    vec!["id".to_owned(), "order".to_owned()]
}

#[test]
fn each_template_writes_its_statement() {
    let write = |template| generate(template, Dialect::Postgres, "t", &columns());
    assert_eq!(write(Template::SelectAll), "SELECT *\nFROM t\nLIMIT 100;");
    assert_eq!(
        write(Template::SelectColumns),
        "SELECT id, \"order\"\nFROM t\nLIMIT 100;"
    );
    assert_eq!(
        write(Template::Insert),
        "INSERT INTO t (id, \"order\")\nVALUES (<id>, <order>);"
    );
    assert_eq!(
        write(Template::Update),
        "UPDATE t\nSET id = <id>, \"order\" = <order>\nWHERE ;"
    );
    assert_eq!(write(Template::Delete), "DELETE FROM t\nWHERE ;");
}

#[test]
fn a_name_follows_the_quotes_of_the_dialect() {
    let name = qualified_name(Some("My App"), "order", Dialect::MySql);
    assert_eq!(name, "`My App`.`order`");
    let name = qualified_name(Some("Sales"), "Orders", Dialect::Postgres);
    assert_eq!(name, "\"Sales\".\"Orders\"");
    assert_eq!(qualified_name(None, "t", Dialect::Sqlite), "t");
    assert_eq!(
        generate(Template::Insert, Dialect::MySql, "t", &columns()),
        "INSERT INTO t (id, `order`)\nVALUES (<id>, <order>);"
    );
}

#[test]
fn only_the_templates_with_columns_need_them() {
    assert!(!Template::SelectAll.needs_columns());
    assert!(!Template::Delete.needs_columns());
    assert!(Template::SelectColumns.needs_columns());
    assert!(Template::Insert.needs_columns());
    assert!(Template::Update.needs_columns());
    assert_eq!(
        Template::READS,
        [Template::SelectAll, Template::SelectColumns]
    );
}

struct Tree {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
    path: std::path::PathBuf,
}

impl Tree {
    fn new(name: &str) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE t (id INT, 'order' TEXT);
                 INSERT INTO t VALUES (1, 'a');
                 CREATE VIEW v AS SELECT id FROM t;",
            )
            .unwrap();
        let options = Options {
            database: Some(path.clone()),
            ..Options::default()
        };
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let app = App::new(options, clipboard.clone(), Theme::plain());
        Self {
            app,
            clipboard,
            path,
        }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }

    fn select(&mut self, kinds: &[RowKind], name: &str) {
        select_row(&mut self.app, 0, kinds, name);
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        let rows = terminal.backend().buffer().content().chunks(100);
        rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn text(&self) -> String {
        self.app.console().editor.text()
    }

    /// Chooses a row of the list, and waits for the columns of its template.
    fn choose(&mut self, row: usize) {
        for _ in 0..row {
            self.press(KeyCode::Down);
        }
        self.press(KeyCode::Enter);
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.navigator_focused() {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the columns did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn rows(&self) -> i64 {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT count(*) FROM t", [], |row| row.get(0))
            .unwrap()
    }

    fn run_all(&mut self) -> String {
        self.app.on_event(Event::Key(ctrl('a')));
        self.press(KeyCode::F(5));
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.app.console().is_busy() {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the run did not end");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
        self.screen()
    }
}

#[test]
fn the_key_inserts_a_template_in_the_console_and_does_not_run_it() {
    let mut tree = Tree::new("generate-insert");
    tree.select(&[RowKind::Table], "t");
    tree.press(KeyCode::Char('g'));
    assert!(tree.screen().contains("Generate SQL"));
    tree.choose(0);
    assert_eq!(tree.text(), "SELECT *\nFROM t\nLIMIT 100;");
    assert!(!tree.app.navigator_focused());
    assert!(tree.app.console().results.is_empty());
}

#[test]
fn a_template_with_columns_reads_them_first() {
    let mut tree = Tree::new("generate-columns");
    tree.select(&[RowKind::Table], "t");
    tree.press(KeyCode::Char('g'));
    tree.choose(2);
    assert_eq!(
        tree.text(),
        "INSERT INTO t (id, \"order\")\nVALUES (<id>, <order>);"
    );
}

#[test]
fn a_column_uses_its_table_and_a_view_lists_only_reads() {
    let mut tree = Tree::new("generate-view");
    tree.select(&[RowKind::View], "v");
    tree.press(KeyCode::Char('g'));
    let screen = tree.screen();
    assert!(screen.contains("SELECT *"), "{screen}");
    assert!(!screen.contains("INSERT"), "{screen}");
    tree.press(KeyCode::Esc);

    tree.select(&[RowKind::Table], "t");
    tree.press(KeyCode::Right);
    let deadline = Instant::now() + Duration::from_secs(10);
    while tree.app.navigator().rows(tree.app.connections()).len() < 6 {
        tree.app.drain_events();
        assert!(Instant::now() < deadline, "the columns did not arrive");
    }
    tree.select(&[RowKind::Column], "id");
    tree.press(KeyCode::Char('g'));
    tree.choose(4);
    assert_eq!(tree.text(), "DELETE FROM t\nWHERE ;");
}

#[test]
fn the_text_goes_to_the_cursor_of_the_console_of_the_connection() {
    let mut tree = Tree::new("generate-cursor");
    tree.app.on_event(Event::Paste("SELECT 1;\n".to_owned()));
    tree.select(&[RowKind::Table], "t");
    tree.press(KeyCode::Char('g'));
    tree.choose(0);
    assert_eq!(tree.text(), "SELECT 1;\nSELECT *\nFROM t\nLIMIT 100;");
    assert_eq!(tree.app.consoles().len(), 1);
}

#[test]
fn the_database_refuses_a_template_that_the_user_did_not_complete() {
    for (row, name) in [(2, "insert"), (3, "update"), (4, "delete")] {
        let mut tree = Tree::new(&format!("generate-refused-{name}"));
        tree.select(&[RowKind::Table], "t");
        tree.press(KeyCode::Char('g'));
        tree.choose(row);
        let screen = tree.run_all();
        assert_eq!(tree.rows(), 1, "{name}: {screen}");
        assert!(!screen.contains("Confirm the run"), "{name}: {screen}");
        assert_eq!(tree.app.console().results.len(), 0, "{name}");
        let message = &tree.app.console().messages;
        assert!(
            message.iter().any(|item| item.text.contains("Statement 1")),
            "{name}: {message:?}"
        );
        assert!(
            !message.iter().any(|item| item.text.contains("changed")),
            "{name}: {message:?}"
        );
    }
}

#[test]
fn an_update_with_a_where_and_no_values_changes_nothing() {
    let mut tree = Tree::new("generate-update-values");
    tree.select(&[RowKind::Table], "t");
    tree.press(KeyCode::Char('g'));
    tree.choose(3);
    tree.app.on_event(Event::Key(ctrl('a')));
    tree.app.on_event(Event::Paste(
        "UPDATE t\nSET id = <id>, \"order\" = <order>\nWHERE id = 1;".to_owned(),
    ));
    tree.run_all();
    let value: Option<i64> = rusqlite::Connection::open(&tree.path)
        .unwrap()
        .query_row("SELECT id FROM t", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, Some(1));
}

#[test]
fn the_copy_key_gives_the_name_of_the_row_without_quotes() {
    let mut tree = Tree::new("generate-copy");
    tree.select(&[RowKind::Table], "t");
    tree.press(KeyCode::Char('y'));
    assert_eq!(tree.clipboard.borrow_mut().paste().as_deref(), Some("t"));

    tree.press(KeyCode::Right);
    let deadline = Instant::now() + Duration::from_secs(10);
    while tree.app.navigator().rows(tree.app.connections()).len() < 6 {
        tree.app.drain_events();
        assert!(Instant::now() < deadline, "the columns did not arrive");
    }
    tree.select(&[RowKind::Column], "order");
    tree.app.on_event(Event::Key(ctrl('c')));
    assert_eq!(
        tree.clipboard.borrow_mut().paste().as_deref(),
        Some("order")
    );
}

#[test]
fn a_connection_row_has_no_template_and_keeps_its_copy() {
    let mut tree = Tree::new("generate-connection");
    tree.select(&[RowKind::Table], "t");
    for _ in 0..10 {
        tree.press(KeyCode::Up);
    }
    let row = tree.app.navigator().current(tree.app.connections());
    assert_eq!(row.map(|row| row.kind), Some(RowKind::Connection));
    tree.press(KeyCode::Char('g'));
    assert!(!tree.screen().contains("Generate SQL"));
    tree.press(KeyCode::Char('y'));
    assert!(tree.clipboard.borrow_mut().paste().is_some());
}

#[test]
fn a_redis_key_has_no_template_and_keeps_its_copy() {
    let target = parse_uri("redis://127.0.0.1:1").unwrap();
    let options = Options {
        connections: vec![Definition::new("cache".to_owned(), Source::Network(target))],
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard.clone(), Theme::plain());
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char('h'))));
    app.on_event(Event::Key(key(KeyCode::Char('g'))));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(!screen.contains("Generate SQL"));
    app.on_event(Event::Key(key(KeyCode::Char('y'))));
    assert_eq!(clipboard.borrow_mut().paste().as_deref(), Some("cache"));
}
