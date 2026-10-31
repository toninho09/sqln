mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::sample_catalog;
use common::{Fixture, ctrl, key};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use edtui::Index2;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use sqln::app::{App, Options};
use sqln::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use sqln::clipboard::MemoryClipboard;
use sqln::editor::{KeyboardMode, SqlEditor};
use sqln::input::Keymap;
use sqln::ui::theme::Theme;
use sqln::ui::{draw_editor, list_area};

fn draw(editor: &mut SqlEditor, theme: &Theme, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_editor(frame, frame.area(), editor, true, theme))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn line(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn screen(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| line(buffer, y) + "\n")
        .collect()
}

fn find(buffer: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        let row = line(buffer, y);
        if let Some(byte) = row.find(text) {
            return (row[..byte].chars().count() as u16, y);
        }
    }
    panic!("{text:?} is not on the screen:\n{}", screen(buffer));
}

fn is_ansi(color: Color) -> bool {
    !matches!(color, Color::Rgb(..) | Color::Indexed(..))
}

#[test]
fn the_list_opens_below_the_cursor_at_the_start_of_the_word() {
    let mut f = Fixture::conventional("SELECT * FROM ");
    f.editor.set_cursor(Index2::new(0, 14));
    f.write("us");
    let buffer = draw(&mut f.editor, &Theme::ansi(), 60, 12);

    let (word_x, word_y) = find(&buffer, "us");
    let (label_x, label_y) = find(&buffer, "users");
    assert_eq!(label_y, word_y + 1, "{}", screen(&buffer));
    assert_eq!(
        label_x,
        word_x + 5,
        "the kind tag fills five columns\n{}",
        screen(&buffer)
    );
    assert!(line(&buffer, label_y).contains("tbl"));
    assert!(line(&buffer, label_y).contains("public"));
}

#[test]
fn the_list_opens_above_the_cursor_on_the_last_rows() {
    let text = "\n".repeat(11) + "SELECT * FROM ";
    let mut f = Fixture::conventional(&text);
    f.editor.set_cursor(Index2::new(11, 14));
    f.write("us");
    let buffer = draw(&mut f.editor, &Theme::ansi(), 60, 12);
    let (_, label_y) = find(&buffer, "users");
    assert!(label_y < 11, "{}", screen(&buffer));
}

#[test]
fn list_area_stays_in_the_bounds() {
    let bounds = Rect::new(10, 5, 40, 10);
    assert_eq!(
        list_area(Position::new(12, 7), bounds, 20, 4),
        Rect::new(12, 8, 20, 4)
    );
    assert_eq!(
        list_area(Position::new(45, 7), bounds, 20, 4),
        Rect::new(30, 8, 20, 4)
    );
    assert_eq!(
        list_area(Position::new(12, 13), bounds, 20, 4),
        Rect::new(12, 9, 20, 4)
    );
    let wide = list_area(Position::new(12, 7), bounds, 100, 4);
    assert_eq!((wide.x, wide.width), (10, 40));
    let tall = list_area(Position::new(12, 9), bounds, 20, 20);
    assert!(tall.y >= bounds.y && tall.bottom() <= bounds.bottom());
}

#[test]
fn the_selected_row_stays_visible_in_a_long_list() {
    let columns: Vec<String> = (0..30).map(|index| format!("column_{index:02}")).collect();
    let mut f = Fixture::conventional("SELECT  FROM wide");
    f.catalog = Catalog {
        schemas: vec![Schema {
            name: "public".to_owned(),
            relations: vec![Relation {
                name: "wide".to_owned(),
                kind: RelationKind::Table,
                columns: Columns::Loaded(columns),
            }],
        }],
        current_schema: None,
    };
    f.editor.set_cursor(Index2::new(0, 7));
    f.write("column");
    for _ in 0..20 {
        f.press(key(KeyCode::Down));
    }
    let buffer = draw(&mut f.editor, &Theme::ansi(), 60, 14);
    find(&buffer, "column_20");
}

