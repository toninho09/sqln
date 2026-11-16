// The order of the connections in the explorer and their folders.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use crate::common::{alt, ctrl, key, select_row, shift, temp_dir};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sqln::app::{App, Options};
use sqln::clipboard::MemoryClipboard;
use sqln::navigator::RowKind;
use sqln::store::config;
use sqln::store::{Root, Scope, Store};
use sqln::ui::text;
use sqln::ui::theme::Theme;

struct Tree {
    app: App,
    store: Store,
}

/// One saved SQLite connection, with an optional folder.
fn entry(id: &str, folder: Option<&str>) -> String {
    let folder = folder.map_or_else(String::new, |name| format!(r#","folder":"{name}""#));
    format!(r#"{{"id":"{id}","name":"{id}","kind":"sqlite","file":"{id}.db"{folder}}}"#)
}

fn write_config(root: &Root, preferences: &str, connections: &[String]) {
    fs::create_dir_all(&root.dir).unwrap();
    let text = format!(
        r#"{{"version":1,"preferences":{preferences},"connections":[{}]}}"#,
        connections.join(",")
    );
    fs::write(root.config_file(), text).unwrap();
}

fn store(name: &str) -> (Store, PathBuf) {
    let base = temp_dir!(name);
    let project = Root::project(&base.join("project"));
    fs::create_dir_all(&project.dir).unwrap();
    let store = Store {
        global: Root::global(base.join("global")),
        project: Some(project),
    };
    (store, base)
}

impl Tree {
    /// Starts with global connections and the explorer focused.
    fn global(name: &str, preferences: &str, connections: &[String]) -> Self {
        let (store, _) = store(name);
        write_config(&store.global, preferences, connections);
        Self::start(store)
    }

    fn start(store: Store) -> Self {
        let clipboard = Rc::new(RefCell::new(MemoryClipboard::default()));
        let options = Options {
            store: Some(store.clone()),
            ..Options::default()
        };
        let mut tree = Self {
            app: App::new(options, clipboard, Theme::plain()),
            store,
        };
        tree.press(ctrl('w'));
        tree.code(KeyCode::Char('h'));
        tree
    }

    fn press(&mut self, event: crossterm::event::KeyEvent) {
        self.app.on_event(Event::Key(event));
    }

    fn code(&mut self, code: KeyCode) {
        self.press(key(code));
    }

    fn write(&mut self, text: &str) {
        self.app.on_event(Event::Paste(text.to_owned()));
    }

    /// Lists the rows of the tree. A folder shows `[name]`, and the indent is two spaces.
    fn rows(&self) -> Vec<String> {
        let connections = self.app.connections();
        self.app
            .navigator()
            .rows(connections)
            .iter()
            .map(|row| {
                let name = row.name(connections);
                let name = if row.kind == RowKind::Folder {
                    format!("[{name}]")
                } else {
                    name.to_owned()
                };
                format!("{}{name}", "  ".repeat(row.indent()))
            })
            .collect()
    }

    fn current(&self) -> String {
        let connections = self.app.connections();
        let row = self.app.navigator().current(connections).unwrap();
        row.name(connections).to_owned()
    }

    /// Moves the cursor to the row with this text.
    fn select(&mut self, name: &str) {
        let rows = self.rows();
        let index = rows
            .iter()
            .position(|row| row.trim_start().trim_matches(['[', ']']) == name)
            .unwrap_or_else(|| panic!("{name} is not in {rows:?}"));
        let cursor = rows
            .iter()
            .position(|row| row.trim_start().trim_matches(['[', ']']) == self.current())
            .unwrap();
        let (step, count) = if index > cursor {
            (KeyCode::Down, index - cursor)
        } else {
            (KeyCode::Up, cursor - index)
        };
        for _ in 0..count {
            self.code(step);
        }
        assert_eq!(self.current(), name);
    }

    /// Returns the saved order of a scope, with the folder of each connection.
    fn saved(&self, scope: Scope) -> Vec<(String, Option<String>)> {
        config::load(&self.store)
            .config
            .unwrap()
            .connections
            .into_iter()
            .filter(|item| item.scope == scope)
            .map(|item| (item.id, item.definition.folder))
            .collect()
    }

    fn status(&mut self) -> String {
        self.screen(100, 30)
    }

    fn screen(&mut self, width: u16, height: u16) -> String {
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

    /// Returns the screen place of the name of a row of the tree.
    fn place(&mut self, name: &str) -> (u16, u16) {
        let screen = self.screen(100, 30);
        // The tree starts below the tab row and the title, at the left of the border.
        for (y, row) in screen.lines().enumerate().skip(2) {
            let tree: String = row.chars().take_while(|c| *c != '│').collect();
            let words: Vec<&str> = tree.split_whitespace().collect();
            if words.get(1) == Some(&name) {
                let byte = tree.find(&format!(" {name}")).unwrap() + 1;
                return (tree[..byte].chars().count() as u16, y as u16);
            }
        }
        panic!("{name:?} is not on the screen:\n{screen}");
    }

    fn mouse(&mut self, kind: MouseEventKind, (column, row): (u16, u16)) {
        self.app.on_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    /// Drags the row of one name to the row of another name.
    fn drag(&mut self, from: &str, to: &str) {
        let start = self.place(from);
        let end = self.place(to);
        self.mouse(MouseEventKind::Down(MouseButton::Left), start);
        self.mouse(MouseEventKind::Drag(MouseButton::Left), end);
        self.mouse(MouseEventKind::Up(MouseButton::Left), end);
    }
}

fn order(items: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    items
        .iter()
        .map(|(id, folder)| ((*id).to_owned(), folder.map(str::to_owned)))
        .collect()
}

fn abc(folder_b: Option<&str>) -> Vec<String> {
    vec![entry("a", None), entry("b", folder_b), entry("c", None)]
}

#[test]
fn alt_down_and_alt_up_move_a_connection_and_save_the_order() {
    let mut tree = Tree::global("folders-alt", "{}", &abc(None));
    assert_eq!(tree.rows(), ["a", "b", "c"]);
    tree.press(alt(KeyCode::Down));
    assert_eq!(tree.rows(), ["b", "a", "c"]);
    assert_eq!(tree.current(), "a");
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("b", None), ("a", None), ("c", None)])
    );
    tree.press(alt(KeyCode::Up));
    assert_eq!(tree.rows(), ["a", "b", "c"]);
}

#[test]
fn the_vim_keys_and_a_custom_shortcut_move_a_connection() {
    let mut vim = Tree::global("folders-vim", r#"{"keyboardMode":"vim"}"#, &abc(None));
    vim.press(shift(KeyCode::Char('J')));
    assert_eq!(vim.rows(), ["b", "a", "c"]);
    vim.press(shift(KeyCode::Char('K')));
    assert_eq!(vim.rows(), ["a", "b", "c"]);

    let custom = r#"{"keymap":{"conventional":{"navigatorMoveDown":["ctrl+j"]}}}"#;
    let mut tree = Tree::global("folders-custom", custom, &abc(None));
    tree.press(alt(KeyCode::Down));
    assert_eq!(tree.rows(), ["a", "b", "c"], "the default key is gone");
    tree.press(ctrl('j'));
    assert_eq!(tree.rows(), ["b", "a", "c"]);
}

#[test]
fn a_move_past_the_edge_explains_the_refusal() {
    let mut tree = Tree::global("folders-edge", "{}", &abc(None));
    tree.press(alt(KeyCode::Up));
    assert_eq!(tree.rows(), ["a", "b", "c"]);
    assert!(tree.status().contains("edge of its scope"));
}

#[test]
fn m_puts_a_connection_in_a_new_folder_and_then_in_that_folder() {
    let mut tree = Tree::global("folders-new", "{}", &abc(None));
    tree.select("b");
    tree.code(KeyCode::Char('m'));
    assert!(tree.status().contains(text::FOLDER_NEW));
    tree.code(KeyCode::Enter);
    assert!(tree.status().contains(text::TITLE_NEW_FOLDER));
    tree.write("Work");
    tree.code(KeyCode::Enter);
    assert_eq!(tree.rows(), ["a", "[Work]", "  b", "c"]);
    assert_eq!(tree.current(), "b");

    tree.select("c");
    tree.code(KeyCode::Char('m'));
    // The list: the new folder, then the folders of the scope.
    tree.code(KeyCode::Down);
    tree.code(KeyCode::Enter);
    assert_eq!(tree.rows(), ["a", "[Work]", "  b", "  c"]);
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("a", None), ("b", Some("Work")), ("c", Some("Work"))])
    );

    // `No folder` puts it after the folder.
    tree.select("b");
    tree.code(KeyCode::Char('m'));
    tree.code(KeyCode::Down);
    tree.code(KeyCode::Enter);
    assert_eq!(tree.rows(), ["a", "[Work]", "  c", "b"]);
}

