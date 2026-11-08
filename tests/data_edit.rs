//! Pending cell changes, safe SQL, and real SQLite transactions through input events.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, open_table, temp_db};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use sqln::app::{App, Options};
use sqln::catalog::RelationRef;
use sqln::clipboard::MemoryClipboard;
use sqln::compare::schema::{Column, Table};
use sqln::connection::Definition;
use sqln::connector::sqlite::SqliteTarget;
use sqln::data_edit::{Edits, Metadata, Patch, SavedRow};
use sqln::editor::KeyboardMode;
use sqln::grid::{Budget, Reservation, ResultTable, Row, Value};
use sqln::sql::Dialect;
use sqln::ui::{text, theme::Theme};

fn row(values: &[Value<'_>]) -> Row {
    values.iter().copied().collect()
}

fn metadata(dialect: Dialect) -> Metadata {
    Metadata {
        relation: RelationRef {
            schema: "main".into(),
            relation: "odd\"table".into(),
        },
        dialect,
        table: Table {
            name: "odd\"table".into(),
            primary: vec!["id".into(), "part".into()],
            columns: vec![
                Column {
                    name: "id".into(),
                    ty: "int".into(),
                    ..Column::default()
                },
                Column {
                    name: "part".into(),
                    ty: "text".into(),
                    ..Column::default()
                },
                Column {
                    name: "value".into(),
                    ty: "text".into(),
                    nullable: true,
                    ..Column::default()
                },
            ],
            ..Table::default()
        },
    }
}

#[test]
fn updates_use_original_composite_keys_and_never_expressions() {
    let budget = Arc::new(Budget::new(1 << 20));
    for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
        let metadata = metadata(dialect);
        let patch = Patch {
            original: SavedRow::reserve(
                row(&[Value::Number("1"), Value::Text("a'b"), Value::Text("old")]),
                &budget,
            )
            .unwrap(),
            current: SavedRow::reserve(
                row(&[
                    Value::Number("1"),
                    Value::Text("a'b"),
                    Value::Text("'; DROP TABLE x; --\\\n雪"),
                ]),
                &budget,
            )
            .unwrap(),
            columns: BTreeSet::from([2]),
        };
        let sql = metadata.update_sql(&patch).unwrap();
        assert!(sql.starts_with("UPDATE "));
        assert!(sql.contains(" WHERE "));
        assert_eq!(sqln::sql::split(&sql, dialect).len(), 1, "{sql}");
        assert!(
            !metadata
                .lock_sql(&patch.original.row)
                .unwrap()
                .contains("LIMIT")
        );
        let mut protected = patch.clone();
        protected.columns = BTreeSet::from([0]);
        assert_eq!(
            metadata.update_sql(&protected).unwrap_err(),
            text::EDIT_PROTECTED
        );
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn pending_values_keep_originals_and_discard_needs_no_free_memory() {
    let budget = Arc::new(Budget::new(1000));
    let original = row(&[Value::Number("1"), Value::Text("old")]);
    let Reservation::Accepted(bytes) = budget.reserve(&original) else {
        panic!()
    };
    let mut table = ResultTable::new(vec!["id".into(), "value".into()]);
    table.append(vec![original.clone()], bytes);
    table.set_width(1, 17);
    let mut edits = Edits::default();
    edits
        .set(&mut table, 0, 1, Value::Text("new"), &budget)
        .unwrap();
    assert_eq!(edits.rows[&0].original.row, original);
    assert_eq!(table.rows[0].get(1), Some(Value::Text("new")));
    assert!(edits.changed(0, 1));
    assert_eq!(table.width(1), 17);
    budget.set_limit(budget.used());
    edits.discard(&mut table, &budget).unwrap();
    assert!(edits.is_empty());
    assert_eq!(table.rows[0], original);
    assert_eq!(budget.used(), table.bytes);
    budget.release(table.bytes);
    assert_eq!(budget.used(), 0);
}

#[test]
fn restoring_a_value_removes_the_pending_change() {
    let budget = Arc::new(Budget::new(1000));
    let original = row(&[Value::Text("old")]);
    let Reservation::Accepted(bytes) = budget.reserve(&original) else {
        panic!()
    };
    let mut table = ResultTable::new(vec!["value".into()]);
    table.append(vec![original], bytes);
    let mut edits = Edits::default();
    edits.set(&mut table, 0, 0, Value::Null, &budget).unwrap();
    edits
        .set(&mut table, 0, 0, Value::Text("old"), &budget)
        .unwrap();
    assert!(edits.is_empty());
    assert_eq!(budget.used(), table.bytes);
    budget.release(table.bytes);
}

struct Fixture {
    app: App,
    path: PathBuf,
}

impl Fixture {
    fn new(name: &str, keyboard: KeyboardMode) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path).unwrap().execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT, amount NUMERIC, required TEXT NOT NULL, computed TEXT GENERATED ALWAYS AS (value || '!') STORED);
             INSERT INTO items (id, value, amount, required) VALUES (1, 'old', 12.5, 'yes'), (2, 'second', 4, 'yes');
             CREATE TABLE no_key (value TEXT); INSERT INTO no_key VALUES ('a');
             CREATE VIEW item_view AS SELECT * FROM items;"
        ).unwrap();
        let options = Options {
            keyboard,
            connections: vec![Definition::sqlite(SqliteTarget::new(&path))],
            ..Options::default()
        };
        let app = App::new(
            options,
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        );
        Self { app, path }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.on_event(Event::Key(key(code)));
    }
    fn idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.app.drain_events();
            if self
                .app
                .table()
                .is_some_and(|tab| !tab.is_busy() && !tab.pending)
            {
                break;
            }
            assert!(Instant::now() < deadline, "{}", self.screen(80, 24));
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn open(&mut self, name: &str) {
        open_table(&mut self.app, 0, name);
        self.idle();
    }
    fn screen(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| self.app.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|line| line.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn click_bar_button(&mut self, label: &str) {
        let screen = self.screen(100, 30);
        let (row, column) = screen
            .lines()
            .enumerate()
            .find_map(|(row, line)| line.find(label).map(|column| (row, column)))
            .unwrap_or_else(|| panic!("{label} not in:\n{screen}"));
        self.app.on_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: column as u16,
            row: row as u16,
            modifiers: KeyModifiers::NONE,
        }));
    }
    fn edit(&mut self, value: &str) {
        self.press(KeyCode::F(2));
        self.idle();
        assert!(self.screen(80, 24).contains(text::EDIT_TITLE));
        self.app.on_event(Event::Key(ctrl('a')));
        self.app.on_event(Event::Paste(value.into()));
        self.press(KeyCode::F(2));
    }
    fn apply(&mut self) {
        self.app.on_event(Event::Key(ctrl('s')));
        self.idle();
        assert!(self.screen(80, 24).contains(text::EDIT_REVIEW));
        self.press(KeyCode::F(2));
        self.idle();
    }
    fn value(&self, id: i32) -> Option<String> {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT value FROM items WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .unwrap()
    }
    fn count(&self) -> i64 {
        rusqlite::Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .unwrap()
    }
}