#[test]
fn syntax_highlight_uses_ansi_colors_only() {
    let text = "SELECT 'text', 12, \"name\" FROM t -- note";
    let mut f = Fixture::conventional(text);
    let theme = Theme::ansi();
    let buffer = draw(&mut f.editor, &theme, 60, 4);

    let color_of = |word: &str| buffer[find(&buffer, word)].fg;
    assert_eq!(color_of("ELECT"), Color::Blue);
    assert_eq!(color_of("'text'"), Color::Green);
    assert_eq!(color_of("12"), Color::Magenta);
    assert_eq!(color_of("\"name\""), Color::Cyan);
    assert_eq!(color_of("-- note"), Color::DarkGray);
    for cell in buffer.content() {
        assert!(is_ansi(cell.fg) && is_ansi(cell.bg), "{cell:?}");
    }
}

#[test]
fn highlight_spans_several_rows_and_ends_on_multibyte_text() {
    let mut f = Fixture::conventional("/* um\ndois */ SELECT\n'ação' açúcar");
    let buffer = draw(&mut f.editor, &Theme::ansi(), 40, 5);
    assert_eq!(buffer[find(&buffer, "dois")].fg, Color::DarkGray);
    assert_eq!(buffer[find(&buffer, "SELECT")].fg, Color::Blue);
    assert_eq!(buffer[find(&buffer, "ação")].fg, Color::Green);
    assert_eq!(buffer[find(&buffer, "açúcar")].fg, Color::Reset);
}

#[test]
fn every_theme_has_no_rgb_color_and_the_plain_theme_has_no_color() {
    let mut f = Fixture::conventional("SELECT 'a'");
    f.editor.set_cursor(Index2::new(0, 10));
    f.write(" fr");
    let buffer = draw(&mut f.editor, &Theme::plain(), 40, 6);
    for cell in buffer.content() {
        assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{cell:?}");
    }
    let keyword = &buffer[find(&buffer, "SELECT")];
    assert!(keyword.modifier.contains(Modifier::BOLD));
}

#[test]
fn selection_style_covers_only_the_selected_characters() {
    let mut f = Fixture::conventional("abcdef");
    f.press_all(&[common::shift(KeyCode::Right), common::shift(KeyCode::Right)]);
    let theme = Theme::ansi();
    let buffer = draw(&mut f.editor, &theme, 20, 3);
    let (x, y) = find(&buffer, "abcdef");
    assert_eq!(buffer[(x, y)].bg, Color::Blue);
    assert_eq!(buffer[(x + 1, y)].bg, Color::Blue);
    assert_ne!(buffer[(x + 3, y)].bg, Color::Blue);
}

#[test]
fn the_current_match_has_its_own_style() {
    let mut f = Fixture::conventional("users users");
    f.press(ctrl('f'));
    f.write("users");
    let buffer = draw(&mut f.editor, &Theme::ansi(), 40, 6);
    let (x, y) = find(&buffer, "users users");
    assert_eq!(buffer[(x + 1, y)].bg, Color::Green);
    assert_eq!(buffer[(x + 6, y)].bg, Color::Yellow);
    assert!(screen(&buffer).contains("1 of 2"));
}

#[test]
fn external_names_cannot_send_terminal_sequences() {
    let mut f = Fixture::conventional("SELECT * FROM ");
    f.catalog = Catalog {
        schemas: vec![Schema {
            name: "public".to_owned(),
            relations: vec![Relation {
                name: "bad\x1b[31mname\x07".to_owned(),
                kind: RelationKind::Table,
                columns: Columns::NotLoaded,
            }],
        }],
        current_schema: None,
    };
    f.editor.set_cursor(Index2::new(0, 14));
    f.write("bad");
    assert_eq!(f.labels().len(), 1);
    let buffer = draw(&mut f.editor, &Theme::ansi(), 60, 6);
    for cell in buffer.content() {
        assert!(!cell.symbol().chars().any(char::is_control), "{cell:?}");
    }
}