#[test]
fn a_folder_name_with_only_spaces_keeps_the_prompt_open() {
    let mut tree = Tree::global("folders-empty-name", "{}", &abc(None));
    tree.select("b");
    tree.code(KeyCode::Char('m'));
    tree.code(KeyCode::Enter);
    tree.write("   ");
    tree.code(KeyCode::Enter);
    assert!(tree.status().contains("A folder needs a name."));
    assert_eq!(tree.rows(), ["a", "b", "c"]);
}

#[test]
fn a_folder_moves_as_one_block_and_its_members_move_inside_it() {
    let items = vec![
        entry("a", None),
        entry("b", Some("Work")),
        entry("c", Some("Work")),
    ];
    let mut tree = Tree::global("folders-block", "{}", &items);
    tree.select("Work");
    tree.press(alt(KeyCode::Up));
    assert_eq!(tree.rows(), ["[Work]", "  b", "  c", "a"]);
    assert_eq!(tree.current(), "Work");
    tree.select("c");
    tree.press(alt(KeyCode::Up));
    assert_eq!(tree.rows(), ["[Work]", "  c", "  b", "a"]);
    tree.press(alt(KeyCode::Up));
    assert!(tree.status().contains("edge of its folder"));
}

#[test]
fn e_renames_a_folder_and_delete_removes_it_after_a_question() {
    let mut tree = Tree::global("folders-rename", "{}", &abc(Some("Work")));
    tree.select("Work");
    tree.code(KeyCode::Char('e'));
    assert!(tree.status().contains(text::TITLE_RENAME_FOLDER));
    for _ in 0.."Work".len() {
        tree.code(KeyCode::Backspace);
    }
    tree.write("Tests");
    tree.code(KeyCode::Enter);
    assert_eq!(tree.rows(), ["a", "[Tests]", "  b", "c"]);
    assert_eq!(tree.current(), "Tests");

    tree.code(KeyCode::Delete);
    let screen = tree.status();
    assert!(screen.contains(text::TITLE_REMOVE_FOLDER), "{screen}");
    tree.code(KeyCode::Esc);
    assert_eq!(tree.rows(), ["a", "[Tests]", "  b", "c"]);

    tree.code(KeyCode::Delete);
    // The question starts on Cancel.
    tree.code(KeyCode::Right);
    tree.code(KeyCode::Enter);
    assert_eq!(tree.rows(), ["a", "b", "c"]);
    assert_eq!(tree.app.connections().len(), 3);
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("a", None), ("b", None), ("c", None)])
    );
}

