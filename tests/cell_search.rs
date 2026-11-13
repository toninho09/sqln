use super::*;
use ratatui::layout::Rect;

fn open(f: &mut Fixture, value: &str) {
    f.run_script(&format!("SELECT '{}' AS value;", value.replace('\'', "''")));
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    f.buffer();
}

fn finish(f: &mut Fixture) {
    f.pump_until(|f| !f.app.has_work());
}

#[test]
fn cell_search_finds_unicode_and_navigates_without_changing_or_running_text() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = Fixture::with("cell-search", mode, Settings::default(), "");
        let value = "Água water\nagain ÁGUA";
        open(&mut f, value);
        let sql = f.app.console().editor.text();
        f.press(ctrl('f'));
        f.write("água");
        finish(&mut f);
        f.assert_sees("Find: água");
        f.assert_sees("1 of 2");
        f.press(key(KeyCode::Enter));
        finish(&mut f);
        f.assert_sees("2 of 2");
        f.press(shift(KeyCode::Enter));
        finish(&mut f);
        f.assert_sees("1 of 2");
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, value);
        f.press(key(KeyCode::Esc));
        assert!(!f.sees("Find:"));
        f.assert_sees("characters");
        assert_eq!(f.app.console().editor.text(), sql);
        assert!(!f.app.console().is_busy());
        f.press(key(KeyCode::Esc));
        assert!(!f.sees("characters"));
    }
}

#[test]
fn search_crosses_display_wraps_and_shows_no_matches() {
    let mut f = Fixture::new("cell-search-wrap");
    let width = sqln::ui::console::overlay_text_area(Rect::new(0, 0, 100, 30), true).width;
    let value = format!("{}boundary", "x".repeat(usize::from(width) - 3));
    open(&mut f, &value);
    f.press(ctrl('f'));
    f.write("boundary");
    finish(&mut f);
    let buffer = f.buffer();
    let area = sqln::ui::console::overlay_text_area(buffer.area, true);
    assert_eq!(buffer[(area.right() - 3, area.y)].symbol(), "b");
    assert!(
        buffer[(area.right() - 3, area.y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    assert!(
        buffer[(area.x, area.y + 1)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    for _ in 0..8 {
        f.press(key(KeyCode::Backspace));
    }
    f.write("missing");
    finish(&mut f);
    f.assert_sees(sqln::ui::text::NO_MATCHES);
    f.press(key(KeyCode::Esc));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, value);
}

#[test]
fn search_preserves_selection_and_its_character_count() {
    let mut f = Fixture::new("cell-search-selection");
    open(&mut f, "ação 日本\nnext");
    for _ in 0..4 {
        f.press(shift(KeyCode::Right));
    }
    f.assert_sees("4 characters selected");
    f.press(ctrl('f'));
    finish(&mut f);
    f.assert_sees("Find: ação");
    f.assert_sees("4 characters selected");
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "ação");
}

#[test]
fn search_query_text_does_not_toggle_json_or_reach_background_commands() {
    let mut f = Fixture::new("cell-search-json");
    let value = r#"{"first":"foo","second":"foo"}"#;
    open(&mut f, value);
    f.press(ctrl('f'));
    f.type_text("foo");
    finish(&mut f);
    f.assert_sees("1 of 2");
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    assert!(!f.app.console().is_busy());
    f.press(ctrl('c'));
    assert_eq!(
        f.clipboard.borrow().text,
        sqln::grid::json::pretty(value).unwrap()
    );
}

#[test]
fn cell_search_accepts_a_custom_find_sequence() {
    let mut f = Fixture::new("cell-search-custom");
    let keys = sqln::input::Keymap::with_overrides(
        KeyboardMode::Conventional,
        &std::collections::HashMap::from([("editorFind".into(), vec!["alt+f f".into()])]),
    )
    .unwrap();
    f.app = App::new(
        Options {
            database: Some(f.path.clone()),
            keymap: Some(keys),
            ..Options::default()
        },
        f.clipboard.clone(),
        Theme::plain(),
    );
    open(&mut f, "find this");
    f.press(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT));
    f.press(key(KeyCode::Char('f')));
    f.write("this");
    finish(&mut f);
    f.assert_sees("1 of 1");
    f.press(key(KeyCode::Esc));
    f.press(key(KeyCode::Esc));
    assert!(!f.app.console().is_busy());
}

#[test]
fn empty_values_allow_search_and_cancel() {
    let mut f = Fixture::new("cell-search-empty");
    open(&mut f, "");
    f.press(ctrl('f'));
    f.write("x");
    finish(&mut f);
    f.assert_sees(sqln::ui::text::NO_MATCHES);
    f.press(key(KeyCode::Esc));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "");
}
