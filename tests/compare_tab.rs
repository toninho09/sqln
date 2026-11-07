// The compare tab in the application: the dialog, the read of two real SQLite files,
// the grid, the script console, the keys, and the close.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::common::{ctrl, key, temp_dir};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::compare::Kind;
use sqln::compare::form::Field;
use sqln::navigator::RowKind;
use sqln::store::{Root, Store};
use sqln::ui::text;
use sqln::ui::theme::Theme;

const SOURCE: &str = "\
    CREATE TABLE owners(id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
    CREATE TABLE items(id INTEGER PRIMARY KEY, owner INTEGER REFERENCES owners(id), label TEXT);\
    CREATE INDEX items_label ON items(label);";

const TARGET: &str = "\
    CREATE TABLE owners(id INTEGER PRIMARY KEY, name TEXT);\
    CREATE TABLE old(id INTEGER);\
    INSERT INTO owners VALUES (1, 'one');";

struct Fixture {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
    target: PathBuf,
}

impl Fixture {
    /// Opens two saved SQLite connections: `source` and `target`.
    fn new(name: &str, source: &str, target: &str) -> Self {
        Self::with(name, source, target, "{}")
    }

    /// Opens the two connections with `preferences` in the configuration.
    fn with(name: &str, source: &str, target: &str, preferences: &str) -> Self {
        let base = temp_dir(name);
        let global = Root::global(base.join("global"));
        fs::create_dir_all(&global.dir).unwrap();
        let files = [("source", source), ("target", target)].map(|(file, sql)| {
            let path = base.join(format!("{file}.db"));
            rusqlite::Connection::open(&path)
                .unwrap()
                .execute_batch(sql)
                .unwrap();
            path
        });
        let entry = |id: &str, path: &PathBuf| {
            format!(
                r#"{{"id":"{id}","name":"{id}","kind":"sqlite","file":"{}"}}"#,
                path.display()
            )
        };
        let config = format!(
            r#"{{"version":1,"preferences":{preferences},"connections":[{},{}]}}"#,
            entry("source", &files[0]),
            entry("target", &files[1])
        );
        fs::write(global.config_file(), config).unwrap();
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let options = Options {
            store: Some(Store {
                global,
                project: None,
            }),
            ..Options::default()
        };
        let [_, target] = files;
        Self {
            app: App::new(options, clipboard.clone(), Theme::plain()),
            clipboard,
            target,
        }
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn char(&mut self, c: char) {
        self.code(KeyCode::Char(c));
    }

    fn wait(&mut self, done: impl Fn(&App) -> bool, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if done(&self.app) {
                break;
            }
            assert!(Instant::now() < deadline, "{what} did not happen");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
    }

    /// Presses `c` on a row of the source and waits for the schemas of both sides.
    fn dialog(&mut self, kinds: &[RowKind], name: &str) {
        crate::common::select_row(&mut self.app, 0, kinds, name);
        self.char('c');
        self.wait(
            |app| {
                app.compare_form()
                    .is_some_and(|form| form.sides.iter().all(|side| side.schemas.is_some()))
            },
            "the schemas of the dialog",
        );
    }

    /// Moves the focus of the dialog to a field.
    fn focus(&mut self, field: Field) {
        for _ in 0..Field::ALL.len() {
            if self
                .app
                .compare_form()
                .is_some_and(|form| form.focus == field)
            {
                return;
            }
            self.code(KeyCode::Tab);
        }
        panic!("the dialog has no field {field:?}");
    }

    /// Presses the button of the dialog and waits for the end of the compare.
    fn compare(&mut self) {
        self.focus(Field::Compare);
        self.code(KeyCode::Enter);
        assert!(self.app.compare_form().is_none(), "the dialog stays open");
        self.idle();
    }

    fn idle(&mut self) {
        self.wait(
            |app| app.compare().is_some_and(|tab| !tab.is_busy()),
            "the end of the compare",
        );
    }

    fn rows(&self) -> Vec<Vec<String>> {
        let tab = self.app.compare().expect("a compare on screen");
        tab.result
            .table
            .rows
            .iter()
            .map(|row| {
                (0..6)
                    .map(|column| row.text(column).unwrap_or_default())
                    .collect()
            })
            .collect()
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
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
}

#[test]
fn c_on_a_connection_compares_its_schema_with_the_next_connection() {
    let mut f = Fixture::new("compare-schema", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.choice(0).unwrap().name, "source");
    assert_eq!(form.choice(1).unwrap().name, "target");
    assert_eq!(form.sides[1].schema_name(), Some("main"));
    f.compare();
    let rows = f.rows();
    assert_eq!(
        rows,
        [
            ["create", "table", "items", "items", "3 columns", ""],
            ["drop", "table", "old", "old", "", "1 column"],
            ["alter", "column", "owners", "name", "TEXT NOT NULL", "TEXT"],
        ]
        .map(|row| row.map(str::to_owned).to_vec())
    );
    let screen = f.screen();
    assert!(
        screen.contains("source · main → target · main · compare"),
        "{screen}"
    );
    assert!(screen.contains("3 differences"), "{screen}");
}

#[test]
fn s_opens_the_script_in_a_new_console_of_the_target_and_the_script_syncs_it() {
    let mut f = Fixture::new("compare-script", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.compare();
    let consoles = f.app.consoles().len();
    f.char('S');
    assert!(f.app.compare().is_none(), "the console takes the screen");
    let console = f.app.console();
    let target = f.app.connections()[1].id;
    assert_eq!(console.connection, Some(target));
    let script = console.editor.text();
    assert!(
        script.starts_with("-- SQLN structure compare\n"),
        "{script}"
    );
    assert!(
        script.contains("-- DROP TABLE \"main\".\"old\";"),
        "{script}"
    );
    assert!(f.app.consoles().len() >= consoles);
    // The script runs as the user would run it, and a new read finds no differences
    // but the removal.
    rusqlite::Connection::open(&f.target)
        .unwrap()
        .execute_batch(&script)
        .unwrap();
    f.press(ctrl('w'));
    f.char('p');
    f.wait(|app| app.compare().is_some(), "the compare tab");
    f.char('R');
    f.idle();
    let rows = f.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0][0], "drop");
    let name: String = rusqlite::Connection::open(&f.target)
        .unwrap()
        .query_row("SELECT name FROM owners WHERE id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(name, "one");
}

#[test]
fn removals_as_statements_make_the_target_equal() {
    let mut f = Fixture::new("compare-removals", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.focus(Field::Removals);
    f.char(' ');
    assert!(f.app.compare_form().unwrap().removals);
    f.compare();
    let script = f.app.compare().unwrap().script.clone();
    assert!(!script.contains("-- DROP"), "{script}");
    rusqlite::Connection::open(&f.target)
        .unwrap()
        .execute_batch(&script)
        .unwrap();
    f.char('R');
    f.idle();
    let tab = f.app.compare().unwrap();
    assert!(tab.differences.is_empty(), "{:?}", tab.differences);
    assert_eq!(
        tab.message.as_ref().map(|message| message.text.as_str()),
        Some(text::COMPARE_EQUAL)
    );
    f.char('S');
    assert!(f.app.compare().is_some(), "no script, so no console");
}

#[test]
fn a_table_row_compares_only_its_table() {
    let mut f = Fixture::new("compare-table", SOURCE, TARGET);
    f.dialog(&[RowKind::Table], "owners");
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.kind, Kind::Data);
    assert_eq!(form.sides[0].table.text(), "owners");
    assert_eq!(form.sides[1].table.text(), "owners");
    f.focus(Field::Kind);
    f.code(KeyCode::Right);
    assert_eq!(f.app.compare_form().unwrap().kind, Kind::Structure);
    f.compare();
    let rows = f.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0][2], "owners");
    assert!(f.app.compare().unwrap().title().contains("main.owners"));
}