#[test]
fn a_closed_folder_stays_closed_at_the_next_start() {
    let mut tree = Tree::global("folders-closed", "{}", &abc(Some("Work")));
    tree.select("Work");
    tree.code(KeyCode::Left);
    assert_eq!(tree.rows(), ["a", "[Work]", "c"]);
    let store = tree.store.clone();
    tree.app.shutdown();
    drop(tree);
    let mut tree = Tree::start(store);
    assert_eq!(tree.rows(), ["a", "[Work]", "c"]);
    tree.select("Work");
    tree.code(KeyCode::Right);
    assert_eq!(tree.rows(), ["a", "[Work]", "  b", "c"]);
}

#[test]
fn the_mouse_moves_a_connection_to_a_row_or_into_a_folder() {
    let items = vec![entry("a", None), entry("b", Some("Work")), entry("c", None)];
    let mut tree = Tree::global("folders-drag", "{}", &items);
    tree.drag("c", "a");
    assert_eq!(tree.rows(), ["c", "a", "[Work]", "  b"]);
    tree.drag("a", "Work");
    assert_eq!(tree.rows(), ["c", "[Work]", "  b", "  a"]);
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("c", None), ("b", Some("Work")), ("a", Some("Work"))])
    );
    // A click without a move changes nothing.
    let place = tree.place("c");
    tree.mouse(MouseEventKind::Down(MouseButton::Left), place);
    tree.mouse(MouseEventKind::Up(MouseButton::Left), place);
    assert_eq!(tree.rows(), ["c", "[Work]", "  b", "  a"]);
}

