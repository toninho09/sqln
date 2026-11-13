use super::*;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Style};

const JSON: &str = r#"{"key":"text","n":-12.5e+2,"yes":true,"no":false,"nil":null}"#;

fn themed(name: &str, mode: KeyboardMode, theme: Theme, setup: &str) -> Fixture {
    let mut f = Fixture::with(name, mode, Settings::default(), setup);
    f.app = App::new(
        Options {
            database: Some(f.path.clone()),
            keyboard: mode,
            ..Options::default()
        },
        f.clipboard.clone(),
        theme,
    );
    f
}

fn find(buffer: &Buffer, area: Rect, needle: &str) -> (u16, u16) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let text: String = (x..area.right()).map(|x| buffer[(x, y)].symbol()).collect();
            if text.starts_with(needle) {
                return (x, y);
            }
        }
    }
    panic!("{needle:?} is not in {area:?}");
}

fn assert_color(buffer: &Buffer, area: Rect, text: &str, style: Style) {
    let (x, y) = find(buffer, area, text);
    assert_eq!(
        buffer[(x, y)].fg,
        style.fg.unwrap_or(Color::Reset),
        "{text:?}"
    );
    assert!(
        buffer[(x, y)].modifier.contains(style.add_modifier),
        "{text:?}"
    );
}

fn assert_json_colors(buffer: &Buffer, area: Rect, theme: &Theme) {
    for (token, style) in [
        ("\"key\"", theme.identifier),
        ("\"text\"", theme.string),
        ("-12.5e+2", theme.number),
        ("true", theme.keyword),
        ("false", theme.keyword),
        ("null", theme.keyword),
    ] {
        assert_color(buffer, area, token, style);
    }
}

fn buffer_at(f: &mut Fixture, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| f.app.draw(frame)).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn json_colors_follow_the_theme_in_both_view_shapes_and_keyboard_modes() {
    for theme in [Theme::astrodark(), Theme::ansi(), Theme::plain()] {
        for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
            let mut f = themed("json-view-colors", mode, theme, "");
            f.run_script(&format!("SELECT '{JSON}' AS doc;"));
            f.prefix('j');
            f.press(key(KeyCode::Enter));
            for formatted in [true, false] {
                for (width, height) in [(100, 30), (80, 24)] {
                    let buffer = buffer_at(&mut f, width, height);
                    let area =
                        sqln::ui::console::overlay_area(buffer.area).inner(Margin::new(1, 1));
                    assert_json_colors(&buffer, area, &theme);
                }
                f.press(ctrl('c'));
                let expected = if formatted {
                    sqln::grid::json::pretty(JSON).unwrap()
                } else {
                    JSON.into()
                };
                assert_eq!(f.clipboard.borrow().text, expected);
                f.press(key(KeyCode::Char('f')));
            }
        }
    }
}

#[test]
fn mouse_selection_stays_above_json_colors_and_copies_only_selected_text() {
    let theme = Theme::astrodark();
    let mut f = themed(
        "json-color-selection",
        KeyboardMode::Conventional,
        theme,
        "",
    );
    f.run_script(r#"SELECT '{"key":"text"}' AS doc;"#);
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    f.press(key(KeyCode::Char('f')));
    let buffer = f.buffer();
    let area = sqln::ui::console::overlay_area(buffer.area).inner(Margin::new(1, 1));
    let (x, y) = find(&buffer, area, "text");
    f.wheel(MouseEventKind::Down(MouseButton::Left), x, y);
    f.wheel(MouseEventKind::Drag(MouseButton::Left), x + 4, y);
    f.wheel(MouseEventKind::Up(MouseButton::Left), x + 4, y);
    let buffer = f.buffer();
    assert_eq!(buffer[(x, y)].bg, theme.selection.bg.unwrap());
    assert_eq!(buffer[(x, y)].fg, theme.string.fg.unwrap());
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, "text");
}

fn open_editor(f: &mut Fixture) {
    common::open_table(&mut f.app, 0, "items");
    f.pump_until(|f| {
        f.app
            .table()
            .is_some_and(|tab| !tab.is_busy() && !tab.pending)
    });
    f.press(key(KeyCode::Right));
    f.press(key(KeyCode::F(2)));
    f.pump_until(|f| f.sees(sqln::ui::text::EDIT_CELL_HINT));
}

