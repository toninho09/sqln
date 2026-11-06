use crate::common::{Fixture, alt, ctrl, key, shift};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::Index2;
use sqln::editor::{KeyboardMode, RunScope};
use sqln::sql::Dialect;

use KeyCode::{Backspace, Delete, Down, End, Enter, Home, Left, Right, Tab, Up};

fn at(row: usize, col: usize) -> Index2 {
    Index2::new(row, col)
}

#[test]
fn typing_needs_no_mode_change() {
    let mut f = Fixture::conventional("");
    f.write("select 1;");
    assert_eq!(f.text(), "select 1;");
    assert_eq!(f.editor.cursor(), at(0, 9));
}

#[test]
fn enter_breaks_the_line_and_text_after_the_cursor_moves() {
    let mut f = Fixture::conventional("ab");
    f.press(key(Right));
    f.press(key(Enter));
    assert_eq!(f.text(), "a\nb");
    assert_eq!(f.editor.cursor(), at(1, 0));
}

#[test]
fn arrows_cross_line_ends() {
    let mut f = Fixture::conventional("ab\ncd");
    f.editor.set_cursor(at(0, 2));
    f.press(key(Right));
    assert_eq!(f.editor.cursor(), at(1, 0));
    f.press(key(Left));
    assert_eq!(f.editor.cursor(), at(0, 2));
}

#[test]
fn vertical_moves_keep_the_wanted_column() {
    let mut f = Fixture::conventional("abcdef\nab\nabcdef");
    f.editor.set_cursor(at(0, 5));
    f.press(key(Down));
    assert_eq!(f.editor.cursor(), at(1, 2));
    f.press(key(Down));
    assert_eq!(f.editor.cursor(), at(2, 5));
}

#[test]
fn shift_selection_excludes_the_character_under_the_cursor() {
    let mut f = Fixture::conventional("select 1;");
    f.press(shift(Right));
    assert_eq!(f.selected().as_deref(), Some("s"));
    for _ in 0..5 {
        f.press(shift(Right));
    }
    assert_eq!(f.selected().as_deref(), Some("select"));
    assert_eq!(f.editor.cursor(), at(0, 6));
    f.press(shift(Left));
    assert_eq!(f.selected().as_deref(), Some("selec"));
}

#[test]
fn backward_selection_keeps_the_same_range() {
    let mut f = Fixture::conventional("select 1;");
    f.editor.set_cursor(at(0, 6));
    for _ in 0..6 {
        f.press(shift(Left));
    }
    assert_eq!(f.selected().as_deref(), Some("select"));
    assert_eq!(f.editor.cursor(), at(0, 0));
}

#[test]
fn selection_can_cross_lines_and_include_the_line_break() {
    let mut f = Fixture::conventional("ab\ncd");
    f.editor.set_cursor(at(0, 1));
    f.press(shift(Down));
    assert_eq!(f.selected().as_deref(), Some("b\nc"));
    f.press(shift(Left));
    assert_eq!(f.selected().as_deref(), Some("b\n"));
}

#[test]
fn arrow_without_shift_collapses_the_selection_to_its_side() {
    let mut f = Fixture::conventional("abcdef");
    f.editor.set_cursor(at(0, 2));
    f.press_all(&[shift(Right), shift(Right)]);
    f.press(key(Left));
    assert_eq!(f.editor.cursor(), at(0, 2));
    assert_eq!(f.selected(), None);

    f.press_all(&[shift(Right), shift(Right)]);
    f.press(key(Right));
    assert_eq!(f.editor.cursor(), at(0, 4));
    assert_eq!(f.selected(), None);
}

#[test]
fn shift_back_to_the_anchor_clears_the_selection() {
    let mut f = Fixture::conventional("abc");
    f.press(shift(Right));
    f.press(shift(Left));
    assert_eq!(f.selected(), None);
    assert!(f.editor.state.selection.is_none());
}

#[test]
fn drawn_selection_matches_the_selected_text() {
    let mut f = Fixture::conventional("abc\ndef");
    f.press_all(&[shift(Right), shift(Right)]);
    let selection = f.editor.state.selection.as_ref().unwrap();
    assert_eq!(selection.copy_from(&f.editor.state.lines).to_string(), "ab");

    let mut f = Fixture::conventional("abc\ndef");
    f.press(shift(Down));
    f.press(shift(Home));
    let selection = f.editor.state.selection.as_ref().unwrap();
    assert_eq!(
        selection.copy_from(&f.editor.state.lines).to_string(),
        "abc\n"
    );
}