#[test]
fn the_mouse_takes_a_connection_out_of_its_folder_on_the_folder_row() {
    let items = vec![entry("a", Some("Work")), entry("b", Some("Work"))];
    let mut tree = Tree::global("folders-drag-out", "{}", &items);
    assert_eq!(tree.rows(), ["[Work]", "  a", "  b"]);
    let start = tree.place("b");
    let end = tree.place("Work");
    tree.mouse(MouseEventKind::Down(MouseButton::Left), start);
    tree.mouse(MouseEventKind::Drag(MouseButton::Left), end);
    assert!(tree.status().contains("out of the folder Work"));
    tree.mouse(MouseEventKind::Up(MouseButton::Left), end);
    assert_eq!(tree.rows(), ["b", "[Work]", "  a"]);
    assert_eq!(tree.current(), "b");
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("b", None), ("a", Some("Work"))])
    );
}

#[test]
fn the_area_below_the_tree_takes_an_item_to_the_end_of_its_scope() {
    let items = vec![
        entry("a", Some("Work")),
        entry("b", Some("Work")),
        entry("c", None),
    ];
    let mut tree = Tree::global("folders-drag-end", "{}", &items);
    assert_eq!(tree.rows(), ["[Work]", "  a", "  b", "c"]);
    let start = tree.place("a");
    let (column, last) = tree.place("c");
    let below = (column, last + 2);
    tree.mouse(MouseEventKind::Down(MouseButton::Left), start);
    tree.mouse(MouseEventKind::Drag(MouseButton::Left), below);
    let screen = tree.status();
    assert!(screen.contains("> End of global"), "{screen}");
    assert!(
        screen.contains("to the end of the global connections"),
        "{screen}"
    );
    tree.mouse(MouseEventKind::Up(MouseButton::Left), below);
    assert_eq!(tree.rows(), ["[Work]", "  b", "c", "a"]);
    assert_eq!(tree.current(), "a");
    assert!(!tree.status().contains("> End of global"));
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("b", Some("Work")), ("c", None), ("a", None)])
    );
}

#[test]
fn the_mouse_shows_the_target_and_refuses_the_other_scope() {
    let (store, _) = store("folders-scope");
    write_config(&store.global, "{}", &[entry("g", None)]);
    write_config(store.project.as_ref().unwrap(), "{}", &[entry("p", None)]);
    let mut tree = Tree::start(store);
    assert_eq!(tree.rows(), ["g", "p"]);
    let start = tree.place("p");
    let end = tree.place("g");
    tree.mouse(MouseEventKind::Down(MouseButton::Left), start);
    tree.mouse(MouseEventKind::Drag(MouseButton::Left), end);
    let screen = tree.status();
    assert!(screen.contains(">"), "{screen}");
    assert!(
        screen.contains("cannot move to the other scope"),
        "{screen}"
    );
    tree.mouse(MouseEventKind::Up(MouseButton::Left), end);
    assert_eq!(tree.rows(), ["g", "p"]);
    assert!(tree.status().contains("cannot move to the other scope"));
    tree.press(alt(KeyCode::Up));
    assert!(tree.status().contains("edge of its scope"));
}

#[test]
fn a_failed_write_keeps_the_order_and_says_why() {
    let mut tree = Tree::global("folders-failed", "{}", &abc(None));
    fs::write(tree.store.global.config_file(), "{").unwrap();
    tree.press(alt(KeyCode::Down));
    assert_eq!(tree.rows(), ["a", "b", "c"]);
    assert!(tree.status().contains("The order was not saved"));
}

