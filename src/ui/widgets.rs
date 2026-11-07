//! The shared pieces of the interface: the chrome of a modal window, the rows of a
//! form, and a window with a filter and a list. One definition keeps the look the
//! same on every screen.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::float_block;
use super::theme::Theme;
use super::{input_spans, text};
use crate::filter_list::FilterList;
use crate::grid::printable;
use crate::line_input::LineInput;

// The width of a modal.

pub(super) const MODAL_FREE_COLUMNS: u16 = 8;
/// The rows that stay free above and below a modal that grows.
pub const MODAL_FREE_ROWS: u16 = 4;
pub(super) const MODAL_WIDTH_LIMIT: u16 = 140;
pub(super) const SHORT_MODAL_WIDTH_LIMIT: u16 = 100;
const LIST_ROWS_LIMIT: u16 = 30;

/// Returns a size that leaves `free` cells of the screen, from `base` to `limit`.
pub fn fitted(screen: u16, free: u16, base: u16, limit: u16) -> u16 {
    screen.saturating_sub(free).clamp(base, limit.max(base))
}

/// Returns the width of a modal. A long path needs the free columns of a wide screen.
pub fn modal_width(screen: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_COLUMNS, base, MODAL_WIDTH_LIMIT)
}

/// Returns the width of a modal with short rows, such as a list of names.
pub fn short_modal_width(screen: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_COLUMNS, base, SHORT_MODAL_WIDTH_LIMIT)
}

