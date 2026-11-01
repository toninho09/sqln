//! The window of the command palette.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::console::{list_rows, short_modal_width};
use super::theme::Theme;
use super::{float_block, input_spans, text};
use crate::grid::printable;
use crate::line_input::LineInput;
use crate::palette::Palette;

const WIDTH: u16 = 64;
/// The rows of the list on a small screen.
const ROWS: u16 = 12;
/// The rows of the window without the list: the borders and the filter.
const CHROME_ROWS: u16 = 3;
const TOP: u16 = 2;
const PROMPT: &str = " > ";

/// Draws a filter on the first row of `area`.
pub(super) fn draw_prompt(frame: &mut Frame, input: &LineInput, area: Rect, theme: &Theme) {
    let chars: Vec<char> = printable(input.text()).chars().collect();
    let field = usize::from(area.width).saturating_sub(PROMPT.len()).max(1);
    let (spans, _) = input_spans(&chars, input.cursor(), field, true, theme);
    let mut prompt = vec![Span::styled(PROMPT, theme.key)];
    prompt.extend(spans);
    frame.render_widget(
        Paragraph::new(Line::from(prompt)),
        Rect { height: 1, ..area },
    );
}

/// Draws the window. It returns the area of the list and the index of its first row.
pub fn draw_palette(frame: &mut Frame, palette: &Palette, theme: &Theme) -> (Rect, usize) {
    let bounds = frame.area();
    let count = palette.shown().len();
    let limit = list_rows(bounds.height, CHROME_ROWS, ROWS);
    let rows = u16::try_from(count).unwrap_or(limit).clamp(1, limit);
    let width = short_modal_width(bounds.width, WIDTH).min(bounds.width);
    let height = (rows + CHROME_ROWS).min(bounds.height.saturating_sub(TOP));
    let area = Rect::new(
        bounds.x + (bounds.width - width) / 2,
        bounds.y + TOP,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = float_block(text::TITLE_PALETTE, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    draw_prompt(frame, palette.input(), inner, theme);

    let list = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    if count == 0 {
        let note = Line::styled(format!(" {}", text::NO_COMMANDS), theme.hint);
        frame.render_widget(Paragraph::new(note), list);
        return (list, 0);
    }
    let visible = usize::from(list.height).max(1);
    let first = (palette.cursor() + 1).saturating_sub(visible);
    for (offset, command) in palette.shown().enumerate().skip(first).take(visible) {
        let row = Rect {
            y: list.y + (offset - first) as u16,
            height: 1,
            ..list
        };
        let selected = offset == palette.cursor();
        let style = if selected {
            theme.list_selected
        } else {
            theme.dialog
        };
        frame.buffer_mut().set_style(row, style);
        frame.render_widget(Paragraph::new(format!(" {}", command.label)), row);
        if let Some(key) = &command.key {
            let key_width = u16::try_from(key.width() + 1)
                .unwrap_or(row.width)
                .min(row.width);
            let place = Rect {
                x: row.right() - key_width,
                width: key_width,
                ..row
            };
            let key_style = if selected {
                theme.list_selected
            } else {
                theme.key
            };
            frame.render_widget(Paragraph::new(Span::styled(key.as_str(), key_style)), place);
        }
    }
    (list, first)
}