#[test]
fn a_reload_shows_the_order_of_the_file() {
    let mut tree = Tree::global("folders-reload", "{}", &abc(None));
    let reordered = vec![entry("c", Some("Work")), entry("a", None), entry("b", None)];
    write_config(&tree.store.global, "{}", &reordered);
    tree.press(ctrl('w'));
    tree.code(KeyCode::Char('r'));
    assert_eq!(tree.rows(), ["[Work]", "  c", "a", "b"]);
}

#[test]
fn the_filtered_tree_refuses_a_move() {
    let base = temp_dir!("folders-filter");
    let file = base.join("a.db");
    rusqlite::Connection::open(&file)
        .unwrap()
        .execute_batch("CREATE TABLE items (id INTEGER);")
        .unwrap();
    let a = format!(
        r#"{{"id":"a","name":"a","kind":"sqlite","file":"{}"}}"#,
        file.display()
    );
    let mut tree = Tree::global("folders-filter-app", "{}", &[a, entry("b", None)]);
    select_row(&mut tree.app, 0, &[RowKind::Table], "items");
    tree.code(KeyCode::Char('/'));
    tree.write("items");
    tree.code(KeyCode::Tab);
    tree.press(alt(KeyCode::Down));
    assert!(tree.status().contains("Clear the filter first"));
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("a", None), ("b", None)])
    );
}

#[test]
fn a_folder_draws_with_its_count_on_a_small_screen_without_icons() {
    let items = vec![
        entry("a", None),
        entry("b", Some("Work")),
        entry("c", Some("Work")),
    ];
    let mut tree = Tree::global("folders-draw", "{}", &items);
    let screen = tree.screen(80, 24);
    assert!(screen.contains(" ▾ Work (2)"), "{screen}");
    assert!(screen.contains("   ▸ b"), "{screen}");
    tree.select("Work");
    let hints = tree.screen(80, 24);
    assert!(
        hints.contains("rename folder") || hints.contains("move up"),
        "{hints}"
    );
}

#[test]
fn the_form_saves_the_folder_of_a_connection() {
    let base = temp_dir!("folders-form-files");
    let items: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|id| {
            let file = base.join(format!("{id}.db"));
            fs::write(&file, "").unwrap();
            format!(
                r#"{{"id":"{id}","name":"{id}","kind":"sqlite","file":"{}"}}"#,
                file.display()
            )
        })
        .collect();
    let mut tree = Tree::global("folders-form", "{}", &items);
    tree.select("c");
    tree.code(KeyCode::Char('e'));
    crate::common::focus_form_field(&mut tree.app, sqln::form::Field::Folder);
    tree.write("Work");
    crate::common::focus_form_field(&mut tree.app, sqln::form::Field::Save);
    tree.code(KeyCode::Enter);
    let screen = tree.status();
    assert_eq!(tree.rows(), ["a", "b", "[Work]", "  c"], "{screen}");
    assert_eq!(
        tree.saved(Scope::Global),
        order(&[("a", None), ("b", None), ("c", Some("Work"))])
    );
}

#[test]
fn the_explorer_page_names_the_commands_of_a_connection_and_of_a_folder() {
    let mut tree = Tree::global("folders-page", "{}", &abc(Some("Work")));
    let page = crate::common::explorer_page(&tree.app);
    for label in ["m Move to folder", "Alt+Up Move up", "Alt+Down Move down"] {
        assert!(page.iter().any(|row| row == label), "{label}: {page:?}");
    }
    tree.select("Work");
    let page = crate::common::explorer_page(&tree.app);
    assert!(
        page.iter().any(|row| row.ends_with("Rename folder")),
        "{page:?}"
    );
    assert!(
        page.iter().any(|row| row.ends_with("Remove folder")),
        "{page:?}"
    );
    assert!(
        !page.iter().any(|row| row.ends_with("Move to folder")),
        "{page:?}"
    );
    assert!(
        !page.iter().any(|row| row.ends_with("New console")),
        "{page:?}"
    );
}