#[test]
fn f2_cell_editor_colors_json_without_changing_the_value_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let theme = Theme::astrodark();
        let mut f = themed(
            "json-cell-editor",
            mode,
            theme,
            &format!(
                "CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{JSON}');"
            ),
        );
        open_editor(&mut f);
        for (width, height) in [(100, 30), (80, 24)] {
            let buffer = buffer_at(&mut f, width, height);
            assert_json_colors(&buffer, Rect::new(3, 3, width - 6, height - 8), &theme);
        }
        f.press(key(KeyCode::F(2)));
        assert_eq!(
            f.app.table().unwrap().result.table.rows[0]
                .text(1)
                .as_deref(),
            Some(JSON)
        );
        assert!(f.app.table().unwrap().result.edits.is_empty());
    }
}

#[test]
fn editor_colors_update_after_edits_and_keep_selection_and_undo() {
    let theme = Theme::astrodark();
    let mut f = themed(
        "json-edit-colors",
        KeyboardMode::Conventional,
        theme,
        r#"CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{"key":"text"}');"#,
    );
    open_editor(&mut f);
    for value in [r#"{"key":"new",}"#, r#"{"key":"new"}"#] {
        f.press(ctrl('a'));
        f.write(value);
        let buffer = f.buffer();
        let expected = if value.contains(",}") {
            theme.identifier
        } else {
            theme.string
        };
        assert_color(&buffer, Rect::new(3, 3, 94, 22), "\"new\"", expected);
    }
    f.press(ctrl('a'));
    let buffer = f.buffer();
    let (x, y) = find(&buffer, Rect::new(3, 3, 94, 22), "\"new\"");
    assert_eq!(buffer[(x, y)].bg, theme.selection.bg.unwrap());
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, r#"{"key":"new"}"#);
    f.press(ctrl('z'));
    assert_color(
        &f.buffer(),
        Rect::new(3, 3, 94, 22),
        "\"new\"",
        theme.identifier,
    );
    f.press(key(KeyCode::Esc));
    assert!(f.app.table().unwrap().result.edits.is_empty());
}

#[test]
fn cell_editor_keeps_json_colors_after_horizontal_scroll() {
    let theme = Theme::ansi();
    let mut f = themed(
        "json-horizontal-colors",
        KeyboardMode::Conventional,
        theme,
        "CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{}');",
    );
    open_editor(&mut f);
    let value = format!(r#"{{"key":"{}雪 tail"}}"#, "x".repeat(300));
    f.press(ctrl('a'));
    f.write(&value);
    f.press(key(KeyCode::End));
    let buffer = buffer_at(&mut f, 80, 24);
    assert_color(&buffer, Rect::new(3, 3, 74, 16), "tail", theme.string);
    f.press(ctrl('a'));
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, value);
}

#[test]
fn sql_consoles_keep_sql_colors_for_json_inside_a_string() {
    let theme = Theme::astrodark();
    let mut editor = common::Fixture::conventional(r#"SELECT '{"key":12}';"#);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            sqln::ui::draw_editor(frame, frame.area(), &mut editor.editor, true, &theme);
        })
        .unwrap();
    assert_color(
        terminal.backend().buffer(),
        Rect::new(0, 0, 80, 24),
        "key",
        theme.string,
    );
}

#[test]
fn json_format_is_explicit_local_and_one_undo_step_in_both_modes() {
    let value = r#"{"z":1,"a":12345678901234567890123.50}"#;
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = themed(
            "json-format-local",
            mode,
            Theme::ansi(),
            &format!(
                "CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{value}');"
            ),
        );
        open_editor(&mut f);
        let buffer = buffer_at(&mut f, 80, 24);
        let (x, y) = find(&buffer, buffer.area, "Format JSON");
        f.wheel(MouseEventKind::Down(MouseButton::Left), x, y);
        f.assert_sees("\"z\": 1,");
        f.assert_sees("12345678901234567890123.50");
        assert!(f.app.table().unwrap().result.edits.is_empty());
        if mode == KeyboardMode::Conventional {
            f.press(ctrl('z'));
        } else {
            f.press(key(KeyCode::Char('u')));
        }
        f.press(key(KeyCode::F(2)));
        assert_eq!(
            f.app.table().unwrap().result.table.rows[0]
                .text(1)
                .as_deref(),
            Some(value)
        );
        assert!(f.app.table().unwrap().result.edits.is_empty());
    }
}

