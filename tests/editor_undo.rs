// Tests of the undo history of the conventional mode and of its change to the Vim mode.

use crate::common::{Fixture, ctrl, key, shift};
use crossterm::event::KeyCode::{
    self, Backspace, Char, Delete, Down, End, Enter, Esc, Left, Right, Tab, Up,
};
use crossterm::event::{KeyEvent, KeyModifiers};
use edtui::Index2;
use sqln::editor::KeyboardMode;

/// A small generator, so a seed always gives the same edits.
struct Random(u64);

impl Random {
    fn below(&mut self, limit: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as usize % limit
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// Keeps each text that the keys give, because a step can end inside a word.
fn note(f: &Fixture, texts: &mut Vec<String>) {
    if texts.last() != Some(&f.text()) {
        texts.push(f.text());
    }
}

fn act(f: &mut Fixture, random: &mut Random, texts: &mut Vec<String>) {
    const ARROWS: [KeyCode; 4] = [Left, Right, Up, Down];
    match random.below(16) {
        0..=3 => {
            for c in random
                .pick(&["select", "ação", "日本語", "a b", "🦀"])
                .chars()
            {
                f.press(key(Char(c)));
                note(f, texts);
            }
        }
        4 => f.press(key(Enter)),
        5 => f.press(key(Backspace)),
        6 => f.press(key(Delete)),
        7 => f.press(KeyEvent::new(Backspace, KeyModifiers::CONTROL)),
        8 => f.press(key(random.pick(&ARROWS))),
        9 => f.press(shift(random.pick(&ARROWS))),
        10 => f.editor.on_paste("one\ntwo\n\nthree"),
        11 => f.press(ctrl('x')),
        12 => f.press(ctrl('a')),
        13 => f.editor.toggle_comment(),
        14 => {
            let _ = f.editor.format_sql();
        }
        _ => {
            f.press(ctrl('f'));
            f.write("se");
            f.press(key(Tab));
            f.write("x\ny");
            f.press(ctrl('a'));
            f.press(key(Esc));
        }
    }
    note(f, texts);
}

fn state(f: &Fixture) -> (String, Index2) {
    (f.text(), f.editor.cursor())
}

/// Presses a key that moves through the history. It returns false at the end of it.
fn step(f: &mut Fixture, event: KeyEvent) -> bool {
    let revision = f.editor.revision();
    f.press(event);
    f.editor.revision() != revision
}

fn undo_count(f: &mut Fixture) -> usize {
    std::iter::repeat_with(|| step(f, ctrl('z')))
        .take_while(|changed| *changed)
        .count()
}

/// Goes through the history to its start, and it returns each state on the way.
fn walk_back(f: &mut Fixture) -> Vec<(String, Index2)> {
    let mut states = vec![state(f)];
    while step(f, ctrl('z')) {
        states.push(state(f));
    }
    states
}

#[test]
fn undo_and_redo_go_through_the_same_states_after_random_edits() {
    for seed in 0..60 {
        let start = "SELECT 1;\nSELECT 2;";
        let mut f = Fixture::conventional(start);
        let mut random = Random(seed);
        let mut texts = vec![f.text()];
        for _ in 0..200 {
            act(&mut f, &mut random, &mut texts);
        }
        let walked = walk_back(&mut f);
        assert_eq!(f.text(), start, "seed {seed}");
        // Each state is an older text, and a step can hold many edits. A step that writes
        // the same text gives the same state again.
        let mut place = texts.len() - 1;
        for (text, _) in &walked {
            let found = texts[..=place].iter().rposition(|old| old == text);
            place = found.unwrap_or_else(|| panic!("seed {seed}: {text:?} is not an old text"));
        }
        for expected in walked.iter().rev().skip(1) {
            f.press(ctrl('y'));
            assert_eq!(state(&f), *expected, "seed {seed}");
        }
        assert!(!step(&mut f, ctrl('y')), "seed {seed}");
        assert_eq!(walk_back(&mut f), walked, "seed {seed}");
    }
}

#[test]
fn undo_returns_the_cursor_of_the_start_of_the_step_and_redo_the_end() {
    let mut f = Fixture::conventional("abc\ndef");
    f.editor.set_cursor(Index2::new(1, 1));
    f.write("xy");
    assert_eq!(f.editor.cursor(), Index2::new(1, 3));
    f.press(ctrl('z'));
    assert_eq!(f.text(), "abc\ndef");
    assert_eq!(f.editor.cursor(), Index2::new(1, 1));
    f.press(ctrl('y'));
    assert_eq!(f.text(), "abc\ndxyef");
    assert_eq!(f.editor.cursor(), Index2::new(1, 3));
}

#[test]
fn deleted_characters_join_one_step_in_their_order() {
    let mut f = Fixture::conventional("abcdef");
    f.editor.set_cursor(Index2::new(0, 3));
    f.press_all(&[key(Backspace), key(Backspace)]);
    assert_eq!(f.text(), "adef");
    f.press_all(&[key(Delete), key(Delete)]);
    assert_eq!(f.text(), "af");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "abcdef");
    assert_eq!(f.editor.cursor(), Index2::new(0, 3));
    f.press(ctrl('y'));
    assert_eq!(f.text(), "af");
    assert_eq!(f.editor.cursor(), Index2::new(0, 1));
}

#[test]
fn a_step_of_deleted_characters_crosses_rows_and_keeps_wide_text() {
    let mut f = Fixture::conventional("日本\n語x");
    f.editor.set_cursor(Index2::new(1, 1));
    f.press_all(&[key(Backspace), key(Backspace), key(Backspace)]);
    assert_eq!(f.text(), "日x");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "日本\n語x");
    assert_eq!(f.editor.cursor(), Index2::new(1, 1));
}