#[test]
fn typing_over_a_selection_keeps_the_clipboard() {
    let mut f = Fixture::conventional("select 1;");
    f.clipboard.borrow_mut().text = "kept".to_owned();
    f.press_all(&[shift(Right), shift(Right)]);
    f.write("X");
    assert_eq!(f.text(), "Xlect 1;");
    assert_eq!(f.clipboard_text(), "kept");
}

#[test]
fn backspace_and_delete_remove_a_selection_without_the_clipboard() {
    for code in [Backspace, Delete] {
        let mut f = Fixture::conventional("abcd");
        f.clipboard.borrow_mut().text = "kept".to_owned();
        f.press_all(&[shift(Right), shift(Right)]);
        f.press(key(code));
        assert_eq!(f.text(), "cd", "{code:?}");
        assert_eq!(f.clipboard_text(), "kept", "{code:?}");
    }
}

#[test]
fn backspace_and_delete_join_lines() {
    let mut f = Fixture::conventional("ab\ncd");
    f.editor.set_cursor(at(1, 0));
    f.press(key(Backspace));
    assert_eq!(f.text(), "abcd");
    assert_eq!(f.editor.cursor(), at(0, 2));

    let mut f = Fixture::conventional("ab\ncd");
    f.editor.set_cursor(at(0, 2));
    f.press(key(Delete));
    assert_eq!(f.text(), "abcd");
    assert_eq!(f.editor.cursor(), at(0, 2));
}

#[test]
fn edits_at_the_document_limits_have_no_effect() {
    let mut f = Fixture::conventional("ab");
    f.press(key(Backspace));
    assert_eq!(f.text(), "ab");
    f.press(key(End));
    f.press(key(Delete));
    assert_eq!(f.text(), "ab");
}

#[test]
fn alt_arrows_move_by_word() {
    let mut f = Fixture::conventional("select  a_b, c");
    f.press(alt(Right));
    assert_eq!(f.editor.cursor(), at(0, 6));
    f.press(alt(Right));
    assert_eq!(f.editor.cursor(), at(0, 11));
    f.press(alt(Right));
    assert_eq!(f.editor.cursor(), at(0, 12));
    f.press(alt(Left));
    assert_eq!(f.editor.cursor(), at(0, 11));
    f.press(alt(Left));
    assert_eq!(f.editor.cursor(), at(0, 8));
}

#[test]
fn alt_shift_arrow_selects_a_word() {
    let mut f = Fixture::conventional("select 1");
    f.press(KeyEvent::new(
        Right,
        KeyModifiers::ALT | KeyModifiers::SHIFT,
    ));
    assert_eq!(f.selected().as_deref(), Some("select"));
}

#[test]
fn alt_backspace_deletes_a_word() {
    let mut f = Fixture::conventional("select one");
    f.press(key(End));
    f.press(alt(Backspace));
    assert_eq!(f.text(), "select ");
}

#[test]
fn ctrl_a_selects_everything() {
    let mut f = Fixture::conventional("ab\ncd");
    f.press(ctrl('a'));
    assert_eq!(f.selected().as_deref(), Some("ab\ncd"));
}

#[test]
fn copy_keeps_the_text_and_the_selection() {
    let mut f = Fixture::conventional("abcd");
    f.press_all(&[shift(Right), shift(Right), ctrl('c')]);
    assert_eq!(f.clipboard_text(), "ab");
    assert_eq!(f.text(), "abcd");
    assert_eq!(f.selected().as_deref(), Some("ab"));
}

#[test]
fn copy_and_cut_without_a_selection_have_no_effect() {
    let mut f = Fixture::conventional("abcd");
    f.clipboard.borrow_mut().text = "kept".to_owned();
    f.press_all(&[ctrl('c'), ctrl('x')]);
    assert_eq!(f.clipboard_text(), "kept");
    assert_eq!(f.text(), "abcd");
}

#[test]
fn cut_then_paste_moves_text() {
    let mut f = Fixture::conventional("abcd");
    f.press_all(&[shift(Right), shift(Right), ctrl('x')]);
    assert_eq!(f.text(), "cd");
    assert_eq!(f.clipboard_text(), "ab");
    f.press(key(End));
    f.press(ctrl('v'));
    assert_eq!(f.text(), "cdab");
    assert_eq!(f.editor.cursor(), at(0, 4));
}

#[test]
fn paste_replaces_a_selection_and_keeps_the_clipboard() {
    let mut f = Fixture::conventional("abcd");
    f.clipboard.borrow_mut().text = "x\ny".to_owned();
    f.press_all(&[shift(Right), shift(Right), ctrl('v')]);
    assert_eq!(f.text(), "x\nycd");
    assert_eq!(f.clipboard_text(), "x\ny");
    assert_eq!(f.editor.cursor(), at(1, 1));
}

