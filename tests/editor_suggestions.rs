mod common;

use common::{Fixture, ctrl, key, shift};
use crossterm::event::KeyCode::{Backspace, Down, Enter, Esc, Left, Tab, Up};
use edtui::Index2;
use sqln::catalog::Columns;
use sqln::catalog::RelationRef;
use sqln::editor::KeyboardMode;
use sqln::sql::Dialect;

#[test]
fn typing_a_word_opens_the_list_and_narrows_it() {
    let mut f = Fixture::conventional("SELECT * FROM ");
    f.editor.set_cursor(Index2::new(0, 14));
    f.write("u");
    assert!(f.labels().contains(&"users".to_owned()));
    f.write("se");
    assert_eq!(f.labels(), ["users", "active_users"]);
    f.write("rs");
    assert_eq!(f.labels(), ["users", "active_users"]);
    f.write("x");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn a_dot_opens_the_list_of_columns() {
    let mut f = Fixture::conventional("SELECT \nFROM users AS u;");
    f.editor.set_cursor(Index2::new(0, 7));
    f.write("u.");
    assert_eq!(f.labels(), ["created_at", "email", "id", "name"]);
}

#[test]
fn ctrl_space_opens_the_list_without_a_prefix() {
    let mut f = Fixture::conventional("SELECT * FROM ");
    f.editor.set_cursor(Index2::new(0, 14));
    assert!(f.editor.suggestions().is_none());
    f.press(ctrl(' '));
    assert!(f.labels().contains(&"orders".to_owned()));
}

#[test]
fn tab_accepts_and_does_not_insert_a_tab() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.press(key(Tab));
    assert_eq!(f.text(), "select");
    assert_eq!(f.editor.cursor(), Index2::new(0, 6));
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn enter_breaks_the_line_and_closes_the_list() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.press(key(Enter));
    assert_eq!(f.text(), "sel\n");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn escape_closes_the_list_and_keeps_the_text() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.press(key(Esc));
    assert_eq!(f.text(), "sel");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn arrows_move_in_the_list_and_wrap() {
    let mut f = Fixture::conventional("SELECT * FROM ");
    f.editor.set_cursor(Index2::new(0, 14));
    f.write("us");
    let count = f.labels().len();
    f.press(key(Down));
    assert_eq!(f.editor.suggestions().unwrap().selected, 1);
    f.press(key(Up));
    f.press(key(Up));
    assert_eq!(f.editor.suggestions().unwrap().selected, count - 1);
    assert_eq!(f.editor.cursor(), Index2::new(0, 16), "the arrows stay in the list");
}

#[test]
fn accepted_item_is_the_selected_one() {
    let mut f = Fixture::conventional("SELECT * FROM ");
    f.editor.set_cursor(Index2::new(0, 14));
    f.write("us");
    f.press(key(Down));
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT * FROM active_users");
}

#[test]
fn accepted_name_uses_the_quotes_of_the_dialect() {
    let mut f = Fixture::with_dialect("SELECT * FROM ", KeyboardMode::Conventional, Dialect::MySql);
    f.editor.set_cursor(Index2::new(0, 14));
    f.write("Order");
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT * FROM `Order Items`");
}

#[test]
fn acceptance_is_one_undo_step_after_the_typed_word() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.press(key(Tab));
    f.press(ctrl('z'));
    assert_eq!(f.text(), "sel");
}

#[test]
fn backspace_updates_an_open_list_and_does_not_open_a_closed_one() {
    let mut f = Fixture::conventional("");
    f.write("sele");
    assert_eq!(f.labels(), ["select"]);
    f.press(key(Backspace));
    assert_eq!(f.labels(), ["select"]);
    f.press(key(Esc));
    f.press(key(Backspace));
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn moving_the_cursor_closes_the_list() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.press(key(Left));
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn shift_arrow_closes_the_list() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.press(shift(Left));
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn a_space_and_a_comma_close_the_list() {
    let mut f = Fixture::conventional("");
    f.write("sel ");
    assert!(f.editor.suggestions().is_none());
    let mut f = Fixture::conventional("SELECT i FROM users");
    f.editor.set_cursor(Index2::new(0, 8));
    f.write("d");
    f.write(",");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn the_list_stays_closed_in_strings_and_comments() {
    let mut f = Fixture::conventional("");
    f.write("-- sel");
    assert!(f.editor.suggestions().is_none());
    let mut f = Fixture::conventional("");
    f.write("SELECT 'sel");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn a_paste_closes_the_list() {
    let mut f = Fixture::conventional("");
    f.write("sel");
    f.editor.on_paste("x");
    assert!(f.editor.suggestions().is_none());
    assert_eq!(f.text(), "selx");
}

#[test]
fn without_a_catalog_keywords_still_appear() {
    let mut f = Fixture::conventional("");
    f.catalog = sqln::catalog::Catalog::default();
    f.write("sel");
    assert_eq!(f.labels(), ["select"]);
}

#[test]
fn multibyte_text_before_the_cursor_keeps_the_acceptance_correct() {
    let mut f = Fixture::conventional("SELECT 'ação', ");
    f.editor.set_cursor(Index2::new(0, 15));
    f.write("disti");
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT 'ação', distinct");
}

#[test]
fn a_later_line_keeps_the_acceptance_correct() {
    let mut f = Fixture::conventional("SELECT 1;\n\nSELECT * FROM ");
    f.editor.set_cursor(Index2::new(2, 14));
    f.write("use");
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT 1;\n\nSELECT * FROM users");
}

#[test]
fn missing_columns_reach_the_application_and_a_refresh_shows_them() {
    let mut f = Fixture::conventional("SELECT  FROM users u");
    f.catalog.schemas[0].relations[0].columns = Columns::NotLoaded;
    f.editor.set_cursor(Index2::new(0, 7));
    f.write("u.");
    assert!(f.editor.suggestions().is_none());
    assert_eq!(f.editor.take_missing_columns(), [RelationRef {
        schema: "public".to_owned(),
        relation: "users".to_owned(),
    }]);

    f.press(ctrl(' '));
    f.catalog.schemas[0].relations[0].columns = Columns::Loaded(vec!["id".to_owned()]);
    f.press(ctrl(' '));
    assert_eq!(f.labels(), ["id"]);
}