#[test]
fn a_step_of_many_ranges_returns_the_text_and_redoes_it() {
    let text = "users x\nmiddle\nlast users end";
    let mut f = Fixture::conventional(text);
    f.press(ctrl('f'));
    f.write("users");
    f.press(key(Tab));
    f.write("a\nb");
    f.press(ctrl('a'));
    f.press(key(Esc));
    let replaced = f.text();
    assert_eq!(replaced, "a\nb x\nmiddle\nlast a\nb end");
    f.press(ctrl('z'));
    assert_eq!(f.text(), text);
    f.press(ctrl('y'));
    assert_eq!(f.text(), replaced);
}

#[test]
fn the_history_keeps_more_than_100_steps() {
    let mut f = Fixture::conventional("");
    for index in 0..150 {
        f.write(&format!("w{index} "));
    }
    assert!(undo_count(&mut f) > 140);
    assert_eq!(f.text(), "");
}

const ROW_BYTES: usize = 300;

fn rows(count: usize) -> String {
    vec!["x".repeat(ROW_BYTES); count].join("\n")
}

/// Empties a row in one step, and the step keeps the text of the row.
fn remove_row(f: &mut Fixture, row: usize) {
    f.editor.set_cursor(Index2::new(row, 0));
    f.press_all(&[shift(End), key(Backspace)]);
}

fn removed_rows(f: &mut Fixture, count: usize) {
    (0..count).for_each(|row| remove_row(f, row));
}

#[test]
fn the_limit_removes_the_oldest_steps_first() {
    let mut f = Fixture::conventional(&rows(6));
    f.editor.set_undo_limit(2 * ROW_BYTES + 200);
    removed_rows(&mut f, 6);
    assert_eq!(undo_count(&mut f), 2);
    let text = f.text();
    let filled: Vec<bool> = text.split('\n').map(|row| !row.is_empty()).collect();
    assert_eq!(filled, [false, false, false, false, true, true]);
}

#[test]
fn a_lower_limit_removes_steps_at_once() {
    let mut f = Fixture::conventional(&rows(6));
    removed_rows(&mut f, 6);
    f.editor.set_undo_limit(2 * ROW_BYTES + 200);
    assert_eq!(undo_count(&mut f), 2);
}

#[test]
fn the_newest_step_stays_above_the_limit() {
    let mut f = Fixture::conventional(&rows(3));
    f.editor.set_undo_limit(1);
    removed_rows(&mut f, 3);
    assert_eq!(undo_count(&mut f), 1);
    f.press(ctrl('y'));
    assert_eq!(undo_count(&mut f), 1, "redo also keeps its newest step");
}

#[test]
fn the_limit_removes_the_farthest_redo_step() {
    let mut f = Fixture::conventional("");
    f.editor.set_undo_limit(200);
    f.editor.on_paste(&"a".repeat(1000));
    f.editor.on_paste(&"b".repeat(1000));
    assert_eq!(undo_count(&mut f), 2);
    assert_eq!(f.text(), "");
    f.press(ctrl('y'));
    assert_eq!(f.text(), "a".repeat(1000));
    assert!(!step(&mut f, ctrl('y')));
}