#[test]
fn terminal_paste_keeps_a_leading_line_break_and_the_clipboard() {
    let mut f = Fixture::conventional("ab");
    f.clipboard.borrow_mut().text = "kept".to_owned();
    f.press(key(Right));
    f.editor.on_paste("\r\nx\ry");
    assert_eq!(f.text(), "a\nx\nyb");
    assert_eq!(f.clipboard_text(), "kept");
}

#[test]
fn paste_is_one_undo_step() {
    let mut f = Fixture::conventional("ab");
    f.editor.on_paste("one two\nthree");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "ab");
}

#[test]
fn undo_steps_follow_words_and_not_keys() {
    let mut f = Fixture::conventional("");
    f.write("select one");
    assert_eq!(f.text(), "select one");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "select");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "");
    assert_eq!(f.editor.cursor(), at(0, 0));
}

#[test]
fn a_long_word_is_one_undo_step() {
    let mut f = Fixture::conventional("");
    f.write(&"x".repeat(300));
    f.press(ctrl('z'));
    assert_eq!(f.text(), "");
}

#[test]
fn moving_the_cursor_starts_a_new_undo_step() {
    let mut f = Fixture::conventional("");
    f.write("ab");
    f.press(key(Left));
    f.write("c");
    assert_eq!(f.text(), "acb");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "ab");
}

#[test]
fn deletion_groups_and_breaks_at_a_change_of_kind() {
    let mut f = Fixture::conventional("");
    f.write("abcd");
    f.press_all(&[key(Backspace), key(Backspace)]);
    assert_eq!(f.text(), "ab");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "abcd");
}

#[test]
fn redo_restores_undone_text() {
    let mut f = Fixture::conventional("");
    f.write("select");
    f.press(ctrl('z'));
    f.press(ctrl('y'));
    assert_eq!(f.text(), "select");
    f.press(ctrl('z'));
    f.press(KeyEvent::new(
        KeyCode::Char('Z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(f.text(), "select");
}

#[test]
fn a_new_edit_clears_the_redo_steps() {
    let mut f = Fixture::conventional("");
    f.write("old");
    f.press(ctrl('z'));
    f.write("new");
    f.press(ctrl('y'));
    assert_eq!(f.text(), "new");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "");
    f.press(ctrl('y'));
    assert_eq!(f.text(), "new");
}

#[test]
fn undo_with_an_empty_history_keeps_redo_safe() {
    let mut f = Fixture::conventional("base");
    f.press(ctrl('z'));
    f.press(ctrl('y'));
    assert_eq!(f.text(), "base");
}

#[test]
fn multibyte_text_edits_by_character() {
    let mut f = Fixture::conventional("ação日本");
    f.press(key(End));
    f.press(key(Backspace));
    assert_eq!(f.text(), "ação日");
    f.press_all(&[shift(Left), shift(Left)]);
    assert_eq!(f.selected().as_deref(), Some("o日"));
    f.write("é");
    assert_eq!(f.text(), "açãé");
}

#[test]
fn tab_inserts_spaces_and_has_no_effect_on_a_selection() {
    let mut f = Fixture::conventional("ab");
    f.press(key(Tab));
    assert_eq!(f.text(), "    ab");
    f.press_all(&[shift(Right), key(Tab)]);
    assert_eq!(f.text(), "    ab");
}

#[test]
fn a_letter_with_the_command_key_is_not_text() {
    let mut f = Fixture::conventional("ab");
    f.press(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::SUPER));
    assert_eq!(f.text(), "ab");
    assert!(f.editor.suggestions().is_none());
}

#[test]
fn ctrl_c_is_not_text() {
    let mut f = Fixture::conventional("ab");
    f.press(ctrl('c'));
    assert_eq!(f.text(), "ab");
}

#[test]
fn home_and_end_are_extras_and_not_the_only_path() {
    let mut f = Fixture::conventional("abc\ndef");
    f.editor.set_cursor(at(0, 1));
    f.press(key(End));
    assert_eq!(f.editor.cursor(), at(0, 3));
    f.press(key(Home));
    assert_eq!(f.editor.cursor(), at(0, 0));
    f.press(KeyEvent::new(End, KeyModifiers::CONTROL));
    assert_eq!(f.editor.cursor(), at(1, 3));
    f.press(KeyEvent::new(Home, KeyModifiers::CONTROL));
    assert_eq!(f.editor.cursor(), at(0, 0));
}

#[test]
fn up_on_the_first_row_keeps_the_cursor_row() {
    let mut f = Fixture::conventional("ab\ncd");
    f.editor.set_cursor(at(0, 1));
    f.press(key(Up));
    assert_eq!(f.editor.cursor(), at(0, 1));
}

#[test]
fn a_comment_goes_on_the_row_of_the_cursor_and_comes_off() {
    let mut f = Fixture::conventional("select 1;\nselect 2;");
    f.editor.set_cursor(at(1, 3));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "select 1;\n-- select 2;");
    assert_eq!(f.editor.cursor(), at(1, 6));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "select 1;\nselect 2;");
    assert_eq!(f.editor.cursor(), at(1, 3));
}