#[test]
fn a_cell_stays_local_until_review_and_apply() {
    let mut f = Fixture::new("edit-local", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("new '雪'\nline");
    assert_eq!(f.value(1).as_deref(), Some("old"));
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    let screen = f.screen(80, 24);
    assert!(screen.contains("Pending: 1 update"), "{screen}");
    f.apply();
    assert_eq!(f.value(1).as_deref(), Some("new '雪'\nline"));
    assert!(f.app.table().unwrap().result.edits.is_empty());
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(4)
            .as_deref(),
        Some("new '雪'\nline!")
    );
}

#[test]
fn the_bar_offers_the_pending_edit_actions_with_the_mouse() {
    let mut f = Fixture::new("edit-bar-buttons", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("bar");
    let screen = f.screen(100, 30);
    for label in [text::EDIT_APPLY, text::BUTTON_REVIEW, text::BUTTON_DISCARD] {
        assert!(screen.contains(label), "{label} not in:\n{screen}");
    }
    // A click on Review opens the review dialog.
    f.click_bar_button(text::BUTTON_REVIEW);
    f.idle();
    assert!(
        f.screen(100, 30).contains(text::EDIT_REVIEW),
        "{}",
        f.screen(100, 30)
    );
    f.press(KeyCode::Esc);
    // A click on Discard opens the discard question.
    f.click_bar_button(text::BUTTON_DISCARD);
    f.idle();
    assert!(f.screen(100, 30).contains(text::EDIT_DISCARD_QUESTION));
    f.press(KeyCode::Esc);
    // A click on Apply changes goes through review.
    f.click_bar_button(text::EDIT_APPLY);
    f.idle();
    assert!(
        f.screen(100, 30).contains(text::EDIT_REVIEW),
        "{}",
        f.screen(100, 30)
    );
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
}

#[test]
fn a_concurrent_change_keeps_pending_text_and_saves_nothing() {
    let mut f = Fixture::new("edit-conflict", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("mine");
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute("UPDATE items SET required = 'outside' WHERE id = 1", [])
        .unwrap();
    f.apply();
    assert_eq!(f.value(1).as_deref(), Some("old"));
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(1)
            .as_deref(),
        Some("mine")
    );
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    assert!(f.screen(100, 30).contains("A row changed"));
}

#[test]
fn an_error_rolls_back_every_pending_row() {
    let mut f = Fixture::new("edit-atomic", KeyboardMode::Conventional);
    rusqlite::Connection::open(&f.path).unwrap().execute_batch("CREATE TRIGGER refuse_second BEFORE UPDATE ON items WHEN NEW.value = 'bad' BEGIN SELECT RAISE(ABORT, 'refused value'); END;").unwrap();
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("first");
    f.press(KeyCode::Down);
    f.edit("bad");
    f.apply();
    assert_eq!(f.value(1).as_deref(), Some("old"));
    assert_eq!(f.value(2).as_deref(), Some("second"));
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 2);
    assert!(f.screen(100, 30).contains("refused value"));
}

