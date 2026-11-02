mod common;

use common::{Fixture, ctrl, key, shift};
use crossterm::event::KeyCode::{BackTab, Backspace, Down, Enter, Esc, Right, Tab, Up};
use edtui::Index2;
use sqln::editor::SearchField;

#[test]
fn search_finds_every_match_without_case() {
    let mut f = Fixture::conventional("users USERS\nUsers");
    f.press(ctrl('f'));
    f.write("users");
    let bar = f.editor.search().unwrap();
    assert_eq!(bar.matches.len(), 3);
    assert_eq!(bar.current, 0);
}

#[test]
fn typing_in_the_bar_leaves_the_text_unchanged() {
    let mut f = Fixture::conventional("users");
    f.press(ctrl('f'));
    f.write("use");
    assert_eq!(f.text(), "users");
}

#[test]
fn enter_and_arrows_visit_the_matches_and_wrap() {
    let mut f = Fixture::conventional("a b a b a");
    f.press(ctrl('f'));
    f.write("a");
    assert_eq!(f.editor.cursor(), Index2::new(0, 0));
    f.press(key(Enter));
    assert_eq!(f.editor.cursor(), Index2::new(0, 4));
    f.press(key(Down));
    assert_eq!(f.editor.cursor(), Index2::new(0, 8));
    f.press(key(Down));
    assert_eq!(f.editor.cursor(), Index2::new(0, 0));
    f.press(key(Up));
    assert_eq!(f.editor.cursor(), Index2::new(0, 8));
}

#[test]
fn the_first_match_follows_the_cursor() {
    let mut f = Fixture::conventional("a b a");
    f.editor.set_cursor(Index2::new(0, 2));
    f.press(ctrl('f'));
    f.write("a");
    assert_eq!(f.editor.cursor(), Index2::new(0, 4));
}

#[test]
fn a_selection_fills_the_find_field() {
    let mut f = Fixture::conventional("users users");
    for _ in 0..5 {
        f.press(shift(Right));
    }
    f.press(ctrl('f'));
    let bar = f.editor.search().unwrap();
    assert_eq!(bar.find, "users");
    assert_eq!(bar.matches.len(), 2);
}

#[test]
fn tab_moves_between_the_fields() {
    let mut f = Fixture::conventional("x");
    f.press(ctrl('f'));
    f.press(key(Tab));
    assert_eq!(f.editor.search().unwrap().field, SearchField::Replace);
    f.press(key(BackTab));
    assert_eq!(f.editor.search().unwrap().field, SearchField::Find);
}

#[test]
fn enter_in_the_replace_field_replaces_one_match() {
    let mut f = Fixture::conventional("users users");
    f.press(ctrl('f'));
    f.write("users");
    f.press(key(Tab));
    f.write("people");
    f.press(key(Enter));
    assert_eq!(f.text(), "people users");
    assert_eq!(f.editor.search().unwrap().matches.len(), 1);
    assert_eq!(f.editor.cursor(), Index2::new(0, 7));
}

#[test]
fn ctrl_a_replaces_all_in_one_undo_step() {
    let mut f = Fixture::conventional("users\nUSERS users");
    f.press(ctrl('f'));
    f.write("users");
    f.press(key(Tab));
    f.write("p");
    f.press(ctrl('a'));
    assert_eq!(f.text(), "p\np p");
    assert!(f.editor.search().unwrap().matches.is_empty());
    f.press(key(Esc));
    f.press(ctrl('z'));
    assert_eq!(f.text(), "users\nUSERS users");
}

#[test]
fn ctrl_a_keeps_the_text_around_each_match() {
    let text = "ab ab\n\nxx ab yy ab\nab";
    for (replacement, result) in [
        ("long", "long long\n\nxx long yy long\nlong"),
        ("", " \n\nxx  yy \n"),
    ] {
        let mut f = Fixture::conventional(text);
        f.press(ctrl('f'));
        f.write("ab");
        f.press(key(Tab));
        f.write(replacement);
        f.press(ctrl('a'));
        assert_eq!(f.text(), result);
        f.press(key(Esc));
        f.press(ctrl('z'));
        assert_eq!(f.text(), text);
    }
}

#[test]
fn replacement_that_contains_the_query_stops_after_one_pass() {
    let mut f = Fixture::conventional("a a");
    f.press(ctrl('f'));
    f.write("a");
    f.press(key(Tab));
    f.write("aa");
    f.press(ctrl('a'));
    assert_eq!(f.text(), "aa aa");
}

#[test]
fn backspace_edits_the_field_and_updates_the_matches() {
    let mut f = Fixture::conventional("users user");
    f.press(ctrl('f'));
    f.write("users");
    assert_eq!(f.editor.search().unwrap().matches.len(), 1);
    f.press(key(Backspace));
    assert_eq!(f.editor.search().unwrap().find, "user");
    assert_eq!(f.editor.search().unwrap().matches.len(), 2);
}

#[test]
fn escape_closes_the_bar_and_keeps_the_text() {
    let mut f = Fixture::conventional("users");
    f.press(ctrl('f'));
    f.write("use");
    f.press(key(Esc));
    assert!(f.editor.search().is_none());
    assert_eq!(f.text(), "users");
    f.write("x");
    assert_eq!(f.text(), "xusers");
}

#[test]
fn multibyte_match_replaces_whole_characters() {
    let mut f = Fixture::conventional("ação ÇÃO");
    f.press(ctrl('f'));
    f.write("ção");
    assert_eq!(f.editor.search().unwrap().matches.len(), 2);
    f.press(key(Tab));
    f.write("x");
    f.press(ctrl('a'));
    assert_eq!(f.text(), "ax x");
}

#[test]
fn a_paste_in_the_bar_uses_its_first_line() {
    let mut f = Fixture::conventional("users");
    f.press(ctrl('f'));
    f.editor.on_paste("use\nother");
    assert_eq!(f.editor.search().unwrap().find, "use");
    assert_eq!(f.text(), "users");
}