#[test]
fn a_selection_that_ends_at_a_row_start_leaves_that_row_out() {
    let mut f = Fixture::conventional("a\nb\nc");
    f.press(shift(Down));
    f.press(shift(Down));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "-- a\n-- b\nc");
    assert!(f.selected().is_some(), "the selection stays");
    f.editor.toggle_comment();
    assert_eq!(f.text(), "a\nb\nc");
}

#[test]
fn rows_with_and_without_a_comment_all_receive_one_at_the_smallest_indent() {
    let mut f = Fixture::conventional("    a\n\n  -- b\n  c");
    f.press(ctrl('a'));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "  --   a\n\n  -- -- b\n  -- c");
}

#[test]
fn rows_that_all_hold_a_comment_lose_it() {
    let mut f = Fixture::conventional("-- a\n--b\n\n    -- c");
    f.press(ctrl('a'));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "a\nb\n\n    c");
}

#[test]
fn a_blank_row_receives_no_comment() {
    let mut f = Fixture::conventional("a\n   \nb");
    f.editor.set_cursor(at(1, 1));
    let revision = f.editor.revision();
    f.editor.toggle_comment();
    assert_eq!(f.text(), "a\n   \nb");
    assert_eq!(f.editor.revision(), revision);
}

#[test]
fn a_comment_is_one_undo_step() {
    let mut f = Fixture::conventional("a\nb");
    f.press(ctrl('a'));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "-- a\n-- b");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "a\nb");
}

#[test]
fn a_cursor_inside_a_removed_comment_stays_at_the_start_of_the_text() {
    let mut f = Fixture::conventional("  -- a");
    f.editor.set_cursor(at(0, 3));
    f.editor.toggle_comment();
    assert_eq!(f.text(), "  a");
    assert_eq!(f.editor.cursor(), at(0, 2));
}

#[test]
fn redis_uses_its_own_comment() {
    let mut f = Fixture::with_dialect("GET a", KeyboardMode::Conventional, Dialect::Redis);
    f.editor.toggle_comment();
    assert_eq!(f.text(), "# GET a");
    f.editor.toggle_comment();
    assert_eq!(f.text(), "GET a");
}

fn fixture_with_marks(text: &str) -> Fixture {
    let mut f = Fixture::conventional(text);
    let plan = f.editor.plan_run(RunScope::Script).unwrap();
    f.editor.set_marks(&plan);
    f
}

fn rows_after_paste(f: &mut Fixture, text: &str) -> Vec<usize> {
    f.press(ctrl('a'));
    f.editor.on_paste(text);
    f.editor.marks().iter().map(|mark| mark.row).collect()
}

#[test]
fn equal_statements_keep_one_mark_each() {
    let mut f = fixture_with_marks("SELECT 1;\nSELECT 1;\nSELECT 2;\nSELECT 1;");
    let text = "\nSELECT 1;\nSELECT 1;\nSELECT 2;\nSELECT 1;";
    assert_eq!(rows_after_paste(&mut f, text), [1, 2, 3, 4]);
}

#[test]
fn a_mark_goes_to_the_nearest_copy_of_its_statement() {
    let mut f = fixture_with_marks("\n\n\n\n\nSELECT 1;");
    let text = "SELECT 1;\n\n\n\n\n\nSELECT 1;";
    assert_eq!(rows_after_paste(&mut f, text), [6]);
}

#[test]
fn two_copies_at_the_same_distance_give_the_mark_to_the_first_one() {
    let mut f = fixture_with_marks("SELECT 9;\nSELECT 1;\nSELECT 8;");
    let text = "SELECT 1;\nSELECT 2;\nSELECT 1;";
    assert_eq!(rows_after_paste(&mut f, text), [0]);
}

#[test]
fn a_mark_without_a_free_copy_of_its_text_goes_away() {
    let mut f = fixture_with_marks("SELECT 1;\nSELECT 1;\nSELECT 2;");
    assert_eq!(rows_after_paste(&mut f, "SELECT 3;\nSELECT 1;"), [1]);
}