#[test]
fn null_empty_and_the_text_null_stay_distinct() {
    let mut f = Fixture::new("edit-nulls", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.app.on_event(Event::Key(ctrl('n')));
    f.idle();
    f.apply();
    assert_eq!(f.value(1), None);
    f.edit("");
    f.apply();
    assert_eq!(f.value(1).as_deref(), Some(""));
    f.edit("NULL");
    f.apply();
    assert_eq!(f.value(1).as_deref(), Some("NULL"));
}

#[test]
fn closing_and_navigation_do_not_discard_pending_values() {
    let mut f = Fixture::new("edit-close", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("pending");
    for event in [
        Event::Key(key(KeyCode::Char('s'))),
        Event::Key(key(KeyCode::Char(']'))),
        Event::Key(ctrl('c')),
        Event::Key(key(KeyCode::Char('e'))),
    ] {
        f.app.on_event(event);
        assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    }
    f.app.on_event(Event::Key(ctrl('w')));
    f.press(KeyCode::Char('c'));
    assert!(f.screen(80, 24).contains(text::EDIT_CLOSE_QUESTION));
    f.press(KeyCode::Esc);
    assert_eq!(f.app.tables().len(), 1);
    f.app.on_event(Event::Key(ctrl('d')));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    assert!(f.app.table().unwrap().result.edits.is_empty());
    assert_eq!(f.value(1).as_deref(), Some("old"));
}

#[test]
fn protected_columns_views_and_tables_without_keys_stay_unmodified() {
    let mut f = Fixture::new("edit-protected", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::F(2));
    f.idle();
    assert!(f.app.table().unwrap().result.edits.is_empty());
    assert!(f.screen(100, 30).contains("column cannot be edited"));
    f.open("no_key");
    f.press(KeyCode::F(2));
    f.idle();
    assert!(f.screen(100, 30).contains("primary key"));
    f.open("item_view");
    f.press(KeyCode::F(2));
    assert!(f.screen(100, 30).contains("does not support editing"));
}

#[test]
fn production_requires_confirmation_after_the_read_only_review() {
    let mut f = Fixture::new("edit-production", KeyboardMode::Conventional);
    let mut definition = Definition::sqlite(SqliteTarget::new(&f.path));
    definition.production = true;
    f.app = App::new(
        Options {
            connections: vec![definition],
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("approved");
    f.app.on_event(Event::Key(ctrl('s')));
    f.idle();
    f.press(KeyCode::F(2));
    let screen = f.screen(80, 24);
    assert!(screen.contains("production"), "{screen}");
    assert_eq!(f.value(1).as_deref(), Some("old"));
    f.press(KeyCode::Esc);
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    f.app.on_event(Event::Key(ctrl('s')));
    f.idle();
    f.press(KeyCode::F(2));
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    f.idle();
    assert_eq!(f.value(1).as_deref(), Some("approved"));
}

#[test]
fn read_only_and_empty_tables_offer_no_edit_command() {
    let mut f = Fixture::new("edit-read-only", KeyboardMode::Conventional);
    let mut definition = Definition::sqlite(SqliteTarget::new(&f.path));
    definition.read_only = true;
    f.app = App::new(
        Options {
            connections: vec![definition],
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    f.open("items");
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    assert!(f.screen(100, 30).contains("read-only"));
    assert!(f.app.table().unwrap().result.edits.is_empty());
    let mut f = Fixture::new("edit-empty", KeyboardMode::Conventional);
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute("DELETE FROM items", [])
        .unwrap();
    f.open("items");
    f.press(KeyCode::F(2));
    assert!(f.screen(100, 30).contains("No cell is selected"));
}

#[test]
fn the_cell_editor_keeps_global_shortcuts_local_and_supports_mouse_buttons() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut f = Fixture::new("edit-modal", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    f.idle();
    f.app.on_event(Event::Key(ctrl('w')));
    f.press(KeyCode::Char('q'));
    f.press(KeyCode::F(5));
    assert!(!f.app.should_quit());
    assert_eq!(f.value(1).as_deref(), Some("old"));
    f.app.on_event(Event::Key(ctrl('a')));
    f.app.on_event(Event::Paste("mouse 雪".into()));
    for (width, height) in [(80, 24), (120, 36), (80, 24)] {
        let screen = f.screen(width, height);
        assert!(screen.contains(text::EDIT_KEEP));
        assert!(screen.contains(text::EDIT_SET_NULL));
    }
    let screen = f.screen(80, 24);
    let (y, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text::EDIT_KEEP))
        .unwrap();
    let x = line.find(text::EDIT_KEEP).unwrap();
    f.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }));
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(1)
            .as_deref(),
        Some("mouse 雪")
    );
}

#[test]
fn custom_edit_shortcuts_appear_in_the_panel_and_preserve_enter() {
    use sqln::input::Keymap;
    let mut f = Fixture::new("edit-custom", KeyboardMode::Conventional);
    let overrides = std::collections::HashMap::from([("gridEditCell".into(), vec!["f6".into()])]);
    let keymap = Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap();
    f.app = App::new(
        Options {
            keymap: Some(keymap),
            connections: vec![Definition::sqlite(SqliteTarget::new(&f.path))],
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    f.open("items");
    f.press(KeyCode::Right);
    let menu: Vec<_> = f
        .app
        .context_menu()
        .into_iter()
        .flat_map(|group| group.keys)
        .map(|(key, label)| format!("{key} {label}"))
        .collect();
    assert!(
        menu.iter()
            .any(|row| row.contains("F6") && row.contains("Edit cell")),
        "{menu:?}"
    );
    f.press(KeyCode::Enter);
    assert!(!f.screen(80, 24).contains(text::EDIT_CELL_HINT));
    f.press(KeyCode::Esc);
    f.press(KeyCode::F(6));
    f.idle();
    assert!(f.screen(80, 24).contains(text::EDIT_TITLE));
}

#[test]
fn a_reload_preserves_cursor_selection_and_draft_filter_text() {
    let mut f = Fixture::new("edit-restore", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("changed");
    f.app
        .on_event(Event::Key(crate::common::shift(KeyCode::Down)));
    let view = f.app.table().unwrap().result.view;
    f.press(KeyCode::Char('f'));
    f.app.on_event(Event::Paste("id = 999".into()));
    f.press(KeyCode::Esc);
    f.apply();
    let tab = f.app.table().unwrap();
    assert_eq!(tab.result.view.row, view.row);
    assert_eq!(tab.result.view.col, view.col);
    assert_eq!(tab.result.view.anchor, view.anchor);
    assert_eq!(tab.filter.text(), "id = 999");
    assert_eq!(tab.applied_filter(), "");
    assert_eq!(f.value(1).as_deref(), Some("changed"));
}

#[test]
fn null_rejection_and_generated_columns_keep_the_database_unchanged() {
    let mut f = Fixture::new("edit-generated", KeyboardMode::Conventional);
    f.open("items");
    for _ in 0..3 {
        f.press(KeyCode::Right);
    }
    f.app.on_event(Event::Key(ctrl('n')));
    f.idle();
    assert!(f.screen(100, 30).contains("does not accept NULL"));
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    assert!(f.screen(100, 30).contains("column cannot be edited"));
    assert!(f.app.table().unwrap().result.edits.is_empty());
}

#[test]
fn a_budget_refusal_keeps_the_original_value_and_releases_temporary_snapshots() {
    let budget = Arc::new(Budget::new(100));
    let original = row(&[Value::Text("old")]);
    let Reservation::Accepted(bytes) = budget.reserve(&original) else {
        panic!()
    };
    let mut table = ResultTable::new(vec!["value".into()]);
    table.append(vec![original.clone()], bytes);
    let mut edits = Edits::default();
    assert!(
        edits
            .set(&mut table, 0, 0, Value::Text(&"x".repeat(200)), &budget)
            .is_err()
    );
    assert_eq!(table.rows[0], original);
    assert!(edits.is_empty());
    assert_eq!(budget.used(), table.bytes);
    budget.release(table.bytes);
}

#[test]
fn keeping_an_unchanged_multiline_value_preserves_control_bytes() {
    let mut f = Fixture::new("edit-control-bytes", KeyboardMode::Conventional);
    let original = "a\r\nb\t\u{1b}[31m雪";
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute("UPDATE items SET value = ?1 WHERE id = 1", [original])
        .unwrap();
    f.open("items");
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    f.idle();
    let screen = f.screen(80, 24);
    assert!(!screen.contains('\u{1b}'));
    f.press(KeyCode::F(2));
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(1)
            .as_deref(),
        Some(original)
    );
    assert!(f.app.table().unwrap().result.edits.is_empty());
}

#[test]
fn schema_changes_refuse_apply_and_keep_the_proposed_value() {
    let mut f = Fixture::new("edit-schema", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("pending");
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute("ALTER TABLE items ADD COLUMN extra TEXT", [])
        .unwrap();
    f.apply();
    assert_eq!(f.value(1).as_deref(), Some("old"));
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
}

#[test]
fn custom_editor_sequences_work_without_capturing_inserted_text() {
    use sqln::input::Keymap;
    let mut f = Fixture::new("edit-sequence", KeyboardMode::Conventional);
    let overrides = std::collections::HashMap::from([
        ("editorSelectAll".into(), vec!["f8 a".into()]),
        ("editorUndo".into(), vec!["u".into()]),
    ]);
    let keymap = Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap();
    f.app = App::new(
        Options {
            keymap: Some(keymap),
            connections: vec![Definition::sqlite(SqliteTarget::new(&f.path))],
            ..Options::default()
        },
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    );
    f.open("items");
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    f.idle();
    f.press(KeyCode::F(8));
    f.press(KeyCode::Char('a'));
    f.press(KeyCode::Char('u'));
    f.press(KeyCode::F(2));
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(1)
            .as_deref(),
        Some("u")
    );
}

#[test]
fn exit_resolves_each_dirty_table_and_cancel_keeps_the_rest() {
    let mut f = Fixture::new("edit-exit", KeyboardMode::Conventional);
    rusqlite::Connection::open(&f.path).unwrap().execute_batch("CREATE TABLE others (id INTEGER PRIMARY KEY, value TEXT); INSERT INTO others VALUES (1, 'other');").unwrap();
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("pending first");
    f.open("others");
    f.press(KeyCode::Right);
    f.edit("pending second");
    f.app.on_event(Event::Key(ctrl('w')));
    f.press(KeyCode::Char('q'));
    assert!(!f.app.should_quit());
    f.press(KeyCode::Tab);
    f.press(KeyCode::Tab);
    f.press(KeyCode::Enter);
    assert_eq!(f.app.tables().len(), 1);
    assert!(!f.app.should_quit());
    f.press(KeyCode::Esc);
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    assert!(!f.app.should_quit());
}

#[test]
fn vim_escape_keeps_navigation_keys_inside_the_value_editor() {
    let mut f = Fixture::new("edit-vim", KeyboardMode::Vim);
    f.open("items");
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    f.idle();
    f.press(KeyCode::Char('i'));
    f.app.on_event(Event::Paste("prefix-".into()));
    f.press(KeyCode::Esc);
    assert!(f.screen(80, 24).contains(text::EDIT_TITLE));
    f.press(KeyCode::F(2));
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(1)
            .as_deref(),
        Some("prefix-old")
    );
    assert_eq!(f.value(1).as_deref(), Some("old"));
}

#[test]
fn a_new_row_stays_local_until_apply_and_uses_the_column_defaults() {
    let mut f = Fixture::new("edit-insert", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Char('n'));
    f.idle();
    let tab = f.app.table().unwrap();
    assert_eq!(tab.result.edits.inserts.len(), 1);
    assert_eq!(tab.result.table.rows.len(), 3);
    let screen = f.screen(80, 24);
    assert!(screen.contains("Pending: 1 insert"), "{screen}");
    // The cursor starts at the id column of the new row, so F2 edits that cell.
    f.press(KeyCode::F(2));
    f.idle();
    assert!(f.screen(80, 24).contains(text::EDIT_TITLE));
    f.app.on_event(Event::Key(ctrl('a')));
    f.app.on_event(Event::Paste("9".into()));
    f.press(KeyCode::F(2));
    assert_eq!(
        f.app.table().unwrap().result.edits.inserts[&2]
            .columns
            .len(),
        1
    );
    // The columns of the fixture: id, value, amount, required, computed.
    for (steps, text) in [(1, "made"), (2, "3")] {
        for _ in 0..steps {
            f.press(KeyCode::Right);
        }
        f.press(KeyCode::F(2));
        f.idle();
        f.app.on_event(Event::Key(ctrl('a')));
        f.app.on_event(Event::Paste(text.into()));
        f.press(KeyCode::F(2));
    }
    assert_eq!(
        f.app.table().unwrap().result.edits.inserts[&2]
            .columns
            .len(),
        3
    );
    assert_eq!(f.count(), 2);
    f.apply();
    assert_eq!(f.count(), 3);
    let (id, value, required): (i64, String, String) = rusqlite::Connection::open(&f.path)
        .unwrap()
        .query_row(
            "SELECT id, value, required FROM items WHERE value = 'made'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((id, value.as_str(), required.as_str()), (9, "made", "3"));
    assert!(f.app.table().unwrap().result.edits.is_empty());
}

#[test]
fn marking_a_row_for_delete_wins_over_its_cell_changes_and_toggles_back() {
    let mut f = Fixture::new("edit-delete", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Right);
    f.edit("draft");
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    f.press(KeyCode::Char('d'));
    let tab = f.app.table().unwrap();
    assert_eq!(tab.result.edits.deletes.len(), 1);
    assert_eq!(tab.result.edits.cells(), 0);
    // The cell returns to the database value, because the delete wins.
    assert_eq!(tab.result.table.rows[0].text(1).as_deref(), Some("old"));
    let screen = f.screen(80, 24);
    assert!(screen.contains("Pending: 1 delete"), "{screen}");
    assert_eq!(f.count(), 2);
    // A second press removes the mark, and the row stays.
    f.press(KeyCode::Char('d'));
    assert!(f.app.table().unwrap().result.edits.is_empty());
    assert_eq!(f.count(), 2);
    // A third press marks it again, and the apply deletes the row.
    f.press(KeyCode::Char('d'));
    f.apply();
    assert_eq!(f.count(), 1);
    let present: i64 = rusqlite::Connection::open(&f.path)
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id = 1)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(present, 0);
}

#[test]
fn removing_a_pending_insert_row_runs_no_statement() {
    let mut f = Fixture::new("edit-insert-remove", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Char('n'));
    f.idle();
    assert_eq!(f.app.table().unwrap().result.table.rows.len(), 3);
    f.press(KeyCode::Char('d'));
    let tab = f.app.table().unwrap();
    assert!(tab.result.edits.is_empty());
    assert_eq!(tab.result.table.rows.len(), 2);
    assert_eq!(f.count(), 2);
}

#[test]
fn editing_a_cell_of_a_deleted_row_refuses_and_shows_the_reason() {
    let mut f = Fixture::new("edit-deleted-refusal", KeyboardMode::Conventional);
    f.open("items");
    f.press(KeyCode::Char('d'));
    f.idle();
    assert_eq!(f.app.table().unwrap().result.edits.deletes.len(), 1);
    f.press(KeyCode::Right);
    f.press(KeyCode::F(2));
    assert!(f.screen(100, 30).contains("marked for delete"));
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 0);
}
