// The diagram tab in the application: the keys, the mouse, the copy, and the read of a
// real SQLite file.

mod common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::{ctrl, key, shift, temp_db};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::console::Phase;
use sqln::diagram::Focus;
use sqln::exec::{ExecEvent, ExecutionEnd, ReadLimit, StatementKey};
use sqln::grid::{Budget, Row, Value, row_cost};
use sqln::navigator::RowKind;
use sqln::ui::theme::Theme;

const SETUP: &str = "\
    CREATE TABLE owners(id INTEGER PRIMARY KEY, name TEXT);\
    CREATE TABLE items(\
      id INTEGER PRIMARY KEY,\
      owner INTEGER REFERENCES owners(id),\
      label TEXT NOT NULL);\
    CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT);\
    CREATE VIEW labels AS SELECT label FROM items;";

struct Fixture {
    app: App,
    clipboard: Rc<RefCell<MemoryClipboard>>,
    path: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::with(name, "")
    }

    /// Adds tables to the schema of the tests.
    fn with(name: &str, more: &str) -> Self {
        let path = temp_db(name);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(&format!("{SETUP}{more}"))
            .unwrap();
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let options = Options {
            database: Some(path.clone()),
            ..Options::default()
        };
        Self {
            app: App::new(options, clipboard.clone(), Theme::plain()),
            clipboard,
            path,
        }
    }

    fn press(&mut self, event: KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        self.app.on_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Presses `d` on a row of the navigator and waits for the end of the read.
    fn diagram_of(&mut self, kinds: &[RowKind], name: &str) {
        common::select_row(&mut self.app, 0, kinds, name);
        self.code(KeyCode::Char('d'));
        self.idle();
    }

    fn idle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.app.drain_events();
            if self.app.diagram().is_none_or(|tab| !tab.is_busy()) {
                break;
            }
            assert!(Instant::now() < deadline, "the read did not end");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.app.drain_events();
    }

    /// Waits until a diagram is on screen, after a read of the catalog, and then for
    /// the end of its own read.
    fn opened(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.app.diagram().is_none() {
            self.app.drain_events();
            assert!(Instant::now() < deadline, "the diagram did not open");
            std::thread::sleep(Duration::from_millis(1));
        }
        self.idle();
    }

    fn selected(&self) -> String {
        let tab = self.app.diagram().unwrap();
        tab.selected_relation().unwrap().relation
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

    fn screen(&mut self) -> String {
        self.screen_at(120, 30)
    }

    /// Returns the column and the row of a text on the screen.
    fn locate(&mut self, text: &str) -> (u16, u16) {
        let screen = self.screen();
        for (row, line) in screen.lines().enumerate() {
            if let Some(start) = line.find(text) {
                let column = line[..start].chars().count();
                return (column as u16, row as u16);
            }
        }
        panic!("{text:?} is not on the screen:\n{screen}");
    }
}

/// Returns SQL for `count` tables without keys, which make the picture tall.
fn tables(count: usize) -> String {
    (0..count)
        .map(|index| format!("CREATE TABLE loose{index:02}(a TEXT, b TEXT, c TEXT);"))
        .collect()
}

#[test]
fn d_on_a_schema_shows_its_tables_and_their_keys() {
    let mut f = Fixture::new("diagram-schema");
    f.diagram_of(&[RowKind::Schema], "main");
    assert!(!f.app.navigator_focused());
    let tab = f.app.diagram().unwrap();
    assert_eq!(tab.focus, Focus::Schema("main".to_owned()));
    let names: Vec<&str> = tab
        .model
        .entities
        .iter()
        .map(|entity| entity.relation.relation.as_str())
        .collect();
    // The view stays out.
    assert_eq!(names, ["items", "notes", "owners"]);
    let screen = f.screen();
    assert!(screen.contains("main (diagram)"), "{screen}");
    assert!(screen.contains("3 tables · 1 key"), "{screen}");
    // The plain set marks the keys with text, and the arrow points to the parent.
    assert!(screen.contains("PK id"), "{screen}");
    assert!(screen.contains("FK owner"), "{screen}");
    assert!(screen.contains("│<"), "{screen}");
    assert!(screen.contains("DIAGRAM"), "{screen}");
}

