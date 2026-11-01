mod common;

use common::{Fixture, ctrl, key};
use crossterm::event::KeyCode::{self, Char, Esc, Tab};
use edtui::{EditorMode, Index2};
use sqln::editor::KeyboardMode;

fn write_keys(f: &mut Fixture, keys: &str) {
    for c in keys.chars() {
        f.press(key(Char(c)));
    }
}

#[test]
fn starts_in_normal_mode_and_inserts_text() {
    let mut f = Fixture::vim("select 1;");
    assert_eq!(f.editor.state.mode, EditorMode::Normal);
    write_keys(&mut f, "ix");
    f.press(key(Esc));
    assert_eq!(f.text(), "xselect 1;");
    assert_eq!(f.editor.state.mode, EditorMode::Normal);
}

#[test]
fn navigation_keys_write_only_in_insert_mode() {
    let mut f = Fixture::vim("ab\ncd");
    write_keys(&mut f, "jl");
    assert_eq!(f.text(), "ab\ncd");
    assert_eq!(f.editor.cursor(), Index2::new(1, 1));
    write_keys(&mut f, "ijk");
    assert_eq!(f.text(), "ab\ncjkd");
}

#[test]
fn undo_and_redo_use_the_vim_keys() {
    let mut f = Fixture::vim("base");
    write_keys(&mut f, "ix");
    f.press(key(Esc));
    write_keys(&mut f, "u");
    assert_eq!(f.text(), "base");
    f.press(ctrl('r'));
    assert_eq!(f.text(), "xbase");
}

#[test]
fn a_new_edit_clears_the_redo_steps() {
    let mut f = Fixture::vim("base");
    write_keys(&mut f, "ix");
    f.press(key(Esc));
    write_keys(&mut f, "u");
    write_keys(&mut f, "iy");
    f.press(key(Esc));
    f.press(ctrl('r'));
    assert_eq!(f.text(), "ybase");
}

#[test]
fn yank_and_put_use_the_shared_clipboard() {
    let mut f = Fixture::vim("abc");
    write_keys(&mut f, "vly");
    assert_eq!(f.clipboard_text(), "ab");
    write_keys(&mut f, "$p");
    assert_eq!(f.text(), "abcab");
}

#[test]
fn visual_selection_is_readable() {
    let mut f = Fixture::vim("abc");
    write_keys(&mut f, "vl");
    assert_eq!(f.selected().as_deref(), Some("ab"));
}

#[test]
fn terminal_paste_in_visual_mode_replaces_the_selection() {
    let mut f = Fixture::vim("abcd");
    write_keys(&mut f, "vl");
    f.clipboard.borrow_mut().text = "kept".to_owned();
    f.editor.on_paste("xy");
    assert_eq!(f.text(), "xycd");
    assert_eq!(f.clipboard_text(), "kept");
    assert_eq!(f.editor.state.mode, EditorMode::Normal);
}

#[test]
fn terminal_paste_is_one_undo_step() {
    let mut f = Fixture::vim("ab");
    f.editor.on_paste("x\ny");
    assert_eq!(f.text(), "x\nyab");
    write_keys(&mut f, "u");
    assert_eq!(f.text(), "ab");
}

#[test]
fn tab_in_insert_mode_uses_spaces() {
    let mut f = Fixture::vim("a");
    write_keys(&mut f, "i");
    f.press(key(Tab));
    assert_eq!(f.text(), "    a");
}

#[test]
fn escape_moves_the_list_first_and_the_mode_second() {
    let mut f = Fixture::vim("");
    write_keys(&mut f, "isel");
    assert_eq!(f.labels(), ["select"]);
    f.press(key(Esc));
    assert!(f.editor.suggestions().is_none());
    assert_eq!(f.editor.state.mode, EditorMode::Insert);
    f.press(key(Esc));
    assert_eq!(f.editor.state.mode, EditorMode::Normal);
}

#[test]
fn suggestions_work_in_insert_mode_and_stay_closed_in_normal_mode() {
    let mut f = Fixture::vim("");
    write_keys(&mut f, "iSELECT u.na");
    assert!(
        f.editor.suggestions().is_none(),
        "no relation is in scope yet"
    );
    f.press(key(Esc));
    write_keys(&mut f, "dd");
    write_keys(&mut f, "iSELECT * FROM us");
    assert!(f.labels().contains(&"users".to_owned()));
    f.press(key(Tab));
    assert_eq!(f.text(), "SELECT * FROM users");
    assert_eq!(f.editor.state.mode, EditorMode::Insert);
    f.press(key(Esc));
    assert_eq!(f.text(), "SELECT * FROM users");

    f.press(ctrl(' '));
    assert!(f.editor.suggestions().is_none());
    write_keys(&mut f, "h");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn accepted_suggestion_is_one_undo_step_in_vim() {
    let mut f = Fixture::vim("");
    write_keys(&mut f, "iselect * from us");
    f.press(key(Tab));
    f.press(key(Esc));
    write_keys(&mut f, "u");
    assert_eq!(f.text(), "select * from us");
}

#[test]
fn slash_opens_the_same_search_bar() {
    let mut f = Fixture::vim("select users from users");
    write_keys(&mut f, "/users");
    assert_eq!(f.editor.search().unwrap().matches.len(), 2);
    assert_eq!(f.editor.cursor(), Index2::new(0, 7));
    f.press(key(Esc));
    assert!(f.editor.search().is_none());
    assert_eq!(f.editor.state.mode, EditorMode::Normal);
}

#[test]
fn replace_works_from_vim_and_undo_restores_it() {
    let mut f = Fixture::vim("users users");
    f.press(ctrl('f'));
    f.write("users");
    f.press(key(Tab));
    f.write("people");
    f.press(key(KeyCode::Enter));
    assert_eq!(f.text(), "people users");
    f.press(key(Esc));
    write_keys(&mut f, "u");
    assert_eq!(f.text(), "users users");
}

#[test]
fn a_change_of_keyboard_keeps_the_text_the_cursor_and_the_history() {
    let mut f = Fixture::vim("first\nsecond");
    write_keys(&mut f, "jx");
    let cursor = f.editor.cursor();
    let revision = f.editor.revision();
    f.editor.set_keyboard(KeyboardMode::Conventional);
    assert_eq!(f.text(), "first\necond");
    assert_eq!(f.editor.cursor(), cursor);
    assert_eq!(f.editor.revision(), revision);
    f.press(ctrl('z'));
    assert_eq!(f.text(), "first\nsecond");
}

#[test]
fn a_change_of_keyboard_keeps_the_selection() {
    let mut f = Fixture::vim("select me");
    write_keys(&mut f, "vll");
    assert_eq!(f.editor.selected_text().as_deref(), Some("sel"));
    f.editor.set_keyboard(KeyboardMode::Conventional);
    assert_eq!(f.editor.selected_text().as_deref(), Some("sel"));
    f.editor.set_keyboard(KeyboardMode::Vim);
    assert_eq!(f.editor.selected_text().as_deref(), Some("sel"));
}

#[test]
fn a_comment_in_normal_mode_is_one_undo_step() {
    let mut f = Fixture::vim("select 1;");
    f.editor.toggle_comment();
    assert_eq!(f.text(), "-- select 1;");
    f.press(key(Char('u')));
    assert_eq!(f.text(), "select 1;");
}

#[test]
fn a_comment_ends_visual_mode() {
    let mut f = Fixture::vim("a\nb\nc");
    f.press(key(Char('v')));
    f.press(key(Char('j')));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "-- a\n-- b\nc");
    assert_eq!(f.editor.state.mode, EditorMode::Normal);
    assert_eq!(f.selected(), None);
}