#[test]
fn json_validation_shows_the_error_and_does_not_add_a_save_restriction() {
    let mut f = themed(
        "json-error-save",
        KeyboardMode::Conventional,
        Theme::ansi(),
        r#"CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{}');"#,
    );
    open_editor(&mut f);
    f.press(ctrl('a'));
    f.write("{\n  \"é\": }\n");
    f.assert_sees("JSON error");
    f.assert_sees("line 2, column 8");
    f.prefix('f');
    f.assert_sees("Cannot format JSON");
    assert!(f.app.table().unwrap().result.edits.is_empty());
    f.press(key(KeyCode::F(2)));
    assert_eq!(
        f.app.table().unwrap().result.table.rows[0]
            .text(1)
            .as_deref(),
        Some("{\n  \"é\": }\n")
    );
    assert_eq!(f.app.table().unwrap().result.edits.cells(), 1);
    let original: String = rusqlite::Connection::open(&f.path)
        .unwrap()
        .query_row("SELECT value FROM items WHERE id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(original, "{}");
}

#[test]
fn json_format_uses_the_custom_format_shortcut_and_keeps_selection_count() {
    let mut f = themed(
        "json-custom-format",
        KeyboardMode::Conventional,
        Theme::ansi(),
        r#"CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{"z":1,"a":2}');"#,
    );
    let keys = sqln::input::Keymap::with_overrides(
        KeyboardMode::Conventional,
        &std::collections::HashMap::from([("formatSql".into(), vec!["f6".into()])]),
    )
    .unwrap();
    f.app = App::new(
        Options {
            database: Some(f.path.clone()),
            keymap: Some(keys),
            ..Options::default()
        },
        f.clipboard.clone(),
        Theme::ansi(),
    );
    open_editor(&mut f);
    f.assert_sees("Format JSON (F6)");
    f.press(key(KeyCode::F(6)));
    f.press(ctrl('a'));
    f.press(ctrl('c'));
    let expected = sqln::grid::json::pretty(r#"{"z":1,"a":2}"#).unwrap();
    assert_eq!(f.clipboard.borrow().text, expected);
    f.assert_sees(&format!("{} characters selected", expected.chars().count()));
    assert!(f.app.table().unwrap().result.edits.is_empty());
}

#[test]
fn escape_closes_cell_editor_search_before_the_editor() {
    let mut f = themed(
        "json-editor-search-escape",
        KeyboardMode::Conventional,
        Theme::ansi(),
        r#"CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO items VALUES(1, '{"key":"value"}');"#,
    );
    open_editor(&mut f);
    f.press(ctrl('f'));
    f.write("value");
    f.press(key(KeyCode::Esc));
    f.assert_sees(sqln::ui::text::EDIT_CELL_HINT);
    f.press(key(KeyCode::Esc));
    assert!(f.app.table().unwrap().result.edits.is_empty());
}

#[test]
fn long_json_strings_keep_their_color_across_wraps_and_scroll() {
    let theme = Theme::ansi();
    let mut f = themed("json-wrap-colors", KeyboardMode::Conventional, theme, "");
    let value = format!(r#"{{"key":"{}\\\"日本"}}"#, "abc".repeat(200));
    f.run_script(&format!("SELECT '{value}' AS doc;"));
    f.prefix('j');
    f.press(key(KeyCode::Enter));
    f.press(key(KeyCode::Char('f')));
    for _ in 0..3 {
        let buffer = buffer_at(&mut f, 80, 24);
        let area = sqln::ui::console::overlay_area(buffer.area).inner(Margin::new(1, 1));
        assert_color(&buffer, area, "abcabc", theme.string);
        f.press(key(KeyCode::Down));
    }
    f.press(ctrl('c'));
    assert_eq!(f.clipboard.borrow().text, value);
}