/// Returns the largest count of rows of a list. `chrome` is the other rows of its window.
pub fn list_rows(screen: u16, chrome: u16, base: u16) -> u16 {
    fitted(screen, MODAL_FREE_ROWS + chrome, base, LIST_ROWS_LIMIT)
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

// The chrome of a modal window.

/// Opens a floating window: it clears the area, draws the border, and returns the
/// place of the contents.
pub fn open_modal(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Rect {
    frame.render_widget(Clear, area);
    let block = float_block(title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// The columns of the sidebar, without the divider.
pub const SIDEBAR_WIDTH: u16 = 16;
/// The columns before a label: a space, the focus mark, and a space.
pub const LEFT: u16 = 3;
/// The empty columns after a value.
pub const RIGHT: u16 = 2;
/// The rows of the note above the buttons.
pub const NOTE_ROWS: u16 = 2;
/// The rows after the last field: a gap, the note, the buttons, and a gap.
pub const FOOTER_ROWS: u16 = NOTE_ROWS + 3;
/// The rows of the top and bottom borders.
pub const BORDER_ROWS: u16 = 2;

/// The areas of a modal with a sidebar: the contents, the rows of the sidebar, and the
/// fields over them.
pub struct SidebarModal {
    pub inner: Rect,
    pub sidebar: Rect,
    pub pane: Rect,
}

impl SidebarModal {
    /// Opens a floating window with a sidebar on the left and a divider between the
    /// sides.
    pub fn open(frame: &mut Frame, area: Rect, title: &str, theme: &Theme) -> Self {
        let inner = open_modal(frame, area, title, theme);
        let sidebar = Rect {
            y: inner.y + 1,
            width: SIDEBAR_WIDTH,
            height: inner.height.saturating_sub(1),
            ..inner
        };
        draw_divider(frame, area, sidebar.right(), theme);
        let pane = Rect {
            x: sidebar.right() + 1,
            width: inner.width.saturating_sub(SIDEBAR_WIDTH + 1),
            ..inner
        };
        Self {
            inner,
            sidebar,
            pane,
        }
    }
}

// The rows of a form.

/// Returns the columns that the button of a path field takes from its value, with a gap.
pub fn browse_width(theme: &Theme) -> u16 {
    u16::try_from(theme.button(text::BUTTON_BROWSE).width() + 1).unwrap_or(0)
}

/// Draws the button of a path field at the end of its row, and returns its area.
pub fn draw_browse_button(frame: &mut Frame, row: Rect, focused: bool, theme: &Theme) -> Rect {
    let width = browse_width(theme).saturating_sub(1);
    let area = Rect {
        x: row.right().saturating_sub(RIGHT + 1 + width),
        width,
        ..row
    };
    let style = if focused {
        theme.dialog_focus
    } else {
        theme.dialog_option
    };
    let name = Span::styled(theme.button(text::BUTTON_BROWSE), style);
    frame.render_widget(Paragraph::new(name), area);
    area
}

/// Returns the columns of the value of a row.
pub fn value_width(row: Rect, label_width: u16) -> usize {
    usize::from(row.width.saturating_sub(LEFT + label_width + RIGHT + 2))
}

/// Draws one row: the focus mark, the label, and the value.
pub fn draw_row(
    frame: &mut Frame,
    area: Rect,
    (name, label_width): (&str, u16),
    focused: bool,
    value: Line,
    theme: &Theme,
) {
    if focused {
        let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
        frame.render_widget(
            Paragraph::new(mark),
            Rect {
                x: area.x + 1,
                width: 1,
                ..area
            },
        );
    }
    let label_style = if focused {
        theme.panel_title_focused
    } else {
        Style::new()
    };
    let label_area = Rect {
        x: area.x + LEFT,
        width: label_width,
        ..area
    };
    frame.render_widget(Paragraph::new(name).style(label_style), label_area);

    let start = area.x + LEFT + label_width;
    let value_area = Rect {
        x: start,
        width: area.right().saturating_sub(start + RIGHT),
        ..area
    };
    // The background comes first, because the text and the cursor have their own styles.
    frame.buffer_mut().set_style(
        value_area,
        if focused {
            theme.field_focus
        } else {
            theme.field
        },
    );
    let text_area = Rect {
        x: value_area.x + 1,
        width: value_area.width.saturating_sub(2),
        ..value_area
    };
    frame.render_widget(Paragraph::new(value), text_area);
}

/// Draws the rows of a sidebar and returns them. The open row stays marked when the
/// focus goes to the fields, as the row of the explorer does.
pub fn draw_sidebar_rows(
    frame: &mut Frame,
    area: Rect,
    titles: &[&str],
    (open, focused): (usize, bool),
    theme: &Theme,
) -> Vec<Rect> {
    let mut rows = Vec::new();
    for (y, (index, title)) in (area.y..).zip(titles.iter().enumerate()) {
        let row = Rect {
            y,
            height: 1,
            ..area
        };
        rows.push(row);
        if index == open {
            let style = if focused {
                theme.tree_selected
            } else {
                theme.tree_cursor
            };
            frame.buffer_mut().set_style(row, style);
        }
        if index == open && focused {
            let mark = Span::styled(theme.icons.focus_mark, theme.panel_title_focused);
            frame.render_widget(
                Paragraph::new(mark),
                Rect {
                    x: row.x + 1,
                    width: 1,
                    ..row
                },
            );
        }
        frame.render_widget(
            Paragraph::new(*title),
            Rect {
                x: row.x + LEFT,
                width: row.width.saturating_sub(LEFT),
                ..row
            },
        );
    }
    rows
}

/// Draws the line between the sidebar and the fields. The line joins the window border.
pub fn draw_divider(frame: &mut Frame, window: Rect, x: u16, theme: &Theme) {
    for y in window.y..window.bottom() {
        let symbol = if y == window.y {
            "┬"
        } else if y + 1 == window.bottom() {
            "┴"
        } else {
            "│"
        };
        frame
            .buffer_mut()
            .set_string(x, y, symbol, theme.float_border);
    }
}

/// Draws a note that ends on the row above `row`. A long note goes up to the next row.
pub fn draw_note(frame: &mut Frame, content: Rect, row: u16, note: &str, style: Style) {
    let message = printable(note);
    let width = usize::from(content.width).max(1);
    let rows = u16::try_from(message.width().div_ceil(width))
        .unwrap_or(NOTE_ROWS)
        .clamp(1, NOTE_ROWS);
    let place = Rect {
        y: row.saturating_sub(rows),
        height: rows,
        ..content
    };
    let paragraph = Paragraph::new(Line::styled(message, style)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, place);
}

/// Draws a row of buttons and returns their areas.
pub fn draw_buttons<T: Copy + PartialEq>(
    frame: &mut Frame,
    content: Rect,
    row: u16,
    (buttons, focus): (&[(T, &str)], T),
    theme: &Theme,
) -> Vec<(T, Rect)> {
    let mut areas = Vec::new();
    let mut spans = Vec::new();
    let mut x = content.x;
    for (field, label) in buttons {
        let style = if focus == *field {
            theme.dialog_focus
        } else {
            theme.dialog_option
        };
        let name = theme.button(label);
        let width = u16::try_from(name.width()).unwrap_or(0);
        areas.push((*field, Rect::new(x, row, width, 1)));
        spans.push(Span::styled(name, style));
        spans.push(Span::raw("  "));
        x += width + 2;
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            y: row,
            height: 1,
            ..content
        },
    );
    areas
}

// A window with a filter and a list.

/// The rows of the list on a small screen.
const ROWS: u16 = 12;
/// The rows of the window without the list: the borders and the filter.
const CHROME_ROWS: u16 = 3;
const TOP: u16 = 2;
const PROMPT: &str = " > ";

/// Draws a filter on the first row of `area`.
pub fn draw_prompt(frame: &mut Frame, input: &LineInput, area: Rect, theme: &Theme) {
    draw_prompt_field(frame, input, area, true, theme);
}

/// Draws the prompt. Only a field in focus shows its cursor.
pub fn draw_prompt_field(
    frame: &mut Frame,
    input: &LineInput,
    area: Rect,
    focused: bool,
    theme: &Theme,
) {
    let chars: Vec<char> = printable(input.text()).chars().collect();
    let field = usize::from(area.width).saturating_sub(PROMPT.len()).max(1);
    let (spans, _) = input_spans(&chars, input.cursor(), field, focused, theme);
    let mut prompt = vec![Span::styled(PROMPT, theme.key)];
    prompt.extend(spans);
    frame.render_widget(
        Paragraph::new(Line::from(prompt)),
        Rect { height: 1, ..area },
    );
}

/// The fixed parts of a window with a filter and a list.
pub struct ListWindow {
    pub title: &'static str,
    /// The note of a filter that keeps no row.
    pub empty: &'static str,
    pub width: u16,
}

/// Draws a window with a filter and a list. `draw_row` receives the area of a row, its
/// item, and the selection. It returns the area of the list and the index of its first row.
pub fn draw_list_window<T>(
    frame: &mut Frame,
    window: &ListWindow,
    list: &FilterList<T>,
    theme: &Theme,
    draw_row: impl Fn(&mut Frame, Rect, &T, bool),
) -> (Rect, usize) {
    let bounds = frame.area();
    let count = list.shown().len();
    let limit = list_rows(bounds.height, CHROME_ROWS, ROWS);
    let rows = u16::try_from(count).unwrap_or(limit).clamp(1, limit);
    let width = short_modal_width(bounds.width, window.width).min(bounds.width);
    let height = (rows + CHROME_ROWS).min(bounds.height.saturating_sub(TOP));
    let area = Rect::new(
        bounds.x + (bounds.width - width) / 2,
        bounds.y + TOP,
        width,
        height,
    );
    let inner = open_modal(frame, area, window.title, theme);

    draw_prompt(frame, list.input(), inner, theme);

    let rows_area = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    if count == 0 {
        let note = Line::styled(format!(" {}", window.empty), theme.hint);
        frame.render_widget(Paragraph::new(note), rows_area);
        return (rows_area, 0);
    }
    let visible = usize::from(rows_area.height).max(1);
    let first = (list.cursor() + 1).saturating_sub(visible);
    for (offset, item) in list.shown().enumerate().skip(first).take(visible) {
        let row = Rect {
            y: rows_area.y + (offset - first) as u16,
            height: 1,
            ..rows_area
        };
        draw_row(frame, row, item, offset == list.cursor());
    }
    (rows_area, first)
}