#[test]
fn horizontal_scroll_keeps_the_cursor_visible() {
    let text = "x".repeat(200);
    let mut f = Fixture::conventional(&text);
    f.editor.set_cursor(Index2::new(0, 200));
    draw(&mut f.editor, &Theme::ansi(), 40, 4);
    let cursor = f.editor.state.cursor_screen_position().unwrap();
    assert!(cursor.x < 40);
}

#[test]
fn a_jump_to_a_far_row_highlights_on_the_first_frame() {
    let text = "SELECT 1;\n".repeat(500);
    let mut f = Fixture::conventional(&text);
    draw(&mut f.editor, &Theme::ansi(), 40, 10);
    f.press(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    let buffer = draw(&mut f.editor, &Theme::ansi(), 40, 10);
    assert_eq!(
        buffer[find(&buffer, "SELECT")].fg,
        Color::Blue,
        "{}",
        screen(&buffer)
    );
}

/// The first editor row of the application screen.
const TOP: u16 = 2;
/// The first text column of the application screen, with one digit in the line numbers.
const LEFT: u16 = 5;

fn app(keyboard: KeyboardMode) -> App {
    let options = Options {
        keyboard,
        catalog: Some(sample_catalog()),
        ..Options::default()
    };
    let mut app = App::new(
        options,
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::ansi(),
    );
    // These tests count screen columns from the first column of the editor.
    send(&mut app, ctrl('w'));
    send(&mut app, key(KeyCode::Char('b')));
    app
}

fn draw_app(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    terminal.backend().buffer().clone()
}

fn send(app: &mut App, event: KeyEvent) {
    app.on_event(Event::Key(event));
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn each_hint_fits_the_minimum_terminal_width() {
    use sqln::input::SuggestionKeys;
    use sqln::ui::statusline::{HintItem, hint_text};
    use sqln::ui::text;
    for parts in [
        text::HINT_FILTER,
        text::HINT_GRID_SEARCH,
        text::HINT_SEARCH,
        text::HINT_PICKER,
        text::HINT_CHOOSE,
        text::HINT_NAME,
        text::HINT_SETTINGS,
        text::HINT_CONFLICTS,
        text::HINT_CONFIG_ERROR,
        text::HINT_FORM,
        text::HINT_DIALOG,
        text::HINT_OVERLAY,
        text::HINT_PREFIX,
        text::HINT_PALETTE,
        text::AUTH_PASSWORD_HINT,
        text::AUTH_REFUSED_HINT,
    ] {
        let hint = hint_text(&HintItem::of(parts));
        assert!(hint.chars().count() < 80, "{hint}");
    }
    for keys in [
        SuggestionKeys::Tab,
        SuggestionKeys::Enter,
        SuggestionKeys::Both,
    ] {
        for others in [text::LIST_EDITOR_KEYS, text::LIST_FILTER_KEYS] {
            let hint = hint_text(&HintItem::of(&text::hint_list(keys, others)));
            assert!(hint.chars().count() < 80, "{hint}");
        }
    }
    for message in [
        text::RESULT_READING,
        text::CLIPBOARD_UNAVAILABLE,
        text::COPY_SENT_TO_TERMINAL,
    ] {
        assert!(message.chars().count() < 80, "{message}");
    }
}

#[test]
fn the_prefix_closes_the_tab_and_the_application() {
    let mut app = app(KeyboardMode::Conventional);
    send(&mut app, ctrl('w'));
    assert!(screen(&draw_app(&mut app, 80, 24)).contains("Close the tab"));
    send(&mut app, key(KeyCode::Char('c')));
    assert!(app.should_quit());
}

#[test]
fn escape_cancels_the_prefix_and_the_next_letter_is_text() {
    let mut app = app(KeyboardMode::Conventional);
    send(&mut app, ctrl('w'));
    send(&mut app, key(KeyCode::Esc));
    send(&mut app, key(KeyCode::Char('c')));
    assert!(!app.should_quit());
    assert_eq!(app.console_mut().editor.text(), "c");
}

#[test]
fn the_prefix_works_in_vim_normal_mode() {
    let mut app = app(KeyboardMode::Vim);
    send(&mut app, ctrl('w'));
    send(&mut app, key(KeyCode::Char('c')));
    assert!(app.should_quit());
}

#[test]
fn the_list_has_priority_over_the_prefix_keys() {
    let mut app = app(KeyboardMode::Conventional);
    for c in "sel".chars() {
        send(&mut app, key(KeyCode::Char(c)));
    }
    assert!(screen(&draw_app(&mut app, 80, 24)).contains("Tab accept"));
    send(&mut app, key(KeyCode::Esc));
    assert!(!screen(&draw_app(&mut app, 80, 24)).contains("Tab accept"));
}

#[test]
fn a_small_terminal_shows_the_size_and_keeps_the_text() {
    let mut app = app(KeyboardMode::Conventional);
    send(&mut app, key(KeyCode::Char('x')));
    let small = screen(&draw_app(&mut app, 52, 14));
    assert!(
        small.contains("52 x 14") && small.contains("80 x 24"),
        "{small}"
    );
    assert!(screen(&draw_app(&mut app, 80, 24)).contains('x'));
    assert_eq!(app.console_mut().editor.text(), "x");
}

#[test]
fn a_key_release_has_no_effect() {
    let mut app = app(KeyboardMode::Conventional);
    let mut event = key(KeyCode::Char('x'));
    event.kind = crossterm::event::KeyEventKind::Release;
    send(&mut app, event);
    assert_eq!(app.console_mut().editor.text(), "");
}

#[test]
fn a_click_moves_the_cursor_and_past_the_end_uses_the_line_end() {
    let mut app = app(KeyboardMode::Conventional);
    app.on_event(Event::Paste("abc\nde".to_owned()));
    draw_app(&mut app, 80, 24);
    let gutter = LEFT;

    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        gutter + 1,
        TOP,
    ));
    assert_eq!(app.console_mut().editor.cursor(), Index2::new(0, 1));
    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        gutter + 30,
        TOP + 1,
    ));
    assert_eq!(app.console_mut().editor.cursor(), Index2::new(1, 2));
    app.on_event(mouse(MouseEventKind::Down(MouseButton::Left), gutter, 10));
    assert_eq!(
        app.console_mut().editor.cursor(),
        Index2::new(1, 2),
        "a click below the text uses its end"
    );
}