#[test]
fn y_copies_the_script_and_the_close_frees_the_budget() {
    let mut f = Fixture::new("compare-copy", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.compare();
    f.char('Y');
    let copied = f.clipboard.borrow().text.clone();
    assert_eq!(copied, f.app.compare().unwrap().script);
    f.press(ctrl('w'));
    f.char('c');
    assert!(f.app.compares().is_empty());
}

#[test]
fn the_dialog_refuses_the_source_as_the_target() {
    let mut f = Fixture::new("compare-same", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.focus(Field::TargetConnection);
    f.code(KeyCode::Right);
    f.wait(
        |app| {
            app.compare_form()
                .is_some_and(|form| form.sides[1].schemas.is_some())
        },
        "the schemas of the target",
    );
    assert_eq!(
        f.app.compare_form().unwrap().choice(1).unwrap().name,
        "source"
    );
    f.focus(Field::Compare);
    f.code(KeyCode::Enter);
    let form = f.app.compare_form().expect("the dialog stays open");
    assert_eq!(form.note.as_deref(), Some(text::COMPARE_SAME_SIDES));
    f.code(KeyCode::Esc);
    assert!(f.app.compare_form().is_none());
    assert!(f.app.compares().is_empty());
}

#[test]
fn the_explorer_page_and_the_palette_offer_the_compare() {
    let mut f = Fixture::new("compare-menu", SOURCE, TARGET);
    crate::common::select_row(&mut f.app, 0, &[RowKind::Table], "owners");
    assert!(crate::common::explorer_page(&f.app).contains(&"c Compare".to_owned()));
    f.press(ctrl('p'));
    f.app.on_event(Event::Paste("compare".to_owned()));
    let screen = f.screen();
    // The focused explorer uses its selected row, without a duplicate global entry.
    assert!(
        screen.contains(text::command_text(sqln::input::Action::NavigatorCompare)),
        "{screen}"
    );
    assert!(screen.contains("owners"), "{screen}");
    assert!(!screen.contains("Ctrl+W v"), "{screen}");
}

#[test]
fn a_view_row_refuses_the_compare() {
    let mut f = Fixture::new(
        "compare-view",
        &format!("{SOURCE}CREATE VIEW labels AS SELECT label FROM items;"),
        TARGET,
    );
    crate::common::select_row(&mut f.app, 0, &[RowKind::View], "labels");
    f.char('c');
    assert!(f.app.compare_form().is_none());
    assert!(!crate::common::explorer_page(&f.app).contains(&"c Compare".to_owned()));
}

#[test]
fn the_dialog_fits_the_smallest_screen() {
    let mut f = Fixture::new("compare-small", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let screen: String = (0..buffer.area.height)
        .map(|y| {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            row + "\n"
        })
        .collect();
    for text in [
        text::TITLE_COMPARE,
        text::FIELD_COMPARE_REMOVALS,
        "< target >",
        "< main >",
        text::BUTTON_COMPARE,
    ] {
        assert!(
            screen.contains(text),
            "{text} is not on the screen:\n{screen}"
        );
    }
}

const DATA_SOURCE: &str = "\
    CREATE TABLE prices(id INTEGER PRIMARY KEY, name TEXT, price NUMERIC, note TEXT);\
    INSERT INTO prices VALUES (1, 'same', 10.5, NULL), (2, 'old name', 3, 'x'), (3, 'new', 1, 'it''s');";

const DATA_TARGET: &str = "\
    CREATE TABLE prices(id INTEGER PRIMARY KEY, name TEXT, price NUMERIC, note TEXT, extra TEXT);\
    INSERT INTO prices VALUES (1, 'same', 10.50, NULL, 'kept'), (2, 'name', 3, NULL, NULL), \
      (4, 'gone', 9, NULL, NULL);";

#[test]
fn a_data_compare_lists_the_rows_and_its_script_makes_the_target_equal() {
    let mut f = Fixture::new("compare-data", DATA_SOURCE, DATA_TARGET);
    f.dialog(&[RowKind::Table], "prices");
    f.compare();
    let tab = f.app.compare().unwrap();
    assert_eq!(
        tab.title(),
        "source · main.prices → target · main.prices · data compare"
    );
    assert_eq!(tab.count, 3);
    let columns = &tab.result.table.columns;
    assert_eq!(columns, &["Change", "id", "name", "price", "note"]);
    let rows: Vec<Vec<String>> = tab
        .result
        .table
        .rows
        .iter()
        .map(|row| {
            (0..5)
                .map(|column| row.text(column).unwrap_or_default())
                .collect()
        })
        .collect();
    assert_eq!(
        rows,
        [
            ["update name, note", "2", "old name", "3", "x"],
            ["  target", "2", "name", "3", ""],
            ["delete", "4", "gone", "9", ""],
            ["insert", "3", "new", "1", "it's"],
        ]
        .map(|row| row.map(str::to_owned).to_vec())
    );
    let script = tab.script.clone();
    assert!(script.starts_with("-- SQLN data compare\n"), "{script}");
    assert!(
        script.contains(
            "UPDATE \"main\".\"prices\" SET \"name\" = 'old name', \"note\" = 'x' WHERE \"id\" = 2;"
        ),
        "{script}"
    );
    assert!(script.contains("-- DELETE FROM \"main\".\"prices\" WHERE \"id\" = 4;"));
    assert!(script.contains(
        "INSERT INTO \"main\".\"prices\" (\"id\", \"name\", \"price\", \"note\") \
         VALUES (3, 'new', 1, 'it''s');"
    ));
    rusqlite::Connection::open(&f.target)
        .unwrap()
        .execute_batch(&script)
        .unwrap();
    f.char('R');
    f.idle();
    let tab = f.app.compare().unwrap();
    // The removal stays a comment, so only the row of the target stays.
    assert_eq!(tab.count, 1, "{:?}", f.rows());
    let extra: String = rusqlite::Connection::open(&f.target)
        .unwrap()
        .query_row("SELECT extra FROM prices WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(extra, "kept");
}

#[test]
fn a_data_compare_refuses_a_table_without_a_key_and_a_missing_column() {
    let mut f = Fixture::new(
        "compare-data-refused",
        "CREATE TABLE loose(a TEXT); CREATE TABLE wide(id INTEGER PRIMARY KEY, b TEXT);",
        "CREATE TABLE loose(a TEXT); CREATE TABLE wide(id INTEGER PRIMARY KEY);",
    );
    let reason = |f: &Fixture| {
        let tab = f.app.compare().unwrap();
        assert!(!tab.done);
        tab.message.as_ref().map(|message| message.text.clone())
    };
    f.dialog(&[RowKind::Table], "loose");
    f.compare();
    assert_eq!(reason(&f).as_deref(), Some(text::COMPARE_NO_KEY));
    f.dialog(&[RowKind::Table], "wide");
    f.compare();
    assert_eq!(reason(&f), Some(text::compare_missing_column("b")));
}

#[test]
fn a_data_compare_that_does_not_fit_the_budget_shows_nothing() {
    let rows: String = (0..2000)
        .map(|id| format!("INSERT INTO big VALUES ({id}, '{}');", "x".repeat(200)))
        .collect();
    let source = format!("BEGIN;CREATE TABLE big(id INTEGER PRIMARY KEY, body TEXT);{rows}COMMIT;");
    let mut f = Fixture::with(
        "compare-data-budget",
        &source,
        "CREATE TABLE big(id INTEGER PRIMARY KEY, body TEXT);",
        r#"{"gridBudgetBytes":65536}"#,
    );
    f.dialog(&[RowKind::Table], "big");
    f.compare();
    let tab = f.app.compare().unwrap();
    assert!(!tab.done);
    assert_eq!(
        tab.message.as_ref().map(|message| message.text.as_str()),
        Some(text::COMPARE_DATA_BUDGET)
    );
}

/// Returns the hint row at a width.
fn hints_at(f: &mut Fixture, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let y = buffer.area.height - 1;
    let row: String = (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect();
    row.trim().to_owned()
}

#[test]
fn the_hint_row_adds_compare_for_each_supported_row_on_a_wide_screen() {
    let mut f = Fixture::new("compare-hints", SOURCE, TARGET);
    crate::common::select_row(&mut f.app, 0, &[RowKind::Table], "owners");
    let row = hints_at(&mut f, 80);
    assert!(row.contains("Ctrl+P commands"), "{row}");
    assert!(hints_at(&mut f, 200).contains("c compare"));
    crate::common::select_row(&mut f.app, 0, &[RowKind::Schema], "main");
    assert!(hints_at(&mut f, 200).contains("c compare"));
    crate::common::select_row(&mut f.app, 0, &[RowKind::Connection], "source");
    let row = hints_at(&mut f, 200);
    assert!(row.contains("c compare"), "{row}");
    assert!(row.contains("Ctrl+P commands"), "{row}");
}

#[test]
fn the_compare_panel_page_fits_and_copies_its_sync_script() {
    let mut f = Fixture::new("compare-panel-page", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.compare();
    let groups = f.app.context_menu();
    f.press(ctrl('w'));
    f.char(' ');
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let screen = (0..24)
        .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    for (_, label) in groups.iter().flat_map(|group| &group.keys) {
        assert!(screen.contains(label), "{label}: {screen}");
    }
    f.char('Y');
    assert!(!f.clipboard.borrow().text.is_empty());
    assert!(f.clipboard.borrow().text.contains("CREATE TABLE"));
}

#[test]
fn ctrl_w_v_is_in_the_first_page_and_opens_the_dialog_of_the_row() {
    let mut f = Fixture::new("compare-prefix", SOURCE, TARGET);
    crate::common::select_row(&mut f.app, 0, &[RowKind::Table], "owners");
    assert!(
        crate::common::prefix_page(&f.app).contains(&"v Compare".to_owned()),
        "{:?}",
        crate::common::prefix_page(&f.app)
    );
    f.press(ctrl('w'));
    f.char('v');
    let form = f.app.compare_form().expect("the dialog");
    assert_eq!(form.kind, Kind::Data);
    assert_eq!(form.sides[0].table.text(), "owners");
}

#[test]
fn ctrl_w_v_in_a_table_tab_uses_its_table_and_in_a_console_its_schema() {
    let mut f = Fixture::new("compare-prefix-tabs", SOURCE, TARGET);
    crate::common::select_row(&mut f.app, 0, &[RowKind::Table], "items");
    f.code(KeyCode::Enter);
    f.wait(
        |app| app.table().is_some_and(|tab| !tab.is_busy()),
        "the table",
    );
    f.press(ctrl('w'));
    f.char('v');
    let form = f.app.compare_form().expect("the dialog of the table");
    assert_eq!(form.kind, Kind::Data);
    assert_eq!(form.sides[0].table.text(), "items");
    // The first `Esc` closes the list of the field, and the second closes the dialog.
    f.code(KeyCode::Esc);
    assert!(f.app.compare_form().is_some());
    f.code(KeyCode::Esc);
    assert!(f.app.compare_form().is_none());
    // A console of the connection compares its schema.
    f.press(ctrl('w'));
    f.char('h');
    crate::common::select_row(&mut f.app, 0, &[RowKind::Connection], "source");
    f.code(KeyCode::Enter);
    assert!(f.app.table().is_none());
    f.press(ctrl('w'));
    f.char('v');
    let form = f.app.compare_form().expect("the dialog of the console");
    assert_eq!(form.kind, Kind::Structure);
    assert_eq!(form.sides[0].table.text(), "");
    assert_eq!(form.choice(0).unwrap().name, "source");
}

#[test]
fn ctrl_w_v_in_a_compare_tab_opens_the_dialog_with_its_sides() {
    let mut f = Fixture::new("compare-again", SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.focus(Field::Removals);
    f.char(' ');
    f.compare();
    f.press(ctrl('w'));
    f.char('v');
    f.wait(
        |app| {
            app.compare_form()
                .is_some_and(|form| form.sides.iter().all(|side| side.schemas.is_some()))
        },
        "the schemas of the dialog",
    );
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.kind, Kind::Structure);
    assert!(form.removals);
    assert_eq!(form.choice(1).unwrap().name, "target");
    assert_eq!(form.focus, Field::Compare);
    f.code(KeyCode::Enter);
    f.idle();
    assert_eq!(f.app.compares().len(), 1, "the same compare reads again");
}

const LIST_SOURCE: &str = "\
    CREATE TABLE owners(id INTEGER PRIMARY KEY);\
    CREATE TABLE orders(id INTEGER PRIMARY KEY);\
    CREATE TABLE order_items(id INTEGER PRIMARY KEY);\
    CREATE VIEW owner_names AS SELECT id FROM owners;";

/// Returns the names of the list of the dialog and the marked name.
fn list(f: &Fixture) -> (Vec<String>, Option<String>) {
    let form = f.app.compare_form().expect("the dialog");
    let list = form.list.as_ref().expect("an open list");
    let names: Vec<String> = list.names.iter().map(|(_, name)| name.clone()).collect();
    let marked = list.selected.map(|place| names[place].clone());
    (names, marked)
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn write(f: &mut Fixture, text: &str) {
    for c in text.chars() {
        f.char(c);
    }
}

#[test]
fn each_field_lists_its_names_and_the_text_filters_the_tables() {
    let mut f = Fixture::new("compare-list", LIST_SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    // The dialog opens on the target connection, with its list.
    assert_eq!(
        list(&f),
        (names(&["source", "target"]), Some("target".to_owned()))
    );
    assert!(f.app.hint().contains("Write to filter"), "{}", f.app.hint());
    f.focus(Field::SourceSchema);
    assert_eq!(list(&f), (names(&["main"]), Some("main".to_owned())));
    // The tables of the schema, without the view. An empty table marks nothing.
    f.focus(Field::SourceTable);
    assert_eq!(
        list(&f),
        (names(&["order_items", "orders", "owners"]), None)
    );
    write(&mut f, "it ord");
    assert_eq!(
        list(&f),
        (names(&["order_items"]), Some("order_items".to_owned()))
    );
    f.code(KeyCode::Enter);
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.sides[0].table.text(), "order_items");
    assert_eq!(form.focus, Field::TargetConnection);
}

#[test]
fn esc_closes_the_list_and_keeps_the_choice() {
    let mut f = Fixture::new("compare-list-esc", LIST_SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.char('s');
    assert_eq!(list(&f), (names(&["source"]), Some("source".to_owned())));
    f.code(KeyCode::Esc);
    let form = f.app.compare_form().expect("the dialog stays open");
    assert!(form.list.is_none());
    assert!(form.search.text().is_empty());
    assert_eq!(form.choice(1).unwrap().name, "target");
    // Without its list, the field moves the focus as before.
    f.code(KeyCode::Down);
    assert_eq!(f.app.compare_form().unwrap().focus, Field::TargetSchema);
    // An empty table marks nothing, so `Tab` keeps it empty.
    f.focus(Field::TargetTable);
    f.code(KeyCode::Tab);
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.focus, Field::Removals);
    assert_eq!(form.sides[1].table.text(), "");
}

#[test]
fn a_chosen_connection_lists_its_schemas_and_then_its_tables() {
    let mut f = Fixture::new("compare-list-cascade", LIST_SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    f.code(KeyCode::Down);
    f.code(KeyCode::Enter);
    f.wait(
        |app| app.compare_form().is_some_and(|form| form.list_shown()),
        "the schemas of the new target",
    );
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.choice(1).unwrap().name, "source");
    assert_eq!(form.focus, Field::TargetSchema);
    assert_eq!(list(&f), (names(&["main"]), Some("main".to_owned())));
    f.code(KeyCode::Enter);
    assert_eq!(list(&f).0, names(&["order_items", "orders", "owners"]));
}

#[test]
fn a_click_on_a_row_of_the_list_chooses_it() {
    let mut f = Fixture::new("compare-list-click", LIST_SOURCE, TARGET);
    f.dialog(&[RowKind::Connection], "source");
    let screen = f.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let y = lines
        .iter()
        .position(|line| line.contains(text::FIELD_COMPARE_TARGET))
        .expect("the target connection row");
    let line = lines[y];
    let byte = line.find("< target >").expect("the value of the row");
    let x = line[..byte].chars().count();
    // The list starts below the value, and its first row is `source`.
    let below: String = lines[y + 1].chars().skip(x).take(8).collect();
    assert_eq!(below, "source  ", "{screen}");
    f.app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: u16::try_from(x).unwrap(),
        row: u16::try_from(y + 1).unwrap(),
        modifiers: KeyModifiers::NONE,
    }));
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.choice(1).unwrap().name, "source");
    assert_eq!(form.focus, Field::TargetSchema);
}

#[test]
fn the_vim_keys_write_in_the_search_of_a_field_with_a_list() {
    let preferences = r#"{"keyboardMode":"vim"}"#;
    let mut f = Fixture::with("compare-list-vim", LIST_SOURCE, TARGET, preferences);
    f.dialog(&[RowKind::Connection], "source");
    write(&mut f, "hjkl ");
    let form = f.app.compare_form().unwrap();
    assert_eq!(form.focus, Field::TargetConnection);
    assert_eq!(form.search.text(), "hjkl ");
    assert_eq!(form.choice(1).unwrap().name, "target");
}
