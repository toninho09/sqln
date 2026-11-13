use super::*;
use ratatui::layout::{Margin, Rect};

fn inner(width: u16, height: u16) -> Rect {
    sqln::ui::console::overlay_area(Rect::new(0, 0, width, height)).inner(Margin::new(1, 1))
}

fn open(f: &mut Fixture, value: &str) {
    f.run_script(&format!("SELECT '{}' AS value;", value.replace('\'', "''")));
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    f.buffer();
}

#[test]
fn shift_arrows_select_original_text_in_both_keyboard_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = Fixture::with("cell-keyboard-selection", mode, Settings::default(), "");
        let value = "ação 日本\nnext";
        open(&mut f, value);
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, value);
        for _ in 0..4 {
            f.press(shift(KeyCode::Right));
        }
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, "ação");
        f.press(shift(KeyCode::Down));
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, value);
        f.press(shift(KeyCode::Left));
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, "ação 日本\nnex");
        f.press(shift(KeyCode::Up));
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, "açã");
        f.press(key(KeyCode::Char('x')));
        f.press(key(KeyCode::Esc));
        f.press(key(KeyCode::Enter));
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, value);
    }
}

#[test]
fn mouse_selection_uses_display_columns_and_clears_on_a_click() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = Fixture::with("cell-mouse-selection", mode, Settings::default(), "");
        let value = "ação 日本\nsecond line";
        open(&mut f, value);
        let area = inner(100, 30);
        f.wheel(MouseEventKind::Down(MouseButton::Left), area.x + 5, area.y);
        f.wheel(MouseEventKind::Drag(MouseButton::Left), area.x + 9, area.y);
        f.wheel(MouseEventKind::Up(MouseButton::Left), area.x + 9, area.y);
        let buffer = f.buffer();
        assert!(
            buffer[(area.x + 5, area.y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, "日本");

        f.wheel(
            MouseEventKind::Down(MouseButton::Left),
            area.x + 6,
            area.y + 1,
        );
        f.wheel(MouseEventKind::Drag(MouseButton::Left), area.x + 5, area.y);
        f.wheel(MouseEventKind::Up(MouseButton::Left), area.x + 5, area.y);
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, "日本\nsecond");
        f.wheel(MouseEventKind::ScrollDown, area.x, area.y);
        f.buffer();
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, "日本\nsecond");
        f.wheel(MouseEventKind::Down(MouseButton::Left), area.x, area.y);
        f.wheel(MouseEventKind::Up(MouseButton::Left), area.x, area.y);
        f.press(ctrl('c'));
        assert_eq!(f.clipboard.borrow().text, value);
    }
}

#[test]
fn mouse_selection_preserves_wrapped_text_and_survives_resize() {
    let mut f = Fixture::new("cell-selection-resize");
    let area = inner(100, 30);
    let value = format!("{}日本\nlast", "x".repeat(usize::from(area.width)));
    open(&mut f, &value);
    f.wheel(
        MouseEventKind::Down(MouseButton::Left),
        area.right() - 2,
        area.y,
    );
    f.wheel(
        MouseEventKind::Drag(MouseButton::Left),
        area.x + 4,
        area.y + 1,
    );
    f.wheel(
        MouseEventKind::Up(MouseButton::Left),
        area.x + 4,
        area.y + 1,
    );
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "xx日本");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "xx日本");
    f.press(shift(KeyCode::Right));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "xx日本\n");
    let area = inner(80, 24);
    f.wheel(MouseEventKind::Down(MouseButton::Left), area.x + 1, area.y);
    f.wheel(MouseEventKind::Drag(MouseButton::Left), area.x + 4, area.y);
    f.wheel(MouseEventKind::Up(MouseButton::Left), area.x + 4, area.y);
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "xxx");
}

#[test]
fn dragging_past_the_window_scrolls_and_background_keys_do_not_run() {
    let mut f = Fixture::new("cell-selection-scroll");
    let value = (0..60)
        .map(|i| format!("line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    open(&mut f, &value);
    let area = inner(100, 30);
    f.wheel(MouseEventKind::Down(MouseButton::Left), area.x, area.y);
    for _ in 0..4 {
        f.wheel(
            MouseEventKind::Drag(MouseButton::Left),
            area.right(),
            area.bottom(),
        );
        f.buffer();
    }
    f.wheel(
        MouseEventKind::Up(MouseButton::Left),
        area.right(),
        area.bottom(),
    );
    f.press(ctrl('c'));
    let copied = f.clipboard.borrow().text.clone();
    assert!(copied.starts_with("line 00\n"));
    assert!(copied.contains(&format!("line {:02}", area.height + 2)));
    f.press(KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT));
    assert!(!f.app.console().is_busy());
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, copied);
}

#[test]
fn changing_rows_clears_selection_and_keeps_row_navigation() {
    let mut f = Fixture::new("row-window-selection");
    f.run_script("SELECT 'first' AS value UNION ALL SELECT 'second';");
    f.prefix('j');
    f.press(key(KeyCode::Char('r')));
    f.buffer();
    f.press(shift(KeyCode::Right));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text.chars().count(), 1);
    f.press(key(KeyCode::Right));
    f.press(ctrl('c'));
    assert!(f.clipboard.borrow().text.contains("second"));
    assert!(!f.clipboard.borrow().text.contains("first"));
}

#[test]
fn switching_json_form_clears_the_old_selection() {
    let mut f = Fixture::new("cell-selection-json");
    open(&mut f, r#"{"a":1}"#);
    f.press(shift(KeyCode::Right));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "{");
    f.press(key(KeyCode::Char('f')));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, r#"{"a":1}"#);
}