#[test]
fn d_on_a_table_shows_the_table_and_its_neighbors() {
    let mut f = Fixture::new("diagram-table");
    f.diagram_of(&[RowKind::Table], "owners");
    let tab = f.app.diagram().unwrap();
    let names: Vec<&str> = tab
        .model
        .entities
        .iter()
        .map(|entity| entity.relation.relation.as_str())
        .collect();
    assert_eq!(names, ["items", "owners"]);
    assert_eq!(f.selected(), "owners");
    let screen = f.screen();
    assert!(screen.contains("owners (diagram)"), "{screen}");
}

#[test]
fn d_on_a_column_shows_the_diagram_of_its_table() {
    let mut f = Fixture::new("diagram-column");
    common::select_row(&mut f.app, 0, &[RowKind::Table], "notes");
    f.code(KeyCode::Right);
    // The columns of a table arrive after its row opens.
    let deadline = Instant::now() + Duration::from_secs(20);
    while f.app.navigator().current(f.app.connections()).unwrap().kind != RowKind::Column {
        f.app.drain_events();
        f.code(KeyCode::Down);
        f.code(KeyCode::Up);
        assert!(Instant::now() < deadline, "the columns did not arrive");
        if f.app.connections()[0]
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.relation(Some("main"), "notes"))
            .is_some_and(|(_, relation)| {
                matches!(relation.columns, sqln::catalog::Columns::Loaded(_))
            })
        {
            f.code(KeyCode::Down);
        }
    }
    f.code(KeyCode::Char('d'));
    f.idle();
    let focus = &f.app.diagram().unwrap().focus;
    assert!(matches!(focus, Focus::Table(table) if table.relation == "notes"));
}

#[test]
fn a_view_and_a_second_d_open_no_new_tab() {
    let mut f = Fixture::new("diagram-view");
    f.diagram_of(&[RowKind::View], "labels");
    assert!(f.app.diagrams().is_empty());
    let screen = f.screen();
    assert!(screen.contains("A view has no keys"), "{screen}");

    f.diagram_of(&[RowKind::Schema], "main");
    f.diagram_of(&[RowKind::Schema], "main");
    assert_eq!(f.app.diagrams().len(), 1);
}

#[test]
fn the_explorer_page_lists_the_diagram_except_on_a_view() {
    let mut f = Fixture::new("diagram-page");
    common::select_row(&mut f.app, 0, &[RowKind::Table], "items");
    let page = common::explorer_page(&f.app);
    assert!(page.contains(&"d ER diagram".to_owned()), "{page:?}");
    common::select_row(&mut f.app, 0, &[RowKind::View], "labels");
    let page = common::explorer_page(&f.app);
    assert!(!page.iter().any(|row| row.contains("diagram")), "{page:?}");
}

#[test]
fn arrows_select_the_nearest_table_and_keys_only_hides_the_other_columns() {
    let mut f = Fixture::new("diagram-select");
    f.diagram_of(&[RowKind::Schema], "main");
    // The first box is at the top left, and the parent stands on the left of its child.
    assert_eq!(f.selected(), "owners");
    f.code(KeyCode::Right);
    assert_eq!(f.selected(), "items");
    f.code(KeyCode::Left);
    assert_eq!(f.selected(), "owners");
    f.code(KeyCode::Right);
    // A table without keys stands under the layers.
    f.code(KeyCode::Down);
    assert_eq!(f.selected(), "notes");

    assert!(f.screen().contains("body"));
    f.code(KeyCode::Char('K'));
    let screen = f.screen();
    assert!(!screen.contains("body"), "{screen}");
    assert!(screen.contains("keys only"), "{screen}");
    assert_eq!(f.selected(), "notes");
    f.code(KeyCode::Char('K'));
    assert!(f.screen().contains("body"));
}

