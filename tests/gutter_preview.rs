use crate::common::{Fixture, ctrl, key, shift};
use crossterm::event::KeyCode;
use edtui::Index2;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use sqln::editor::{KeyboardMode, MarkState};
use sqln::ui::draw_editor;
use sqln::ui::theme::Theme;

fn draw(f: &mut Fixture, theme: &Theme, width: u16, height: u16, focused: bool) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_editor(frame, frame.area(), &mut f.editor, focused, theme))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn is_target(buffer: &Buffer, x: u16, y: u16, theme: &Theme) -> bool {
    let cell = &buffer[(x, y)];
    match theme.list_selected.bg {
        Some(background) => cell.bg == background,
        None => cell.modifier.contains(Modifier::REVERSED),
    }
}

#[test]
fn statement_lines_have_a_continuous_number_highlight_in_both_modes_and_all_color_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        for theme in [Theme::astrodark(), Theme::ansi(), Theme::plain()] {
            let mut f = Fixture::new("-- note\nSELECT\n  'ação';\n\nSELECT 2;", mode);
            f.editor.set_cursor(Index2::new(1, 0));
            let before = f.text();
            let cursor = f.editor.cursor();
            let revision = f.editor.revision();
            let buffer = draw(&mut f, &theme, 80, 24, true);
            for y in 0..5 {
                for x in 0..3 {
                    assert_eq!(
                        is_target(&buffer, x, y, &theme),
                        y == 1 || y == 2,
                        "x={x}, y={y}, mode={mode:?}"
                    );
                }
            }
            assert_eq!(buffer[(1, 1)].symbol(), "2");
            assert_eq!(buffer[(1, 2)].symbol(), "3");
            assert_eq!(f.text(), before);
            assert_eq!(f.editor.cursor(), cursor);
            assert_eq!(f.editor.revision(), revision);
            assert!(f.editor.selection_range().is_none());
            if theme == Theme::plain() {
                assert!(
                    buffer
                        .content()
                        .iter()
                        .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
                );
            }
        }
    }
}

#[test]
fn cursor_moves_update_the_highlight_and_blank_lines_clear_it() {
    let theme = Theme::ansi();
    let mut f = Fixture::conventional("SELECT 1;\n\nSELECT\n 2;");
    let first = draw(&mut f, &theme, 80, 24, true);
    assert!(is_target(&first, 1, 0, &theme));
    f.press(key(KeyCode::Down));
    let blank = draw(&mut f, &theme, 80, 24, true);
    assert!((0..4).all(|y| !is_target(&blank, 1, y, &theme)));
    f.press(key(KeyCode::Down));
    let second = draw(&mut f, &theme, 80, 24, false);
    assert!(!is_target(&second, 1, 0, &theme));
    assert!(is_target(&second, 1, 2, &theme));
    assert!(is_target(&second, 1, 3, &theme));
}

#[test]
fn selections_and_literal_cell_editors_do_not_mark_a_cursor_statement() {
    let theme = Theme::ansi();
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = Fixture::new("SELECT 1;\nSELECT 2;", mode);
        if mode == KeyboardMode::Conventional {
            f.press(shift(KeyCode::Right));
        } else {
            f.press(key(KeyCode::Char('v')));
        }
        let selection = f.editor.selection_range();
        let selected = draw(&mut f, &theme, 80, 24, true);
        assert!((0..2).all(|y| !is_target(&selected, 1, y, &theme)));
        assert_eq!(f.editor.selection_range(), selection);
        f.editor.use_literal_text("SELECT\n 1;");
        let literal = draw(&mut f, &theme, 80, 24, true);
        assert!((0..2).all(|y| !is_target(&literal, 1, y, &theme)));
    }
}

#[test]
fn pending_ai_text_does_not_change_the_highlighted_statement() {
    let theme = Theme::ansi();
    let mut f = Fixture::conventional("SELECT\n 1;\nSELECT 2;");
    f.editor
        .start_ghost(Index2::new(0, 6), " more\n-- pending line");
    let buffer = draw(&mut f, &theme, 80, 24, true);
    for y in 0..24 {
        let number = buffer[(1, y)].symbol();
        assert_eq!(
            is_target(&buffer, 1, y, &theme),
            number == "1" || number == "2"
        );
    }
    assert!(f.editor.has_ghosts());
    assert_eq!(
        f.editor
            .plan_run(sqln::editor::RunScope::Default)
            .unwrap()
            .statements,
        ["SELECT\n 1"]
    );
    assert_eq!(f.text(), "SELECT\n 1;\nSELECT 2;");
}

#[test]
fn inline_review_keeps_its_own_gutter_styles() {
    let theme = Theme::ansi();
    let mut f = Fixture::conventional("SELECT 1;");
    let hunks = sqln::editor::review::diff_hunks(
        "SELECT 1;",
        "SELECT 2;",
        0,
        std::time::Duration::from_millis(100),
    );
    f.editor.start_review(sqln::editor::review::Review::diff(
        sqln::editor::review::Source::Ai,
        "AI".into(),
        hunks,
    ));
    let buffer = draw(&mut f, &theme, 80, 24, true);
    assert!((0..24).all(|y| !is_target(&buffer, 1, y, &theme)));
    assert!(f.editor.review().is_some());
    assert_eq!(f.text(), "SELECT 1;");
}

#[test]
fn the_highlight_keeps_execution_marks_and_only_paints_visible_line_numbers() {
    let theme = Theme::ansi();
    let sql = format!("SELECT\n{}1;\nSELECT 2;", "-- note\n".repeat(30));
    let mut f = Fixture::conventional(&sql);
    let plan = f.editor.plan_run(sqln::editor::RunScope::Default).unwrap();
    f.editor.set_marks(&plan);
    f.editor.set_mark_state(0, MarkState::Success);
    let first = draw(&mut f, &theme, 80, 24, true);
    assert_eq!(first[(4, 0)].symbol(), "✓");
    assert_eq!(first[(4, 0)].fg, theme.mark_good.fg.unwrap());
    f.editor.set_cursor(Index2::new(31, 0));
    for (width, height) in [(80, 24), (40, 6), (100, 30)] {
        let buffer = draw(&mut f, &theme, width, height, true);
        let (_, top) = f.editor.state.viewport_offset();
        for y in 0..height {
            let row = top + usize::from(y);
            if row < f.editor.state.lines.len() {
                assert_eq!(is_target(&buffer, 1, y, &theme), row <= 31);
            }
        }
    }
    assert_eq!(f.editor.marks()[0].state, Some(MarkState::Success));
    f.press(ctrl('a'));
    let selected = draw(&mut f, &theme, 80, 24, true);
    assert!((0..24).all(|y| !is_target(&selected, 1, y, &theme)));
}
