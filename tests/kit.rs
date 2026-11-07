// The shared pieces of the interface: their sizes, their placement, and their styles.

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use sqln::ui::kit::buttons::{self, ButtonState, RowOptions};
use sqln::ui::kit::geometry::{centered, fitted, list_rows, modal_width, short_modal_width};
use sqln::ui::theme::Theme;

#[test]
fn a_modal_grows_to_its_limit_and_keeps_the_free_columns() {
    assert_eq!(modal_width(80, 72), 72);
    assert_eq!(modal_width(100, 72), 92);
    assert_eq!(modal_width(300, 72), 140);
    assert_eq!(short_modal_width(60, 48), 52);
    assert_eq!(short_modal_width(50, 48), 48);
}

#[test]
fn a_list_keeps_the_rows_free_above_and_below_its_window() {
    assert_eq!(list_rows(24, 3, 12), 17);
    assert_eq!(list_rows(60, 3, 12), 30);
    assert_eq!(list_rows(120, 3, 12), 30);
}

#[test]
fn a_fitted_size_stays_between_the_base_and_the_limit() {
    assert_eq!(fitted(50, 8, 20, 30), 30);
    assert_eq!(fitted(25, 8, 20, 30), 20);
    assert_eq!(fitted(300, 8, 20, 30), 30);
    assert_eq!(fitted(10, 8, 20, 100), 20);
    assert_eq!(fitted(5, 8, 20, 100), 20);
}

#[test]
fn a_centered_area_keeps_the_odd_column_on_the_left() {
    let screen = Rect::new(0, 0, 80, 24);
    assert_eq!(centered(screen, 40, 10), Rect::new(20, 7, 40, 10));
    let odd = Rect::new(1, 2, 81, 25);
    assert_eq!(centered(odd, 40, 10), Rect::new(21, 9, 40, 10));
}

#[test]
fn a_centered_area_never_leaves_the_screen() {
    let screen = Rect::new(5, 5, 10, 4);
    assert_eq!(centered(screen, 20, 10), screen);
}

fn button_row(
    width: u16,
    options: RowOptions,
    items: &[(usize, String, ButtonState)],
) -> (Buffer, Vec<(usize, Rect)>) {
    let theme = Theme::astrodark();
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    let mut rects = Vec::new();
    terminal
        .draw(|frame: &mut Frame| {
            rects = buttons::draw(frame, frame.area(), options, items, &theme);
        })
        .unwrap();
    (terminal.backend().buffer().clone(), rects)
}

#[test]
fn a_left_row_places_each_label_after_the_gap() {
    let items = [
        (0, "aa".to_owned(), ButtonState::Idle),
        (1, "bbb".to_owned(), ButtonState::Idle),
        (2, "c".to_owned(), ButtonState::Idle),
    ];
    let (_, rects) = button_row(20, RowOptions::DIALOG, &items);
    assert_eq!(rects[0].1, Rect::new(0, 0, 2, 1));
    assert_eq!(rects[1].1, Rect::new(4, 0, 3, 1));
    assert_eq!(rects[2].1, Rect::new(9, 0, 1, 1));
}

#[test]
fn a_right_row_keeps_the_last_label_at_the_right_edge() {
    let items = [
        (0, "left".to_owned(), ButtonState::Idle),
        (1, "right".to_owned(), ButtonState::Idle),
    ];
    let (_, rects) = button_row(30, RowOptions::INLINE_RIGHT, &items);
    assert_eq!(rects[0].1.right() + 2, rects[1].1.x);
    assert_eq!(rects[1].1.right(), 30);
}

#[test]
fn a_toolbar_stops_before_a_button_that_does_not_fit() {
    let items = [
        (0, "aaaa".to_owned(), ButtonState::Enabled),
        (1, "bbbb".to_owned(), ButtonState::Enabled),
        (2, "cccc".to_owned(), ButtonState::Enabled),
    ];
    let (_, rects) = button_row(10, RowOptions::TOOLBAR, &items);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].1, Rect::new(1, 0, 4, 1));
}

#[test]
fn each_button_state_has_its_own_style() {
    let theme = Theme::astrodark();
    let items = [
        (0, "a".to_owned(), ButtonState::Focused),
        (1, "b".to_owned(), ButtonState::Idle),
    ];
    let (buffer, _) = button_row(20, RowOptions::DIALOG, &items);
    assert_eq!(buffer[(0, 0)].bg, theme.dialog_focus.bg.unwrap());
    assert_eq!(buffer[(3, 0)].bg, theme.dialog_option.bg.unwrap());

    let items = [
        (0, "a".to_owned(), ButtonState::Enabled),
        (1, "b".to_owned(), ButtonState::Disabled),
    ];
    let (buffer, _) = button_row(20, RowOptions::TOOLBAR, &items);
    assert_eq!(buffer[(1, 0)].fg, theme.control.fg.unwrap());
    assert_eq!(buffer[(4, 0)].fg, theme.control_off.fg.unwrap());
}

#[test]
fn a_dialog_item_wears_the_button_marks_and_the_focus() {
    let theme = Theme::plain();
    let items = buttons::dialog_items([(0, "Save"), (1, "Cancel")], 1, &theme);
    assert_eq!(items[0].1, "[Save]");
    assert_eq!(items[0].2, ButtonState::Idle);
    assert_eq!(items[1].2, ButtonState::Focused);
}

#[test]
fn a_toolbar_item_follows_the_enabled_flag() {
    let items =
        buttons::toolbar_items([(0, "Run".to_owned(), true), (1, "Stop".to_owned(), false)]);
    assert_eq!(items[0].2, ButtonState::Enabled);
    assert_eq!(items[1].2, ButtonState::Disabled);
}