#[test]
fn a_click_on_wide_characters_keeps_the_column() {
    let mut app = app(KeyboardMode::Conventional);
    app.on_event(Event::Paste("日本語".to_owned()));
    draw_app(&mut app, 80, 24);
    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        LEFT + 2,
        TOP,
    ));
    assert_eq!(app.console_mut().editor.cursor(), Index2::new(0, 1));
}

#[test]
fn a_tab_character_counts_as_its_display_width() {
    let mut app = app(KeyboardMode::Conventional);
    app.on_event(Event::Paste("\tab".to_owned()));
    draw_app(&mut app, 80, 24);
    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        LEFT + 5,
        TOP,
    ));
    assert_eq!(app.console_mut().editor.cursor(), Index2::new(0, 2));
}

#[test]
fn a_click_on_the_hint_row_leaves_the_cursor_in_place() {
    let mut app = app(KeyboardMode::Conventional);
    app.on_event(Event::Paste("ab\ncd".to_owned()));
    draw_app(&mut app, 80, 24);
    app.on_event(mouse(MouseEventKind::Down(MouseButton::Left), 2, 23));
    assert_eq!(app.console_mut().editor.cursor(), Index2::new(1, 2));
}

#[test]
fn vim_click_stops_on_the_last_character_and_a_drag_selects() {
    let mut app = app(KeyboardMode::Vim);
    app.on_event(Event::Paste("abcdef".to_owned()));
    draw_app(&mut app, 80, 24);
    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        LEFT + 40,
        TOP,
    ));
    assert_eq!(app.console_mut().editor.cursor(), Index2::new(0, 5));
    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        LEFT + 1,
        TOP,
    ));
    app.on_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        LEFT + 3,
        TOP,
    ));
    assert_eq!(
        app.console_mut().editor.selected_text().as_deref(),
        Some("bcd")
    );
}

