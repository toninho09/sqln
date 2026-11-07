//! The window of the command palette.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::text;
use super::theme::Theme;
use super::widgets::{ListWindow, draw_list_window};
use crate::palette::{Command, Palette};

const WIDTH: u16 = 64;

fn draw_command(frame: &mut Frame, row: Rect, command: &Command, selected: bool, theme: &Theme) {
    let style = if selected {
        theme.list_selected
    } else {
        theme.dialog
    };
    frame.buffer_mut().set_style(row, style);
    let key_room = command.key.as_ref().map_or(0, |key| key.width() + 2);
    let label_area = Rect {
        width: row
            .width
            .saturating_sub(u16::try_from(key_room).unwrap_or(row.width)),
        ..row
    };
    frame.render_widget(Paragraph::new(format!(" {}", command.label)), label_area);
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

/// Draws the window. It returns the area of the list and the index of its first row.
pub fn draw_palette(frame: &mut Frame, palette: &Palette, theme: &Theme) -> (Rect, usize) {
    let window = ListWindow {
        title: text::TITLE_PALETTE,
        empty: text::NO_COMMANDS,
        width: WIDTH,
    };
    let list = palette.list();
    draw_list_window(
        frame,
        &window,
        list,
        theme,
        |frame, row, command, selected| {
            draw_command(frame, row, command, selected, theme);
        },
    )
}
