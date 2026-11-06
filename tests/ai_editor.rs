mod common;
use common::{Fixture, ctrl, key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::Index2;
use sqln::input::{Action, SuggestionKeys};

#[test]
fn accepting_a_vim_visual_replacement_ends_visual_mode_and_undo_restores_text() {
    let mut f = Fixture::vim("SELECT café;");
    f.editor.set_cursor(Index2::new(0, 7));
    f.press(key(KeyCode::Char('v')));
    for _ in 0..3 {
        f.press(key(KeyCode::Char('l')));
    }
    assert_eq!(f.editor.selected_text().as_deref(), Some("café"));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    assert_eq!(f.editor.selected_text().as_deref(), Some("café"));
    // A selection opens a diff review; acceptance applies it as one undo step.
    let hunks = sqln::editor::review::diff_hunks(
        "SELECT café;",
        "SELECT name;",
        0,
        std::time::Duration::from_millis(100),
    );
    f.editor.start_review(sqln::editor::review::Review::diff(
        sqln::editor::review::Source::Ai,
        "AI".into(),
        hunks,
    ));
    f.editor.review_decide(true, true).unwrap();
    assert_eq!(f.editor.text(), "SELECT name;");
    f.editor.on_command(Action::EditorUndo, None);
    assert_eq!(f.editor.text(), "SELECT café;");
}
#[test]
fn ai_activation_uses_the_configured_suggestion_keys() {
    for keys in [
        SuggestionKeys::Tab,
        SuggestionKeys::Enter,
        SuggestionKeys::Both,
    ] {
        let mut f = Fixture::conventional("-- query");
        f.editor.set_cursor(Index2::new(0, 8));
        f.editor.set_ai_available(true);
        f.press(ctrl(' '));
        let code = if keys == SuggestionKeys::Enter {
            KeyCode::Enter
        } else {
            KeyCode::Tab
        };
        assert!(f.editor.on_list_key(key(code), keys));
        assert!(f.editor.take_ai_request());
        assert_eq!(f.editor.text(), "-- query");
    }
}
#[test]
fn disabling_ai_closes_a_selected_ai_row_without_editing() {
    let mut f = Fixture::conventional("-- query");
    f.editor.set_cursor(Index2::new(0, 8));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    f.editor.set_ai_available(false);
    assert!(f.editor.suggestions().is_none());
    assert!(!f.editor.take_ai_request());
    assert_eq!(f.editor.text(), "-- query");
}
#[test]
fn a_modified_acceptance_key_does_not_start_ai() {
    let mut f = Fixture::conventional("-- query");
    f.editor.set_cursor(Index2::new(0, 8));
    f.editor.set_ai_available(true);
    f.press(ctrl(' '));
    assert!(!f.editor.on_list_key(
        KeyEvent::new(KeyCode::Tab, KeyModifiers::ALT),
        SuggestionKeys::Both
    ));
    assert!(!f.editor.take_ai_request());
}