#[test]
fn shift_arrows_and_the_wheel_move_the_view() {
    let mut f = Fixture::with("diagram-pan", &tables(30));
    f.diagram_of(&[RowKind::Schema], "main");
    // A small view leaves part of the picture out.
    f.screen_at(80, 24);
    f.press(shift(KeyCode::Down));
    let tab = f.app.diagram().unwrap();
    assert!(tab.top > 0, "the view did not move");
    let top = tab.top;
    f.press(shift(KeyCode::Up));
    assert!(f.app.diagram().unwrap().top < top);
    // The wheel over the picture moves the view, and no grid of the console.
    f.screen_at(80, 24);
    f.mouse(MouseEventKind::ScrollDown, 50, 10);
    assert!(f.app.diagram().unwrap().top > 0);
}

#[test]
fn a_click_selects_a_box_and_a_drag_moves_the_view() {
    let mut f = Fixture::with("diagram-mouse", &tables(30));
    f.diagram_of(&[RowKind::Schema], "main");
    let (column, row) = f.locate(" owners ");
    f.mouse(
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column,
        row,
    );
    assert_eq!(f.selected(), "owners");
    f.screen_at(80, 24);
    let (column, row) = (60, 15);
    f.mouse(
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column,
        row,
    );
    f.mouse(
        MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        column,
        row - 3,
    );
    f.mouse(
        MouseEventKind::Up(crossterm::event::MouseButton::Left),
        column,
        row - 3,
    );
    assert_eq!(f.app.diagram().unwrap().top, 3);
}

#[test]
fn enter_opens_the_data_and_i_opens_the_structure() {
    let mut f = Fixture::new("diagram-open");
    f.diagram_of(&[RowKind::Schema], "main");
    f.code(KeyCode::Enter);
    assert!(f.app.diagram().is_none());
    assert_eq!(f.app.table().unwrap().relation.relation, "owners");

    f.press(ctrl('w'));
    f.code(KeyCode::Char('p'));
    assert_eq!(f.selected(), "owners");
    f.code(KeyCode::Char('i'));
    let table = f.app.table().unwrap();
    assert!(table.structure.is_some());
    assert_eq!(table.relation.relation, "owners");
}

#[test]
fn y_copies_the_mermaid_text_of_the_boxes() {
    let mut f = Fixture::new("diagram-copy");
    f.diagram_of(&[RowKind::Schema], "main");
    f.code(KeyCode::Char('y'));
    let copied = f.clipboard.borrow().text.clone();
    assert!(copied.starts_with("erDiagram\n"), "{copied}");
    assert!(copied.contains("e3 |o..o{ e1 : \"owner\""), "{copied}");
    assert!(f.screen().contains("Copied."));
}

#[test]
fn r_reads_the_schema_again() {
    let mut f = Fixture::new("diagram-reload");
    f.diagram_of(&[RowKind::Schema], "main");
    rusqlite::Connection::open(&f.path)
        .unwrap()
        .execute_batch("CREATE TABLE tags(id INTEGER PRIMARY KEY, item INT REFERENCES items);")
        .unwrap();
    assert_eq!(f.app.diagram().unwrap().model.entities.len(), 3);
    f.code(KeyCode::Char('R'));
    f.idle();
    let tab = f.app.diagram().unwrap();
    assert_eq!(tab.model.entities.len(), 4);
    assert_eq!(tab.model.links.len(), 2);
    assert_eq!(f.selected(), "owners");
}

#[test]
fn the_hint_row_and_f1_name_the_keys_of_the_diagram() {
    let mut f = Fixture::new("diagram-hint");
    f.diagram_of(&[RowKind::Schema], "main");
    let hint = f.app.hint();
    assert!(
        hint.contains("Enter data")
            && hint.contains("i struct")
            && hint.contains("K keys only")
            && hint.contains("y mermaid"),
        "{hint}"
    );
    f.code(KeyCode::F(1));
    let screen = f.screen();
    assert!(screen.contains("Diagram"), "{screen}");
    assert!(screen.contains("Select the table above"), "{screen}");
}

