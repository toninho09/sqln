//! The window of the command palette.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use super::{float_block, input_spans, text};
use crate::grid::printable;
use crate::palette::Palette;

const WIDTH: u16 = 64;
const MAX_ROWS: usize = 12;
const TOP: u16 = 2;
const PROMPT: &str = " > ";

/// Draws the window. It returns the area of the list and the index of its first row.
pub fn draw_palette(frame: &mut Frame, palette: &Palette, theme: &Theme) -> (Rect, usize) {
    let bounds = frame.area();
    let count = palette.shown().len();
    let rows = count.clamp(1, MAX_ROWS);
    let width = WIDTH.min(bounds.width);
    let height = u16::try_from(rows + 3)
        .unwrap_or(u16::MAX)
        .min(bounds.height.saturating_sub(TOP));
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

    let chars: Vec<char> = printable(palette.input().text()).chars().collect();
    let field = usize::from(inner.width).saturating_sub(PROMPT.len()).max(1);
    let (spans, _) = input_spans(&chars, palette.input().cursor(), field, true, theme);
    let mut prompt = vec![Span::styled(PROMPT, theme.key)];
    prompt.extend(spans);
    frame.render_widget(
        Paragraph::new(Line::from(prompt)),
        Rect { height: 1, ..inner },
    );

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
