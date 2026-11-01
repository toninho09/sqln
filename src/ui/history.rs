//! The window of the query history.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::palette::{ListWindow, draw_list_window};
use super::text;
use super::theme::Theme;
use crate::history::{HistoryList, HistoryRow};

const WIDTH: u16 = 100;
/// The column of the statement, for the age of each row.
const AGE_WIDTH: usize = 9;

fn draw_row(frame: &mut Frame, area: Rect, row: &HistoryRow, selected: bool, theme: &Theme) {
    let (mark, mark_style) = if row.ok {
        (theme.icons.done, theme.mark_good)
    } else {
        (theme.icons.failed, theme.mark_bad)
    };
    let (base, mark_style) = if selected {
        (theme.list_selected, theme.list_selected)
    } else {
        (theme.dialog, mark_style)
    };
    frame.buffer_mut().set_style(area, base);
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(mark, mark_style),
        Span::raw(format!(" {:<AGE_WIDTH$} ", row.age)),
        Span::raw(row.line.as_str()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Draws the window. It returns the area of the list and the index of its first row.
pub fn draw_history(frame: &mut Frame, list: &HistoryList, theme: &Theme) -> (Rect, usize) {
    let window = ListWindow {
        title: text::TITLE_HISTORY,
        empty: text::NO_HISTORY_MATCH,
        width: WIDTH,
    };
    draw_list_window(frame, &window, list, theme, |frame, area, row, selected| {
        draw_row(frame, area, row, selected, theme);
    })
}