#[test]
fn closing_the_diagram_shows_the_tab_at_its_place() {
    let mut f = Fixture::new("diagram-close");
    f.diagram_of(&[RowKind::Schema], "main");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('c'));
    assert!(f.app.diagram().is_none());
    assert!(f.app.diagrams().is_empty());
    assert!(!f.screen().contains("(diagram)"));
}

// The read of a tab, without the application.

fn rows(texts: &[&[&str]]) -> Vec<Row> {
    texts
        .iter()
        .map(|cells| cells.iter().map(|cell| Value::Text(cell)).collect())
        .collect()
}

#[test]
fn a_read_returns_its_rows_to_the_budget_at_the_end_and_at_the_close() {
    use sqln::diagram::tab::DiagramTab;

    let budget = Budget::new(1 << 20);
    let focus = Focus::Schema("main".to_owned());
    // Each row enters the budget, as the sink of a connector does.
    let event = |execution, index, cells: &[&[&str]]| {
        let rows = rows(cells);
        for row in &rows {
            budget.reserve(row);
        }
        let bytes: usize = rows.iter().map(row_cost).sum();
        ExecEvent::Rows {
            key: StatementKey {
                console: 7,
                execution,
                index,
            },
            rows,
            bytes,
        }
    };
    let columns: &[&[&str]] = &[&["main", "t", "id", "INTEGER", "no", "1"]];

    let mut tab = DiagramTab::new(7, 1, focus.clone(), 1);
    tab.started(1);
    tab.on_event(event(1, 0, columns), &budget);
    // The rows of an old read leave at once.
    tab.on_event(event(0, 0, columns), &budget);
    tab.on_event(
        ExecEvent::ResultDone {
            key: StatementKey {
                console: 7,
                execution: 1,
                index: 0,
            },
            rows: 1,
            limit: ReadLimit::Budget,
        },
        &budget,
    );
    tab.on_event(
        ExecEvent::ExecutionFinished {
            console: 7,
            execution: 1,
            end: ExecutionEnd::Completed,
            message: None,
        },
        &budget,
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(tab.phase, Phase::Idle);
    assert!(tab.partial);
    assert_eq!(tab.model.entities.len(), 1);

    let mut tab = DiagramTab::new(8, 1, focus, 2);
    tab.started(2);
    tab.on_event(event(2, 1, columns), &budget);
    assert!(budget.used() > 0);
    tab.release(&budget);
    assert_eq!(budget.used(), 0);
}

#[test]
fn hostile_names_reach_neither_the_screen_nor_the_copy() {
    let evil = "CREATE TABLE \"bad\u{1b}[31m\u{7}\" (\"c\u{1b}]0;x\u{7}\" INT, \"q\"\"\" INT);";
    let mut f = Fixture::with("diagram-evil", evil);
    f.diagram_of(&[RowKind::Schema], "main");
    assert_eq!(f.app.diagram().unwrap().model.entities.len(), 4);
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    for cell in terminal.backend().buffer().content() {
        assert!(!cell.symbol().chars().any(char::is_control), "{cell:?}");
    }
    f.code(KeyCode::Char('y'));
    let copied = f.clipboard.borrow().text.clone();
    assert!(
        !copied.chars().any(|c| c.is_control() && c != '\n'),
        "{copied}"
    );
}

#[test]
fn the_vim_keys_select_the_tables() {
    let path = temp_db("diagram-vim");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(SETUP)
        .unwrap();
    let options = Options {
        database: Some(path.clone()),
        keyboard: sqln::editor::KeyboardMode::Vim,
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut f = Fixture {
        app: App::new(options, clipboard.clone(), Theme::plain()),
        clipboard,
        path,
    };
    f.diagram_of(&[RowKind::Schema], "main");
    assert_eq!(f.selected(), "owners");
    f.code(KeyCode::Char('l'));
    assert_eq!(f.selected(), "items");
    f.code(KeyCode::Char('j'));
    assert_eq!(f.selected(), "notes");
    f.code(KeyCode::Char('k'));
    assert_eq!(f.selected(), "owners");
}

#[test]
fn a_paste_on_a_diagram_does_not_reach_the_console() {
    let mut f = Fixture::new("diagram-paste");
    f.diagram_of(&[RowKind::Schema], "main");
    f.app.on_event(Event::Paste("DROP TABLE items;".to_owned()));
    f.code(KeyCode::Char('x'));
    assert!(f.app.console().editor.is_blank());
}

#[test]
fn ctrl_w_d_opens_the_diagram_of_the_row_the_table_or_the_console() {
    let mut f = Fixture::new("diagram-prefix");
    let menu = |f: &Fixture| common::prefix_page(&f.app);
    // The console of the file shows the current schema.
    assert!(menu(&f).contains(&"D ER diagram".to_owned()));
    f.press(ctrl('w'));
    f.press(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT));
    f.opened();
    assert_eq!(
        f.app.diagram().unwrap().focus,
        Focus::Schema("main".to_owned())
    );
    // On a diagram, the row leaves the menu.
    assert!(!menu(&f).contains(&"D ER diagram".to_owned()));

    // A table tab shows the diagram of its table.
    common::open_table(&mut f.app, 0, "items");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('D'));
    f.idle();
    let focus = &f.app.diagram().unwrap().focus;
    assert!(matches!(focus, Focus::Table(table) if table.relation == "items"));

    // In the explorer, the row decides, and a view gives the reason.
    common::select_row(&mut f.app, 0, &[RowKind::View], "labels");
    assert!(!menu(&f).contains(&"D ER diagram".to_owned()));
    common::select_row(&mut f.app, 0, &[RowKind::Table], "notes");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('D'));
    f.idle();
    let focus = &f.app.diagram().unwrap().focus;
    assert!(matches!(focus, Focus::Table(table) if table.relation == "notes"));
    assert_eq!(f.app.diagrams().len(), 3);
}

