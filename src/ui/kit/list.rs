//! A window with a filter and a list, and the viewport of any list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::super::theme::Theme;
use super::geometry::{list_rows, short_modal_width};
use super::input::{self, FieldMode, FieldText};
use super::modal::open_modal;
use crate::filter_list::FilterList;
use crate::grid::printable;
use crate::line_input::LineInput;

const ROWS: u16 = 12;
/// The rows of the window without the list: the borders and the filter.
const CHROME_ROWS: u16 = 3;
const TOP: u16 = 2;
const PROMPT: &str = " > ";

/// The first visible row, the visible count, and the selected row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub first: usize,
    pub rows: usize,
    pub cursor: usize,
}

/// Keeps the selected row on screen.
pub fn viewport(cursor: usize, count: usize, rows: u16) -> Viewport {
    let rows = usize::from(rows).max(1).min(count);
    let first = if rows == 0 {
        0
    } else {
        (cursor + 1).saturating_sub(rows)
    };
    Viewport {
        first,
        rows,
        cursor,
    }
}

/// Keeps the selected row in the middle of the window when it can.
pub fn viewport_centered(cursor: usize, count: usize, rows: u16) -> Viewport {
    let rows = usize::from(rows).min(count);
    Viewport {
        first: cursor
            .saturating_sub(rows / 2)
            .min(count.saturating_sub(rows)),
        rows,
        cursor,
    }
}

/// Draws the visible rows. `row` receives the area, the item, and the selection.
pub fn draw_rows<'a, T: 'a>(
    frame: &mut Frame,
    area: Rect,
    items: impl IntoIterator<Item = &'a T>,
    view: Viewport,
    mut row: impl FnMut(&mut Frame, Rect, &T, bool),
) {
    for (offset, item) in items
        .into_iter()
        .enumerate()
        .skip(view.first)
        .take(view.rows)
    {
        let place = Rect {
            y: area.y + (offset - view.first) as u16,
            height: 1,
            ..area
        };
        row(frame, place, item, offset == view.cursor);
    }
}

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
    let field = usize::from(area.width).saturating_sub(PROMPT.len()).max(1);
    let text = printable(input.text());
    let spans = input::view(
        FieldText {
            text: &text,
            cursor: input.cursor(),
            width: field,
            focused,
            mode: FieldMode::Text,
        },
        theme,
    )
    .spans;
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
    let view = viewport(
        list.cursor(),
        count,
        u16::try_from(usize::from(rows_area.height).max(1)).unwrap_or(u16::MAX),
    );
    draw_rows(frame, rows_area, list.shown(), view, draw_row);
    (rows_area, view.first)
}