use sqln::ui::kit::input::{self, FieldMode, FieldText};

fn view(text: &str, cursor: usize, width: usize, focused: bool, mode: FieldMode) -> Vec<String> {
    input::view(
        FieldText {
            text,
            cursor,
            width,
            focused,
            mode,
        },
        &Theme::plain(),
    )
    .spans
    .iter()
    .map(|span| span.content.to_string())
    .collect()
}

#[test]
fn a_text_field_keeps_the_cursor_inside_its_width() {
    let spans = view("abcd", 2, 10, true, FieldMode::Text);
    assert_eq!(spans, ["ab", "c", "d"]);
    let spans = view("abcd", 4, 3, true, FieldMode::Text);
    assert_eq!(spans[0], "cd");
    assert_eq!(spans[1], " ");
    assert_eq!(spans[2], "");
}

#[test]
fn a_field_without_focus_shows_no_cursor() {
    let spans = view("abcd", 2, 3, false, FieldMode::Text);
    assert_eq!(spans, ["abc"]);
}

#[test]
fn a_mask_hides_the_text_and_respects_its_limit() {
    let spans = view(
        "secret",
        6,
        10,
        false,
        FieldMode::Masked {
            mask: '•',
            limit: None,
        },
    );
    let shown: String = spans.concat();
    assert_eq!(shown, "••••••");
    let spans = view(
        "secret",
        6,
        10,
        false,
        FieldMode::Masked {
            mask: '*',
            limit: Some(3),
        },
    );
    let shown: String = spans.concat();
    assert_eq!(shown, "***");
}

#[test]
fn a_path_without_focus_shows_its_end() {
    let spans = view("/a/very/long/path/file.db", 0, 8, false, FieldMode::Path);
    assert_eq!(spans.concat(), "/file.db");
    let spans = view("/a/very/long/path/file.db", 0, 8, true, FieldMode::Path);
    assert_eq!(spans.concat(), "/a/very/");
}

#[test]
fn the_used_columns_count_a_wide_character_as_two() {
    let view = input::view(
        FieldText {
            text: "ação",
            cursor: 4,
            width: 10,
            focused: false,
            mode: FieldMode::Text,
        },
        &Theme::plain(),
    );
    assert_eq!(view.used, 4);
}

use sqln::ui::kit::list::{viewport, viewport_centered};

#[test]
fn a_viewport_keeps_the_selected_row_visible() {
    let view = viewport(0, 100, 10);
    assert_eq!((view.first, view.rows, view.cursor), (0, 10, 0));
    let view = viewport(50, 100, 10);
    assert_eq!((view.first, view.rows), (41, 10));
    let view = viewport(99, 100, 10);
    assert_eq!((view.first, view.rows), (90, 10));
}

#[test]
fn a_viewport_holds_a_list_shorter_than_the_window() {
    let view = viewport(2, 3, 10);
    assert_eq!((view.first, view.rows), (0, 3));
    let view = viewport(0, 0, 10);
    assert_eq!((view.first, view.rows), (0, 0));
}

#[test]
fn a_centered_viewport_puts_the_selection_in_the_middle() {
    let view = viewport_centered(50, 100, 10);
    assert_eq!(view.first, 45);
    let view = viewport_centered(0, 100, 10);
    assert_eq!(view.first, 0);
    let view = viewport_centered(99, 100, 10);
    assert_eq!(view.first, 90);
}

use sqln::ui::kit::measure::{fit, last_chars};

#[test]
fn fit_counts_the_columns_of_a_wide_character() {
    assert_eq!(fit("a日本", 5), ("a日本".to_owned(), 5));
    assert_eq!(fit("ação", 4), ("ação".to_owned(), 4));
    assert_eq!(fit("ação", 3), ("açã".to_owned(), 3));
}

#[test]
fn last_chars_keeps_the_end_of_a_long_text() {
    let chars: Vec<char> = "path/file.db".chars().collect();
    assert_eq!(last_chars(&chars, 7), "file.db");
    assert_eq!(last_chars(&chars, 3), ".db");
    assert_eq!(last_chars(&chars, 100), "path/file.db");
}

use sqln::ui::kit::style::{self, ListRow, NoteLevel};

#[test]
fn each_note_level_has_one_semantic_style() {
    let theme = Theme::astrodark();
    assert_eq!(style::note_style(NoteLevel::Info, &theme), theme.hint);
    assert_eq!(style::note_style(NoteLevel::Hint, &theme), theme.hint);
    assert_eq!(
        style::note_style(NoteLevel::Success, &theme),
        theme.mark_good
    );
    assert_eq!(
        style::note_style(NoteLevel::Warning, &theme),
        theme.mark_neutral
    );
    assert_eq!(style::note_style(NoteLevel::Error, &theme), theme.error);
}

#[test]
fn a_list_row_has_one_of_two_styles() {
    let theme = Theme::astrodark();
    assert_eq!(
        style::list_row_style(ListRow::Selected, &theme),
        theme.list_selected
    );
    assert_eq!(style::list_row_style(ListRow::Normal, &theme), theme.list);
}

#[test]
fn a_picker_marks_its_selection_with_the_list_style() {
    let theme = Theme::astrodark();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| {
            sqln::ui::console::draw_picker(frame, "Choose", &["one", "two"], 1, false, &theme);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut place = None;
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(byte) = row.find("two") {
            place = Some((row[..byte].chars().count() as u16, y));
        }
    }
    let (x, y) = place.unwrap();
    assert_eq!(buffer[(x, y)].bg, theme.list_selected.bg.unwrap());
}
