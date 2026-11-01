#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use sqln::app::App;
use sqln::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::{KeyboardMode, SqlEditor};
use sqln::sql::Dialect;

/// A fixed catalog for a test without a database.
pub fn sample_catalog() -> Catalog {
    let relation = |name: &str, kind, columns: &[&str]| Relation {
        name: name.to_owned(),
        kind,
        columns: Columns::Loaded(columns.iter().map(|column| (*column).to_owned()).collect()),
    };
    Catalog {
        schemas: vec![
            Schema {
                name: "public".to_owned(),
                relations: vec![
                    relation(
                        "users",
                        RelationKind::Table,
                        &["id", "name", "email", "created_at"],
                    ),
                    relation(
                        "orders",
                        RelationKind::Table,
                        &["id", "user_id", "total", "status"],
                    ),
                    relation("active_users", RelationKind::View, &["id", "name"]),
                    relation(
                        "Order Items",
                        RelationKind::Table,
                        &["order_id", "sku", "quantity"],
                    ),
                ],
                partial: false,
            },
            Schema {
                name: "sales".to_owned(),
                relations: vec![relation(
                    "invoices",
                    RelationKind::Table,
                    &["number", "due_date"],
                )],
                partial: false,
            },
        ],
        current_schema: Some("public".to_owned()),
    }
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

pub fn alt(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub struct Fixture {
    pub editor: SqlEditor,
    pub clipboard: Rc<RefCell<MemoryClipboard>>,
    pub catalog: Catalog,
}

impl Fixture {
    pub fn new(text: &str, keyboard: KeyboardMode) -> Self {
        Self::with_dialect(text, keyboard, Dialect::Postgres)
    }

    pub fn with_dialect(text: &str, keyboard: KeyboardMode, dialect: Dialect) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let editor = SqlEditor::new(text, keyboard, dialect, clipboard.clone());
        Self {
            editor,
            clipboard,
            catalog: sample_catalog(),
        }
    }

    pub fn conventional(text: &str) -> Self {
        Self::new(text, KeyboardMode::Conventional)
    }

    pub fn vim(text: &str) -> Self {
        Self::new(text, KeyboardMode::Vim)
    }

    /// Sends one key through the path that the application uses.
    pub fn press(&mut self, key: KeyEvent) {
        if !self
            .editor
            .on_list_key(key, sqln::input::SuggestionKeys::Tab)
        {
            self.editor.on_key(key, Some(&self.catalog));
        }
    }

    pub fn press_all(&mut self, keys: &[KeyEvent]) {
        for key in keys {
            self.press(*key);
        }
    }

    pub fn write(&mut self, text: &str) {
        for c in text.chars() {
            self.press(key(KeyCode::Char(c)));
        }
    }

    pub fn text(&self) -> String {
        self.editor.text()
    }

    pub fn labels(&self) -> Vec<String> {
        self.editor.suggestions().map_or_else(Vec::new, |list| {
            list.completion
                .items
                .iter()
                .map(|item| item.label.clone())
                .collect()
        })
    }

    pub fn clipboard_text(&self) -> String {
        self.clipboard.borrow().text.clone()
    }

    pub fn selected(&self) -> Option<String> {
        self.editor.selected_text()
    }
}

/// Returns a path for a database of one test. The file does not exist.
pub fn temp_db(name: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.db"));
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    path
}

/// Opens a table with the keys of the navigator. `Enter` on its row does it.
/// The call waits for the catalog, and it does not wait for the first page.
pub fn open_table(app: &mut App, connection: usize, name: &str) {
    use sqln::navigator::RowKind;

    select_row(app, connection, &[RowKind::Table, RowKind::View], name);
    app.on_event(Event::Key(key(KeyCode::Enter)));
}

/// Moves the cursor of the navigator to a row, with the keys of the navigator. It opens
/// the rows above the target and waits for the catalog.
pub fn select_row(
    app: &mut App,
    connection: usize,
    kinds: &[sqln::navigator::RowKind],
    name: &str,
) {
    use sqln::navigator::RowKind;

    use sqln::connection::CatalogState;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // A catalog in a new read has no new table yet.
    while app.connections()[connection].catalog_state == CatalogState::Loading {
        app.drain_events();
        assert!(
            std::time::Instant::now() < deadline,
            "the catalog did not arrive"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let press = |app: &mut App, code| app.on_event(Event::Key(key(code)));
    app.on_event(Event::Key(ctrl('w')));
    press(app, KeyCode::Char('h'));
    assert!(app.navigator_focused());
    // The search goes down from the first row.
    for _ in 0..50 {
        press(app, KeyCode::Up);
    }
    for _ in 0..500 {
        let row = app.navigator().current(app.connections()).unwrap();
        let is_target = row.connection == connection
            && kinds.contains(&row.kind)
            && row.name(app.connections()) == name;
        if is_target {
            return;
        }
        let branch = matches!(row.kind, RowKind::Connection | RowKind::Schema);
        let closed = !app.navigator().is_open(&row, app.connections());
        if row.connection == connection && branch && closed {
            press(app, KeyCode::Right);
            // A connection waits for its catalog, and a schema opens at once.
            while row.kind == RowKind::Connection && app.connections()[connection].catalog.is_none()
            {
                app.drain_events();
                assert!(
                    std::time::Instant::now() < deadline,
                    "the catalog did not arrive"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        } else {
            press(app, KeyCode::Down);
        }
    }
    panic!("{name} is not in the navigator");
}

/// Returns an empty folder that belongs to one test.
pub fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("dir-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}