#[test]
fn a_drag_selects_with_the_exclusive_end() {
    let mut app = app(KeyboardMode::Conventional);
    app.on_event(Event::Paste("abcdef".to_owned()));
    draw_app(&mut app, 80, 24);
    app.on_event(mouse(
        MouseEventKind::Down(MouseButton::Left),
        LEFT + 1,
        TOP,
    ));
    app.on_event(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        LEFT + 4,
        TOP,
    ));
    assert_eq!(
        app.console_mut().editor.selected_text().as_deref(),
        Some("bcd")
    );
    send(&mut app, key(KeyCode::Char('x')));
    assert_eq!(app.console_mut().editor.text(), "axef");
}

#[test]
fn the_wheel_scrolls_the_text() {
    let mut app = app(KeyboardMode::Conventional);
    app.on_event(Event::Paste("row\n".repeat(200)));
    draw_app(&mut app, 80, 24);
    let (_, before) = app.console_mut().editor.state.viewport_offset();
    app.on_event(mouse(MouseEventKind::ScrollUp, 10, 5));
    let (_, after) = app.console_mut().editor.state.viewport_offset();
    assert_eq!(after, before - 1);
}

fn hint_app(keyboard: KeyboardMode, keymap: Option<Keymap>) -> App {
    let options = Options {
        keyboard,
        keymap,
        ..Options::default()
    };
    App::new(
        options,
        Rc::new(RefCell::new(MemoryClipboard::default())),
        Theme::plain(),
    )
}

fn press_prefix(app: &mut App, c: char) {
    app.on_event(Event::Key(ctrl('w')));
    app.on_event(Event::Key(key(KeyCode::Char(c))));
}

#[test]
fn the_keys_of_the_hint_bar_fit_the_minimum_width() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut app = hint_app(keyboard, None);
        let mut hints = vec![app.hint()];
        app.on_event(Event::Key(ctrl('w')));
        hints.push(app.hint());
        app.on_event(Event::Key(key(KeyCode::Esc)));
        press_prefix(&mut app, 'j');
        hints.push(app.hint());
        press_prefix(&mut app, 'h');
        hints.push(app.hint());
        for hint in hints {
            assert!(!hint.is_empty());
            assert!(hint.chars().count() <= 80, "{hint}");
        }
    }
}

#[test]
fn the_hint_bar_shows_the_keys_of_the_key_map() {
    let overrides = HashMap::from([
        ("run".to_owned(), vec!["f6".to_owned()]),
        ("help".to_owned(), Vec::new()),
        ("nextTab".to_owned(), vec!["ctrl+w u".to_owned()]),
    ]);
    let keymap = Keymap::with_overrides(KeyboardMode::Conventional, &overrides).unwrap();
    let app = hint_app(KeyboardMode::Conventional, Some(keymap));
    let hint = app.hint();
    assert!(hint.contains("F6 run"), "{hint}");
    assert!(!hint.starts_with("F5"), "{hint}");
    assert!(!hint.contains("keys"), "{hint}");
    let keys: Vec<String> = app
        .prefix_menu()
        .into_iter()
        .flat_map(|group| group.keys)
        .map(|(key, _)| key)
        .collect();
    assert!(keys.contains(&"u".to_owned()), "{keys:?}");
    assert!(keys.contains(&"N".to_owned()), "{keys:?}");
    assert!(!keys.contains(&"?".to_owned()), "{keys:?}");
}