#[test]
fn d_on_a_connection_reads_its_catalog_and_then_opens_its_schema() {
    let mut f = Fixture::new("diagram-connection");
    f.press(ctrl('w'));
    f.code(KeyCode::Char('h'));
    assert_eq!(
        f.app.navigator().current(f.app.connections()).unwrap().kind,
        RowKind::Connection
    );
    assert!(f.app.connections()[0].catalog.is_none());
    assert!(f.app.hint().contains("d diagram"), "{}", f.app.hint());
    f.code(KeyCode::Char('d'));
    f.opened();
    let tab = f.app.diagram().unwrap();
    assert_eq!(tab.focus, Focus::Schema("main".to_owned()));
    assert_eq!(tab.model.entities.len(), 3);
}

#[test]
fn two_connections_that_wait_for_their_catalogs_open_both_diagrams() {
    use sqln::connection::Definition;
    use sqln::connector::sqlite::SqliteTarget;

    let definitions = ["diagram-wait-a", "diagram-wait-b"].map(|name| {
        let path = temp_db(name);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(SETUP)
            .unwrap();
        Definition::sqlite(SqliteTarget::new(&path))
    });
    let options = Options {
        connections: definitions.to_vec(),
        ..Options::default()
    };
    let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
    let mut app = App::new(options, clipboard, Theme::plain());
    let press = |app: &mut App, event| app.on_event(Event::Key(event));
    press(&mut app, ctrl('w'));
    press(&mut app, key(KeyCode::Char('h')));
    press(&mut app, key(KeyCode::Char('d')));
    press(&mut app, key(KeyCode::Down));
    let row = app.navigator().current(app.connections()).unwrap();
    assert_eq!((row.kind, row.connection), (RowKind::Connection, 1));
    press(&mut app, key(KeyCode::Char('d')));

    let deadline = Instant::now() + Duration::from_secs(20);
    while app.diagrams().len() < 2 {
        app.drain_events();
        assert!(Instant::now() < deadline, "a diagram did not open");
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut opened: Vec<_> = app.diagrams().iter().map(|tab| tab.connection).collect();
    opened.sort();
    opened.dedup();
    assert_eq!(opened.len(), 2);
}