#[test]
fn a_new_edit_ends_the_redo_steps() {
    let mut f = Fixture::conventional("");
    f.write("one");
    f.press(ctrl('z'));
    f.write("two");
    assert!(!step(&mut f, ctrl('y')));
    assert_eq!(f.text(), "two");
}

fn vim_undo(f: &mut Fixture) -> bool {
    step(f, key(Char('u')))
}

#[test]
fn the_vim_mode_undoes_each_step_of_the_conventional_mode() {
    let mut f = Fixture::conventional("");
    f.write("one two three");
    let last = f.text();
    f.editor.set_keyboard(KeyboardMode::Vim);
    assert_eq!(f.text(), last);
    assert!(vim_undo(&mut f));
    assert_eq!(f.text(), "one two");
    assert!(vim_undo(&mut f));
    assert_eq!(f.text(), "one");
    assert!(step(&mut f, ctrl('r')));
    assert_eq!(f.text(), "one two");
    assert!(step(&mut f, ctrl('r')));
    assert_eq!(f.text(), last);
    assert!(!step(&mut f, ctrl('r')));
}

#[test]
fn the_vim_mode_receives_the_100_newest_steps() {
    let mut f = Fixture::conventional("");
    for index in 0..130 {
        f.write(&format!("w{index} "));
    }
    f.editor.set_keyboard(KeyboardMode::Vim);
    let count = std::iter::repeat_with(|| vim_undo(&mut f))
        .take_while(|changed| *changed)
        .count();
    assert_eq!(count, 100);
    assert!(f.text().starts_with("w0 "), "{}", f.text());
}

/// The reverse of a paste keeps the text, so the limit must not act during the change.
#[test]
fn the_change_to_vim_keeps_the_text_with_a_limit_below_the_steps() {
    let mut f = Fixture::conventional("");
    f.editor.set_undo_limit(200);
    for _ in 0..3 {
        f.editor.on_paste(&"x".repeat(1000));
    }
    let before = state(&f);
    f.editor.set_keyboard(KeyboardMode::Vim);
    assert_eq!(state(&f).0, before.0);
}

#[test]
fn a_change_to_vim_ends_the_steps_that_wait_for_a_redo() {
    let mut f = Fixture::conventional("");
    f.write("one two");
    f.press(ctrl('z'));
    f.editor.set_keyboard(KeyboardMode::Vim);
    assert_eq!(f.text(), "one");
    assert!(!step(&mut f, ctrl('r')));
    assert!(vim_undo(&mut f));
    assert_eq!(f.text(), "");
}

#[test]
fn the_conventional_mode_reaches_the_steps_of_the_vim_mode_after_its_own() {
    let mut f = Fixture::vim("abc");
    f.press(key(Char('x')));
    assert_eq!(f.text(), "bc");
    f.editor.set_keyboard(KeyboardMode::Conventional);
    f.editor.set_cursor(Index2::new(0, 2));
    f.write("d");
    assert_eq!(f.text(), "bcd");

    f.press(ctrl('z'));
    assert_eq!(f.text(), "bc");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "abc");
    assert!(!step(&mut f, ctrl('z')));

    f.press(ctrl('y'));
    assert_eq!(f.text(), "bc");
    f.press(ctrl('y'));
    assert_eq!(f.text(), "bcd");
    assert!(!step(&mut f, ctrl('y')));
}

#[test]
fn a_new_edit_ends_the_redo_steps_of_both_histories() {
    let mut f = Fixture::vim("abc");
    f.press(key(Char('x')));
    f.editor.set_keyboard(KeyboardMode::Conventional);
    f.write("d");
    f.press(ctrl('z'));
    f.press(ctrl('z'));
    assert_eq!(f.text(), "abc");
    f.write("e");
    assert!(!step(&mut f, ctrl('y')));
    assert_eq!(f.text(), "eabc");
}

#[test]
fn a_change_there_and_back_keeps_the_history() {
    let mut f = Fixture::conventional("");
    f.write("one two");
    f.editor.set_keyboard(KeyboardMode::Vim);
    f.editor.set_keyboard(KeyboardMode::Conventional);
    f.press(ctrl('z'));
    assert_eq!(f.text(), "one");
    f.press(ctrl('z'));
    assert_eq!(f.text(), "");
    f.press(ctrl('y'));
    assert_eq!(f.text(), "one");
}
